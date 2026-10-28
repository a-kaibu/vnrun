//! Runs an Eomaia game (FG REMAKE's 夜勤病棟 リメイク trial) a frame at a time, 1920x1080.
//!
//! The scripts are a stack machine (see [`crate::script`]). `0x17` calls a builtin: the class
//! and its function are popped, then the arguments. Story scripts mostly call the functions of
//! the game's libraries (`11.2`: arguments, number, 'library') and the events and functions of
//! its UI packages (`12.3`, `12.2`). We do not run the libraries and packages themselves (they
//! drive the exe's own layout and image classes); we run their functions here instead, by
//! name and number:
//!
//! - `eomaia_image` #103 (file) the background, #106 (file, n) / #107 (file, x, y, n) pictures
//!   over it; #109 shows them at once, #110 (ms) cross-fades to them
//! - `eomaia_char` #102 (_, file.char, _, _, x, _, slot) a character, #103 (_, part, n, slot)
//!   its face, #105 (slot, ms) shows it, #106 (_, slot, ms) / #107 (_, ms) take them away
//! - `cross_anime` #102 waits for the cross-fade; `eomaia_fade` #102 (out?, ARGB, ms) fades
//!   to or from a colour and #101 says whether the screen is faded out
//! - `audio` #105 (file, loop) music, #101 (ms) stops it, #118 (file) a sound, #130 (file) a voice
//! - `meswnd` event: shows the message built by `0x14` text, `0x1a` line breaks and the name of
//!   `23.7`, and waits for a click; `keywait` event (ms) waits; `22.0`, `0x19` choices, the
//!   `select` event and `select` #101 (the chosen one, from 1); `title_try` event: the title
//! - `0.2` (flag, on), `0.3` (flag) flags; `2.0` (n, format, args) formats; `10.0` / `10.1`
//!   jump to / run another script
//!
//! The rest (status window, map screen, movies, the libraries' own save backups) is left out;
//! a right click opens our own save slots.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::arc::Arcs;
use crate::script::{Op, Script};

pub const W: u32 = 1920;
pub const H: u32 = 1080;
const SLOTS: usize = 12;
/// the message window (meswnd.style)
const WIN_Y: i32 = 716;
const TEXT_AT: (i32, i32) = (374, 822);
const NAME_AT: (i32, i32) = (374, 776);
const TEXT_SIZE: i32 = 44;
const TEXT_LH: i32 = 58;
const TEXT_COLS: usize = 28;
/// a fade or cross-fade of -1 ms
const DEFAULT_MS: i32 = 500;
/// scripts of the exe's own setup, which we leave out
const SKIPPED: [&str; 2] = ["initialize.script", "setting.script"];
/// the title's buttons (title_try.style): name, picture, place
const TITLE_BUTTONS: [(&str, &str, i32, i32); 4] = [
    ("start", "start", 154, 556),
    ("resume", "resume", 154, 628),
    ("load", "load", 154, 700),
    ("exit", "trexit", 154, 988),
];
const BUTTON_W: i32 = 496;
const BUTTON_H: i32 = 67;
/// the choices (select.style)
const SELECT_AT: (i32, i32) = (409, 248);
const SELECT_W: i32 = 1120;
const SELECT_H: i32 = 112;
const SELECT_STEP: i32 = 130;
/// The places of the map (map.style): where, how big, and which icon places they use.
const PLACES: [(i32, i32, i32, i32, usize); 11] = [
    (306, 865, 1045, 198, 0),
    (620, 592, 441, 278, 1),
    (306, 592, 370, 278, 2),
    (981, 592, 370, 278, 3),
    (620, 400, 441, 278, 1),
    (306, 400, 370, 278, 2),
    (981, 400, 370, 278, 3),
    (620, 205, 441, 278, 1),
    (306, 205, 370, 278, 2),
    (981, 205, 370, 278, 3),
    (306, 12, 1045, 278, 4),
];
/// Where a place's character icons go: one character, then the first and second of two.
const ICONS: [[(i32, i32); 3]; 5] = [
    [(406, 0), (310, 0), (501, 0)],
    [(92, 32), (-4, 32), (187, 32)],
    [(44, 80), (-52, 80), (139, 80)],
    [(45, 80), (-51, 80), (140, 80)],
    [(406, 34), (310, 34), (501, 34)],
];
const ICON: i32 = 202;
/// "back to work": the end of the day's rounds
const WORK: [i32; 4] = [1420, 498, 1420 + 483, 498 + 190];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum V {
    I(i32),
    S(String),
    /// a variable passed by reference (`17.0`): a store and its index
    R(String, i32),
}

impl V {
    fn int(&self) -> i32 {
        match self {
            V::I(i) => *i,
            V::S(s) => s.trim().parse().unwrap_or(0),
            V::R(..) => 0,
        }
    }
    fn text(&self) -> String {
        match self {
            V::I(i) => i.to_string(),
            V::S(s) => s.clone(),
            V::R(..) => String::new(),
        }
    }
    fn truth(&self) -> bool {
        match self {
            V::I(i) => *i != 0,
            V::S(s) => !s.is_empty(),
            V::R(..) => true,
        }
    }
}

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

/// A running script, or a function of a library we run ourselves (`map`, `util`, ...).
struct Frame {
    script: Rc<Script>,
    pc: usize,
    /// where its variables (`L`) live: a library's are kept, a script's are its own
    store: String,
    /// a function's arguments (`A`; references for `R`)
    args: Vec<V>,
    /// a function: its result goes on the stack when it returns
    func: bool,
    /// the height of the stack when it started
    base: usize,
}

/// A function of a library or package: its arguments and code.
#[derive(Clone)]
struct Def {
    nargs: usize,
    body: usize,
    script: String,
}

