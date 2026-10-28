//! The CMVS engine (Purple software), as read from cmvs32.exe and the scripts of the
//! 未来ノスタルジア trial (2011).
//!
//! # How it runs
//!
//! The scenario scripts (snky*.ps2) run in the PS2 machine of [`crate::ps2`]. Each script
//! starts with a copy of the shared library (labels below the script's entry: messages,
//! backgrounds, characters, sound), which drives the engine's planes through some 400
//! commands. We do not run the library: every call of a library function is taken over here
//! by what it does on screen ([`Engine::call`], by the function's number in the first
//! library layout, see [`lib_id`]), with a small scene model:
//!
//! - a 3D background (plane 28, L144 and the L27 / L48..54 sets) and up to eight characters
//!   (planes 20..27, L116 / L117 / L94, moved by L118 / L120) placed in a camera space
//!   (L77; x, y = 36 eye level, z away from the camera; a picture of scale s at depth d is
//!   drawn s / d of its size),
//! - a 2D picture over them (plane 29, L151 / L153; L152 takes it away),
//! - a fade to black or white (L145, L147), the message window (L38, L66 / L11) and choices.
//!
//! Which file a background or character number names is the business of intcode.ps2, whose
//! entries 33 (background: `case G[700]` -> command 30 "file", G[701]) and 34 (character:
//! G[700] character, 701 costume, 702 pose, 704 kind -> global string 11) we do run. The few
//! library functions that name their files themselves (cmv backgrounds, layered backgrounds,
//! sound effects) run in the machine with the commands they reach stubbed, and we keep the
//! files they load ([`Capture`]).
//!
//! The title screen, the message window, the menu and the saves are our own.

use std::collections::HashMap;
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};
use vncommon::rule::Rule;

use crate::cpz::Cpz;
use crate::pb3;
use crate::ps2::{Done, Host, Script, Vm};

pub const WIDTH: u32 = 1024;
pub const HEIGHT: u32 = 768;
const FRAME_MS: f64 = 1000.0 / 60.0;
/// Pixels per unit of x at depth 1 (our guess of the engine's camera).
const FOCAL: f32 = 768.0;
const STEPS: usize = 200_000;
const SLOTS: usize = 10;
const CHARS: usize = 8;
/// The soft edge of rule wipes (the engine's own width not read).
const RULE_VAGUE: f32 = 0.3;
const ARCHIVES: [&str; 9] = [
    "script", "bg", "event", "stand", "balloon", "chip", "se", "voice", "ps",
];
/// Bytes of arguments each command drops (from cmvs32.exe's handlers), for the commands we
/// stub.
const POPS: &[(u16, u16)] = &include!("pops.in");

pub struct SoundEvent {
    pub channel: String,
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

#[derive(Clone, Copy, Default, Debug)]
struct Tween {
    from: f32,
    to: f32,
    start: f64,
    ms: f64,
}

impl Tween {
    fn new(v: f32) -> Tween {
        Tween {
            from: v,
            to: v,
            start: 0.0,
            ms: 0.0,
        }
    }

    fn at(&self, now: f64) -> f32 {
        if self.ms <= 0.0 || now >= self.start + self.ms {
            return self.to;
        }
        let t = ((now - self.start) / self.ms).clamp(0.0, 1.0) as f32;
        let t = t * t * (3.0 - 2.0 * t);
        self.from + (self.to - self.from) * t
    }

    fn go(&mut self, v: f32, now: f64, ms: f64) {
        self.from = self.at(now);
        self.to = v;
        self.start = now;
        self.ms = ms.max(0.0);
    }

    fn settle(&mut self) {
        self.from = self.to;
        self.ms = 0.0;
    }
}

#[derive(Clone, Debug)]
struct Chara {
    id: i32,
    pose: i32,
    kind: i32,
    file: String,
    pos: [Tween; 3],
    alpha: Tween,
    leaving: bool,
}

#[derive(Clone)]
struct Scene {
    bg: Option<String>,
    bg_scale: f32,
    overlay: Option<String>,
    chars: Vec<Option<Chara>>,
    /// x, y, z
    cam: [Tween; 3],
    fade_rgb: [u8; 3],
    fade: Tween,
}

impl Default for Scene {
    fn default() -> Scene {
        Scene {
            bg: None,
            bg_scale: 70.0,
            overlay: None,
            chars: vec![None; CHARS],
            cam: [Tween::new(0.0), Tween::new(36.0), Tween::new(0.0)],
            fade_rgb: [0, 0, 0],
            fade: Tween::new(0.0),
        }
    }
}

impl Scene {
    fn settle(&mut self) {
        for t in &mut self.cam {
            t.settle();
        }
        self.fade.settle();
        for c in self.chars.iter_mut() {
            if c.as_ref().is_some_and(|c| c.leaving) {
                *c = None;
            }
            if let Some(c) = c {
                c.alpha.settle();
                for t in &mut c.pos {
                    t.settle();
                }
            }
        }
    }
}

#[derive(Clone, Default)]
struct Msg {
    name: Vec<u8>,
    lines: Vec<Vec<u8>>,
}

#[derive(Clone, PartialEq)]
enum Wait {
    Click,
    Until(f64),
    UntilOrClick(f64),
    Choice(Vec<Vec<u8>>),
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Title,
    Game,
    Menu,
    Slots { save: bool, from_title: bool },
}

/// A library function run in the machine to see which files it loads.
struct Capture {
    id: usize,
    depth: usize,
    steps: usize,
    pc_after: usize,
    sp_after: usize,
    args: Vec<i32>,
}

struct Trans {
    from: Rc<Image>,
    start: f64,
    ms: f64,
    /// effects 21..80: the old screen goes by a rule picture (mask*.pb3)
    rule: Option<Rc<Image>>,
}

/// What a save holds.
#[derive(Clone)]
struct Snap {
    title: String,
    script: String,
    pc: usize,
    stack: Vec<u8>,
    frames: Vec<usize>,
    work: Vec<u8>,
    g: Vec<(usize, i32)>,
    bits: Vec<(usize, u8)>,
    gf: Vec<(usize, u32)>,
    gs: Vec<(usize, Vec<u8>)>,
    scene: Scene,
    window: bool,
    msg: Msg,
    bgm: Option<String>,
}

pub struct Engine {
    files: Box<dyn Files>,
    pub width: u32,
    pub height: u32,
    pub log: bool,
    archives: Vec<Cpz>,
    /// base name (lower case) -> (archive, entry name)
    index: HashMap<String, (usize, String)>,
    scripts: HashMap<String, Rc<Script>>,
    intcode: Option<Rc<Script>>,
    ic_work: Vec<u8>,
    entries: HashMap<i32, usize>,
    vm: Option<Box<Vm>>,
    depth: usize,
    capture: Option<Capture>,
    got: Vec<(String, i32)>,
    got_sounds: Vec<(Vec<u8>, Vec<i32>)>,
    images: HashMap<String, Option<Rc<Image>>>,
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<(Vec<u8>, [u8; 3], i32), Rc<Image>>,
    sounds: Vec<SoundEvent>,
    bgm: Option<String>,
    now: f64,
    view: View,
    scene: Scene,
    window: bool,
    msg: Msg,
    wait: Option<Wait>,
    trans: Option<Trans>,
    clicked: bool,
    skip: bool,
    pointer: (i32, i32),
    next_delay: f64,
    pending: Vec<(f64, i32, i32)>,
    armed: Vec<(f64, i32, i32)>,
    chapter: Vec<u8>,
    snap_pending: bool,
    saved: Option<Snap>,
    /// set by the library's end of the trial (L168)
    to_title: bool,
    ended: bool,
    rules: Vec<(usize, Rule)>,
}

fn solid_image(rgba: [u8; 4]) -> Rc<Image> {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<[u8; 4], Rc<Image>>> = Default::default();
    }
    CACHE.with(|c| {
        c.borrow_mut()
            .entry(rgba)
            .or_insert_with(|| {
                let mut i = Image::new(1, 1);
                i.fill(rgba);
                Rc::new(i)
            })
            .clone()
    })
}

