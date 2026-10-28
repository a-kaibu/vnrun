//! Propeller's script engine, as read from BB.exe (Bullet Butlers web trial, 2007).
//!
//! # Machine
//!
//! Scripts run in threads (BB.exe keeps a context of 0x2048 bytes for each): the script of
//! the current mode (0 opening, 1 title, set by 1.01 and switched by 0.07), the scenario
//! (loaded by 0.08, run in mode 2), eight threads started by 0.20 for effects, the game
//! screen's own script (mode 2, run to its end once) and short runs for sprite events.
//! A thread runs until it waits (0.05, 0.17, 3.00, 5.00) or stops (0.06, or 0.04 with an
//! empty call stack). Variables: 1024 game variables (value kind 1, `g`) and 1024 system
//! variables (kind 2, `s`); numbers count from 1.
//!
//! # Commands (category.command operands)
//!
//! ```text
//! 0.00 B kind(1 g, 2 s) W var B op(0 =, 1 +=, 2 -=) V      0.01 V a B cmp(== != < >) V b
//!      B act(0 goto, 1 gosub, 2 return) V label             0.02 V goto   0.03 V gosub
//! 0.04 return (stops with an empty stack)   0.05 V ms B clickable: wait   0.06 stop
//! 0.07 B mode (0 opening, 1 title, 2 game, 3 dialog, 4 log, 5 save, 6 load)
//! 0.08 S load the scenario script        0.0a B open system dialog (1.0c labels)
//! 0.0c hide the message window   0.0d auto on   0.0e skip on   0.13 quick save
//! 0.16 B ?   0.17 V sprite: wait for its frame animation
//! 0.20 V thread(1..9) V global label (0 stops it) B ?      0.30 / 0.32 jump / autosave
//! 1.00 S caption   1.01 B mode S script   1.02 V base sprite (value kind 4)
//! 1.03 S S S character   1.05 / 1.06 / 1.07 V V sprite ranges of the message window,
//! dialog and log planes   1.08 V idx V r g b V r g b: text colour and shadow
//! 1.09 S S S B name brackets and separator   1.0a V V line width (full-width characters)
//! and ?   1.0c B V dialog labels   1.0d V S S scene title   1.17 S S archive prefix, file
//! 2.00 V id S picture (empty: remove)   2.01 V id V x V y   2.02 V id B shown
//! 2.03 V id V V V V the part shown (l t r b), drawn at the sprite's place   2.04 V id V degrees   2.05 V id V V scale (%)
//! 2.06 V id V curve   2.07 V id V ease (%: + speeds up, - slows down)
//! 2.08 V id V x V y V ms: move   2.09 V id V V V V V ms: rectangle to
//! 2.0a V id V deg V ms   2.0b V id V V V ms: scale to   2.0c V id V from V to V ms B loop:
//! frame animation   2.0d V id V frame (from 1)   2.0e V id V alpha (%)   2.0f V id V tone
//! 2.10 V id V alpha V ms   2.11 V id V tone V ms   2.12 V id V x V y V size V colour S text
//! 2.13 V id V parent   2.15 V id V V V wave   3.00 V kind S rule V ms B keep window:
//! transition   3.01 V kind V ms B   3.02 V screen effect   3.03 S B movie
//! 4.00 V id B event (0 leave, 1 enter, 2 click, 3 locked) V label   4.01 V id B lock
//! 4.02 V id B role (2 auto, 3 skip, 4 log, 5 voice, 9 save)
//! 5.00 V number S text   5.01 V window V x V y V size V colours x4   5.02 V plate V frame
//! V x V y V size V colours x2   5.04 V label V label: run when text goes / waits
//! 6.00 S music B loop V fade-in ms   6.01 V fade-out ms   6.02 S voice   6.03 no voice
//! 6.04 V channel S sound   6.05 V channel B loop   6.06 V channel: stop   6.07 voice again
//! ```
//!
//! Text: `【name】/【real name】` starts a line spoken, `_r` breaks lines, `<base,ruby>`.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::mgr;
use crate::mpk::Mpk;
use crate::msc::{Arg, Ins, Script, Val};

const FRAME_MS: f64 = 1000.0 / 60.0;
/// The screen size of BB.exe (2007); sukima_trial2.exe (2011, the wide script layout) has
/// 1024x576. Both are constants in the exe.
const SCREEN_V1: (u32, u32) = (800, 600);
const SCREEN_V2: (u32, u32) = (1024, 576);
const SPRITES: usize = 257;
const THREADS: usize = 9;
const SLOTS: usize = 10;
const STEPS: usize = 20_000;

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Key {
    Init,
    Mode,
    Scen,
    Sub(usize),
    Ui,
    Dialog,
    Event(usize, usize),
    Msg,
}

#[derive(Clone, Debug, PartialEq)]
enum Wait {
    None,
    Frames(u32, bool),
    Sprite(usize),
    Text,
}

#[derive(Clone)]
struct Thread {
    script: Rc<Script>,
    pc: usize,
    stack: Vec<usize>,
    running: bool,
    wait: Wait,
}