/// Libraries whose functions we stand in for; the others (`map`, `mapfindcharevent*`,
/// `statuswnd`, `library`, `util`: the game's days, places and numbers) run as they are,
/// with the exe's own UI classes (`38.x` layouts, `15.x` settings) left out.
const NATIVE_LIBS: [&str; 13] = [
    "eomaia_image",
    "eomaia_char",
    "eomaia_fade",
    "eomaia_movie",
    "cross_anime",
    "audio",
    "debug",
    "hook",
    "low_image",
    "sysmenu",
    "config_sound",
    "config_system",
    "loaddevelop",
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Chara {
    file: String,
    /// where its middle goes
    x: i32,
    /// a part (face) and its number
    face: Option<(String, i32)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Scene {
    bg: Option<String>,
    /// pictures over the background by number: file, x, y
    subs: BTreeMap<i32, (String, i32, i32)>,
    chars: BTreeMap<i32, Chara>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Wait {
    None,
    Click,
    Frames(u32),
    ClickOrFrames(u32),
    Trans,
    Fade,
    Title,
    Select,
    Map,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Game,
    Slots { save: bool },
}

/// The screen covered by a colour: `alpha` goes from `from` to `to`.
#[derive(Clone, Copy)]
struct Fade {
    rgb: [u8; 3],
    from: f32,
    to: f32,
    t: u32,
    dur: u32,
}

impl Fade {
    fn alpha(&self) -> f32 {
        if self.t >= self.dur {
            return self.to;
        }
        self.from + (self.to - self.from) * self.t as f32 / self.dur.max(1) as f32
    }
}

/// A cross-fade from the screen as it was.
struct Trans {
    old: Vec<Draw>,
    t: u32,
    dur: u32,
}

struct Shake {
    t: u32,
    dur: u32,
    ax: f32,
    ay: f32,
}

/// A character's .char file: its canvas, base pictures and parts.
struct CharDef {
    w: i32,
    base: Vec<(String, i32, i32)>,
    /// part -> (x, y, sheet)
    subs: HashMap<String, (i32, i32, String)>,
}

/// The frames of a sprite sheet (.json): name -> x0 y0 x1 y1.
type Sheet = HashMap<String, [i32; 4]>;

pub struct Engine {
    /// the messages read so far (the wheel turned up)
    backlog: vncommon::backlog::Backlog,
    files: Box<dyn Files>,
    arcs: Arcs,
    scripts: HashMap<String, Rc<Script>>,
    /// (11 library or 12 package, name, number) -> the function
    defs: HashMap<(i32, String, i32), Def>,
    calls: Vec<Frame>,
    stack: Vec<V>,
    /// the variables of libraries and scripts by store
    stores: HashMap<String, HashMap<i32, V>>,
    retval: Option<V>,
    map_up: bool,
    globals: HashMap<i32, V>,
    others: HashMap<i32, V>,
    flags: u32,
    stage: Scene,
    shown: Scene,
    trans: Option<Trans>,
    fade: Fade,
    shake: Option<Shake>,
    name_buf: String,
    text_buf: String,
    name: String,
    text: String,
    window: bool,
    choices: Vec<String>,
    select: Option<Vec<String>>,
    chosen: i32,
    title: bool,
    wait: Wait,
    view: View,
    skip: bool,
    frames: u64,
    pointer: (i32, i32),
    bgm: Option<String>,
    sounds: Vec<SoundEvent>,
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<(String, i32), Rc<Image>>,
    images: HashMap<String, Option<Rc<Image>>>,
    sheets: HashMap<String, Rc<Sheet>>,
    char_defs: HashMap<String, Option<Rc<CharDef>>>,
    seed: u32,
    ended: bool,
    warned: std::collections::HashSet<String>,
    pub trace: bool,
}

fn ms_frames(ms: i32) -> u32 {
    let ms = if ms < 0 { DEFAULT_MS } else { ms };
    (ms.min(600_000) as u32 * 60).div_ceil(1000)
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
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
        clip: [0.0, 0.0, W as f32, H as f32],
    }
}

/// printf with %s and %d (width and zero padding too).
fn format(fmt: &str, args: &[V]) -> String {
    let mut out = String::new();
    let mut it = fmt.chars().peekable();
    let mut args = args.iter();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&d) = it.peek() {
            if d.is_ascii_digit() {
                spec.push(d);
                it.next();
            } else {
                break;
            }
        }
        match it.next() {
            Some('%') => out.push('%'),
            Some('s') => out.push_str(&args.next().map(V::text).unwrap_or_default()),
            Some('d') => {
                let v = args.next().map(V::int).unwrap_or(0);
                let w: usize = spec.parse().unwrap_or(0);
                if spec.starts_with('0') {
                    out.push_str(&format!("{v:0w$}"));
                } else {
                    out.push_str(&format!("{v:w$}"));
                }
            }
            Some(o) => {
                out.push('%');
                out.push(o);
            }
            None => out.push('%'),
        }
    }
    out
}

/// Splits a message into lines of at most `n` characters (closing marks stay with the line
/// before).
fn wrap(s: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in s.split('\n') {
        let chars: Vec<char> = para.chars().collect();
        let mut i = 0;
        loop {
            let mut end = (i + n).min(chars.len());
            while end < chars.len() && "」』）。、！？…―ー〜っゃゅょ".contains(chars[end])
            {
                end += 1;
            }
            out.push(chars[i..end].iter().collect());
            i = end;
            if i >= chars.len() {
                break;
            }
        }
    }
    out
}