fn place(alpha: f32) -> Place {
    Place {
        x: 0.0,
        y: 0.0,
        sx: 1.0,
        sy: 1.0,
        cx: 0.0,
        cy: 0.0,
        rot: 0.0,
        alpha,
        add: false,
        flip: (false, false),
        clip: [0.0, 0.0, 1e6, 1e6],
    }
}

fn draw_at(image: &Rc<Image>, src: [i32; 4], dst: [f32; 4], alpha: f32) -> Draw {
    Draw {
        image: image.clone(),
        src,
        dst,
        place: place(alpha),
    }
}

fn draw(image: &Rc<Image>, x: f32, y: f32, alpha: f32) -> Draw {
    let (w, h) = (image.w as f32, image.h as f32);
    draw_at(
        image,
        [0, 0, image.w as i32, image.h as i32],
        [x, y, x + w, y + h],
        alpha,
    )
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
}

fn f(v: i32) -> f32 {
    f32::from_bits(v as u32)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02x}")).collect()
}

fn unhex(t: &str) -> Vec<u8> {
    (0..t.len() / 2)
        .filter_map(|i| u8::from_str_radix(t.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

/// The number of a library function in the first library layout (scripts whose entry is at
/// 0x2E352, 172 library labels), or None for the script's own labels. The second layout
/// (174 labels, snky01 and snky07..12) moved the later functions; its four new ones get
/// 1000 + their number (1156 is the choice).
pub fn lib_id(s: &Script, label: usize) -> Option<usize> {
    let at = *s.labels.get(label)?;
    if at >= s.entry {
        return None;
    }
    let n_lib = s.labels.iter().filter(|&&a| a < s.entry).count();
    if n_lib < 174 {
        return Some(label);
    }
    Some(match label {
        157 => 160,
        158 => 156,
        160 => 157,
        161 => 158,
        162 => 159,
        163 => 161,
        164 => 162,
        166 => 164,
        167 => 165,
        169 => 167,
        170 => 168,
        171 => 169,
        172 => 170,
        173 => 171,
        156 | 159 | 165 | 168 => 1000 + label,
        n => n,
    })
}

/// The functions run in the machine to see the files they load: cmv backgrounds (27),
/// layered backgrounds (48, 50..54), sound effects (32).
const CAPTURED: [usize; 10] = [22, 24, 27, 32, 48, 50, 51, 52, 53, 54];

/// Message text: `{base/ruby}` keeps the base, `\n` breaks the line, `\t` is dropped.
fn clean(t: &[u8]) -> Vec<Vec<u8>> {
    let mut lines = vec![Vec::new()];
    let cs = nexas::sjis::chars(t);
    let mut i = 0;
    let mut ruby = 0; // 1 in the base, 2 in the ruby
    while i < cs.len() {
        let c = cs[i];
        match c {
            b"\\" if i + 1 < cs.len() => {
                if cs[i + 1] == b"n" {
                    lines.push(Vec::new());
                }
                i += 2;
                continue;
            }
            b"{" => ruby = 1,
            b"/" if ruby == 1 => ruby = 2,
            b"}" if ruby > 0 => ruby = 0,
            _ if ruby == 2 => {}
            _ => lines.last_mut().unwrap().extend_from_slice(c),
        }
        i += 1;
    }
    lines
}

/// Breaks a line every `cols` columns (a full-width character is two).
fn wrap(line: &[u8], cols: usize) -> Vec<Vec<u8>> {
    let mut out = vec![Vec::new()];
    let mut n = 0;
    for c in nexas::sjis::chars(line) {
        let w = if c.len() == 2 { 2 } else { 1 };
        // keep closing punctuation on the line it ends
        let closing = matches!(
            nexas::sjis::decode(c).as_str(),
            "。" | "、" | "」" | "』" | "）" | "…" | "―" | "！" | "？" | "ー"
        );
        if n + w > cols && !closing {
            out.push(Vec::new());
            n = 0;
        }
        out.last_mut().unwrap().extend_from_slice(c);
        n += w;
    }
    out
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let mut archives = Vec::new();
        let mut index = HashMap::new();
        for a in ARCHIVES {
            let path = format!("data/pack/{a}.cpz");
            if files.size(&path).is_none() {
                continue;
            }
            let cpz = Cpz::open(files.as_mut(), &path)?;
            let k = archives.len();
            for name in cpz.entries.keys() {
                let base = name.rsplit('/').next().unwrap_or(name).to_string();
                index.entry(base).or_insert((k, name.clone()));
            }
            archives.push(cpz);
        }
        if !index.contains_key("intcode.ps2") {
            return Err("CMVS: data/pack/script.cpz has no intcode.ps2".into());
        }
        let mut e = Engine {
            files,
            width: WIDTH,
            height: HEIGHT,
            log: false,
            archives,
            index,
            scripts: HashMap::new(),
            intcode: None,
            ic_work: Vec::new(),
            entries: HashMap::new(),
            vm: None,
            depth: 0,
            capture: None,
            got: Vec::new(),
            got_sounds: Vec::new(),
            images: HashMap::new(),
            font: None,
            text_cache: HashMap::new(),
            sounds: Vec::new(),
            bgm: None,
            now: 0.0,
            view: View::Title,
            scene: Scene::default(),
            window: false,
            msg: Msg::default(),
            wait: None,
            trans: None,
            clicked: false,
            skip: false,
            pointer: (0, 0),
            next_delay: 0.0,
            pending: Vec::new(),
            armed: Vec::new(),
            chapter: Vec::new(),
            snap_pending: false,
            saved: None,
            to_title: false,
            ended: false,
            rules: Vec::new(),
        };
        let ic = e
            .script("intcode.ps2")
            .ok_or("CMVS: intcode.ps2 does not parse")?;
        e.ic_work = ic.data.clone();
        e.ic_work.resize(ic.work.max(ic.data.len()), 0);
        e.intcode = Some(ic.clone());
        // intcode's label 0 registers its entries (command 0x88: 0, 0, 0, address, number)
        let mut vm = Box::new(Vm::new(ic.clone()));
        vm.pc = ic.labels.first().copied().unwrap_or(0) as usize;
        e.depth = 1;
        for _ in 0..10_000 {
            if vm.step(&mut e).is_none() {
                break;
            }
        }
        e.depth = 0;
        if e.entries.is_empty() {
            return Err("CMVS: intcode.ps2 registers no entries".into());
        }
        Ok(e)
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        self.font = Some(Rc::new(f));
        self.text_cache.clear();
        Ok(())
    }

    fn read_entry(&mut self, name: &str) -> Option<Vec<u8>> {
        let (k, full) = self.index.get(&name.to_ascii_lowercase())?.clone();
        self.archives[k].read(self.files.as_mut(), &full)
    }

    fn script(&mut self, name: &str) -> Option<Rc<Script>> {
        let name = name.to_ascii_lowercase();
        if let Some(s) = self.scripts.get(&name) {
            return Some(s.clone());
        }
        let b = self.read_entry(&name)?;
        let s = Rc::new(Script::parse(&name, &b).ok()?);
        self.scripts.insert(name, s.clone());
        Some(s)
    }

    fn image(&mut self, name: &str) -> Option<Rc<Image>> {
        let key = name.to_ascii_lowercase();
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let img = self.load_image(&key).map(Rc::new);
        if img.is_none() {
            self.trace(&format!("no picture {key}"));
        }
        if self.images.len() > 40 {
            self.images.clear();
        }
        self.images.insert(key, img.clone());
        img
    }

    fn load_image(&mut self, name: &str) -> Option<Image> {
        let name = match name.strip_suffix(".cmv") {
            // a moving background: its picture of the same name
            Some(stem) => format!("{stem}.pb3"),
            None => name.to_string(),
        };
        let (k, full) = self.index.get(&name)?.clone();
        let b = self.archives[k].read(self.files.as_mut(), &full)?;
        let dir = full.rsplit_once('/').map_or("", |d| d.0).to_string();
        let cpz = &self.archives[k];
        let files = self.files.as_mut();
        pb3::decode(&b, &mut |n| {
            let full = if dir.is_empty() {
                format!("{n}.pb3")
            } else {
                format!("{dir}/{n}.pb3")
            };
            let b = cpz.read(files, &full.to_ascii_lowercase())?;
            pb3::decode(&b, &mut |_| None).ok()
        })
        .ok()
    }

    fn trace(&self, s: &str) {
        if self.log {
            eprintln!("cmvs: {s}");
        }
    }

    fn label(&mut self, s: &[u8], rgb: [u8; 3], size: i32) -> Rc<Image> {
        let key = (s.to_vec(), rgb, size);
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let img = match &self.font {
            Some(f) => Rc::new(Font::outline(f.clone(), size).render(s, rgb, 2)),
            None => Rc::new(Image::new(1, 1)),
        };
        if self.text_cache.len() > 300 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, img.clone());
        img
    }

    // ---- sound ----

    fn play(&mut self, channel: &str, data: Option<Vec<u8>>, repeat: bool, fade_ms: f32) {
        self.sounds.push(SoundEvent {
            channel: channel.into(),
            data,
            volume: 1.0,
            repeat,
            fade_ms,
        });
    }

    fn play_bgm(&mut self, name: &str) {
        if self.bgm.as_deref() == Some(name) {
            return;
        }
        let data = self.files.read(&format!("data/music/{name}"));
        self.bgm = Some(name.into());
        self.play("bgm", data, true, 0.0);
    }

    fn stop_bgm(&mut self, ms: f32) {
        self.bgm = None;
        self.play("bgm", None, false, ms);
    }

    fn stop_se(&mut self, ms: f32) {
        for ch in ["se0", "se1", "se2"] {
            self.play(ch, None, false, ms);
        }
    }

    // ---- the machine ----

    fn new_game(&mut self) {
        let Some(s) = self.script("snky00b.ps2") else {
            self.trace("no snky00b.ps2");
            return;
        };
        let mut vm = Box::new(Vm::new(s));
        vm.log = self.log;
        self.vm = Some(vm);
        self.scene = Scene::default();
        self.window = false;
        self.msg = Msg::default();
        self.wait = None;
        self.view = View::Game;
        self.chapter.clear();
        self.saved = None;
        self.pending.clear();
        self.armed.clear();
    }

    fn run_vm(&mut self) {
        let Some(mut vm) = self.vm.take() else {
            return;
        };
        let mut to_title = false;
        for _ in 0..STEPS {
            if self.wait.is_some() || self.view != View::Game {
                break;
            }
            if let Some(name) = vm.goto.take() {
                let name = name.to_ascii_lowercase();
                self.trace(&format!("goto {name}"));
                match self.script(&name) {
                    Some(s) if name.starts_with("snky") => vm.load(s),
                    _ => {
                        to_title = true;
                        break;
                    }
                }
                continue;
            }
            let r = vm.step(self);
            if let Some(c) = self.capture.as_mut() {
                c.steps += 1;
                if vm.frames.len() <= c.depth {
                    let c = self.capture.take().unwrap();
                    self.finish_capture(&mut vm, c);
                } else if c.steps > 300_000 {
                    let c = self.capture.take().unwrap();
                    self.trace(&format!("L{} runs too long", c.id));
                    vm.frames.truncate(c.depth);
                    vm.sp = c.sp_after;
                    vm.pc = c.pc_after;
                }
            }
            if self.to_title {
                self.to_title = false;
                to_title = true;
                break;
            }
            if self.snap_pending {
                self.snap_pending = false;
                self.saved = Some(self.snap(&vm));
            }
            match r {
                Some(true) => break,
                Some(false) => {}
                None => {
                    self.trace("the script ended");
                    to_title = true;
                    break;
                }
            }
        }
        self.vm = Some(vm);
        if to_title {
            self.back_to_title();
        }
    }

    /// Runs entry `n` of intcode.ps2 (command 0x8A) to its end.
    fn run_entry(&mut self, vm: &mut Vm, n: i32) {
        let (Some(&addr), Some(ic)) = (self.entries.get(&n), self.intcode.clone()) else {
            return;
        };
        let script = std::mem::replace(&mut vm.script, ic);
        let (pc, sp) = (vm.pc, vm.sp);
        let frames = std::mem::replace(&mut vm.frames, vec![vm.sp]);
        let work = std::mem::replace(&mut vm.work, std::mem::take(&mut self.ic_work));
        vm.pc = addr;
        self.depth += 1;
        for _ in 0..200_000 {
            if vm.step(self).is_none() {
                break;
            }
        }
        self.depth -= 1;
        self.ic_work = std::mem::replace(&mut vm.work, work);
        vm.script = script;
        vm.pc = pc;
        vm.sp = sp;
        vm.frames = frames;
    }

    fn bg_file(&mut self, vm: &mut Vm, id: i32) -> Option<String> {
        vm.g[700] = id;
        vm.g[701] = 29;
        self.got.clear();
        self.run_entry(vm, 33);
        let r = self.got.first().map(|g| g.0.clone());
        self.trace(&format!("bg {id} -> {r:?}"));
        r
    }

    fn stand_file(&mut self, vm: &mut Vm, ch: i32, pose: i32, kind: i32) -> String {
        let costume = vm.g.get(160 + ch as usize).copied().unwrap_or(0);
        vm.g[700] = ch;
        vm.g[701] = costume;
        vm.g[702] = pose;
        vm.g[704] = kind;
        vm.gs[11].clear();
        self.run_entry(vm, 34);
        let r = nexas::sjis::decode(&vm.gs[11]).to_ascii_lowercase();
        self.trace(&format!(
            "char {ch} costume {costume} pose {pose} kind {kind} -> {r}"
        ));
        r
    }

    fn finish_capture(&mut self, vm: &mut Vm, c: Capture) {
        let got = std::mem::take(&mut self.got);
        let sounds = std::mem::take(&mut self.got_sounds);
        self.trace(&format!(
            "L{} {:?} loaded {got:?} (pc {:x} / {:x}, sp {} / {})",
            c.id, c.args, vm.pc, c.pc_after, vm.sp, c.sp_after
        ));
        match c.id {
            32 => {
                // command 0xB0: ?, loop, ?, name, channel
                if let Some((n, a)) = sounds.first() {
                    let name = nexas::sjis::decode(n);
                    let data = self.read_entry(&name);
                    let ch = format!("se{}", a[4].clamp(0, 2));
                    self.play(&ch, data, a[1] != 0, 0.0);
                }
            }
            _ => {
                if let Some((name, _)) = got.into_iter().next() {
                    self.scene.bg = Some(name);
                }
            }
        }
        let _ = vm;
    }

    // ---- the library, taken over ----

    fn transition(&mut self, ms: i32, effect: i32) {
        if ms <= 0 || self.skip {
            self.trans = None;
            return;
        }
        let ds = self.scene_draws();
        let from = Rc::new(self.render_draws(&ds));
        let rule = self.rule(effect);
        self.trans = Some(Trans {
            from,
            start: self.now,
            ms: ms as f64,
            rule,
        });
    }

    /// The rule picture of a transition effect, as the library's L34 / L40 name it: 21..40
    /// mask(n - 20)a.pb3 (command 0x22A), 41..60 the same masks the other way (0x234),
    /// 61..70 and 71..80 again (n - 60 / n - 70). Other effects are fades.
    fn rule(&mut self, effect: i32) -> Option<Rc<Image>> {
        let (n, back) = match effect {
            21..=40 => (effect - 20, false),
            41..=60 => (effect - 40, true),
            61..=70 => (effect - 60, false),
            71..=80 => (effect - 70, true),
            _ => return None,
        };
        let name = format!("mask{n:03}a.pb3");
        let key = format!("{name}~");
        if back {
            if let Some(i) = self.images.get(&key) {
                return i.clone();
            }
            let mut inv = (*self.image(&name)?).clone();
            for p in inv.px.chunks_mut(4) {
                for c in &mut p[..3] {
                    *c = 255 - *c;
                }
            }
            inv.renew();
            let inv = Rc::new(inv);
            self.images.insert(key, Some(inv.clone()));
            return Some(inv);
        }
        self.image(&name)
    }

    fn wait_ms(&mut self, ms: f64) {
        if ms > 0.0 && !self.skip {
            self.wait = Some(Wait::Until(self.now + ms));
        }
    }

    fn slot_of(&self, ch: i32) -> Option<usize> {
        self.scene
            .chars
            .iter()
            .position(|c| c.as_ref().is_some_and(|c| c.id == ch && !c.leaving))
    }

    #[allow(clippy::too_many_arguments)]
    fn show_char(&mut self, vm: &mut Vm, ms: i32, pos: [f32; 3], pose: i32, ch: i32, kind: i32) {
        let file = self.stand_file(vm, ch, pose, kind);
        let slot = self.slot_of(ch).or_else(|| {
            self.scene
                .chars
                .iter()
                .position(|c| c.as_ref().is_none_or(|c| c.leaving))
        });
        let Some(slot) = slot else {
            return;
        };
        let now = self.now;
        let fresh = self.scene.chars[slot]
            .as_ref()
            .is_none_or(|c| c.leaving || c.id != ch);
        let mut alpha = Tween::new(1.0);
        if fresh && ms > 0 && !self.skip {
            alpha = Tween::new(0.0);
            alpha.go(1.0, now, ms as f64);
        }
        self.scene.chars[slot] = Some(Chara {
            id: ch,
            pose,
            kind,
            file,
            pos: pos.map(Tween::new),
            alpha,
            leaving: false,
        });
    }

    fn set_pose(&mut self, vm: &mut Vm, pose: i32, ch: i32) {
        let Some(slot) = self.slot_of(ch) else {
            return;
        };
        let kind = self.scene.chars[slot].as_ref().map_or(0, |c| c.kind);
        let file = self.stand_file(vm, ch, pose, kind);
        if let Some(c) = self.scene.chars[slot].as_mut() {
            c.pose = pose;
            c.file = file;
        }
    }

    fn move_char(&mut self, ms: i32, d: [f32; 3], ch: i32, leave: bool) {
        let Some(slot) = self.slot_of(ch) else {
            return;
        };
        let now = self.now;
        let ms = if self.skip { 0.0 } else { ms.max(0) as f64 };
        if let Some(c) = self.scene.chars[slot].as_mut() {
            for (t, d) in c.pos.iter_mut().zip(d) {
                let to = t.to + d;
                t.go(to, now, ms);
            }
            if leave {
                if ms <= 0.0 {
                    self.scene.chars[slot] = None;
                } else {
                    c.leaving = true;
                    c.alpha.go(0.0, now, ms);
                }
            }
        }
    }

    fn message(&mut self, text: i32, voice: i32, name: i32, face: i32, ch: i32, vm: &mut Vm) {
        let t = vm.text(text);
        let mut lines = Vec::new();
        for l in clean(&t) {
            lines.extend(wrap(&l, 66));
        }
        let name = vm.text(name);
        let voice = vm.text(voice);
        if face >= 0 && ch >= 1 {
            self.set_pose(vm, face, ch - 1);
        }
        self.msg = Msg {
            name: clean(&name).concat(),
            lines,
        };
        self.window = true;
        if voice.is_empty() {
            self.play("voice", None, false, 0.0);
        } else {
            let n = nexas::sjis::decode(&voice);
            let data = self.read_entry(&n);
            if data.is_none() {
                self.trace(&format!("no voice {n}"));
            }
            self.play("voice", data, false, 0.0);
        }
        let now = self.now;
        for (d, pose, ch) in std::mem::take(&mut self.pending) {
            self.armed.push((now + d, pose, ch));
        }
        self.wait = Some(Wait::Click);
        self.snap_pending = true;
    }

    fn take_over(&mut self, vm: &mut Vm, id: usize, a: &[i32]) {
        let ai = |i: usize| a.get(i).copied().unwrap_or(0);
        let af = |i: usize| f(ai(i));
        let now = self.now;
        match id {
            // messages: text, voice, name, face, motion, speaker + 1, number
            66 | 11 => self.message(ai(0), ai(1), ai(2), ai(3), ai(5), vm),
            // the window
            38 => {
                self.window = ai(0) != 0;
                if !self.window {
                    self.msg = Msg::default();
                }
            }
            // music: (1, number), stop, fade out
            55 => {
                let n = ai(1);
                self.play_bgm(&format!("bgm{n:02}.ogg"));
            }
            57 => self.stop_bgm(0.0),
            58 => self.stop_bgm(ai(0).max(0) as f32),
            59..=64 => self.stop_se(0.0),
            65 => self.stop_se(ai(0).max(0) as f32),
            // the camera: ms, y, z, x
            77 => {
                let ms = if self.skip { 0.0 } else { ai(0).max(0) as f64 };
                let to = [af(3), af(1), af(2)];
                for (t, v) in self.scene.cam.iter_mut().zip(to) {
                    t.go(v, now, ms);
                }
            }
            98 => {
                if let Some(g) = vm.g.get_mut(160 + ai(1) as usize) {
                    *g = ai(0);
                }
            }
            // characters: (ms, y, z, x, pose, char)
            116 => self.show_char(vm, ai(0), [af(3), af(1), af(2)], ai(4), ai(5), 0),
            // (ms, ?, ?, dy, dz, dx, y, z, x, pose, char)
            117 => self.show_char(vm, ai(0), [af(8), af(6), af(7)], ai(9), ai(10), 0),
            // (fade, y, z, x, kind, pose, char)
            94 => {
                let kind = if ai(4) == 0 { 2 } else { 3 };
                self.show_char(vm, ai(0) * 500, [af(3), af(1), af(2)], ai(5), ai(6), kind)
            }
            // (ms, ?, ?, dy, dz, dx, char): move; 120 also takes the character away
            118 | 120 => self.move_char(ai(0), [af(5), af(3), af(4)], ai(6), id == 120),
            // (fade, pose, char)
            12 | 13 => self.set_pose(vm, ai(1), ai(2)),
            128 => self.next_delay = ai(0).max(0) as f64,
            129 => self.pending.push((self.next_delay, ai(1), ai(2))),
            111 => self.scene.chars.iter_mut().for_each(|c| *c = None),
            // backgrounds
            142 => {
                const SCALES: [f32; 7] = [70.0, 100.0, 150.0, 200.0, 50.0, 30.0, 20.0];
                self.scene.bg_scale = SCALES[ai(0).clamp(0, 6) as usize];
            }
            144 => self.scene.bg = self.bg_file(vm, ai(0)),
            // the trial's end
            1168 => self.to_title = true,
            // the end of the prologue's picture
            23 => self.scene.bg = None,
            // (ms, effect, window, id): a picture over everything
            151 | 153 => {
                let file = self.bg_file(vm, ai(3));
                self.transition(ai(0), ai(1));
                self.scene.overlay = file;
                self.scene.chars.iter_mut().for_each(|c| *c = None);
                if id == 151 {
                    self.scene.bg = None;
                }
                self.scene.cam = Scene::default().cam;
                self.wait_ms(ai(0) as f64);
            }
            // (ms, effect, window): the picture goes, showing the scene
            152 => {
                self.transition(ai(0), ai(1));
                self.scene.overlay = None;
                self.wait_ms(ai(0) as f64);
            }
            // fades: (ms, white, background) in, (ms, white) out
            147 => {
                if ai(2) >= 0 {
                    self.scene.bg = self.bg_file(vm, ai(2));
                }
                let ms = if ai(0) == 0 { 1250 } else { ai(0) };
                self.scene.fade_rgb = if ai(1) == 0 { [0; 3] } else { [255; 3] };
                self.scene.fade = Tween::new(1.0);
                self.scene
                    .fade
                    .go(0.0, now, if self.skip { 0.0 } else { ms as f64 });
                self.wait_ms(ms as f64);
            }
            145 => {
                let ms = if ai(0) == 0 { 1250 } else { ai(0) };
                self.scene.fade_rgb = if ai(1) == 0 { [0; 3] } else { [255; 3] };
                self.scene
                    .fade
                    .go(1.0, now, if self.skip { 0.0 } else { ms as f64 });
                self.wait_ms(ms as f64);
                self.window = false;
                self.msg = Msg::default();
                let fade = (self.scene.fade, self.scene.fade_rgb);
                self.scene = Scene::default();
                (self.scene.fade, self.scene.fade_rgb) = fade;
            }
            // waits
            167 => {
                if ai(0) > 0 && !self.skip {
                    self.wait = Some(Wait::UntilOrClick(now + ai(0) as f64));
                }
            }
            // the chapter's title
            162 => self.chapter = vm.text(ai(0)),
            // the choice: (?, first, second) -> G[64]
            1156 => {
                let opts: Vec<Vec<u8>> = a[1..]
                    .iter()
                    .map(|&r| vm.text(r))
                    .filter(|t| !t.is_empty())
                    .collect();
                if !opts.is_empty() {
                    self.window = false;
                    self.wait = Some(Wait::Choice(opts));
                }
            }
            _ => {}
        }
    }

    // ---- frames ----

    pub fn frame(&mut self) -> Result<(), String> {
        self.now += FRAME_MS;
        if let Some(t) = &self.trans
            && self.now - t.start >= t.ms
        {
            self.trans = None;
        }
        let now = self.now;
        let due: Vec<(f64, i32, i32)> = self.armed.iter().filter(|a| a.0 <= now).copied().collect();
        if !due.is_empty() {
            self.armed.retain(|a| a.0 > now);
            if let Some(mut vm) = self.vm.take() {
                for (_, pose, ch) in due {
                    self.set_pose(&mut vm, pose, ch);
                }
                self.vm = Some(vm);
            }
        }
        for c in self.scene.chars.iter_mut() {
            if c.as_ref()
                .is_some_and(|c| c.leaving && c.alpha.at(now) <= 0.0)
            {
                *c = None;
            }
        }
        if self.view == View::Game {
            match &self.wait {
                Some(Wait::Until(t)) if now >= *t || self.skip => self.wait = None,
                Some(Wait::UntilOrClick(t)) if now >= *t || self.skip || self.clicked => {
                    self.wait = None
                }
                Some(Wait::Click) if self.skip => self.wait = None,
                _ => {}
            }
            self.run_vm();
        }
        self.clicked = false;
        Ok(())
    }

    fn scene_draws(&mut self) -> Vec<Draw> {
        let now = self.now;
        let mut out = vec![draw_at(
            &solid_image([0, 0, 0, 255]),
            [0, 0, 1, 1],
            [0.0, 0.0, WIDTH as f32, HEIGHT as f32],
            1.0,
        )];
        let cam = [
            self.scene.cam[0].at(now),
            self.scene.cam[1].at(now),
            self.scene.cam[2].at(now),
        ];
        let project =
            |p: [f32; 3], s: f32, pivot: (f32, f32), img: &Rc<Image>| -> Option<[f32; 4]> {
                let d = p[2] - cam[2];
                if d < 0.5 {
                    return None;
                }
                let k = FOCAL / d;
                let (sx, sy) = (
                    WIDTH as f32 / 2.0 + (p[0] - cam[0]) * k,
                    HEIGHT as f32 / 2.0 - (p[1] - cam[1]) * k,
                );
                let sc = s / d;
                let x0 = sx - pivot.0 * sc;
                let y0 = sy - pivot.1 * sc;
                Some([x0, y0, x0 + img.w as f32 * sc, y0 + img.h as f32 * sc])
            };
        if let Some(n) = self.scene.bg.clone()
            && let Some(img) = self.image(&n)
        {
            let s = self.scene.bg_scale;
            if let Some(dst) = project(
                [0.0, 36.0, s],
                s,
                (img.w as f32 / 2.0, img.h as f32 / 2.0),
                &img,
            ) {
                out.push(draw_at(&img, [0, 0, img.w as i32, img.h as i32], dst, 1.0));
            }
        }
        let mut chars: Vec<Chara> = self.scene.chars.iter().flatten().cloned().collect();
        chars.sort_by(|a, b| b.pos[2].at(now).total_cmp(&a.pos[2].at(now)));
        for c in chars {
            let Some(img) = self.image(&c.file) else {
                continue;
            };
            let pivot = if c.kind == 0 {
                (img.w as f32 / 2.0, img.h as f32 - 1154.0)
            } else {
                (img.w as f32 / 2.0, img.h as f32 / 2.0)
            };
            let p = [c.pos[0].at(now), c.pos[1].at(now), c.pos[2].at(now)];
            if let Some(dst) = project(p, 20.0, pivot, &img) {
                out.push(draw_at(
                    &img,
                    [0, 0, img.w as i32, img.h as i32],
                    dst,
                    c.alpha.at(now),
                ));
            }
        }
        if let Some(n) = self.scene.overlay.clone()
            && let Some(img) = self.image(&n)
        {
            out.push(draw(&img, 0.0, 0.0, 1.0));
        }
        let fa = self.scene.fade.at(now);
        if fa > 0.0 {
            let [r, g, b] = self.scene.fade_rgb;
            out.push(draw_at(
                &solid_image([r, g, b, 255]),
                [0, 0, 1, 1],
                [0.0, 0.0, WIDTH as f32, HEIGHT as f32],
                fa,
            ));
        }
        out
    }

    fn solid(&self, rgba: [u8; 4], r: [i32; 4]) -> Draw {
        draw_at(
            &solid_image(rgba),
            [0, 0, 1, 1],
            [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32],
            1.0,
        )
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        if !self.window {
            return;
        }
        out.push(self.solid([10, 10, 30, 170], [24, 572, 1000, 752]));
        let m = self.msg.clone();
        if !m.name.is_empty() {
            let mut n = nexas::sjis::encode("【");
            n.extend_from_slice(&m.name);
            n.extend_from_slice(&nexas::sjis::encode("】"));
            let img = self.label(&n, [255, 230, 190], 24);
            out.push(draw(&img, 56.0, 580.0, 1.0));
        }
        for (k, l) in m.lines.iter().take(4).enumerate() {
            let img = self.label(l, [255, 255, 255], 26);
            out.push(draw(&img, 64.0, 616.0 + k as f32 * 33.0, 1.0));
        }
    }

    fn choice_rects(&self, n: usize) -> Vec<[i32; 4]> {
        let (bw, bh, gap) = (640, 56, 24);
        let top = (HEIGHT as i32 - (bh * n as i32 + gap * (n as i32 - 1))) / 2 - 60;
        (0..n as i32)
            .map(|i| {
                let y = top + i * (bh + gap);
                [(WIDTH as i32 - bw) / 2, y, (WIDTH as i32 + bw) / 2, y + bh]
            })
            .collect()
    }

    fn button(&mut self, t: &[u8], r: [i32; 4], out: &mut Vec<Draw>) {
        let over = inside(r, self.pointer.0, self.pointer.1);
        out.push(self.solid(
            if over {
                [200, 110, 160, 235]
            } else {
                [60, 30, 60, 215]
            },
            r,
        ));
        let img = self.label(t, [255, 255, 255], 24);
        let x = (r[0] + r[2]) as f32 / 2.0 - img.w as f32 / 2.0;
        let y = (r[1] + r[3]) as f32 / 2.0 - img.h as f32 / 2.0;
        out.push(draw(&img, x, y, 1.0));
    }

    fn title_rects(&self) -> Vec<[i32; 4]> {
        (0..4)
            .map(|i| [736, 272 + 40 * i, 736 + 270, 272 + 40 * i + 36])
            .collect()
    }

    fn title_draws(&mut self, out: &mut Vec<Draw>) {
        out.push(self.solid([255, 255, 255, 255], [0, 0, WIDTH as i32, HEIGHT as i32]));
        let Some(chip) = self.image("title_chip_tr2.pb3") else {
            for (i, t) in ["はじめから", "つづきから", "", "ゲーム終了"]
                .iter()
                .enumerate()
            {
                if !t.is_empty() {
                    let r = self.title_rects()[i];
                    self.button(&nexas::sjis::encode(t), r, out);
                }
            }
            return;
        };
        out.push(draw_at(
            &chip,
            [0, 0, 1024, 768],
            [0.0, 0.0, 1024.0, 768.0],
            1.0,
        ));
        for (i, r) in self.title_rects().into_iter().enumerate() {
            if i == 2 {
                continue; // コンフィグ: there is none
            }
            let over = inside(r, self.pointer.0, self.pointer.1) as i32;
            let sy = 770 + 40 * i as i32;
            out.push(draw_at(
                &chip,
                [272 * over, sy, 272 * over + 270, sy + 36],
                [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32],
                1.0,
            ));
        }
    }

    fn menu_items(&self) -> Vec<(&'static str, [i32; 4])> {
        let names = ["セーブ", "ロード", "タイトルへ戻る", "ゲームに戻る"];
        let (bw, bh, gap) = (320, 50, 18);
        let top = (HEIGHT as i32 - (bh * 4 + gap * 3)) / 2;
        names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let y = top + i as i32 * (bh + gap);
                (
                    *n,
                    [(WIDTH as i32 - bw) / 2, y, (WIDTH as i32 + bw) / 2, y + bh],
                )
            })
            .collect()
    }

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        let step = 56;
        let (bw, bh) = (WIDTH as i32 - 160, step - 10);
        let top = (HEIGHT as i32 - step * (SLOTS as i32 + 1)) / 2 + 16;
        (0..=SLOTS as i32)
            .map(|i| {
                let y = top + i * step;
                [80, y, 80 + bw, y + bh]
            })
            .collect()
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        self.rules.clear();
        if matches!(
            self.view,
            View::Title
                | View::Slots {
                    from_title: true,
                    ..
                }
        ) {
            self.title_draws(&mut out);
        } else {
            out = self.scene_draws();
            if let Some(t) = &self.trans {
                let p = ((self.now - t.start) / t.ms.max(1.0)).clamp(0.0, 1.0) as f32;
                match &t.rule {
                    Some(r) => {
                        self.rules.push((
                            out.len(),
                            Rule {
                                image: r.clone(),
                                progress: p,
                                vague: RULE_VAGUE,
                                appear: false,
                            },
                        ));
                        out.push(draw(&t.from.clone(), 0.0, 0.0, 1.0));
                    }
                    None => out.push(draw(&t.from.clone(), 0.0, 0.0, 1.0 - p)),
                }
            }
            self.window_draws(&mut out);
            if let Some(Wait::Choice(opts)) = self.wait.clone() {
                for (o, r) in opts.iter().zip(self.choice_rects(opts.len())) {
                    self.button(o, r, &mut out);
                }
            }
        }
        match self.view {
            View::Menu => {
                out.push(self.solid([0, 0, 0, 150], [0, 0, WIDTH as i32, HEIGHT as i32]));
                for (t, r) in self.menu_items() {
                    self.button(&nexas::sjis::encode(t), r, &mut out);
                }
            }
            View::Slots { save, .. } => {
                out.push(self.solid([0, 0, 0, 200], [0, 0, WIDTH as i32, HEIGHT as i32]));
                let rects = self.slot_rects();
                for (i, r) in rects.iter().enumerate() {
                    let label = if i == SLOTS {
                        "戻る".to_string()
                    } else {
                        match self.read_slot(i) {
                            Some(s) => format!("{:02}  {}", i + 1, s.title),
                            None => format!("{:02}  ----", i + 1),
                        }
                    };
                    self.button(&nexas::sjis::encode(&label), *r, &mut out);
                }
                let head = self.label(
                    &nexas::sjis::encode(if save { "セーブ" } else { "ロード" }),
                    [255, 255, 255],
                    26,
                );
                out.push(draw(&head, 80.0, (rects[0][1] - 40) as f32, 1.0));
            }
            _ => {}
        }
        out
    }

    fn render_draws(&self, ds: &[Draw]) -> Image {
        self.render_ruled(ds, &[])
    }

    fn render_ruled(&self, ds: &[Draw], rules: &[(usize, Rule)]) -> Image {
        let mut img = Image::new(WIDTH, HEIGHT);
        img.fill([0, 0, 0, 255]);
        for (i, d) in ds.iter().enumerate() {
            let r = rules.iter().find(|r| r.0 == i).map(|r| &r.1);
            vncommon::rule::draw_ruled(&mut img, &d.image, d.src, d.dst, &d.place, r);
        }
        img
    }

    /// The rule wipes of the last [`Engine::draws`]: (draw index, rule).
    pub fn rules(&self) -> &[(usize, Rule)] {
        &self.rules
    }

    /// The frame as one picture (for the native runner).
    pub fn render(&mut self) -> Image {
        let ds = self.draws();
        self.render_ruled(&ds, &self.rules.clone())
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
        match self.view {
            View::Title => {
                let hit = self.title_rects().iter().position(|r| inside(*r, x, y));
                match hit {
                    Some(0) => self.new_game(),
                    Some(1) => {
                        self.view = View::Slots {
                            save: false,
                            from_title: true,
                        }
                    }
                    Some(3) => self.ended = true,
                    _ => {}
                }
            }
            View::Menu => {
                let hit = self.menu_items().iter().position(|(_, r)| inside(*r, x, y));
                match hit {
                    Some(0) => {
                        self.view = View::Slots {
                            save: true,
                            from_title: false,
                        }
                    }
                    Some(1) => {
                        self.view = View::Slots {
                            save: false,
                            from_title: false,
                        }
                    }
                    Some(2) => self.back_to_title(),
                    Some(3) => self.view = View::Game,
                    _ => {}
                }
            }
            View::Slots { save, from_title } => {
                let Some(i) = self.slot_rects().iter().position(|r| inside(*r, x, y)) else {
                    return;
                };
                if i == SLOTS {
                    self.view = if from_title { View::Title } else { View::Menu };
                } else if save {
                    self.save(i);
                } else {
                    self.load_slot(i);
                }
            }
            View::Game => self.advance(Some((x, y))),
        }
    }

    fn advance(&mut self, at: Option<(i32, i32)>) {
        self.clicked = true;
        match self.wait.clone() {
            Some(Wait::Click) => {
                if self.trans.is_some() {
                    self.trans = None;
                }
                self.wait = None;
            }
            Some(Wait::Choice(opts)) => {
                let Some((x, y)) = at else {
                    return;
                };
                if let Some(i) = self
                    .choice_rects(opts.len())
                    .iter()
                    .position(|r| inside(*r, x, y))
                    && let Some(vm) = self.vm.as_mut()
                {
                    vm.g[64] = i as i32;
                    self.wait = None;
                }
            }
            _ => {}
        }
    }

    /// A right click: the menu, or back out of it.
    pub fn cancel(&mut self) {
        self.view = match self.view {
            View::Game => {
                self.skip = false;
                View::Menu
            }
            View::Menu => View::Game,
            View::Slots {
                from_title: true, ..
            } => View::Title,
            View::Slots { .. } => View::Menu,
            View::Title => View::Title,
        };
    }

    /// 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip.
    pub fn key(&mut self, k: usize) {
        match k {
            4 if self.view == View::Game => self.advance(None),
            5 => self.cancel(),
            6 if self.view == View::Game => self.skip = !self.skip,
            _ => {}
        }
    }

    pub fn wheel(&mut self, up: bool) {
        if !up && self.view == View::Game {
            self.advance(None);
        }
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    fn back_to_title(&mut self) {
        self.view = View::Title;
        self.vm = None;
        self.wait = None;
        self.trans = None;
        self.skip = false;
        self.scene = Scene::default();
        self.window = false;
        self.stop_bgm(500.0);
        self.stop_se(0.0);
        self.play("voice", None, false, 0.0);
    }

    // ---- saves ----

    fn snap(&self, vm: &Vm) -> Snap {
        let title = {
            let line: String = self
                .msg
                .lines
                .iter()
                .map(|l| nexas::sjis::decode(l))
                .collect();
            let line = if self.msg.name.is_empty() {
                line
            } else {
                format!("【{}】{line}", nexas::sjis::decode(&self.msg.name))
            };
            format!("{} {line}", nexas::sjis::decode(&self.chapter))
        };
        Snap {
            title,
            script: vm.script.name.clone(),
            pc: vm.pc,
            stack: vm.stack[..vm.sp.min(vm.stack.len())].to_vec(),
            frames: vm.frames.clone(),
            work: vm.work.clone(),
            g: vm
                .g
                .iter()
                .enumerate()
                .filter(|v| *v.1 != 0)
                .map(|(i, v)| (i, *v))
                .collect(),
            bits: vm
                .bits
                .iter()
                .enumerate()
                .filter(|v| *v.1 != 0)
                .map(|(i, v)| (i, *v))
                .collect(),
            gf: vm
                .gf
                .iter()
                .enumerate()
                .filter(|v| *v.1 != 0.0)
                .map(|(i, v)| (i, v.to_bits()))
                .collect(),
            gs: vm
                .gs
                .iter()
                .enumerate()
                .filter(|v| !v.1.is_empty())
                .map(|(i, v)| (i, v.clone()))
                .collect(),
            scene: self.scene.clone(),
            window: self.window,
            msg: self.msg.clone(),
            bgm: self.bgm.clone(),
        }
    }

    fn read_slot(&mut self, i: usize) -> Option<Snap> {
        let b = self.files.read_save(&format!("slot{i:02}"))?;
        parse_snap(&String::from_utf8_lossy(&b))
    }

    fn save(&mut self, i: usize) {
        let Some(mut s) = self.saved.clone() else {
            return;
        };
        s.title = s.title.chars().take(30).collect();
        self.files
            .write_save(&format!("slot{i:02}"), write_snap(&s).as_bytes());
        self.view = View::Game;
    }

    fn load_slot(&mut self, i: usize) {
        let Some(s) = self.read_slot(i) else {
            return;
        };
        let Some(script) = self.script(&s.script) else {
            return;
        };
        let mut vm = Box::new(Vm::new(script));
        vm.log = self.log;
        vm.pc = s.pc;
        vm.stack[..s.stack.len().min(0x40000)]
            .copy_from_slice(&s.stack[..s.stack.len().min(0x40000)]);
        vm.sp = s.stack.len();
        vm.frames = s.frames.clone();
        if !s.work.is_empty() {
            vm.work = s.work.clone();
        }
        for &(k, v) in &s.g {
            if let Some(r) = vm.g.get_mut(k) {
                *r = v;
            }
        }
        for &(k, v) in &s.bits {
            if let Some(r) = vm.bits.get_mut(k) {
                *r = v;
            }
        }
        for &(k, v) in &s.gf {
            if let Some(r) = vm.gf.get_mut(k) {
                *r = f32::from_bits(v);
            }
        }
        for (k, v) in &s.gs {
            if let Some(r) = vm.gs.get_mut(*k) {
                *r = v.clone();
            }
        }
        let from = if self.view == View::Title
            || matches!(
                self.view,
                View::Slots {
                    from_title: true,
                    ..
                }
            ) {
            None
        } else {
            let ds = self.scene_draws();
            Some(Rc::new(self.render_draws(&ds)))
        };
        self.stop_se(0.0);
        self.play("voice", None, false, 0.0);
        match s.bgm.clone() {
            Some(n) => {
                self.bgm = None;
                self.play_bgm(&n);
            }
            None => self.stop_bgm(0.0),
        }
        self.vm = Some(vm);
        self.scene = s.scene.clone();
        self.scene.settle();
        self.window = s.window;
        self.msg = s.msg.clone();
        self.wait = Some(Wait::Click);
        self.view = View::Game;
        self.skip = false;
        self.pending.clear();
        self.armed.clear();
        self.saved = Some(s);
        self.trans = from.map(|from| Trans {
            from,
            start: self.now,
            ms: 400.0,
            rule: None,
        });
    }
}