impl Thread {
    fn new(script: Rc<Script>, pc: usize) -> Thread {
        Thread {
            script,
            pc,
            stack: Vec::new(),
            running: true,
            wait: Wait::None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Anim {
    from: [f64; 4],
    to: [f64; 4],
    k: u32,
    n: u32,
}

#[derive(Clone, Debug, Default)]
struct FrameAnim {
    from: i32,
    to: i32,
    every: u32,
    k: u32,
    repeat: bool,
}

#[derive(Clone, Debug)]
struct Sprite {
    name: String,
    frame: i32,
    x: f64,
    y: f64,
    shown: bool,
    rect: Option<[f64; 4]>,
    rot: f64,
    sx: f64,
    sy: f64,
    alpha: f64,
    tone: f64,
    ease: f64,
    parent: usize,
    text: Option<(i32, i32, i32, usize, Vec<u8>)>,
    moving: Option<Anim>,
    clipping: Option<Anim>,
    turning: Option<Anim>,
    scaling: Option<Anim>,
    fading: Option<Anim>,
    toning: Option<Anim>,
    frames: Option<FrameAnim>,
}

impl Default for Sprite {
    fn default() -> Sprite {
        Sprite {
            name: String::new(),
            frame: 1,
            x: 0.0,
            y: 0.0,
            shown: true,
            rect: None,
            rot: 0.0,
            sx: 1.0,
            sy: 1.0,
            alpha: 1.0,
            tone: 0.0,
            ease: 0.0,
            parent: 0,
            text: None,
            moving: None,
            clipping: None,
            turning: None,
            scaling: None,
            fading: None,
            toning: None,
            frames: None,
        }
    }
}

/// What a sprite does as the pointer comes, goes or clicks.
#[derive(Clone, Default)]
struct Events {
    /// leave, enter, (click: `click`), locked: a script and where to start
    run: [Option<(Rc<Script>, usize)>; 4],
    click: Option<(Key, usize)>,
    /// 0 new, 1 out, 2 over, 4 locked
    state: u8,
    lock: bool,
}

#[derive(Clone, Default)]
struct MsgConf {
    window: usize,
    x: i32,
    y: i32,
    size: i32,
    colour: usize,
    plate: (usize, usize),
    name_x: i32,
    name_y: i32,
    name_size: i32,
    name_colour: usize,
    /// run when the text goes away / starts to wait
    gone: Option<(Rc<Script>, usize)>,
    waiting: Option<(Rc<Script>, usize)>,
}

struct Trans {
    from: Rc<Image>,
    start: f64,
    ms: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Game,
    Menu,
    Slots { save: bool },
}

/// Where a save starts again: the scenario at a text.
#[derive(Clone)]
struct Snapshot {
    script: String,
    pc: usize,
    stack: Vec<usize>,
    vars: Vec<(usize, i32)>,
    sprites: Vec<(usize, Sprite)>,
    subs: Vec<(usize, String, usize, Vec<usize>)>,
    bgm: Option<(String, bool)>,
    se: BTreeMap<usize, String>,
    title: String,
}

pub struct Engine {
    /// the screen size
    pub width: u32,
    pub height: u32,
    /// the 2011 script layout (wide fields), told by which way system.msc decodes
    v2: bool,
    files: Box<dyn Files>,
    pub log: bool,
    listing: Option<Vec<String>>,
    /// archive prefix (lower case) to the file in the game folder
    archives: HashMap<String, String>,
    mpks: HashMap<String, Option<Rc<Mpk>>>,
    scripts: HashMap<String, Rc<Script>>,
    globals: HashMap<u32, (Rc<Script>, usize)>,
    pictures: HashMap<String, Rc<Vec<Rc<Image>>>>,
    mode_scripts: [String; 8],
    mode: u8,
    base: i32,
    gvars: Vec<i32>,
    svars: Vec<i32>,
    threads: BTreeMap<Key, Thread>,
    /// the running thread, out of `threads` while it runs
    cur: Option<(Key, Thread)>,
    sprites: Vec<Sprite>,
    events: Vec<Events>,
    roles: [usize; 10],
    planes: [(usize, usize); 3],
    colours: Vec<[u8; 6]>,
    brackets: (Vec<u8>, Vec<u8>, Vec<u8>),
    line_chars: usize,
    dialogs: HashMap<u8, usize>,
    msg: MsgConf,
    window: bool,
    text: Vec<Vec<u8>>,
    name: Option<Vec<u8>>,
    voice: Option<String>,
    last_voice: Option<String>,
    trans: Option<Trans>,
    last_draws: Vec<Draw>,
    sounds: Vec<SoundEvent>,
    bgm: Option<(String, bool)>,
    se_names: BTreeMap<usize, String>,
    se_loops: BTreeMap<usize, String>,
    auto: bool,
    skip: bool,
    auto_at: f64,
    clicked: bool,
    now: f64,
    rand: u64,
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<(Vec<u8>, [u8; 3], i32), Rc<Image>>,
    pointer: (i32, i32),
    view: View,
    title_load: bool,
    scene_title: String,
    saved_at: Option<Snapshot>,
    ended: bool,
}

fn draw(image: &Rc<Image>, x: f32, y: f32, alpha: f32) -> Draw {
    let (w, h) = (image.w as f32, image.h as f32);
    Draw {
        image: image.clone(),
        src: [0, 0, image.w as i32, image.h as i32],
        dst: [x, y, x + w, y + h],
        place: Place {
            x,
            y,
            sx: 1.0,
            sy: 1.0,
            cx: 0.0,
            cy: 0.0,
            rot: 0.0,
            alpha,
            add: false,
            flip: (false, false),
            clip: [0.0, 0.0, 1e6, 1e6],
        },
    }
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
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

/// Frames of `ms` milliseconds, as BB.exe counts them (1000 / 60 rounded down).
fn frames(ms: i32) -> u32 {
    (ms.max(0) / 16) as u32
}

fn eased(t: f64, e: f64) -> f64 {
    if e > 0.0 {
        t.powf(e + 1.0)
    } else if e < 0.0 {
        1.0 - (1.0 - t).powf(1.0 - e)
    } else {
        t
    }
}

/// Splits Shift_JIS text into characters.
fn chars(s: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        let n = if (0x81..=0x9f).contains(&c) || (0xe0..=0xfc).contains(&c) {
            2
        } else {
            1
        };
        out.push(&s[i..(i + n).min(s.len())]);
        i += n;
    }
    out
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    // on character boundaries only
    let mut i = 0;
    while i + n.len() <= h.len() {
        if &h[i..i + n.len()] == n {
            return Some(i);
        }
        let c = h[i];
        i += if (0x81..=0x9f).contains(&c) || (0xe0..=0xfc).contains(&c) {
            2
        } else {
            1
        };
    }
    None
}

impl Engine {
    pub fn open(files: Box<dyn Files>) -> Result<Engine, String> {
        let mut e = Engine {
            width: SCREEN_V1.0,
            height: SCREEN_V1.1,
            v2: false,
            files,
            log: false,
            listing: None,
            archives: HashMap::new(),
            mpks: HashMap::new(),
            scripts: HashMap::new(),
            globals: HashMap::new(),
            pictures: HashMap::new(),
            mode_scripts: Default::default(),
            mode: 0,
            base: 1,
            gvars: vec![0; 1025],
            svars: vec![0; 1025],
            threads: BTreeMap::new(),
            cur: None,
            sprites: vec![Sprite::default(); SPRITES],
            events: vec![Events::default(); SPRITES],
            roles: [0; 10],
            planes: [(0, 0); 3],
            colours: vec![[255, 255, 255, 0, 0, 0]; 33],
            brackets: ("【".into(), "】".into(), "/".into()),
            line_chars: 27,
            dialogs: HashMap::new(),
            msg: MsgConf {
                window: 0,
                x: 26,
                y: 435,
                size: 25,
                colour: 1,
                name_size: 25,
                name_colour: 1,
                ..Default::default()
            },
            window: false,
            text: Vec::new(),
            name: None,
            voice: None,
            last_voice: None,
            trans: None,
            last_draws: Vec::new(),
            sounds: Vec::new(),
            bgm: None,
            se_names: BTreeMap::new(),
            se_loops: BTreeMap::new(),
            auto: false,
            skip: false,
            auto_at: 0.0,
            clicked: false,
            now: 0.0,
            rand: 0x2545_f491_4f6c_dd1d,
            font: None,
            text_cache: HashMap::new(),
            pointer: (0, 0),
            view: View::Game,
            title_load: false,
            scene_title: String::new(),
            saved_at: None,
            ended: false,
        };
        e.brackets = (
            nexas::sjis::encode("【"),
            nexas::sjis::encode("】"),
            b"/".to_vec(),
        );
        // BB.exe starts System\\system.msc from System.mpk, sukima_trial2.exe
        // Scenario\\system.msc from Scenario.mpk.
        let mut start = None;
        for (prefix, file) in [("system", "system.mpk"), ("scenario", "scenario.mpk")] {
            let Some(f) = e.find_file(file) else {
                continue;
            };
            e.archives.insert(prefix.into(), f);
            let path = format!("{prefix}\\system.msc");
            let Some(b) = e.read(&path) else {
                continue;
            };
            for v2 in [false, true] {
                if Script::parse(&path, &b, v2).is_some_and(|s| s.decodes()) {
                    start = Some((path.clone(), v2));
                    break;
                }
            }
            if start.is_some() {
                break;
            }
        }
        let (path, v2) = start.ok_or("no System\\system.msc or Scenario\\system.msc")?;
        e.v2 = v2;
        (e.width, e.height) = if v2 { SCREEN_V2 } else { SCREEN_V1 };
        let s = e.script(&path).ok_or("system.msc")?;
        e.run_now(Key::Init, Thread::new(s, 0));
        if let Some(d) = e.script(&e.mode_scripts[3].clone()) {
            e.run_now(Key::Dialog, Thread::new(d, 0));
        }
        let m = e.mode_scripts[7].clone();
        if !m.is_empty() {
            e.script(&m);
        }
        if !e.threads.contains_key(&Key::Mode) {
            e.switch_mode(0);
        }
        Ok(e)
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        self.font = Some(Rc::new(
            fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
                .map_err(|e| e.to_string())?,
        ));
        self.text_cache.clear();
        Ok(())
    }

    fn trace(&self, s: &str) {
        if self.log {
            eprintln!("propeller: {s}");
        }
    }

    // ---- files ----

    fn find_file(&mut self, name: &str) -> Option<String> {
        if self.listing.is_none() {
            self.listing = Some(self.files.list());
        }
        self.listing
            .as_ref()?
            .iter()
            .find(|f| f.eq_ignore_ascii_case(name))
            .cloned()
    }

    fn mpk(&mut self, file: &str) -> Option<Rc<Mpk>> {
        let key = file.to_ascii_lowercase();
        if let Some(m) = self.mpks.get(&key) {
            return m.clone();
        }
        let m = self
            .find_file(file)
            .and_then(|p| Mpk::open(self.files.as_mut(), &p).map(Rc::new));
        self.mpks.insert(key, m.clone());
        m
    }

    /// `prefix\name` from its archive (patch.mpk first).
    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let path = path.replace('/', "\\");
        let (prefix, name) = path.rsplit_once('\\').unwrap_or(("", &path));
        let name = name.to_ascii_lowercase();
        if let Some(p) = self.mpk("patch.mpk")
            && let Some(b) = p.read(self.files.as_mut(), &name)
        {
            return Some(b);
        }
        let file = self
            .archives
            .get(&prefix.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| format!("{prefix}.mpk"));
        let m = self.mpk(&file)?;
        m.read(self.files.as_mut(), &name)
    }

    fn script(&mut self, path: &str) -> Option<Rc<Script>> {
        let key = path.to_ascii_lowercase();
        if let Some(s) = self.scripts.get(&key) {
            let s = s.clone();
            self.register(&s);
            return Some(s);
        }
        let b = self.read(path)?;
        let s = Rc::new(Script::parse(path, &b, self.v2)?);
        self.scripts.insert(key, s.clone());
        self.register(&s);
        Some(s)
    }

    fn register(&mut self, s: &Rc<Script>) {
        for (&id, &off) in &s.globals {
            self.globals.insert(id, (s.clone(), off));
        }
    }

    fn picture(&mut self, name: &str) -> Option<Rc<Vec<Rc<Image>>>> {
        let key = name.to_ascii_lowercase();
        if let Some(p) = self.pictures.get(&key) {
            return Some(p.clone());
        }
        let b = self.read(name)?;
        let frames: Vec<Rc<Image>> = mgr::frames(&b)
            .iter()
            .filter_map(|f| mgr::bmp(f).map(Rc::new))
            .collect();
        if frames.is_empty() {
            return None;
        }
        if self.pictures.len() > 96 {
            self.pictures.clear();
        }
        let p = Rc::new(frames);
        self.pictures.insert(key, p.clone());
        Some(p)
    }

    // ---- values ----

    fn random(&mut self, n: i32) -> i32 {
        self.rand ^= self.rand << 13;
        self.rand ^= self.rand >> 7;
        self.rand ^= self.rand << 17;
        if n <= 0 {
            0
        } else {
            (self.rand % n as u64) as i32
        }
    }

    fn val(&mut self, v: &Val) -> i32 {
        match *v {
            Val::Num(n) => n,
            Val::Var(n) => self
                .gvars
                .get((n - 1).max(0) as usize)
                .copied()
                .unwrap_or(0),
            Val::Sys(n) => self
                .svars
                .get((n - 1).max(0) as usize)
                .copied()
                .unwrap_or(0),
            Val::Rand(n) => self.random(n),
            Val::Base => self.base,
            Val::Special(7) => self.pointer.0,
            Val::Special(8) => self.pointer.1,
            Val::Special(_) => 0,
        }
    }

    // ---- threads ----

    fn thread_mut(&mut self, k: Key) -> Option<&mut Thread> {
        if let Some((ck, t)) = self.cur.as_mut()
            && *ck == k
        {
            return Some(t);
        }
        self.threads.get_mut(&k)
    }

    fn set_thread(&mut self, k: Key, t: Thread) {
        if let Some((ck, cur)) = self.cur.as_mut()
            && *ck == k
        {
            *cur = t;
            return;
        }
        self.threads.insert(k, t);
    }

    fn stop_thread(&mut self, k: Key) {
        if let Some(t) = self.thread_mut(k) {
            t.running = false;
        }
    }

    /// Runs a thread until it stops (for the scripts run once at the start).
    fn run_now(&mut self, k: Key, t: Thread) {
        self.threads.insert(k, t);
        for _ in 0..1000 {
            self.run_thread(k);
            let Some(t) = self.threads.get_mut(&k) else {
                break;
            };
            if !t.running {
                break;
            }
            // waits do not count here
            t.wait = Wait::None;
        }
    }

    fn run_thread(&mut self, k: Key) {
        let Some(t) = self.threads.remove(&k) else {
            return;
        };
        let prev = self.cur.replace((k, t));
        for _ in 0..STEPS {
            let Some((_, t)) = self.cur.as_ref() else {
                break;
            };
            if !t.running || t.wait != Wait::None {
                break;
            }
            let Some(ins) = t.script.decode(t.pc) else {
                let n = t.script.name.clone();
                self.trace(&format!("{n}: bad code at {:x}", t.pc));
                if let Some((_, t)) = self.cur.as_mut() {
                    t.running = false;
                }
                break;
            };
            let at = t.pc;
            if let Some((_, t)) = self.cur.as_mut() {
                t.pc = ins.next;
            }
            if self.log {
                let n = self.cur.as_ref().map(|c| c.1.script.name.clone());
                self.trace(&format!(
                    "{k:?} {} {at:05x} {}.{:02x} {:?}",
                    n.unwrap_or_default(),
                    ins.cat,
                    ins.op,
                    ins.args
                ));
            }
            self.exec(k, at, &ins);
            if self.cur.as_ref().is_none_or(|c| c.0 != k) {
                break;
            }
        }
        if let Some((ck, t)) = self.cur.take()
            && ck == k
        {
            self.threads.entry(k).or_insert(t);
        }
        self.cur = prev;
    }

    fn cur_thread(&mut self) -> Option<&mut Thread> {
        self.cur.as_mut().map(|c| &mut c.1)
    }

    fn wait(&mut self, w: Wait) {
        if let Some(t) = self.cur_thread() {
            t.wait = w;
        }
    }

    fn jump_local(&mut self, label: i32, call: bool) {
        let Some(t) = self.cur_thread() else {
            return;
        };
        match u32::try_from(label)
            .ok()
            .and_then(|l| t.script.labels.get(&l))
        {
            Some(&off) => {
                if call {
                    if t.stack.len() >= 8 {
                        t.running = false;
                        return;
                    }
                    t.stack.push(t.pc);
                }
                t.pc = off;
            }
            None => t.running = false,
        }
    }

    fn ret(&mut self) {
        if let Some(t) = self.cur_thread() {
            match t.stack.pop() {
                Some(pc) => t.pc = pc,
                None => t.running = false,
            }
        }
    }

    fn switch_mode(&mut self, m: u8) {
        self.trace(&format!("mode {m}"));
        match m {
            0 | 1 => {
                self.clear_scene();
                self.threads.remove(&Key::Scen);
                if self.cur.as_ref().is_some_and(|c| c.0 == Key::Scen) {
                    self.cur = None;
                }
                self.music_stop(0);
                self.mode = m;
                let name = self.mode_scripts[m as usize].clone();
                if let Some(s) = self.script(&name) {
                    self.set_thread(Key::Mode, Thread::new(s, 0));
                }
            }
            2 => {
                if self.mode != 2 {
                    self.clear_scene();
                    self.mode = 2;
                    self.stop_thread(Key::Mode);
                    let name = self.mode_scripts[2].clone();
                    if let Some(s) = self.script(&name) {
                        let saved = self.cur.take();
                        self.run_now(Key::Ui, Thread::new(s, 0));
                        self.cur = saved;
                    }
                }
            }
            5 => self.view = View::Slots { save: true },
            6 => {
                self.view = View::Slots { save: false };
                self.title_load = self.mode != 2;
            }
            _ => {}
        }
    }

    fn clear_scene(&mut self) {
        for s in self.sprites.iter_mut() {
            *s = Sprite::default();
        }
        for e in self.events.iter_mut() {
            *e = Events::default();
        }
        for i in 0..THREADS {
            self.threads.remove(&Key::Sub(i));
        }
        self.threads
            .retain(|k, _| !matches!(k, Key::Event(..) | Key::Msg));
        self.window = false;
        self.text.clear();
        self.name = None;
        self.auto = false;
        self.skip = false;
    }

    // ---- commands ----

    fn exec(&mut self, key: Key, at: usize, ins: &Ins) {
        let a = &ins.args;
        let v = |e: &mut Engine, i: usize| match a.get(i) {
            Some(Arg::V(v)) => e.val(v),
            _ => 0,
        };
        let b = |i: usize| match a.get(i) {
            Some(Arg::B(b)) => *b,
            _ => 0,
        };
        let s = |i: usize| match a.get(i) {
            Some(Arg::S(s)) => s.clone(),
            _ => Vec::new(),
        };
        let st = |i: usize| encoding_rs::SHIFT_JIS.decode(&s(i)).0.into_owned();
        match (ins.cat, ins.op) {
            (0, 0x00) => {
                let idx = match a.get(1) {
                    Some(Arg::W(w)) => *w - 1,
                    _ => -1,
                };
                let x = v(self, 3);
                let vars = if b(0) == 2 {
                    &mut self.svars
                } else {
                    &mut self.gvars
                };
                if let Some(r) = usize::try_from(idx).ok().and_then(|i| vars.get_mut(i)) {
                    match b(2) {
                        0 => *r = x,
                        1 => *r = r.wrapping_add(x),
                        2 => *r = r.wrapping_sub(x),
                        _ => {}
                    }
                }
            }
            (0, 0x01) => {
                let x = v(self, 0);
                let y = v(self, 2);
                let target = v(self, 4);
                let ok = match b(1) {
                    0 => x == y,
                    1 => x != y,
                    2 => x < y,
                    3 => x > y,
                    _ => false,
                };
                if ok {
                    match b(3) {
                        0 => self.jump_local(target, false),
                        1 => self.jump_local(target, true),
                        2 => self.ret(),
                        _ => {}
                    }
                }
            }
            (0, 0x02) => {
                let l = v(self, 0);
                self.jump_local(l, false)
            }
            (0, 0x03) => {
                let l = v(self, 0);
                self.jump_local(l, true)
            }
            (0, 0x04) => self.ret(),
            (0, 0x05) => {
                let n = if self.skip { 0 } else { frames(v(self, 0)) };
                if n > 0 {
                    self.wait(Wait::Frames(n, b(1) != 0));
                }
            }
            (0, 0x06) => {
                if let Some(t) = self.cur_thread() {
                    t.running = false;
                }
            }
            (0, 0x07) => {
                let m = b(0);
                if m == 4 {
                    return;
                }
                self.switch_mode(m);
            }
            (0, 0x08) => {
                let name = st(0);
                if let Some(sc) = self.script(&name) {
                    self.set_thread(Key::Scen, Thread::new(sc, 0));
                } else {
                    self.trace(&format!("no script {name}"));
                }
            }
            (0, 0x0a) => {
                if self.mode == 2 {
                    self.view = if b(0) == 4 {
                        View::Slots { save: false }
                    } else {
                        View::Menu
                    };
                }
            }
            (0, 0x0c) => self.window = false,
            (0, 0x0d) => {
                self.auto = true;
                self.auto_at = self.now + 1500.0;
            }
            (0, 0x0e) => self.skip = true,
            (0, 0x13) | (0, 0x14) => {
                if self.mode == 2 && self.saved_at.is_some() {
                    self.view = View::Slots { save: true };
                }
            }
            (0, 0x17) => {
                let id = v(self, 0) as usize;
                if self.sprites.get(id).is_some_and(|s| s.frames.is_some()) && !self.skip {
                    self.wait(Wait::Sprite(id));
                } else if let Some(s) = self.sprites.get_mut(id)
                    && let Some(f) = s.frames.take()
                {
                    s.frame = f.to;
                }
            }
            (0, 0x20) => {
                let slot = v(self, 0) - 1;
                let label = v(self, 1);
                if !(0..THREADS as i32).contains(&slot) {
                    return;
                }
                let k = Key::Sub(slot as usize);
                match u32::try_from(label)
                    .ok()
                    .and_then(|l| self.globals.get(&l))
                    .cloned()
                {
                    Some((sc, off)) if label > 0 => self.set_thread(k, Thread::new(sc, off)),
                    _ => {
                        self.threads.remove(&k);
                        if self.cur.as_ref().is_some_and(|c| c.0 == k) {
                            self.cur = None;
                        }
                    }
                }
            }
            (1, 0x01) => {
                let k = b(0) as usize;
                if k < 8 {
                    self.mode_scripts[k] = st(1);
                }
            }
            (1, 0x02) => self.base = v(self, 0),
            (1, 0x05..=0x07) => {
                let (x, y) = (v(self, 0).max(0) as usize, v(self, 1).max(0) as usize);
                self.planes[(ins.op - 5) as usize] = (x.min(y), x.max(y));
            }
            (1, 0x08) => {
                let i = v(self, 0) as usize;
                let c: Vec<u8> = (1..7).map(|k| v(self, k).clamp(0, 255) as u8).collect();
                if let Some(slot) = self.colours.get_mut(i) {
                    slot.copy_from_slice(&c);
                }
            }
            (1, 0x09) => self.brackets = (s(0), s(1), s(2)),
            (1, 0x0a) => self.line_chars = v(self, 0).clamp(4, 60) as usize,
            (1, 0x0c) => {
                let l = v(self, 1);
                self.dialogs.insert(b(0), l.max(0) as usize);
            }
            (1, 0x0d) => self.scene_title = st(1),
            (1, 0x17) => {
                let file = st(1);
                self.archives.insert(st(0).to_ascii_lowercase(), file);
            }
            (2, _) => self.sprite_command(key, ins),
            (3, 0x00) | (3, 0x01) => {
                let (ms, keep) = if ins.op == 0 {
                    (v(self, 2), b(3))
                } else {
                    (v(self, 1), b(2))
                };
                self.transition(ms, keep != 0);
            }
            (4, 0x00) => {
                let id = v(self, 0) as usize;
                let ev = b(1) as usize;
                let label = v(self, 2);
                let Some((k, t)) = self.cur.as_ref() else {
                    return;
                };
                let Some(&off) = u32::try_from(label)
                    .ok()
                    .and_then(|l| t.script.labels.get(&l))
                else {
                    return;
                };
                let (k, sc) = (*k, t.script.clone());
                if let Some(e) = self.events.get_mut(id) {
                    match ev {
                        2 => e.click = Some((k, off)),
                        0 | 1 | 3 => e.run[ev] = Some((sc, off)),
                        _ => {}
                    }
                }
            }
            (4, 0x01) => {
                let id = v(self, 0) as usize;
                let on = b(1) != 0;
                if on && id == self.roles[4] {
                    return;
                }
                if let Some(e) = self.events.get_mut(id) {
                    e.lock = on;
                    if !on {
                        e.state = 0;
                    }
                }
            }
            (4, 0x02) => {
                let id = v(self, 0) as usize;
                if let Some(r) = self.roles.get_mut(b(1) as usize) {
                    *r = id;
                }
            }
            (5, 0x00) => {
                let _n = v(self, 0);
                let t = s(1);
                self.show_text(key, at, t);
            }
            (5, 0x01) => {
                let w = v(self, 0) as usize;
                let x = v(self, 1);
                let y = v(self, 2);
                let size = v(self, 3);
                let c = v(self, 4) as usize;
                self.msg.window = w;
                self.msg.x = x;
                self.msg.y = y;
                self.msg.size = size.clamp(8, 64);
                self.msg.colour = c;
            }
            (5, 0x02) => {
                let p = v(self, 0) as usize;
                let f = v(self, 1) as usize;
                self.msg.plate = (p, f);
                self.msg.name_x = v(self, 2);
                self.msg.name_y = v(self, 3);
                self.msg.name_size = v(self, 4).clamp(8, 64);
                self.msg.name_colour = v(self, 5) as usize;
            }
            (5, 0x04) => {
                let (g, w) = (v(self, 0), v(self, 1));
                let Some((_, t)) = self.cur.as_ref() else {
                    return;
                };
                let sc = t.script.clone();
                let at = |l: i32| {
                    u32::try_from(l)
                        .ok()
                        .and_then(|l| sc.labels.get(&l))
                        .map(|&o| (sc.clone(), o))
                };
                self.msg.gone = at(g);
                self.msg.waiting = at(w);
            }
            (6, 0x00) => {
                let name = st(0);
                let repeat = b(1) != 0;
                let fade = v(self, 2);
                if self
                    .bgm
                    .as_ref()
                    .is_some_and(|b| b.0.eq_ignore_ascii_case(&name))
                {
                    return;
                }
                let data = self.read(&name);
                self.bgm = Some((name, repeat));
                self.sounds.push(SoundEvent {
                    channel: "bgm".into(),
                    data,
                    volume: 1.0,
                    repeat,
                    fade_ms: fade as f32,
                });
            }
            (6, 0x01) => {
                let ms = v(self, 0);
                self.music_stop(ms);
            }
            (6, 0x02) => self.voice = Some(st(0)),
            (6, 0x03) => self.voice = None,
            (6, 0x04) => {
                let ch = v(self, 0).max(0) as usize;
                self.se_names.insert(ch, st(1));
            }
            (6, 0x05) => {
                let ch = v(self, 0).max(0) as usize;
                let repeat = b(1) != 0;
                if let Some(n) = self.se_names.get(&ch).cloned() {
                    if self.skip && !repeat {
                        return;
                    }
                    let data = self.read(&n);
                    if repeat {
                        self.se_loops.insert(ch, n);
                    } else {
                        self.se_loops.remove(&ch);
                    }
                    self.sounds.push(SoundEvent {
                        channel: format!("se{ch}"),
                        data,
                        volume: 1.0,
                        repeat,
                        fade_ms: 0.0,
                    });
                }
            }
            (6, 0x06) => {
                let ch = v(self, 0).max(0) as usize;
                self.se_loops.remove(&ch);
                self.sounds.push(SoundEvent {
                    channel: format!("se{ch}"),
                    data: None,
                    volume: 1.0,
                    repeat: false,
                    fade_ms: 0.0,
                });
            }
            (6, 0x07) => {
                if let Some(n) = self.last_voice.clone() {
                    self.play_voice(&n);
                }
            }
            _ => {}
        }
    }

    fn music_stop(&mut self, ms: i32) {
        if self.bgm.take().is_some() {
            self.sounds.push(SoundEvent {
                channel: "bgm".into(),
                data: None,
                volume: 1.0,
                repeat: false,
                fade_ms: ms as f32,
            });
        }
    }

    fn play_voice(&mut self, n: &str) {
        let data = self.read(n);
        self.sounds.push(SoundEvent {
            channel: "voice".into(),
            data,
            volume: 1.0,
            repeat: false,
            fade_ms: 0.0,
        });
    }

    fn sprite_command(&mut self, key: Key, ins: &Ins) {
        let a = &ins.args;
        let mut vals = Vec::with_capacity(a.len());
        for x in a {
            vals.push(match x {
                Arg::V(v) => self.val(v),
                Arg::B(b) => *b as i32,
                Arg::W(w) => *w,
                Arg::S(_) => 0,
            });
        }
        let text = a.iter().find_map(|x| match x {
            Arg::S(s) => Some(s.clone()),
            _ => None,
        });
        let id = vals.first().copied().unwrap_or(0);
        if !(1..SPRITES as i32).contains(&id) {
            return;
        }
        let id = id as usize;
        let n = |i: usize| vals.get(i).copied().unwrap_or(0);
        let skip = self.skip;
        let anim = |from: [f64; 4], to: [f64; 4], ms: i32| -> Option<Anim> {
            let n = if skip { 0 } else { frames(ms) };
            Some(Anim { from, to, k: 0, n })
        };
        let _ = key;
        match ins.op {
            0x00 => {
                let name = text
                    .map(|t| encoding_rs::SHIFT_JIS.decode(&t).0.into_owned())
                    .unwrap_or_default();
                self.sprites[id] = Sprite {
                    name: name.clone(),
                    ..Sprite::default()
                };
                self.events[id] = Events::default();
                if !name.is_empty() && self.picture(&name).is_none() {
                    self.trace(&format!("no picture {name}"));
                }
            }
            0x01 => {
                let s = &mut self.sprites[id];
                s.x = n(1) as f64;
                s.y = n(2) as f64;
                s.moving = None;
            }
            0x02 => self.sprites[id].shown = n(1) != 0,
            0x03 => {
                self.sprites[id].rect = Some([n(1) as f64, n(2) as f64, n(3) as f64, n(4) as f64]);
            }
            0x04 => self.sprites[id].rot = n(1) as f64,
            0x05 => {
                let s = &mut self.sprites[id];
                s.sx = n(1) as f64 * 0.01;
                s.sy = n(2) as f64 * 0.01;
            }
            0x07 => self.sprites[id].ease = n(1) as f64 * 0.01,
            0x08 => {
                let s = &self.sprites[id];
                let m = anim(
                    [s.x, s.y, 0.0, 0.0],
                    [n(1) as f64, n(2) as f64, 0.0, 0.0],
                    n(3),
                );
                self.sprites[id].moving = m;
            }
            0x09 => {
                let (w, h) = self.size(id);
                let s = &self.sprites[id];
                let r = s.rect.unwrap_or([0.0, 0.0, w as f64, h as f64]);
                let m = anim(
                    r,
                    [n(1) as f64, n(2) as f64, n(3) as f64, n(4) as f64],
                    n(5),
                );
                self.sprites[id].clipping = m;
            }
            0x0a => {
                let s = &self.sprites[id];
                let m = anim([s.rot, 0.0, 0.0, 0.0], [n(1) as f64, 0.0, 0.0, 0.0], n(2));
                self.sprites[id].turning = m;
            }
            0x0b => {
                let s = &self.sprites[id];
                let to = [n(1) as f64 * 0.01, n(2) as f64 * 0.01, 0.0, 0.0];
                let m = anim([s.sx, s.sy, 0.0, 0.0], to, n(3));
                self.sprites[id].scaling = m;
            }
            0x0c => {
                let s = &mut self.sprites[id];
                s.frame = n(1);
                s.frames = Some(FrameAnim {
                    from: n(1),
                    to: n(2),
                    every: frames(n(3)),
                    k: 0,
                    repeat: n(4) != 0,
                });
                if skip && n(4) == 0 {
                    s.frame = n(2);
                    s.frames = None;
                }
            }
            0x0d => {
                let s = &mut self.sprites[id];
                s.frame = n(1);
                s.frames = None;
            }
            0x0e => {
                let s = &mut self.sprites[id];
                s.alpha = n(1) as f64 * 0.01;
                s.fading = None;
            }
            0x0f => self.sprites[id].tone = n(1) as f64 * 0.01,
            0x10 => {
                let s = &self.sprites[id];
                let m = anim(
                    [s.alpha, 0.0, 0.0, 0.0],
                    [n(1) as f64 * 0.01, 0.0, 0.0, 0.0],
                    n(2),
                );
                self.sprites[id].fading = m;
            }
            0x11 => {
                let s = &self.sprites[id];
                let m = anim(
                    [s.tone, 0.0, 0.0, 0.0],
                    [n(1) as f64 * 0.01, 0.0, 0.0, 0.0],
                    n(2),
                );
                self.sprites[id].toning = m;
            }
            0x12 => {
                let t = text.unwrap_or_default();
                self.sprites[id].text = Some((n(1), n(2), n(3), n(4).max(0) as usize, t));
            }
            0x13 => {
                let p = n(1);
                self.sprites[id].parent = if (1..SPRITES as i32).contains(&p) && p as usize != id {
                    p as usize
                } else {
                    0
                };
            }
            _ => {}
        }
    }

    fn size(&mut self, id: usize) -> (u32, u32) {
        let s = &self.sprites[id];
        if s.name.is_empty() {
            return (0, 0);
        }
        let (name, f) = (s.name.clone(), s.frame);
        match self.picture(&name) {
            Some(p) => {
                let i = p[((f.max(1) - 1) as usize).min(p.len() - 1)].clone();
                (i.w, i.h)
            }
            None => (0, 0),
        }
    }

    fn transition(&mut self, ms: i32, keep_window: bool) {
        if !keep_window {
            self.window = false;
        }
        if self.skip || ms <= 0 {
            self.trans = None;
            return;
        }
        let from = Rc::new(self.render_draws(&self.last_draws.clone()));
        self.trans = Some(Trans {
            from,
            start: self.now,
            ms: ms as f64,
        });
        self.wait(Wait::Frames(frames(ms), false));
    }

    // ---- text ----

    fn show_text(&mut self, key: Key, at: usize, t: Vec<u8>) {
        if key == Key::Scen {
            self.snapshot(at);
        }
        let (name, body) = self.split_name(&t);
        self.name = name;
        self.text = self.layout(&body);
        if key == Key::Scen {
            self.title_snapshot();
        }
        self.window = true;
        if let Some(v) = self.voice.take() {
            if !self.skip {
                self.play_voice(&v);
            }
            self.last_voice = Some(v);
        } else {
            self.last_voice = None;
        }
        self.wait(Wait::Text);
        self.auto_at =
            self.now + 1500.0 + 60.0 * self.text.iter().map(|l| l.len() as f64 / 2.0).sum::<f64>();
        if let Some((sc, off)) = self.msg.waiting.clone() {
            self.threads.insert(Key::Msg, Thread::new(sc, off));
        }
    }

    /// `【name】/【real name】text`: the name shown and the rest.
    fn split_name(&self, t: &[u8]) -> (Option<Vec<u8>>, Vec<u8>) {
        let (open, close, sep) = &self.brackets;
        if !t.starts_with(open) {
            return (None, t.to_vec());
        }
        let Some(end) = find(&t[open.len()..], close) else {
            return (None, t.to_vec());
        };
        let name = t[open.len()..open.len() + end].to_vec();
        let mut rest = &t[open.len() + end + close.len()..];
        if !sep.is_empty() && rest.starts_with(sep) && rest[sep.len()..].starts_with(open) {
            let r = &rest[sep.len() + open.len()..];
            if let Some(e2) = find(r, close) {
                rest = &r[e2 + close.len()..];
            }
        }
        (Some(name), rest.to_vec())
    }

    /// Lines of at most `line_chars` full-width characters; `_r` breaks, `<base,ruby>` keeps
    /// the base.
    fn layout(&self, t: &[u8]) -> Vec<Vec<u8>> {
        let mut plain = Vec::new();
        let mut i = 0;
        while i < t.len() {
            if t[i] == b'_' && i + 1 < t.len() {
                match t[i + 1] {
                    b'r' | b'n' => plain.push(b'\n'),
                    _ => {}
                }
                i += 2;
                continue;
            }
            if t[i] == b'<'
                && let Some(end) = t[i..].iter().position(|&c| c == b'>')
            {
                let inner = &t[i + 1..i + end];
                let base = match find(inner, b",") {
                    Some(c) => &inner[..c],
                    None => inner,
                };
                plain.extend_from_slice(base);
                i += end + 1;
                continue;
            }
            if t[i] == b'['
                && let Some(end) = t[i..].iter().position(|&c| c == b']')
            {
                i += end + 1;
                continue;
            }
            let c = t[i];
            let n = if (0x81..=0x9f).contains(&c) || (0xe0..=0xfc).contains(&c) {
                2
            } else {
                1
            };
            plain.extend_from_slice(&t[i..(i + n).min(t.len())]);
            i += n;
        }
        let max = self.line_chars * 2;
        let mut lines = Vec::new();
        for part in plain.split(|&c| c == b'\n') {
            let mut line = Vec::new();
            for c in chars(part) {
                if line.len() + c.len() > max {
                    lines.push(std::mem::take(&mut line));
                }
                line.extend_from_slice(c);
            }
            lines.push(line);
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines
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

    fn colour(&self, i: usize) -> [u8; 3] {
        let c = self.colours.get(i).copied().unwrap_or([255; 6]);
        [c[0], c[1], c[2]]
    }

    // ---- frames ----

    pub fn frame(&mut self) -> Result<(), String> {
        self.now += FRAME_MS;
        if let Some(t) = &self.trans
            && self.now - t.start >= t.ms
        {
            self.trans = None;
        }
        self.animate();
        if self.view == View::Game {
            self.hover();
            if self.auto && self.now >= self.auto_at {
                self.advance();
            }
            if self.skip {
                self.advance();
            }
            self.run_threads();
        }
        self.clicked = false;
        Ok(())
    }

    fn run_threads(&mut self) {
        let keys: Vec<Key> = self.threads.keys().copied().collect();
        for k in keys {
            if k == Key::Scen && self.mode != 2 {
                continue;
            }
            if k == Key::Mode && self.mode == 2 {
                continue;
            }
            let Some(t) = self.threads.get_mut(&k) else {
                continue;
            };
            if !t.running {
                if matches!(k, Key::Event(..) | Key::Msg) {
                    self.threads.remove(&k);
                }
                continue;
            }
            let clicked = self.clicked;
            match t.wait.clone() {
                Wait::Frames(n, click) => {
                    if (click && clicked) || n <= 1 || self.skip {
                        t.wait = Wait::None;
                    } else {
                        t.wait = Wait::Frames(n - 1, click);
                        continue;
                    }
                }
                Wait::Sprite(id) => {
                    if self.sprites.get(id).is_some_and(|s| s.frames.is_some()) && !self.skip {
                        continue;
                    }
                    t.wait = Wait::None;
                }
                Wait::Text => continue,
                Wait::None => {}
            }
            self.run_thread(k);
        }
    }

    fn animate(&mut self) {
        for s in self.sprites.iter_mut() {
            if s.name.is_empty() {
                continue;
            }
            let e = s.ease;
            let step = |a: &mut Option<Anim>| -> Option<[f64; 4]> {
                let m = a.as_mut()?;
                if m.k >= m.n {
                    let to = m.to;
                    *a = None;
                    return Some(to);
                }
                m.k += 1;
                let t = eased(m.k as f64 / m.n as f64, e);
                let out: [f64; 4] = std::array::from_fn(|i| m.from[i] + (m.to[i] - m.from[i]) * t);
                if m.k >= m.n {
                    *a = None;
                }
                Some(out)
            };
            if let Some(v) = step(&mut s.moving) {
                (s.x, s.y) = (v[0], v[1]);
            }
            if let Some(v) = step(&mut s.clipping) {
                s.rect = Some(v);
            }
            if let Some(v) = step(&mut s.turning) {
                s.rot = v[0];
            }
            if let Some(v) = step(&mut s.scaling) {
                (s.sx, s.sy) = (v[0], v[1]);
            }
            if let Some(v) = step(&mut s.fading) {
                s.alpha = v[0];
            }
            if let Some(v) = step(&mut s.toning) {
                s.tone = v[0];
            }
            if let Some(f) = s.frames.as_mut() {
                if f.k >= f.every {
                    f.k = 0;
                    if s.frame == f.to {
                        if f.repeat {
                            s.frame = f.from;
                        } else {
                            s.frames = None;
                        }
                    } else if f.from < f.to {
                        s.frame += 1;
                    } else {
                        s.frame -= 1;
                    }
                } else {
                    f.k += 1;
                }
            }
        }
    }

    // ---- pictures ----

    fn plane(&self, id: usize) -> u8 {
        for (k, &(a, b)) in self.planes.iter().enumerate() {
            if a > 0 && (a..=b).contains(&id) {
                return k as u8 + 2;
            }
        }
        1
    }

    fn plane_shown(&self, p: u8) -> bool {
        match p {
            1 => true,
            2 => self.window && self.mode == 2,
            _ => false,
        }
    }

    /// The picture of a sprite as drawn: the image and its place on the screen.
    fn sprite_draws(&mut self, id: usize, out: &mut Vec<Draw>) {
        let s = self.sprites[id].clone();
        if s.name.is_empty() || !s.shown || s.alpha <= 0.0 {
            return;
        }
        if self.mode == 2
            && (id == self.msg.plate.0 || id == self.msg.plate.1)
            && self.name.is_none()
        {
            return;
        }
        let Some(p) = self.picture(&s.name) else {
            return;
        };
        let img = p[((s.frame.max(1) - 1) as usize).min(p.len() - 1)].clone();
        let (mut x, mut y) = (s.x, s.y);
        if s.parent > 0 {
            let q = &self.sprites[s.parent];
            x += q.x;
            y += q.y;
        }
        // 2.03's rectangle is the part of the picture shown, drawn at the sprite's place and
        // scaled about its own centre (BB co007's split screens, sukima's cut-in windows).
        let (w, h) = (img.w as f64, img.h as f64);
        let r = s.rect.unwrap_or([0.0, 0.0, w, h]);
        let r = [r[0].max(0.0), r[1].max(0.0), r[2].min(w), r[3].min(h)];
        if r[2] <= r[0] || r[3] <= r[1] {
            return;
        }
        let (rw, rh) = (r[2] - r[0], r[3] - r[1]);
        let place = Place {
            x: x as f32,
            y: y as f32,
            sx: s.sx as f32,
            sy: s.sy as f32,
            cx: (x + rw / 2.0) as f32,
            cy: (y + rh / 2.0) as f32,
            rot: s.rot.to_radians() as f32,
            alpha: s.alpha.clamp(0.0, 1.0) as f32,
            add: false,
            flip: (false, false),
            clip: [0.0, 0.0, 1e6, 1e6],
        };
        let d = Draw {
            image: img.clone(),
            src: [r[0] as i32, r[1] as i32, r[2] as i32, r[3] as i32],
            dst: [x as f32, y as f32, (x + rw) as f32, (y + rh) as f32],
            place,
        };
        out.push(d.clone());
        if s.tone > 0.0 {
            let mut t = d;
            t.place.add = true;
            t.place.alpha = (s.tone.min(1.0) * s.alpha) as f32;
            out.push(t);
        }
        if let Some((tx, ty, size, c, t)) = &s.text {
            let rgb = self.colour(*c);
            let img = self.label(t, rgb, (*size).clamp(8, 64));
            let mut d = draw(
                &img,
                (x + *tx as f64) as f32 - 2.0,
                (y + *ty as f64) as f32 - 2.0,
                s.alpha as f32,
            );
            d.place.cx = place.cx;
            d.place.cy = place.cy;
            d.place.sx = place.sx;
            d.place.sy = place.sy;
            d.place.rot = place.rot;
            out.push(d);
        }
    }

    fn text_draws(&mut self, out: &mut Vec<Draw>) {
        if !self.plane_shown(2) {
            return;
        }
        let m = self.msg.clone();
        if let Some(n) = self.name.clone() {
            let img = self.label(&n, self.colour(m.name_colour), m.name_size);
            out.push(draw(
                &img,
                (m.name_x + 4) as f32,
                (m.name_y + 1) as f32,
                1.0,
            ));
        }
        let rgb = self.colour(m.colour);
        let step = m.size + 7;
        for (k, l) in self.text.clone().iter().enumerate() {
            let img = self.label(l, rgb, m.size);
            out.push(draw(
                &img,
                (m.x - 2) as f32,
                (m.y + k as i32 * step - 2) as f32,
                1.0,
            ));
        }
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = vec![draw(&solid_image([0, 0, 0, 255]), 0.0, 0.0, 1.0)];
        out[0].dst = [0.0, 0.0, self.width as f32, self.height as f32];
        for p in [1u8, 2] {
            if !self.plane_shown(p) {
                continue;
            }
            for id in 1..SPRITES {
                if self.plane(id) == p {
                    self.sprite_draws(id, &mut out);
                }
            }
            if p == 2 {
                self.text_draws(&mut out);
            }
        }
        self.last_draws = out.clone();
        if let Some(t) = &self.trans {
            let p = ((self.now - t.start) / t.ms.max(1.0)).clamp(0.0, 1.0) as f32;
            out.push(draw(&t.from, 0.0, 0.0, 1.0 - p));
        }
        match self.view {
            View::Menu => self.menu_draws(&mut out),
            View::Slots { save } => self.slot_draws(save, &mut out),
            View::Game => {}
        }
        out
    }

    fn render_draws(&self, ds: &[Draw]) -> Image {
        let mut img = Image::new(self.width, self.height);
        img.fill([0, 0, 0, 255]);
        for d in ds {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }

    /// The frame as one picture (for the native runner).
    pub fn render(&mut self) -> Image {
        let ds = self.draws();
        self.render_draws(&ds)
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    // ---- input ----

    fn hit_rect(&mut self, id: usize) -> Option<[i32; 4]> {
        let s = &self.sprites[id];
        if s.name.is_empty() || !s.shown {
            return None;
        }
        let (mut x, mut y) = (s.x, s.y);
        if s.parent > 0 {
            let q = &self.sprites[s.parent];
            x += q.x;
            y += q.y;
        }
        let rect = s.rect;
        let (w, h) = self.size(id);
        let r = rect.unwrap_or([0.0, 0.0, w as f64, h as f64]);
        Some([
            x as i32,
            y as i32,
            (x + r[2] - r[0]) as i32,
            (y + r[3] - r[1]) as i32,
        ])
    }

    /// The sprite with events under the pointer (the top one).
    fn hovered(&mut self) -> Option<usize> {
        let (px, py) = self.pointer;
        for id in (1..SPRITES).rev() {
            let e = &self.events[id];
            if e.click.is_none() && e.run.iter().all(|r| r.is_none()) {
                continue;
            }
            if !self.plane_shown(self.plane(id)) {
                continue;
            }
            if let Some(r) = self.hit_rect(id)
                && inside(r, px, py)
            {
                return Some(id);
            }
        }
        None
    }

    fn fire(&mut self, id: usize, ev: usize) {
        if let Some((sc, off)) = self.events[id].run[ev].clone() {
            self.threads
                .insert(Key::Event(id, ev), Thread::new(sc, off));
        }
    }

    fn hover(&mut self) {
        let over = self.hovered();
        for id in 1..SPRITES {
            let e = &self.events[id];
            if e.click.is_none() && e.run.iter().all(|r| r.is_none()) {
                continue;
            }
            if e.lock {
                if e.state != 4 {
                    self.events[id].state = 4;
                    self.fire(id, 3);
                }
                continue;
            }
            if over == Some(id) {
                if e.state != 2 {
                    self.events[id].state = 2;
                    self.fire(id, 1);
                }
            } else if e.state != 1 {
                self.events[id].state = 1;
                self.fire(id, 0);
            }
        }
    }

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    fn advance(&mut self) {
        let waiting = self
            .threads
            .get(&Key::Scen)
            .is_some_and(|t| t.wait == Wait::Text);
        if !waiting || self.trans.is_some() && !self.skip {
            return;
        }
        if let Some(t) = self.threads.get_mut(&Key::Scen) {
            t.wait = Wait::None;
        }
        if !self.skip {
            self.sounds.push(SoundEvent {
                channel: "voice".into(),
                data: None,
                volume: 1.0,
                repeat: false,
                fade_ms: 0.0,
            });
        }
        if let Some((sc, off)) = self.msg.gone.clone() {
            self.threads.insert(Key::Msg, Thread::new(sc, off));
        }
        self.auto_at = f64::MAX;
    }

    fn unlock(&mut self, role: usize) {
        let id = self.roles[role];
        if id > 0 && id < SPRITES {
            self.events[id].lock = false;
            self.events[id].state = 0;
        }
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
        match self.view {
            View::Menu => {
                let hit = self
                    .menu_items()
                    .into_iter()
                    .position(|(_, r)| inside(r, x, y));
                match hit {
                    Some(0) if self.saved_at.is_some() => self.view = View::Slots { save: true },
                    Some(1) => self.view = View::Slots { save: false },
                    Some(2) => self.back_to_title(),
                    Some(3) => self.view = View::Game,
                    _ => {}
                }
                return;
            }
            View::Slots { save } => {
                let hit = self.slot_rects().into_iter().position(|r| inside(r, x, y));
                match hit {
                    Some(i) if i < SLOTS => {
                        if save {
                            self.save(i);
                        } else if self.read_slot(i).is_some() {
                            self.load_slot(i);
                        }
                    }
                    Some(_) => self.close_slots(),
                    None => {}
                }
                return;
            }
            View::Game => {}
        }
        if let Some(id) = self.hovered()
            && let Some((k, off)) = self.events[id].click
        {
            if self.events[id].lock {
                return;
            }
            // a choice stops auto and skip; their own buttons toggle them
            if id != self.roles[2] && id != self.roles[3] {
                if self.auto {
                    self.auto = false;
                    self.unlock(2);
                }
                if self.skip {
                    self.skip = false;
                    self.unlock(3);
                }
            }
            if let Some(t) = self.thread_mut(k) {
                t.pc = off;
                t.running = true;
                t.wait = Wait::None;
                t.stack.clear();
            }
            return;
        }
        if self.auto {
            self.auto = false;
            self.unlock(2);
            return;
        }
        if self.skip {
            self.skip = false;
            self.unlock(3);
            return;
        }
        if self.mode == 2 && !self.window && !self.text.is_empty() {
            let waiting = self
                .threads
                .get(&Key::Scen)
                .is_some_and(|t| t.wait == Wait::Text);
            if waiting {
                self.window = true;
                return;
            }
        }
        self.clicked = true;
        self.advance();
    }

    fn close_slots(&mut self) {
        if self.title_load {
            self.title_load = false;
            self.view = View::Game;
        } else {
            self.view = if self.mode == 2 {
                View::Menu
            } else {
                View::Game
            };
        }
    }

    pub fn cancel(&mut self) {
        match self.view {
            View::Menu => self.view = View::Game,
            View::Slots { .. } => self.close_slots(),
            View::Game => {
                if self.skip {
                    self.skip = false;
                    self.unlock(3);
                } else if self.mode == 2 && self.saved_at.is_some() {
                    self.view = View::Menu;
                }
            }
        }
    }

    /// 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip.
    pub fn key(&mut self, k: usize) {
        match k {
            4 if self.view == View::Game => {
                self.clicked = true;
                self.advance();
            }
            5 => self.cancel(),
            6 => self.skip = !self.skip,
            _ => {}
        }
    }

    pub fn wheel(&mut self, up: bool) {
        if !up && self.view == View::Game {
            self.advance();
        }
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    // ---- menus of this player ----

    fn solid(&self, rgba: [u8; 4], r: [i32; 4]) -> Draw {
        let mut d = draw(&solid_image(rgba), 0.0, 0.0, 1.0);
        d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        d
    }

    fn button(&mut self, t: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let over = inside(r, self.pointer.0, self.pointer.1);
        out.push(self.solid(
            if over {
                [120, 80, 50, 235]
            } else {
                [40, 28, 20, 215]
            },
            r,
        ));
        let img = self.label(&nexas::sjis::encode(t), [255, 255, 255], 22);
        let x = (r[0] + r[2]) as f32 / 2.0 - img.w as f32 / 2.0;
        let y = (r[1] + r[3]) as f32 / 2.0 - img.h as f32 / 2.0;
        out.push(draw(&img, x, y, 1.0));
    }

    fn menu_items(&self) -> Vec<(&'static str, [i32; 4])> {
        let names = ["セーブ", "ロード", "タイトルへ戻る", "ゲームに戻る"];
        let (bw, bh, gap) = (320, 50, 18);
        let top = (self.height as i32 - (bh * 4 + gap * 3)) / 2;
        names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let y = top + i as i32 * (bh + gap);
                (
                    *n,
                    [
                        (self.width as i32 - bw) / 2,
                        y,
                        (self.width as i32 + bw) / 2,
                        y + bh,
                    ],
                )
            })
            .collect()
    }

    fn menu_draws(&mut self, out: &mut Vec<Draw>) {
        out.push(self.solid(
            [0, 0, 0, 150],
            [0, 0, self.width as i32, self.height as i32],
        ));
        for (t, r) in self.menu_items() {
            self.button(t, r, out);
        }
    }

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        let step = 46;
        let (bw, bh) = (self.width as i32 - 160, step - 8);
        let top = (self.height as i32 - step * (SLOTS as i32 + 1)) / 2 + 16;
        (0..=SLOTS as i32)
            .map(|i| {
                let y = top + i * step;
                [80, y, 80 + bw, y + bh]
            })
            .collect()
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        out.push(self.solid(
            [0, 0, 0, 200],
            [0, 0, self.width as i32, self.height as i32],
        ));
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
            self.button(&label, *r, out);
        }
        let head = self.label(
            &nexas::sjis::encode(if save { "セーブ" } else { "ロード" }),
            [255, 255, 255],
            26,
        );
        out.push(draw(&head, 80.0, (rects[0][1] - 38) as f32, 1.0));
    }

    fn back_to_title(&mut self) {
        self.view = View::Game;
        self.saved_at = None;
        self.voice = None;
        for ch in std::mem::take(&mut self.se_loops).into_keys() {
            self.sounds.push(SoundEvent {
                channel: format!("se{ch}"),
                data: None,
                volume: 1.0,
                repeat: false,
                fade_ms: 0.0,
            });
        }
        self.mode = 2;
        self.switch_mode(1);
    }

    // ---- saves ----

    fn snapshot(&mut self, at: usize) {
        let Some((_, t)) = self.cur.as_ref() else {
            return;
        };
        let (script, stack) = (t.script.name.clone(), t.stack.clone());
        let vars = self
            .gvars
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0)
            .map(|(i, v)| (i, *v))
            .collect();
        let sprites = (1..SPRITES)
            .filter(|&i| self.plane(i) == 1 && !self.sprites[i].name.is_empty())
            .map(|i| (i, self.sprites[i].clone()))
            .collect();
        let subs = (0..THREADS)
            .filter_map(|i| {
                let t = self.threads.get(&Key::Sub(i))?;
                t.running
                    .then(|| (i, t.script.name.clone(), t.pc, t.stack.clone()))
            })
            .collect();
        self.saved_at = Some(Snapshot {
            script,
            pc: at,
            stack,
            vars,
            sprites,
            subs,
            bgm: self.bgm.clone(),
            se: self.se_loops.clone(),
            title: String::new(),
        });
    }

    /// Names a save after the scene and the line on screen.
    fn title_snapshot(&mut self) {
        let line: String = self
            .text
            .iter()
            .map(|l| encoding_rs::SHIFT_JIS.decode(l).0.into_owned())
            .collect::<String>();
        let line = match &self.name {
            Some(n) => format!("【{}】{line}", encoding_rs::SHIFT_JIS.decode(n).0),
            None => line,
        };
        let title = format!("{} {line}", self.scene_title);
        if let Some(s) = self.saved_at.as_mut() {
            s.title = title;
        }
    }

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&format!("slot{i:02}"))?;
        Slot::parse(&String::from_utf8_lossy(&b))
    }