fn sheet_of(j: &Json) -> Sheet {
    let mut out = Sheet::new();
    if let Some(frames) = j.get("frames").and_then(Json::as_object) {
        for (k, v) in frames {
            let f = &v["frame"];
            let n = |k: &str| f.get(k).and_then(Json::as_i64).unwrap_or(0) as i32;
            let (x, y) = (n("x"), n("y"));
            out.insert(k.clone(), [x, y, x + n("w"), y + n("h")]);
        }
    }
    out
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arcs = Arcs::open(files.as_mut())?;
        let mut e = Engine {
            backlog: Default::default(),
            files,
            arcs,
            scripts: HashMap::new(),
            defs: HashMap::new(),
            calls: Vec::new(),
            stack: Vec::new(),
            stores: HashMap::new(),
            retval: None,
            map_up: false,
            globals: HashMap::new(),
            others: HashMap::new(),
            flags: 0,
            stage: Scene::default(),
            shown: Scene::default(),
            trans: None,
            fade: Fade {
                rgb: [0, 0, 0],
                from: 1.0,
                to: 1.0,
                t: 0,
                dur: 0,
            },
            shake: None,
            name_buf: String::new(),
            text_buf: String::new(),
            name: String::new(),
            text: String::new(),
            window: false,
            choices: Vec::new(),
            select: None,
            chosen: 0,
            title: false,
            wait: Wait::None,
            view: View::Game,
            skip: false,
            frames: 0,
            pointer: (0, 0),
            bgm: None,
            sounds: Vec::new(),
            font: None,
            text_cache: HashMap::new(),
            images: HashMap::new(),
            sheets: HashMap::new(),
            char_defs: HashMap::new(),
            seed: 12345,
            ended: false,
            warned: Default::default(),
            trace: false,
        };
        for n in e.arcs.names() {
            if (n.ends_with(".lib") || n.ends_with(".package"))
                && let Ok(s) = e.script(&n)
            {
                for (k, (nargs, body)) in s.defs() {
                    let script = n.clone();
                    e.defs.insert(
                        k,
                        Def {
                            nargs,
                            body,
                            script,
                        },
                    );
                }
            }
        }
        let start = e.script("start.script")?;
        e.push_script(start);
        Ok(e)
    }

    /// Runs a script (on top of the current one).
    fn push_script(&mut self, s: Rc<Script>) {
        let store = format!("{}#{}", s.name, self.calls.len());
        self.stores.remove(&store);
        self.calls.push(Frame {
            pc: s.base,
            script: s,
            store,
            args: Vec::new(),
            func: false,
            base: self.stack.len(),
        });
    }

    fn deref(&self, v: V) -> V {
        match v {
            V::R(s, i) => self
                .stores
                .get(&s)
                .and_then(|m| m.get(&i))
                .cloned()
                .unwrap_or(V::I(0)),
            v => v,
        }
    }

    /// Drops what the current frame left on the stack.
    fn clear_stack(&mut self) {
        let base = self.calls.last().map_or(0, |f| f.base);
        self.stack.truncate(base);
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        self.font = Some(Rc::new(f));
        Ok(())
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Whether choices or the map are up.
    pub fn choosing(&self) -> bool {
        matches!(self.wait, Wait::Select | Wait::Map)
    }

    fn log(&self, s: &str) {
        if self.trace {
            eprintln!("eomaia: {s}");
        }
    }

    fn warn(&mut self, s: String) {
        if self.trace || self.warned.len() < 200 && self.warned.insert(s.clone()) {
            eprintln!("eomaia: {s}");
        }
    }

    fn script(&mut self, name: &str) -> Result<Rc<Script>, String> {
        let key = name.to_lowercase();
        if let Some(s) = self.scripts.get(&key) {
            return Ok(s.clone());
        }
        let d = self
            .arcs
            .read(self.files.as_mut(), &key)
            .ok_or_else(|| format!("no script {name}"))?;
        let s = Rc::new(Script::parse(&key, d)?);
        self.scripts.insert(key, s.clone());
        Ok(s)
    }

    // ---- files ----

    fn image(&mut self, name: &str) -> Option<Rc<Image>> {
        let key = name.to_lowercase();
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let img = self
            .arcs
            .read(self.files.as_mut(), &key)
            .and_then(|d| Image::decode(&d).ok())
            .map(Rc::new);
        if img.is_none() {
            self.warn(format!("missing picture {name}"));
        }
        if self.images.len() > 40 {
            self.images.clear();
        }
        self.images.insert(key, img.clone());
        img
    }

    fn json(&mut self, name: &str) -> Option<Json> {
        let d = self.arcs.read(self.files.as_mut(), name)?;
        let d = d.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&d);
        serde_json::from_slice(d).ok()
    }

    /// A sprite sheet by its name without extension.
    fn sheet(&mut self, name: &str) -> Rc<Sheet> {
        if let Some(s) = self.sheets.get(name) {
            return s.clone();
        }
        let s = Rc::new(
            self.json(&format!("{name}.json"))
                .map(|j| sheet_of(&j))
                .unwrap_or_default(),
        );
        self.sheets.insert(name.to_string(), s.clone());
        s
    }

    fn char_def(&mut self, file: &str) -> Option<Rc<CharDef>> {
        if let Some(c) = self.char_defs.get(file) {
            return c.clone();
        }
        let def = self.json(file).map(|j| {
            let n = |v: &Json, k: &str| v.get(k).and_then(Json::as_i64).unwrap_or(0) as i32;
            let w = j.get("size").map_or(W as i32, |s| n(s, "width"));
            let mut base = Vec::new();
            if let Some(b) = j.get("base").and_then(Json::as_object) {
                for v in b.values() {
                    let id = v.get("fileid").and_then(Json::as_str).unwrap_or("");
                    base.push((id.to_string(), n(v, "x"), n(v, "y")));
                }
            }
            let mut subs = HashMap::new();
            if let Some(b) = j.get("sub").and_then(Json::as_object) {
                for (k, v) in b {
                    let id = v.get("fileid").and_then(Json::as_str).unwrap_or("");
                    subs.insert(k.clone(), (n(v, "x"), n(v, "y"), id.to_string()));
                }
            }
            Rc::new(CharDef { w, base, subs })
        });
        if def.is_none() {
            self.warn(format!("missing character {file}"));
        }
        self.char_defs.insert(file.to_string(), def.clone());
        def
    }

    fn play(&mut self, channel: &str, file: &str, repeat: bool) {
        let Some(data) = self.arcs.read(self.files.as_mut(), file) else {
            self.warn(format!("missing sound {file}"));
            return;
        };
        self.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data: Some(data),
            volume: 1.0,
            repeat,
            fade_ms: 0.0,
        });
    }

    fn stop(&mut self, channel: &str, fade_ms: f32) {
        self.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data: None,
            volume: 0.0,
            repeat: false,
            fade_ms,
        });
    }

    fn play_bgm(&mut self, file: &str) {
        if self.bgm.as_deref() == Some(file) {
            return;
        }
        self.bgm = Some(file.to_string());
        self.play("bgm", file, true);
    }

    // ---- the machine ----

    /// The top of the stack (a reference: what it refers to).
    fn pop(&mut self) -> V {
        let v = self.pop_raw();
        self.deref(v)
    }

    fn pop_raw(&mut self) -> V {
        let base = self.calls.last().map_or(0, |f| f.base);
        if self.stack.len() > base {
            self.stack.pop().unwrap_or(V::I(0))
        } else {
            V::I(0)
        }
    }

    fn pop_int(&mut self) -> i32 {
        self.pop().int()
    }

    /// `n` arguments, the first one popped first.
    fn args(&mut self, n: usize) -> Vec<V> {
        (0..n).map(|_| self.pop()).collect()
    }

    fn rand(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (self.seed >> 16) & 0x7fff
    }

    fn step(&mut self) -> Result<(), String> {
        let Some(f) = self.calls.last() else {
            self.ended = true;
            return Ok(());
        };
        let script = f.script.clone();
        let pc = f.pc;
        let Some((op, next)) = script.op(pc) else {
            self.log(&format!("{} ran off its end", script.name));
            self.calls.pop();
            return Ok(());
        };
        self.calls.last_mut().unwrap().pc = next;
        let jump = |e: &mut Engine, to: usize| e.calls.last_mut().unwrap().pc = to;
        match op {
            Op::Int(v) => self.stack.push(V::I(v)),
            Op::Str(s) => self.stack.push(V::S(s)),
            Op::Text(s) => self.text_buf.push_str(&s),
            Op::Br(_) => self.text_buf.push('\n'),
            Op::Jz(to) => {
                if !self.pop().truth() {
                    jump(self, to);
                }
            }
            Op::Jmp(to) => jump(self, to),
            Op::Line(..) => self.clear_stack(),
            Op::Choice(to) => {
                self.pop();
                let mut label = String::new();
                let mut p = next;
                while p < to {
                    match script.op(p) {
                        Some((Op::Text(s), n)) => {
                            label.push_str(&s);
                            p = n;
                        }
                        Some((Op::End, _)) | None => break,
                        Some((_, n)) => p = n,
                    }
                }
                self.choices.push(label);
                jump(self, to);
            }
            Op::Var(op) => self.var(op),
            Op::Bin(0x39) => {
                let n = self.pop_int();
                let r = if n > 0 { self.rand() as i32 % n } else { 0 };
                self.stack.push(V::I(r));
            }
            Op::Bin(op) => {
                let b = self.pop();
                let a = self.pop();
                self.stack.push(Self::bin(op, a, b));
            }
            Op::Call => self.call()?,
            Op::Ret | Op::End => {
                let f = self.calls.pop().unwrap();
                self.stack.truncate(f.base);
                if f.func {
                    let r = self.retval.take().unwrap_or(V::I(0));
                    self.stack.push(r);
                } else {
                    self.log(&format!("{} ends", script.name));
                }
            }
            Op::Unknown(op) => {
                return Err(format!("{}: unknown op {op:#x} at {pc:#x}", script.name));
            }
        }
        Ok(())
    }

    fn var(&mut self, op: u8) {
        match op {
            // loads: (scope) index
            0x0a..=0x0d => {
                let i = self.pop_int();
                self.pop();
                let f = self.calls.last();
                let v = match op {
                    0x0a => self.globals.get(&i).cloned(),
                    0x0b => self.others.get(&i).cloned(),
                    0x0c => f
                        .and_then(|f| self.stores.get(&f.store))
                        .and_then(|m| m.get(&i))
                        .cloned(),
                    _ => f
                        .and_then(|f| f.args.get(i.max(0) as usize))
                        .cloned()
                        .map(|v| self.deref(v)),
                };
                self.stack.push(v.unwrap_or(V::I(0)));
            }
            // stores: value index
            _ => {
                let i = self.pop_int();
                let v = self.pop();
                let Some(f) = self.calls.last_mut() else {
                    return;
                };
                match op {
                    0x0e => {
                        self.globals.insert(i, v);
                    }
                    0x0f => {
                        self.others.insert(i, v);
                    }
                    0x10 => {
                        let s = f.store.clone();
                        self.stores.entry(s).or_default().insert(i, v);
                    }
                    // through an argument passed by reference
                    _ => match f.args.get_mut(i.max(0) as usize) {
                        Some(V::R(s, j)) => {
                            let (s, j) = (s.clone(), *j);
                            self.stores.entry(s).or_default().insert(j, v);
                        }
                        Some(a) => *a = v,
                        None => {}
                    },
                }
            }
        }
    }

    fn bin(op: u8, a: V, b: V) -> V {
        let (x, y) = (a.int(), b.int());
        let both_int = matches!((&a, &b), (V::I(_), V::I(_)));
        V::I(match op {
            0x34 if !both_int => return V::S(a.text() + &b.text()),
            0x34 => x.wrapping_add(y),
            0x35 => x.wrapping_sub(y),
            0x36 => x.wrapping_mul(y),
            0x37 => x.checked_div(y).unwrap_or(0),
            0x38 => x.checked_rem(y).unwrap_or(0),
            0x3a => (a.truth() && b.truth()) as i32,
            0x3b => (a.truth() || b.truth()) as i32,
            0x3c => x & y,
            0x3d => x | y,
            0x3e => (x < y) as i32,
            0x3f => (x > y) as i32,
            0x40 => (x <= y) as i32,
            0x41 => (x >= y) as i32,
            0x42 if both_int => (x == y) as i32,
            0x42 => (a.text() == b.text()) as i32,
            0x43 if both_int => (x != y) as i32,
            _ => (a.text() != b.text()) as i32,
        })
    }

    fn call(&mut self) -> Result<(), String> {
        let cid = self.pop_int();
        let sub = self.pop_int();
        match (cid, sub) {
            (11 | 12, 2) => {
                let name = self.pop().text();
                let m = self.pop_int();
                let def = self.defs.get(&(cid, name.clone(), m)).cloned();
                if def.is_none() {
                    self.warn(format!("{name}#{m} is not defined"));
                }
                let n = def.as_ref().map_or(0, |d| d.nargs);
                let raw: Vec<V> = (0..n).map(|_| self.pop_raw()).collect();
                self.log(&format!("{name}#{m} {raw:?}"));
                if cid == 11
                    && !NATIVE_LIBS.contains(&name.as_str())
                    && let Some(d) = def
                {
                    let script = self.script(&d.script)?;
                    self.calls.push(Frame {
                        script,
                        pc: d.body,
                        store: d.script,
                        args: raw,
                        func: true,
                        base: self.stack.len(),
                    });
                    return Ok(());
                }
                let args: Vec<V> = raw.into_iter().map(|v| self.deref(v)).collect();
                let r = self.lib(&name, m, &args);
                self.stack.push(r);
            }
            (12, 3) => {
                let name = self.pop().text();
                let n = match name.as_str() {
                    "meswnd" | "keywait" | "title_try" => 1,
                    _ => 0,
                };
                let args = self.args(n);
                self.log(&format!("event {name} {args:?}"));
                if let Some(r) = self.event(&name, &args) {
                    self.stack.push(r);
                }
            }
            (11..=14, 1) => {
                // a definition (not in the scripts we run): skip its body
                let f = self.calls.last_mut().unwrap();
                let at = f.pc;
                let end = f
                    .script
                    .code
                    .get(at..at + 4)
                    .map_or(0, |b| u32::from_be_bytes(b.try_into().unwrap()) as usize);
                f.pc = f.script.base + end;
                self.clear_stack();
            }
            (11 | 12, 0) => {
                // loading a library or package: nothing to do
                self.args(2);
            }
            (0, 13) => {
                let v = self.pop();
                self.retval = Some(v);
            }
            (0, 8) => {
                let key = self.pop().text();
                let v = match key.as_str() {
                    "Screen:Size:width" => W as i32,
                    "Screen:Size:height" => H as i32,
                    _ => 0,
                };
                self.stack.push(V::I(v));
            }
            (0, 2) => {
                let flag = self.pop_int() as u32;
                if self.pop().truth() {
                    self.flags |= flag;
                } else {
                    self.flags &= !flag;
                }
            }
            (0, 3) => {
                let flag = self.pop_int() as u32;
                self.stack.push(V::I((self.flags & flag != 0) as i32));
            }
            (2, 0) => {
                let n = self.pop_int().max(0) as usize;
                let fmt = self.pop().text();
                let args = self.args(n);
                self.stack.push(V::S(format(&fmt, &args)));
            }
            (2, 1) => {
                self.args(2);
            }
            (2, 3) => {
                let s = self.pop().text();
                let sep = self.pop().text();
                let i = self.pop_int().max(0) as usize;
                let part = if sep.is_empty() {
                    s
                } else {
                    s.split(sep.as_str()).nth(i).unwrap_or("").to_string()
                };
                self.stack.push(V::S(part));
            }
            (2, 4) => {
                let v = self.pop().int();
                self.stack.push(V::I(v));
            }
            (10, 0 | 1) => {
                let name = self.pop().text();
                if SKIPPED.contains(&name.to_lowercase().as_str()) {
                    return Ok(());
                }
                let s = self.script(&name)?;
                self.log(&format!("run {name}"));
                if sub == 0 {
                    self.calls.clear();
                    self.stack.clear();
                } else {
                    self.clear_stack();
                }
                self.push_script(s);
            }
            (22, 0) => {
                self.pop();
                self.choices.clear();
            }
            (23, 2 | 3) => {}
            (23, 4) => {
                self.pop();
            }
            (23, 7) => {
                self.args(2);
                self.name_buf = self.pop().text();
            }
            (17, 0) => {
                // (scope, first, count): references to variables, the first one on top
                self.pop();
                let first = self.pop_int();
                let count = self.pop_int().clamp(0, 1000);
                let store = self
                    .calls
                    .last()
                    .map(|f| f.store.clone())
                    .unwrap_or_default();
                for i in (0..count).rev() {
                    self.stack.push(V::R(store.clone(), first + i));
                }
            }
            (18, 0) => {
                self.pop();
            }
            _ => {
                self.warn(format!("builtin {cid}.{sub} left out"));
                self.clear_stack();
                self.stack.push(V::I(0));
            }
        }
        Ok(())
    }

    /// Starts showing the stage: at once, or cross-fading over `ms` (-1: the usual time).
    fn commit(&mut self, ms: Option<i32>) {
        if self.shown == self.stage {
            return;
        }
        match ms {
            Some(ms) if ms != 0 && !self.skip => {
                // a second change in the same step joins the cross-fade under way
                if !self.trans.as_ref().is_some_and(|t| t.t == 0) {
                    let old = self.scene_draws();
                    self.trans = Some(Trans {
                        old,
                        t: 0,
                        dur: ms_frames(ms),
                    });
                }
            }
            _ => self.trans = None,
        }
        self.shown = self.stage.clone();
    }

    fn lib(&mut self, lib: &str, m: i32, a: &[V]) -> V {
        let int = |i: usize| a.get(i).map_or(0, V::int);
        let text = |i: usize| a.get(i).map(V::text).unwrap_or_default();
        match (lib, m) {
            ("eomaia_image", 103) => {
                self.stage.bg = Some(text(0));
                self.stage.subs.clear();
            }
            ("eomaia_image", 106) => {
                self.stage.subs.insert(int(1), (text(0), 0, 0));
            }
            ("eomaia_image", 107) => {
                self.stage.subs.insert(int(3), (text(0), int(1), int(2)));
            }
            ("eomaia_image", 109) => self.commit(None),
            ("eomaia_image", 110) => self.commit(Some(int(0))),
            ("eomaia_image", 117) => {
                if !self.skip {
                    let dur = (int(2).max(1) as u32) * 6;
                    self.shake = Some(Shake {
                        t: 0,
                        dur,
                        ax: int(0) as f32,
                        ay: int(1) as f32,
                    });
                    self.wait = Wait::Frames(dur);
                }
            }
            ("eomaia_char", 102) => {
                self.stage.chars.insert(
                    int(6),
                    Chara {
                        file: text(1),
                        x: int(4),
                        face: None,
                    },
                );
            }
            ("eomaia_char", 103) => {
                if let Some(c) = self.stage.chars.get_mut(&int(3)) {
                    c.face = Some((text(1), int(2)));
                }
            }
            ("eomaia_char", 105) => self.commit(Some(int(1))),
            ("eomaia_char", 106) => {
                self.stage.chars.remove(&int(1));
                self.commit(Some(int(2)));
            }
            ("eomaia_char", 107) => {
                self.stage.chars.clear();
                self.commit(Some(int(1)));
            }
            ("cross_anime", 102) => {
                if self.trans.is_some() {
                    self.wait = Wait::Trans;
                }
            }
            ("eomaia_fade", 101) => return V::I((self.fade.alpha() >= 1.0) as i32),
            ("eomaia_fade", 102) => {
                let c = int(1) as u32;
                let to = if int(0) == 1 { 1.0 } else { 0.0 };
                if int(0) == 1 {
                    self.fade.rgb = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                }
                let dur = if self.skip { 0 } else { ms_frames(int(2)) };
                self.fade = Fade {
                    from: self.fade.alpha(),
                    to,
                    t: 0,
                    dur,
                    ..self.fade
                };
                if dur > 0 && self.fade.from != to {
                    self.wait = Wait::Fade;
                }
            }
            ("audio", 105) => {
                let f = text(0);
                self.play_bgm(&f);
            }
            ("audio", 101) => {
                if self.bgm.take().is_some() {
                    self.stop("bgm", int(0).max(0) as f32);
                }
            }
            ("audio", 104) => return V::I(1000),
            ("audio", 118) => {
                let f = text(0);
                self.play("se", &f, false);
            }
            ("audio", 130) => {
                if !self.skip {
                    let f = text(0);
                    self.play("voice", &f, false);
                }
            }
            ("meswnd", 101 | 106) => {
                self.window = false;
            }
            ("select", 101) => return V::I(self.chosen),
            _ if NATIVE_LIBS.contains(&lib) || matches!(lib, "meswnd" | "map" | "select") => {}
            _ => self.warn(format!("{lib}#{m} left out")),
        }
        V::I(0)
    }

    fn event(&mut self, name: &str, a: &[V]) -> Option<V> {
        match name {
            "meswnd" => {
                self.name = std::mem::take(&mut self.name_buf);
                self.text = std::mem::take(&mut self.text_buf);
                self.window = true;
                self.wait = Wait::Click;
                self.log(&format!("message {}: {}", self.name, self.text));
            }
            "keywait" => {
                let ms = a.first().map_or(-1, V::int);
                self.wait = if ms < 0 {
                    Wait::Click
                } else if self.skip {
                    Wait::None
                } else {
                    Wait::ClickOrFrames(ms_frames(ms))
                };
            }
            "select" => {
                self.select = Some(std::mem::take(&mut self.choices));
                self.wait = Wait::Select;
                return None;
            }
            "title_try" => {
                self.title = true;
                self.window = false;
                self.skip = false;
                self.wait = Wait::Title;
                return None;
            }
            "map" => {
                self.map_up = true;
                self.window = false;
                self.skip = false;
                self.wait = Wait::Map;
                return None;
            }
            _ => self.warn(format!("event {name} left out")),
        }
        Some(V::I(0))
    }

    pub fn frame(&mut self) -> Result<(), String> {
        self.log_page();
        self.frames += 1;
        if self.view != View::Game || self.ended {
            return Ok(());
        }
        if let Some(t) = &mut self.trans {
            t.t += 1;
            if t.t >= t.dur {
                self.trans = None;
            }
        }
        if self.fade.t < self.fade.dur {
            self.fade.t += 1;
        }
        if let Some(s) = &mut self.shake {
            s.t += 1;
            if s.t >= s.dur {
                self.shake = None;
            }
        }
        match self.wait {
            Wait::Frames(n) | Wait::ClickOrFrames(n) => {
                self.wait = match self.wait {
                    _ if n <= 1 => Wait::None,
                    Wait::Frames(_) => Wait::Frames(n - 1),
                    _ => Wait::ClickOrFrames(n - 1),
                }
            }
            Wait::Trans if self.trans.is_none() => self.wait = Wait::None,
            Wait::Fade if self.fade.t >= self.fade.dur => self.wait = Wait::None,
            Wait::Click if self.skip && self.frames.is_multiple_of(3) => self.advance(),
            _ => {}
        }
        let mut steps = 0;
        while self.wait == Wait::None && !self.ended {
            self.step()?;
            steps += 1;
            if steps > 1_000_000 {
                return Err("the script runs without waiting".into());
            }
        }
        Ok(())
    }

    fn advance(&mut self) {
        match self.wait {
            Wait::Click => {
                self.stop("voice", 0.0);
                self.wait = Wait::None;
            }
            Wait::Fade => {
                self.fade.t = self.fade.dur;
                self.wait = Wait::None;
            }
            Wait::Trans => {
                self.trans = None;
                self.wait = Wait::None;
            }
            Wait::Frames(_) | Wait::ClickOrFrames(_) => {
                self.shake = None;
                self.wait = Wait::None;
            }
            _ => {}
        }
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    fn title_rects() -> impl Iterator<Item = (&'static str, &'static str, [i32; 4])> {
        TITLE_BUTTONS
            .iter()
            .map(|&(n, p, x, y)| (n, p, [x, y, x + BUTTON_W, y + BUTTON_H]))
    }

    fn select_rects(n: usize) -> Vec<[i32; 4]> {
        let top = SELECT_AT.1 + (3 - n.min(3) as i32) * SELECT_STEP / 2;
        (0..n as i32)
            .map(|i| {
                let y = top + i * SELECT_STEP;
                [SELECT_AT.0, y, SELECT_AT.0 + SELECT_W, y + SELECT_H]
            })
            .collect()
    }

    /// Who is at a place of the map, as map.lib worked it out: (character, 1 an event or 2 a
    /// talk) for each of its two slots.
    fn map_slots(&self, p: usize) -> Vec<(i32, i32)> {
        let l = self.stores.get("map.lib");
        let get = |i: usize| l.and_then(|m| m.get(&(i as i32))).map_or(0, V::int);
        (0..2)
            .map(|k| (get(p * 8 + k), get(p * 8 + 2 + k)))
            .filter(|s| s.1 != 0)
            .collect()
    }

    /// The map's buttons: what clicking them answers, and where they are.
    fn map_buttons(&self) -> Vec<(String, [i32; 4])> {
        let mut out = Vec::new();
        for (p, &(x, y, w, h, icons)) in PLACES.iter().enumerate() {
            let slots = self.map_slots(p);
            for k in 0..slots.len() {
                let (dx, dy) = ICONS[icons][if slots.len() == 1 { 0 } else { k + 1 }];
                let (ix, iy) = (x + dx, y + dy);
                out.push((format!("{p}:{k}"), [ix, iy, ix + ICON, iy + ICON]));
            }
            if slots.len() < 2 {
                let k = if slots.is_empty() { 2 } else { 0 };
                out.push((format!("{p}:{k}"), [x, y, x + w, y + h]));
            }
        }
        out.push(("work".into(), WORK));
        out
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if self.backlog.is_open() {
            self.backlog.close();
            return;
        }
        self.pointer = (x, y);
        if let View::Slots { save } = self.view {
            self.slots_click(save, x, y);
            return;
        }
        match self.wait {
            Wait::Map => {
                if let Some((r, _)) = self.map_buttons().into_iter().find(|b| inside(b.1, x, y)) {
                    self.log(&format!("map: {r}"));
                    self.map_up = false;
                    self.stack.push(V::S(r));
                    self.wait = Wait::None;
                }
            }
            Wait::Title => match Self::title_rects().find(|b| inside(b.2, x, y)).map(|b| b.0) {
                Some("start") => {
                    self.title = false;
                    self.stack.push(V::S("start".into()));
                    self.wait = Wait::None;
                }
                Some("resume" | "load") => self.view = View::Slots { save: false },
                Some("exit") => self.ended = true,
                _ => {}
            },
            Wait::Select => {
                let n = self.select.as_ref().map_or(0, Vec::len);
                if let Some(i) = Self::select_rects(n).iter().position(|r| inside(*r, x, y)) {
                    self.chosen = i as i32 + 1;
                    self.select = None;
                    self.stack.push(V::I(0));
                    self.wait = Wait::None;
                }
            }
            _ => {
                self.skip = false;
                self.advance();
            }
        }
    }

    /// A right click: our save slots (load only away from a message), or back.
    pub fn cancel(&mut self) {
        if self.backlog.is_open() {
            self.backlog.close();
            return;
        }
        self.view = match self.view {
            View::Game if self.wait == Wait::Click && self.window => View::Slots { save: true },
            View::Game => View::Slots { save: false },
            _ => View::Game,
        };
    }

    /// A key: 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip.
    pub fn key(&mut self, k: usize) {
        match k {
            4 if self.view == View::Game && !matches!(self.wait, Wait::Title | Wait::Select) => {
                self.skip = false;
                self.advance();
            }
            5 => self.cancel(),
            6 if !matches!(self.wait, Wait::Title) => self.skip = !self.skip,
            _ => {}
        }
    }

    /// The wheel: down goes forward.
    pub fn wheel(&mut self, up: bool) {
        // turned up: the backlog
        if self.backlog.is_open() || up {
            self.log_page();
            if self.backlog.is_open()
                || (self.view == View::Game && matches!(self.wait, Wait::Click) && self.window)
            {
                self.backlog.wheel(up);
                return;
            }
        }
        if !up && self.view == View::Game && self.wait == Wait::Click {
            self.advance();
        }
    }

    // ---- drawing ----

    fn whole(img: &Rc<Image>, x: f32, y: f32, alpha: f32) -> Draw {
        Draw {
            image: img.clone(),
            src: [0, 0, img.w as i32, img.h as i32],
            dst: [x, y, x + img.w as f32, y + img.h as f32],
            place: place(alpha),
        }
    }

    fn part(img: &Rc<Image>, src: [i32; 4], x: f32, y: f32, alpha: f32) -> Draw {
        Draw {
            image: img.clone(),
            src,
            dst: [
                x,
                y,
                x + (src[2] - src[0]) as f32,
                y + (src[3] - src[1]) as f32,
            ],
            place: place(alpha),
        }
    }

    fn solid(rgb: [u8; 3], alpha: f32, r: [i32; 4]) -> Draw {
        let mut img = Image::new(1, 1);
        img.px.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        Draw {
            image: Rc::new(img),
            src: [0, 0, 1, 1],
            dst: r.map(|v| v as f32),
            place: place(alpha),
        }
    }

    /// A frame of a sprite sheet `{sheet}.png` named `{sheet}_{n}`.
    fn sheet_draw(&mut self, sheet: &str, n: i32, x: f32, y: f32) -> Option<Draw> {
        let s = self.sheet(sheet);
        let r = *s.get(&format!("{sheet}_{n}"))?;
        let img = self.image(&format!("{sheet}.png"))?;
        Some(Self::part(&img, r, x, y, 1.0))
    }

    fn scene_draws(&mut self) -> Vec<Draw> {
        let scene = self.shown.clone();
        let mut out = Vec::new();
        if let Some(bg) = &scene.bg
            && let Some(img) = self.image(bg)
        {
            out.push(Self::whole(&img, 0.0, 0.0, 1.0));
        }
        for (file, x, y) in scene.subs.values() {
            if let Some(img) = self.image(file) {
                out.push(Self::whole(&img, *x as f32, *y as f32, 1.0));
            }
        }
        for c in scene.chars.values() {
            let Some(def) = self.char_def(&c.file) else {
                continue;
            };
            let dx = (c.x - def.w / 2) as f32;
            for (id, x, y) in &def.base {
                if let Some(img) = self.image(&format!("{id}.png")) {
                    out.push(Self::whole(&img, dx + *x as f32, *y as f32, 1.0));
                }
            }
            if let Some((part, n)) = &c.face
                && let Some((x, y, sheet)) = def.subs.get(part)
                && let Some(d) = self.sheet_draw(sheet, *n, dx + *x as f32, *y as f32)
            {
                out.push(d);
            }
        }
        out
    }

    fn text_img(&mut self, s: &str, size: i32) -> Rc<Image> {
        let key = (s.to_string(), size);
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let img = match &self.font {
            Some(f) => Rc::new(Font::outline(f.clone(), size).render(
                &nexas::sjis::encode(s),
                [255, 255, 255],
                3,
            )),
            None => Rc::new(Image::new(1, 1)),
        };
        if self.text_cache.len() > 300 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, img.clone());
        img
    }

    fn text_draw(&mut self, s: &str, size: i32, x: i32, y: i32, out: &mut Vec<Draw>) {
        let img = self.text_img(s, size);
        out.push(Self::whole(&img, x as f32, y as f32, 1.0));
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = self.draws_inner();
        if self.backlog.is_open() {
            let log = std::mem::take(&mut self.backlog);
            let (w, h) = (W, H);
            let size = (h as i32 / 27).max(16);
            out.extend(log.draws(w, h, size, &mut |s, rgb| {
                let _ = rgb;
                self.text_img(s, size)
            }));
            self.backlog = log;
        }
        out
    }

    /// Keeps the message waiting for a click in the backlog (vncommon::backlog).
    fn log_page(&mut self) {
        if !(self.view == View::Game && matches!(self.wait, Wait::Click) && self.window) {
            return;
        }
        let (name, text) = (self.name.clone(), self.text.clone());
        self.backlog.push(name.trim(), &text);
    }

    fn draws_inner(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        if self.title {
            self.title_draws(&mut out);
        } else if self.map_up {
            self.map_draws(&mut out);
        } else {
            let mut scene = self.scene_draws();
            if let Some(t) = &self.trans {
                let k = t.t as f32 / t.dur.max(1) as f32;
                out.extend(t.old.iter().cloned());
                for d in &mut scene {
                    d.place.alpha *= k;
                }
            }
            out.extend(scene);
            if let Some(s) = &self.shake {
                let k = (s.t as f32 * 1.7).sin() * (1.0 - s.t as f32 / s.dur.max(1) as f32);
                for d in &mut out {
                    for i in [0, 2] {
                        d.dst[i] += s.ax * k;
                        d.dst[i + 1] += s.ay * k;
                    }
                }
            }
            let a = self.fade.alpha();
            if a > 0.0 {
                out.push(Self::solid(self.fade.rgb, a, [0, 0, W as i32, H as i32]));
            }
            if self.window && a < 1.0 {
                self.window_draws(&mut out);
            }
            if self.select.is_some() {
                self.select_draws(&mut out);
            }
        }
        if let View::Slots { save } = self.view {
            self.slot_draws(save, &mut out);
        }
        out
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        if let Some(img) = self.image("meswnd_base.png") {
            out.push(Self::whole(&img, 0.0, WIN_Y as f32, 1.0));
        } else {
            out.push(Self::solid([0, 0, 0], 0.6, [0, WIN_Y, W as i32, H as i32]));
        }
        if !self.name.is_empty() {
            let n = self.name.clone();
            self.text_draw(&n, TEXT_SIZE, NAME_AT.0, NAME_AT.1, out);
        }
        let (x, mut y) = TEXT_AT;
        for line in wrap(&self.text.clone(), TEXT_COLS) {
            self.text_draw(&line, TEXT_SIZE, x, y, out);
            y += TEXT_LH;
        }
    }

    fn select_draws(&mut self, out: &mut Vec<Draw>) {
        let items = self.select.clone().unwrap_or_default();
        let (px, py) = self.pointer;
        for (r, label) in Self::select_rects(items.len()).into_iter().zip(items) {
            let lit = inside(r, px, py) as i32;
            match self.sheet_draw("select_select", lit, r[0] as f32, r[1] as f32) {
                Some(d) => out.push(d),
                None => out.push(Self::solid([30, 30, 60], 0.85, r)),
            }
            let img = self.text_img(&label, TEXT_SIZE);
            let x = r[0] + (SELECT_W - img.w as i32) / 2;
            let y = r[1] + (SELECT_H - img.h as i32) / 2;
            out.push(Self::whole(&img, x as f32, y as f32, 1.0));
        }
    }

    fn map_draws(&mut self, out: &mut Vec<Draw>) {
        if let Some(img) = self.image("map_base.png") {
            out.push(Self::whole(&img, 0.0, 0.0, 1.0));
        }
        let (px, py) = self.pointer;
        let lit = |r: [i32; 4]| inside(r, px, py) as i32;
        for (p, &(x, y, w, h, _)) in PLACES.iter().enumerate() {
            let on = lit([x, y, x + w, y + h]);
            if let Some(d) = self.sheet_draw(&format!("map_{p}"), on, x as f32, y as f32) {
                out.push(d);
            }
        }
        for (p, &(x, y, _, _, icons)) in PLACES.iter().enumerate() {
            let slots = self.map_slots(p);
            for (k, &(ch, flag)) in slots.iter().enumerate() {
                let (dx, dy) = ICONS[icons][if slots.len() == 1 { 0 } else { k + 1 }];
                let (ix, iy) = (x + dx, y + dy);
                let sheet = if flag == 1 { "map_ev" } else { "map_tk" };
                let n = ch.max(0) * 3 + lit([ix, iy, ix + ICON, iy + ICON]);
                if let Some(d) = self.sheet_draw(sheet, n, ix as f32, iy as f32) {
                    out.push(d);
                }
            }
        }
        if let Some(d) = self.sheet_draw("map_work", lit(WORK), WORK[0] as f32, WORK[1] as f32) {
            out.push(d);
        }
    }

    fn title_draws(&mut self, out: &mut Vec<Draw>) {
        if let Some(img) = self.image("title_tr_bg.png") {
            out.push(Self::whole(&img, 0.0, 0.0, 1.0));
        }
        if let Some(img) = self.image("title_tr_logo.png") {
            out.push(Self::whole(&img, 891.0, 772.0, 1.0));
        }
        let (px, py) = self.pointer;
        for (_, pic, r) in Self::title_rects() {
            let sheet = format!("title_tr_{pic}");
            let lit = inside(r, px, py) as i32;
            if let Some(d) = self.sheet_draw(&sheet, lit, r[0] as f32, r[1] as f32) {
                out.push(d);
            }
        }
    }

    // ---- saves ----

    fn slot_rects() -> Vec<[i32; 4]> {
        (0..SLOTS as i32)
            .map(|i| {
                let (c, r) = (i % 2, i / 2);
                let (x, y) = (90 + c * 885, 190 + r * 140);
                [x, y, x + 855, y + 120]
            })
            .collect()
    }

    /// The SAVE and LOAD tabs.
    fn tab_rects() -> [[i32; 4]; 2] {
        [[90, 60, 370, 150], [400, 60, 680, 150]]
    }

    fn can_save(&self) -> bool {
        self.wait == Wait::Click && self.window && !self.title
    }

    fn slots_click(&mut self, save: bool, x: i32, y: i32) {
        let tabs = Self::tab_rects();
        if inside(tabs[0], x, y) {
            if self.can_save() {
                self.view = View::Slots { save: true };
            }
            return;
        }
        if inside(tabs[1], x, y) {
            self.view = View::Slots { save: false };
            return;
        }
        match Self::slot_rects().iter().position(|r| inside(*r, x, y)) {
            Some(i) if save => {
                self.save(i);
                self.view = View::Game;
            }
            Some(i) => {
                if self.load(i) {
                    self.view = View::Game;
                }
            }
            None => self.view = View::Game,
        }
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        out.push(Self::solid([20, 10, 30], 0.9, [0, 0, W as i32, H as i32]));
        let (px, py) = self.pointer;
        for (i, (r, label)) in Self::tab_rects()
            .into_iter()
            .zip(["SAVE", "LOAD"])
            .enumerate()
        {
            let c = if save == (i == 0) {
                [200, 40, 60]
            } else {
                [80, 30, 50]
            };
            out.push(Self::solid(c, 0.95, r));
            self.text_draw(label, 48, r[0] + 80, r[1] + 18, out);
        }
        for (i, r) in Self::slot_rects().into_iter().enumerate() {
            let c = if inside(r, px, py) {
                [200, 40, 60]
            } else {
                [90, 40, 70]
            };
            out.push(Self::solid(c, 0.9, r));
            let label = match self.read_slot(i) {
                Some(s) => format!("{:02}  {}", i + 1, s.label),
                None => format!("{:02}  ----", i + 1),
            };
            self.text_draw(&label, 36, r[0] + 24, r[1] + 38, out);
        }
    }

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&format!("slot{i:02}.json"))?;
        serde_json::from_slice(&b).ok()
    }

    fn save(&mut self, i: usize) {
        if !self.can_save() {
            return;
        }
        let chapter = self.globals.get(&252).map(V::text).unwrap_or_default();
        let t: String = self
            .text
            .replace('\n', " ")
            .trim()
            .chars()
            .take(14)
            .collect();
        let s = Slot {
            label: format!("{chapter} {t}").trim().to_string(),
            calls: self
                .calls
                .iter()
                .map(|f| SavedFrame {
                    script: f.script.name.clone(),
                    pc: f.pc,
                    store: f.store.clone(),
                    args: f.args.clone(),
                    func: f.func,
                    base: f.base,
                })
                .collect(),
            stores: self
                .stores
                .iter()
                .map(|(k, m)| (k.clone(), sorted(m)))
                .collect(),
            stack: self.stack.clone(),
            globals: sorted(&self.globals),
            others: sorted(&self.others),
            flags: self.flags,
            scene: self.shown.clone(),
            fade: (self.fade.rgb, self.fade.to),
            bgm: self.bgm.clone(),
            name: self.name.clone(),
            text: self.text.clone(),
        };
        match serde_json::to_vec(&s) {
            Ok(b) => self.files.write_save(&format!("slot{i:02}.json"), &b),
            Err(e) => self.warn(format!("save: {e}")),
        }
    }

    fn load(&mut self, i: usize) -> bool {
        let Some(s) = self.read_slot(i) else {
            return false;
        };
        let mut calls = Vec::new();
        for f in s.calls {
            match self.script(&f.script) {
                Ok(script) => calls.push(Frame {
                    script,
                    pc: f.pc,
                    store: f.store,
                    args: f.args,
                    func: f.func,
                    base: f.base,
                }),
                Err(e) => {
                    self.warn(e);
                    return false;
                }
            }
        }
        self.calls = calls;
        self.stores = s
            .stores
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();
        self.retval = None;
        self.map_up = false;
        self.stack = s.stack;
        self.globals = s.globals.into_iter().collect();
        self.others = s.others.into_iter().collect();
        self.flags = s.flags;
        self.stage = s.scene.clone();
        self.shown = s.scene;
        self.trans = None;
        self.shake = None;
        self.fade = Fade {
            rgb: s.fade.0,
            from: s.fade.1,
            to: s.fade.1,
            t: 0,
            dur: 0,
        };
        match s.bgm {
            Some(b) => {
                self.bgm = None;
                self.play_bgm(&b);
            }
            None => {
                if self.bgm.take().is_some() {
                    self.stop("bgm", 0.0);
                }
            }
        }
        self.stop("voice", 0.0);
        self.name = s.name;
        self.text = s.text;
        self.name_buf.clear();
        self.text_buf.clear();
        self.choices.clear();
        self.select = None;
        self.title = false;
        self.window = true;
        self.skip = false;
        self.wait = Wait::Click;
        true
    }

    /// The whole screen as one picture (for the native runner).
    pub fn render(&mut self) -> Image {
        let mut img = Image::new(W, H);
        img.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    /// What is on screen, for the runner's log: the script, the message.
    pub fn status(&self) -> String {
        let at = self
            .calls
            .last()
            .map(|f| format!("{}@{:x}", f.script.name, f.pc))
            .unwrap_or_default();
        format!("{at} {:?} [{}] {}", self.wait, self.name, self.text)
    }
}