fn write_snap(s: &Snap) -> String {
    let mut o = String::new();
    let mut put = |k: &str, v: String| {
        o.push_str(k);
        o.push(' ');
        o.push_str(&v);
        o.push('\n');
    };
    put("title", s.title.replace('\n', " "));
    put("script", s.script.clone());
    put("pc", s.pc.to_string());
    put("stack", hex(&s.stack));
    put(
        "frames",
        s.frames
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" "),
    );
    put("work", hex(&s.work));
    let pairs = |v: &mut dyn Iterator<Item = String>| v.collect::<Vec<_>>().join(" ");
    put("g", pairs(&mut s.g.iter().map(|(i, v)| format!("{i}:{v}"))));
    put(
        "bits",
        pairs(&mut s.bits.iter().map(|(i, v)| format!("{i}:{v}"))),
    );
    put(
        "gf",
        pairs(&mut s.gf.iter().map(|(i, v)| format!("{i}:{v}"))),
    );
    put(
        "gs",
        pairs(&mut s.gs.iter().map(|(i, v)| format!("{i}:{}", hex(v)))),
    );
    let sc = &s.scene;
    put("bg", sc.bg.clone().unwrap_or("-".into()));
    put("bgscale", sc.bg_scale.to_string());
    put("overlay", sc.overlay.clone().unwrap_or("-".into()));
    put(
        "cam",
        sc.cam
            .iter()
            .map(|t| t.to.to_string())
            .collect::<Vec<_>>()
            .join(" "),
    );
    put(
        "fade",
        format!(
            "{} {} {} {}",
            sc.fade.to, sc.fade_rgb[0], sc.fade_rgb[1], sc.fade_rgb[2]
        ),
    );
    for (i, c) in sc.chars.iter().enumerate() {
        if let Some(c) = c
            && !c.leaving
        {
            put(
                "char",
                format!(
                    "{i} {} {} {} {} {} {} {}",
                    c.id, c.pose, c.kind, c.pos[0].to, c.pos[1].to, c.pos[2].to, c.file
                ),
            );
        }
    }
    put("window", (s.window as u8).to_string());
    put("name", hex(&s.msg.name));
    put("lines", pairs(&mut s.msg.lines.iter().map(|l| hex(l))));
    put("bgm", s.bgm.clone().unwrap_or("-".into()));
    o
}