    fn save(&mut self, i: usize) {
        let Some(mut s) = self.saved_at.clone() else {
            return;
        };
        s.title = s.title.chars().take(30).collect();
        self.files
            .write_save(&format!("slot{i:02}"), Slot::write(&s).as_bytes());
        self.view = View::Game;
    }

    fn load_slot(&mut self, i: usize) {
        let Some(slot) = self.read_slot(i) else {
            return;
        };
        let s = slot.snap;
        let Some(sc) = self.script(&s.script) else {
            return;
        };
        self.view = View::Game;
        self.title_load = false;
        let from = Rc::new(self.render_draws(&self.last_draws.clone()));
        self.threads.remove(&Key::Scen);
        self.music_stop(0);
        for ch in std::mem::take(&mut self.se_loops).into_keys() {
            self.sounds.push(SoundEvent {
                channel: format!("se{ch}"),
                data: None,
                volume: 1.0,
                repeat: false,
                fade_ms: 0.0,
            });
        }
        self.mode = 1;
        self.switch_mode(2);
        self.gvars.iter_mut().for_each(|v| *v = 0);
        for (k, v) in s.vars {
            if let Some(r) = self.gvars.get_mut(k) {
                *r = v;
            }
        }
        for (k, sp) in s.sprites {
            if k < SPRITES {
                self.sprites[k] = sp;
            }
        }
        for (k, name, pc, stack) in s.subs {
            if let Some(sc) = self.script(&name) {
                let mut t = Thread::new(sc, pc);
                t.stack = stack;
                self.threads.insert(Key::Sub(k), t);
            }
        }
        if let Some((n, repeat)) = s.bgm {
            let data = self.read(&n);
            self.bgm = Some((n, repeat));
            self.sounds.push(SoundEvent {
                channel: "bgm".into(),
                data,
                volume: 1.0,
                repeat,
                fade_ms: 500.0,
            });
        }
        for (ch, n) in s.se {
            self.se_names.insert(ch, n.clone());
            let data = self.read(&n);
            self.se_loops.insert(ch, n);
            self.sounds.push(SoundEvent {
                channel: format!("se{ch}"),
                data,
                volume: 1.0,
                repeat: true,
                fade_ms: 0.0,
            });
        }
        let mut t = Thread::new(sc, s.pc);
        t.stack = s.stack;
        self.threads.insert(Key::Scen, t);
        self.trans = Some(Trans {
            from,
            start: self.now,
            ms: 400.0,
        });
    }
}