fn sorted(m: &HashMap<i32, V>) -> Vec<(i32, V)> {
    let mut v: Vec<(i32, V)> = m.iter().map(|(k, v)| (*k, v.clone())).collect();
    v.sort_by_key(|x| x.0);
    v
}

#[derive(Serialize, Deserialize)]
struct SavedFrame {
    script: String,
    pc: usize,
    store: String,
    args: Vec<V>,
    func: bool,
    base: usize,
}

#[derive(Serialize, Deserialize)]
struct Slot {
    label: String,
    calls: Vec<SavedFrame>,
    stores: BTreeMap<String, Vec<(i32, V)>>,
    stack: Vec<V>,
    globals: Vec<(i32, V)>,
    others: Vec<(i32, V)>,
    flags: u32,
    scene: Scene,
    fade: ([u8; 3], f32),
    bgm: Option<String>,
    name: String,
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_printf() {
        assert_eq!(format("%s", &[V::S("竜二".into())]), "竜二");
        assert_eq!(
            format("%s%d", &[V::S("a".into()), V::I(3)]),
            "a3".to_string()
        );
        assert_eq!(format("%03d%%", &[V::I(7)]), "007%");
    }

    #[test]
    fn wraps_with_closing_marks() {
        let w = wrap("あいう。\nえ", 3);
        assert_eq!(w, vec!["あいう。".to_string(), "え".to_string()]);
    }

    #[test]
    fn compares_strings_and_numbers() {
        assert_eq!(
            Engine::bin(0x42, V::S("fi".into()), V::S("fo".into())),
            V::I(0)
        );
        assert_eq!(Engine::bin(0x3e, V::I(1), V::I(2)), V::I(1));
        assert_eq!(
            Engine::bin(0x34, V::S("ryu".into()), V::S(".script".into())),
            V::S("ryu.script".into())
        );
    }
}