fn parse_snap(t: &str) -> Option<Snap> {
    let mut s = Snap {
        title: String::new(),
        script: String::new(),
        pc: 0,
        stack: Vec::new(),
        frames: Vec::new(),
        work: Vec::new(),
        g: Vec::new(),
        bits: Vec::new(),
        gf: Vec::new(),
        gs: Vec::new(),
        scene: Scene::default(),
        window: false,
        msg: Msg::default(),
        bgm: None,
    };
    let opt = |v: &str| (v != "-" && !v.is_empty()).then(|| v.to_string());
    for line in t.lines() {
        let (k, v) = line.split_once(' ').unwrap_or((line, ""));
        let pairs = || {
            v.split_whitespace()
                .filter_map(|p| p.split_once(':'))
                .collect::<Vec<_>>()
        };
        match k {
            "title" => s.title = v.into(),
            "script" => s.script = v.into(),
            "pc" => s.pc = v.parse().ok()?,
            "stack" => s.stack = unhex(v),
            "frames" => {
                s.frames = v
                    .split_whitespace()
                    .filter_map(|x| x.parse().ok())
                    .collect()
            }
            "work" => s.work = unhex(v),
            "g" => {
                s.g = pairs()
                    .iter()
                    .filter_map(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
                    .collect()
            }
            "bits" => {
                s.bits = pairs()
                    .iter()
                    .filter_map(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
                    .collect()
            }
            "gf" => {
                s.gf = pairs()
                    .iter()
                    .filter_map(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
                    .collect()
            }
            "gs" => {
                s.gs = pairs()
                    .iter()
                    .filter_map(|(a, b)| Some((a.parse().ok()?, unhex(b))))
                    .collect()
            }
            "bg" => s.scene.bg = opt(v),
            "bgscale" => s.scene.bg_scale = v.parse().unwrap_or(70.0),
            "overlay" => s.scene.overlay = opt(v),
            "cam" => {
                for (t, x) in s.scene.cam.iter_mut().zip(v.split_whitespace()) {
                    *t = Tween::new(x.parse().unwrap_or(0.0));
                }
            }
            "fade" => {
                let x: Vec<f32> = v
                    .split_whitespace()
                    .filter_map(|x| x.parse().ok())
                    .collect();
                if x.len() == 4 {
                    s.scene.fade = Tween::new(x[0]);
                    s.scene.fade_rgb = [x[1] as u8, x[2] as u8, x[3] as u8];
                }
            }
            "char" => {
                let p: Vec<&str> = v.splitn(8, ' ').collect();
                if p.len() == 8 {
                    let n = |i: usize| p[i].parse::<f32>().unwrap_or(0.0);
                    let slot: usize = p[0].parse().ok()?;
                    if slot < CHARS {
                        s.scene.chars[slot] = Some(Chara {
                            id: p[1].parse().ok()?,
                            pose: p[2].parse().ok()?,
                            kind: p[3].parse().ok()?,
                            pos: [Tween::new(n(4)), Tween::new(n(5)), Tween::new(n(6))],
                            file: p[7].into(),
                            alpha: Tween::new(1.0),
                            leaving: false,
                        });
                    }
                }
            }
            "window" => s.window = v == "1",
            "name" => s.msg.name = unhex(v),
            "lines" => s.msg.lines = v.split_whitespace().map(unhex).collect(),
            "bgm" => s.bgm = opt(v),
            _ => {}
        }
    }
    (!s.script.is_empty()).then_some(s)
}

impl Host for Engine {
    fn command(&mut self, vm: &mut Vm, op: u16) -> Done {
        let quiet = self.depth > 0 || self.capture.is_some();
        vm.sys[0] = 0;
        match op {
            0 => {
                if quiet {
                    Done::Next(0)
                } else {
                    Done::Yield(0)
                }
            }
            // load a picture into a plane: name, plane (and a third number)
            0x30 => {
                let a = vm.args(3);
                let (name, plane, n) = {
                    let t = vm.text(a[1]);
                    let lower = String::from_utf8_lossy(&t).to_ascii_lowercase();
                    if lower.ends_with(".pb3") || lower.ends_with(".cmv") {
                        (lower, a[2], 8)
                    } else {
                        let t = vm.text(a[0]);
                        (String::from_utf8_lossy(&t).to_ascii_lowercase(), a[1], 12)
                    }
                };
                if quiet {
                    self.got.push((name, plane));
                }
                Done::Next(n)
            }
            // load with a crossfade: ?, ms, name, plane
            0x2c4 => {
                let a = vm.args(4);
                if quiet {
                    let t = vm.text(a[2]);
                    self.got
                        .push((String::from_utf8_lossy(&t).to_ascii_lowercase(), a[3]));
                }
                Done::Next(16)
            }
            0x88 => {
                let a = vm.args(5);
                self.entries.insert(a[4], a[3] as usize);
                Done::Next(20)
            }
            0x8a => {
                let n = vm.args(1)[0];
                self.run_entry(vm, n);
                Done::Next(4)
            }
            // a sound effect: ?, ?, ?, name, ?
            0xb0 => {
                let a = vm.args(5);
                if quiet {
                    self.got_sounds.push((vm.text(a[3]), a.clone()));
                }
                Done::Next(20)
            }
            0x80 if !quiet => {
                let n = vm.args(1)[0];
                vm.goto = Some(String::from_utf8_lossy(&vm.text(n)).into_owned());
                Done::Next(4)
            }
            // has the sound stopped: yes
            0xb7 if quiet => {
                vm.sys[0] = -1;
                Done::Next(4)
            }
            // is the plane in use: say yes, so that changes of a picture go through
            0x27 if quiet => {
                vm.sys[0] = 1;
                Done::Next(8)
            }
            0x1a0 => {
                vm.sys[0] = self.clicked as i32;
                Done::Next(0)
            }
            0x1a1 => {
                self.clicked = false;
                Done::Next(0)
            }
            _ => {
                let n = POPS
                    .binary_search_by_key(&op, |p| p.0)
                    .map_or(0, |i| POPS[i].1) as usize;
                // never below the frame: a guess too many would eat the return address
                let room = vm.sp.saturating_sub(vm.frames.last().copied().unwrap_or(0));
                Done::Next(n.min(room) as u32)
            }
        }
    }

    fn call(&mut self, vm: &mut Vm, label: usize) -> Option<Done> {
        if self.depth > 0 || self.capture.is_some() {
            return None;
        }
        let s = vm.script.clone();
        let id = lib_id(&s, label)?;
        let bytes = s.arg_bytes.get(label).copied().unwrap_or(0) as usize;
        let args = vm.args(bytes / 4);
        if self.log {
            eprintln!("{} {:06x} L{id} {:?}", s.name, vm.pc, args);
        }
        if CAPTURED.contains(&id) {
            self.got.clear();
            self.got_sounds.clear();
            self.capture = Some(Capture {
                id,
                depth: vm.frames.len(),
                steps: 0,
                pc_after: vm.pc + 4,
                sp_after: vm.sp.saturating_sub(bytes),
                args,
            });
            return None;
        }
        self.take_over(vm, id, &args);
        Some(if self.wait.is_some() {
            Done::Yield(bytes as u32)
        } else {
            Done::Next(bytes as u32)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_cleaning() {
        let t = nexas::sjis::encode("幼なじみの{羽/は}{鳥/とり}。");
        assert_eq!(nexas::sjis::decode(&clean(&t)[0]), "幼なじみの羽鳥。");
        let t = nexas::sjis::encode("「わっ」\\n\\t「ひゃっ」");
        let l = clean(&t);
        assert_eq!(l.len(), 2);
        assert_eq!(nexas::sjis::decode(&l[1]), "「ひゃっ」");
    }

    #[test]
    fn snap_round_trip() {
        let mut s = parse_snap("script snky01.ps2\npc 12\n").unwrap();
        s.g = vec![(64, 1), (160, -2)];
        s.gs = vec![(11, b"st.pb3".to_vec())];
        s.stack = vec![1, 2, 3, 4];
        s.scene.bg = Some("bg080a.pb3".into());
        let t = write_snap(&s);
        let r = parse_snap(&t).unwrap();
        assert_eq!(r.pc, 12);
        assert_eq!(r.g, s.g);
        assert_eq!(r.gs, s.gs);
        assert_eq!(r.stack, s.stack);
        assert_eq!(r.scene.bg, s.scene.bg);
    }
}