struct Slot {
    title: String,
    snap: Snapshot,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02x}")).collect()
}

fn unhex(t: &str) -> Vec<u8> {
    (0..t.len() / 2)
        .filter_map(|i| u8::from_str_radix(t.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

fn anim_str(a: &Option<Anim>, now: [f64; 4]) -> [f64; 4] {
    a.as_ref().map_or(now, |a| a.to)
}

impl Slot {
    fn write(s: &Snapshot) -> String {
        let mut o = format!("title {}\n", s.title.replace('\n', " "));
        let stack: Vec<String> = s.stack.iter().map(|p| p.to_string()).collect();
        o += &format!(
            "at {} {} {}\n",
            hex(s.script.as_bytes()),
            s.pc,
            stack.join(",")
        );
        let vars: Vec<String> = s.vars.iter().map(|(i, v)| format!("{i}:{v}")).collect();
        o += &format!("vars {}\n", vars.join(" "));
        for (i, p) in &s.sprites {
            let pos = anim_str(&p.moving, [p.x, p.y, 0.0, 0.0]);
            let rect = match (&p.clipping, p.rect) {
                (Some(a), _) => Some(a.to),
                (None, r) => r,
            };
            let rect = rect.map_or("-".to_string(), |r| {
                format!("{},{},{},{}", r[0], r[1], r[2], r[3])
            });
            let rot = anim_str(&p.turning, [p.rot; 4])[0];
            let sc = anim_str(&p.scaling, [p.sx, p.sy, 0.0, 0.0]);
            let alpha = anim_str(&p.fading, [p.alpha; 4])[0];
            let tone = anim_str(&p.toning, [p.tone; 4])[0];
            let fa = match &p.frames {
                Some(f) if f.repeat => format!("{},{},{}", f.from, f.to, f.every),
                Some(f) => format!("={}", f.to),
                None => "-".into(),
            };
            let text = match &p.text {
                Some((x, y, z, c, t)) => format!("{x},{y},{z},{c},{}", hex(t)),
                None => "-".into(),
            };
            o += &format!(
                "sprite {i} {} {} {} {} {} {rect} {rot} {} {} {alpha} {tone} {} {} {fa} {text}\n",
                hex(p.name.as_bytes()),
                p.frame,
                pos[0],
                pos[1],
                p.shown as u8,
                sc[0],
                sc[1],
                p.ease,
                p.parent,
            );
        }
        for (k, name, pc, stack) in &s.subs {
            let st: Vec<String> = stack.iter().map(|p| p.to_string()).collect();
            o += &format!("sub {k} {} {pc} {}\n", hex(name.as_bytes()), st.join(","));
        }
        if let Some((n, r)) = &s.bgm {
            o += &format!("bgm {} {}\n", hex(n.as_bytes()), *r as u8);
        }
        for (c, n) in &s.se {
            o += &format!("se {c} {}\n", hex(n.as_bytes()));
        }
        o
    }

    fn parse(s: &str) -> Option<Slot> {
        let txt = |h: &str| String::from_utf8_lossy(&unhex(h)).into_owned();
        let nums =
            |t: &str| -> Vec<usize> { t.split(',').filter_map(|x| x.parse().ok()).collect() };
        let mut snap = Snapshot {
            script: String::new(),
            pc: 0,
            stack: Vec::new(),
            vars: Vec::new(),
            sprites: Vec::new(),
            subs: Vec::new(),
            bgm: None,
            se: BTreeMap::new(),
            title: String::new(),
        };
        let mut title = String::new();
        for line in s.lines() {
            let (k, rest) = line.split_once(' ').unwrap_or((line, ""));
            let f: Vec<&str> = rest.split(' ').collect();
            match k {
                "title" => title = rest.to_string(),
                "at" => {
                    snap.script = txt(f.first()?);
                    snap.pc = f.get(1)?.parse().ok()?;
                    snap.stack = nums(f.get(2).unwrap_or(&""));
                }
                "vars" => {
                    for p in f {
                        if let Some((i, v)) = p.split_once(':')
                            && let (Ok(i), Ok(v)) = (i.parse(), v.parse())
                        {
                            snap.vars.push((i, v));
                        }
                    }
                }
                "sprite" if f.len() >= 16 => {
                    let n = |i: usize| f[i].parse::<f64>().unwrap_or(0.0);
                    let mut p = Sprite {
                        name: txt(f[1]),
                        frame: f[2].parse().unwrap_or(1),
                        x: n(3),
                        y: n(4),
                        shown: f[5] == "1",
                        rot: n(7),
                        sx: n(8),
                        sy: n(9),
                        alpha: n(10),
                        tone: n(11),
                        ease: n(12),
                        parent: f[13].parse().unwrap_or(0),
                        ..Sprite::default()
                    };
                    if f[6] != "-" {
                        let r: Vec<f64> = f[6].split(',').filter_map(|x| x.parse().ok()).collect();
                        if r.len() == 4 {
                            p.rect = Some([r[0], r[1], r[2], r[3]]);
                        }
                    }
                    if let Some(t) = f[14].strip_prefix('=') {
                        p.frame = t.parse().unwrap_or(p.frame);
                    } else if f[14] != "-" {
                        let r: Vec<i32> = f[14].split(',').filter_map(|x| x.parse().ok()).collect();
                        if r.len() == 3 {
                            p.frames = Some(FrameAnim {
                                from: r[0],
                                to: r[1],
                                every: r[2].max(0) as u32,
                                k: 0,
                                repeat: true,
                            });
                        }
                    }
                    if f[15] != "-" {
                        let r: Vec<&str> = f[15].split(',').collect();
                        if r.len() == 5 {
                            p.text = Some((
                                r[0].parse().unwrap_or(0),
                                r[1].parse().unwrap_or(0),
                                r[2].parse().unwrap_or(24),
                                r[3].parse().unwrap_or(1),
                                unhex(r[4]),
                            ));
                        }
                    }
                    snap.sprites.push((f[0].parse().ok()?, p));
                }
                "sub" if f.len() >= 3 => snap.subs.push((
                    f[0].parse().ok()?,
                    txt(f[1]),
                    f[2].parse().ok()?,
                    nums(f.get(3).unwrap_or(&"")),
                )),
                "bgm" if f.len() >= 2 => snap.bgm = Some((txt(f[0]), f[1] == "1")),
                "se" if f.len() >= 2 => {
                    snap.se.insert(f[0].parse().ok()?, txt(f[1]));
                }
                _ => {}
            }
        }
        if snap.script.is_empty() {
            return None;
        }
        Some(Slot { title, snap })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing() {
        assert_eq!(eased(0.5, 0.0), 0.5);
        assert!(eased(0.5, 1.0) < 0.5);
        assert!(eased(0.5, -1.0) > 0.5);
    }

    #[test]
    fn slot_round_trip() {
        let mut p = Sprite {
            name: "BG\\IM00.mgr".into(),
            x: 10.0,
            ..Sprite::default()
        };
        p.text = Some((1, 2, 24, 1, b"ab".to_vec()));
        let s = Snapshot {
            script: "Scenario\\co001.msc".into(),
            pc: 42,
            stack: vec![7],
            vars: vec![(3, -5)],
            sprites: vec![(2, p)],
            subs: vec![(1, "Scenario\\macro.msc".into(), 9, vec![])],
            bgm: Some(("m\\BGM01.ogv".into(), true)),
            se: BTreeMap::new(),
            title: "x".into(),
        };
        let back = Slot::parse(&Slot::write(&s)).unwrap().snap;
        assert_eq!(back.script, s.script);
        assert_eq!(back.pc, 42);
        assert_eq!(back.stack, vec![7]);
        assert_eq!(back.vars, vec![(3, -5)]);
        assert_eq!(back.sprites[0].1.name, "BG\\IM00.mgr");
        assert_eq!(back.sprites[0].1.x, 10.0);
        assert_eq!(back.sprites[0].1.text.as_ref().unwrap().4, b"ab");
        assert_eq!(back.subs[0].2, 9);
    }
}
