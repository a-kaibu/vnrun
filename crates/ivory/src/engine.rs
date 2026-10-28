//! Runs the `.OCB` scripts from `START.OCB` a frame at a time. What the instructions do was
//! read from how the scripts use them:
//!
//! - `01 id text` narration, `02 id name text voice` a spoken line (cTEX offsets, the voice a
//!   cFNM offset or -1); `\xffs` and `\xffn` in a text break the line
//! - `03 _ music _ _` plays music, `04 _ seconds` stops it; `05 se _` a sound, `1d _` stops
//!   them; `16 channel sound _ _` loops a sound on a channel, `17 channel _` stops it
//! - `06 picture instant` a new background without the other layers, `15 picture _` only the
//!   background; `8e layer picture _` a layer's
//!   picture (-1: none); `95`/`96 layer|offset<<16 seconds` move a layer across or down
//! - `98` waits for the pictures' fades, `94 _` for the moves; `10`/`1c seconds` wait
//! - `0a target` jumps (a code offset or a script), `0b script` calls, `0c` returns,
//!   `0d variable 0xa value target` jumps when the variable is not the value, `14 variable 5
//!   value` sets one (variables are four letters as a u32; the values floats)
//! - `09 id count (text target)...` a choice, `43` the title menu, `25 1/0` and `35 1/0` hide the
//!   window, `37` closes it
//!
//! Pictures are `SG` files with their own place on the screen. The message window, the title,
//! the choices and the save slots are drawn here from the game's `FRAME_*`, `TITLE_*`,
//! `SELECT_*` and `SAVE_BASE`/`LOAD_BASE` pictures.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::ocb::{Ocb, Target};
use crate::pk::Packs;

pub const W: u32 = 1280;
pub const H: u32 = 720;
const SLOTS: usize = 12;
/// the layer of the background
const BG: i32 = 0;

#[derive(Clone, Copy, PartialEq)]
enum Act {
    None,
    New,
    Save,
    Load,
    QSave,
    QLoad,
    Skip,
    Quit,
}

/// How a game lays out its window and title.
struct Skin {
    /// the title's picture, its buttons and their hovered look
    title: &'static str,
    title_buttons: &'static str,
    title_hover: &'static str,
    title_acts: &'static [Act],
    buttons: &'static [Act],
    /// the most space between the letters of a window button's label
    gap: i32,
    text: (i32, i32),
    size: i32,
    line: i32,
    wrap: usize,
}

/// お嬢様はイイナリみたいです!? (2016): pictures in folders, a column of title buttons
const SKIN_2016: Skin = Skin {
    title: "TITLE_BASE_TRIAL",
    title_buttons: "TITLE_BUTTON_NORMAL",
    title_hover: "TITLE_BUTTON_ROLLOVER",
    title_acts: &[
        Act::New,
        Act::None,
        Act::Load,
        Act::None,
        Act::None,
        Act::Quit,
    ],
    buttons: &[
        Act::None,
        Act::QSave,
        Act::QLoad,
        Act::Save,
        Act::Load,
        Act::None,
        Act::None,
        Act::Skip,
        Act::None,
        Act::None,
    ],
    gap: 0,
    text: (170, 598),
    size: 26,
    line: 34,
    wrap: 38,
};

/// later games: a row of title buttons
const SKIN_2020: Skin = Skin {
    title: "TITLE_BASE",
    title_buttons: "TITLE_BUTTON_NORMAL",
    title_hover: "TITLE_BUTTON_DOWN",
    title_acts: &[
        Act::New,
        Act::Load,
        Act::None,
        Act::None,
        Act::None,
        Act::None,
        Act::Quit,
    ],
    buttons: &[
        Act::QSave,
        Act::QLoad,
        Act::Save,
        Act::Load,
        Act::Skip,
        Act::None,
        Act::None,
        Act::None,
        Act::None,
        Act::None,
    ],
    gap: 15,
    text: (180, 522),
    size: 28,
    line: 42,
    wrap: 34,
};

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

/// A picture placed where its file says.
#[derive(Clone)]
struct Pic {
    name: String,
    img: Rc<Image>,
    x: i32,
    y: i32,
}

/// A move of a layer's offset: from, to, frames, frames done.
type Move = (f32, f32, u32, u32);

#[derive(Clone, Default)]
struct Layer {
    cur: Option<Pic>,
    /// the picture fading out
    old: Option<Pic>,
    /// the fade: frames done, frames
    t: u32,
    dur: u32,
    dx: f32,
    dy: f32,
    mx: Option<Move>,
    my: Option<Move>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Wait {
    None,
    Click,
    Frames(u32),
    Fades,
    Moves,
    Title,
    Choice,
    Ended,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Game,
    Slots { save: bool },
}

pub struct Engine {
    /// the messages read so far (the wheel turned up)
    backlog: vncommon::backlog::Backlog,
    files: Box<dyn Files>,
    packs: Packs,
    scripts: HashMap<String, Rc<Ocb>>,
    pics: HashMap<String, Option<Pic>>,
    /// the script running, its next instruction, and the calls to return to
    script: String,
    code: Rc<Ocb>,
    pc: usize,
    stack: Vec<(String, usize)>,
    /// the message shown: its script and offset (where a load starts again)
    msg_at: (String, usize),
    vars: BTreeMap<u32, f32>,
    layers: BTreeMap<i32, Layer>,
    text: String,
    name: Option<String>,
    window: bool,
    /// the window hidden by `25 1` and by `35 1` (the scripts' system parts: logos, eyecatches)
    hidden: [bool; 2],
    wait: Wait,
    view: View,
    choices: Vec<(String, u32)>,
    pointer: (i32, i32),
    sounds: Vec<SoundEvent>,
    bgm: Option<String>,
    loops: BTreeMap<u32, String>,
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<(String, i32), Rc<Image>>,
    skip: bool,
    skin: &'static Skin,
    pub trace: bool,
    pub frames: u64,
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

/// A variable's name as the scripts write it: up to four letters as a u32.
const fn var(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

/// the fade times of the pictures (out and in, seconds), set around a change; 0 is the default
const FADE_OUT: u32 = var(b"FLG\xf2");
const FADE_IN: u32 = var(b"FLG\xf3");
const FADE_DEFAULT: f32 = 0.4;

/// The runs of a picture's opaque columns (`across`) or rows: the buttons drawn in it. Runs
/// less than `gap` apart are one button (the letters of a word).
fn runs(img: &Image, across: bool, gap: i32) -> Vec<(i32, i32)> {
    let (n, m) = if across {
        (img.w, img.h)
    } else {
        (img.h, img.w)
    };
    let on = |i: u32| {
        (0..m).any(|j| {
            let (x, y) = if across { (i, j) } else { (j, i) };
            img.px[((y * img.w + x) * 4 + 3) as usize] > 40
        })
    };
    let mut out = Vec::new();
    let mut start = None;
    for i in 0..=n {
        let v = i < n && on(i);
        match (v, start) {
            (true, None) => match out.last() {
                Some(&(s, e)) if i as i32 - e < gap => {
                    out.pop();
                    start = Some(s);
                }
                _ => start = Some(i as i32),
            },
            (false, Some(s)) => {
                out.push((s, i as i32));
                start = None;
            }
            _ => {}
        }
    }
    out
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let packs = Packs::open(files.as_mut());
        let skin = if packs.entry(&format!("{}.SG", SKIN_2016.title)).is_some() {
            &SKIN_2016
        } else {
            &SKIN_2020
        };
        let mut e = Engine {
            backlog: Default::default(),
            files,
            packs,
            scripts: HashMap::new(),
            pics: HashMap::new(),
            script: String::new(),
            code: Rc::new(Ocb::parse(b"fAGS\0\0\0\0cCOD\x10\0\0\0\x0c\0\0\0\0\0\0\0")?),
            pc: 0,
            stack: Vec::new(),
            msg_at: (String::new(), 0),
            vars: BTreeMap::new(),
            layers: BTreeMap::new(),
            text: String::new(),
            name: None,
            window: false,
            hidden: [false; 2],
            wait: Wait::None,
            view: View::Game,
            choices: Vec::new(),
            pointer: (0, 0),
            sounds: Vec::new(),
            bgm: None,
            loops: BTreeMap::new(),
            font: None,
            text_cache: HashMap::new(),
            skip: false,
            skin,
            trace: std::env::var("IVORY_LOG").is_ok(),
            frames: 0,
        };
        if !e.goto_script("start") {
            return Err("START.OCB not found".into());
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

    fn log(&self, s: &str) {
        if self.trace {
            eprintln!("[{}] {s}", self.frames);
        }
    }

    pub fn ended(&self) -> bool {
        self.wait == Wait::Ended
    }

    fn load_script(&mut self, name: &str) -> Option<Rc<Ocb>> {
        let key = name.to_lowercase();
        if let Some(s) = self.scripts.get(&key) {
            return Some(s.clone());
        }
        let b = self
            .packs
            .read(self.files.as_mut(), &format!("{name}.OCB"))?;
        let s = Rc::new(Ocb::parse(&b).ok()?);
        self.scripts.insert(key, s.clone());
        Some(s)
    }

    fn goto_script(&mut self, name: &str) -> bool {
        match self.load_script(name) {
            Some(s) => {
                self.log(&format!("script {name}"));
                self.code = s;
                self.script = name.to_lowercase();
                self.pc = 0;
                true
            }
            None => {
                self.log(&format!("{name}: no such script"));
                false
            }
        }
    }

    fn jump(&mut self, t: Target) {
        match t {
            Target::Offset(o) => self.pc = o,
            Target::Script(s) => {
                if !self.goto_script(&s) {
                    self.wait = Wait::Ended;
                }
            }
        }
    }

    fn ret(&mut self) {
        match self.stack.pop() {
            Some((s, pc)) => {
                if self.goto_script(&s) {
                    self.pc = pc;
                }
            }
            None => self.wait = Wait::Ended,
        }
    }

    fn pic(&mut self, name: &str) -> Option<Pic> {
        let key = name.to_lowercase();
        if let Some(p) = self.pics.get(&key) {
            return p.clone();
        }
        let p = self
            .packs
            .read(self.files.as_mut(), &format!("{name}.SG"))
            .and_then(|b| match crate::sg::decode(&b) {
                Ok(sg) => Some(Pic {
                    name: name.to_string(),
                    img: Rc::new(sg.img),
                    x: sg.x,
                    y: sg.y,
                }),
                Err(e) => {
                    self.log(&format!("{name}: {e}"));
                    None
                }
            });
        if p.is_none() {
            self.log(&format!("{name}.SG: not found"));
        }
        if self.pics.len() > 48 {
            self.pics.clear();
        }
        self.pics.insert(key, p.clone());
        p
    }

    fn audio(&mut self, name: &str) -> Option<Vec<u8>> {
        let b = self
            .packs
            .read(self.files.as_mut(), &format!("{name}.PX"))?;
        // fPX, then a cTRK section whose header holds the format; the data is Ogg Vorbis
        let head = u32::from_le_bytes(b.get(20..24)?.try_into().ok()?) as usize;
        Some(b.get(8 + head..)?.to_vec())
    }

    fn play(&mut self, channel: &str, name: &str, repeat: bool, volume: f32) {
        match self.audio(name) {
            Some(d) => self.sounds.push(SoundEvent {
                channel: channel.into(),
                data: Some(d),
                volume,
                repeat,
                fade_ms: 0.0,
            }),
            None => self.log(&format!("{name}.PX: not found")),
        }
    }

    fn stop(&mut self, channel: &str, fade_ms: f32) {
        self.sounds.push(SoundEvent {
            channel: channel.into(),
            data: None,
            volume: 0.0,
            repeat: false,
            fade_ms,
        });
    }

    fn fade_frames(&self) -> u32 {
        if self.skip {
            return 0;
        }
        let v = |k| self.vars.get(&k).copied().unwrap_or(0.0);
        let s = v(FADE_OUT).max(v(FADE_IN));
        let s = if s > 0.0 { s } else { FADE_DEFAULT };
        (s * 60.0) as u32
    }

    fn set_layer(&mut self, k: i32, name: Option<String>, instant: bool) {
        let pic = name.and_then(|n| self.pic(&n));
        let dur = if instant { 0 } else { self.fade_frames() };
        let l = self.layers.entry(k).or_default();
        l.old = if dur > 0 { l.cur.take() } else { None };
        l.cur = pic;
        l.t = 0;
        l.dur = dur;
    }

    fn move_layer(&mut self, a: u32, secs: f32, across: bool) {
        let k = (a & 0xffff) as i32;
        let to = (a >> 16) as u16 as i16 as f32;
        let frames = if self.skip {
            0
        } else {
            (secs.max(0.0) * 60.0) as u32
        };
        let l = self.layers.entry(k).or_default();
        let from = if across { l.dx } else { l.dy };
        let m = Some((from, to, frames, 0));
        if across {
            l.mx = m;
        } else {
            l.my = m;
        }
    }

    fn tick_layers(&mut self) {
        let step = |v: &mut f32, m: &mut Option<Move>| {
            if let Some((from, to, n, done)) = m {
                *done += 1;
                if *done >= *n {
                    *v = *to;
                    *m = None;
                } else {
                    *v = *from + (*to - *from) * *done as f32 / *n as f32;
                }
            }
        };
        for l in self.layers.values_mut() {
            if l.t < l.dur {
                l.t += 1;
            }
            if l.t >= l.dur {
                l.old = None;
            }
            step(&mut l.dx, &mut l.mx);
            step(&mut l.dy, &mut l.my);
        }
    }

    fn fading(&self) -> bool {
        self.layers.values().any(|l| l.t < l.dur)
    }

    fn moving(&self) -> bool {
        self.layers
            .values()
            .any(|l| l.mx.is_some() || l.my.is_some())
    }

    pub fn frame(&mut self) -> Result<(), String> {
        self.log_page();
        self.frames += 1;
        self.tick_layers();
        if self.skip {
            for l in self.layers.values_mut() {
                l.t = l.dur;
                l.old = None;
            }
        }
        if self.view != View::Game {
            return Ok(());
        }
        self.wait = match self.wait {
            Wait::Frames(n) if n <= 1 || self.skip => Wait::None,
            Wait::Frames(n) => Wait::Frames(n - 1),
            Wait::Fades if !self.fading() => Wait::None,
            Wait::Moves if !self.moving() && !self.fading() => Wait::None,
            Wait::Click if self.skip && self.frames.is_multiple_of(3) => {
                self.advance();
                Wait::None
            }
            w => w,
        };
        let mut n = 0;
        while self.wait == Wait::None && self.view == View::Game {
            n += 1;
            if n > 20000 {
                return Err(format!("{}: no stop at {:#x}", self.script, self.pc));
            }
            self.step();
        }
        Ok(())
    }

    fn step(&mut self) {
        let at = self.pc;
        let Some(i) = self.code.ins(at) else {
            self.ret();
            return;
        };
        self.pc = i.next;
        let a = |k: usize| i.args.get(k).copied().unwrap_or(u32::MAX);
        let f = |k: usize| f32::from_bits(a(k));
        let code = self.code.clone();
        if self.trace {
            let args: Vec<String> = i.args.iter().map(|v| format!("{v:x}")).collect();
            self.log(&format!(
                "{}:{at:05x} {:02x} {}",
                self.script,
                i.op,
                args.join(" ")
            ));
        }
        match i.op {
            0x00 => self.ret(),
            0x01 | 0x02 => {
                let (name, text, voice) = if i.op == 1 {
                    (None, code.tex(a(1)), None)
                } else {
                    (Some(code.tex(a(1))), code.tex(a(2)), code.name(a(3)))
                };
                self.stop("voice", 0.0);
                if let Some(v) = voice {
                    self.play("voice", &v, false, 1.0);
                }
                // 0xff then `s` or `n` breaks the line (0xff decodes as U+FFFD)
                self.text = text
                    .replace("\u{fffd}s", "\n")
                    .replace("\u{fffd}n", "\n")
                    .replace('\u{fffd}', "");
                self.name = name.filter(|n| !n.is_empty());
                self.window = true;
                self.msg_at = (self.script.clone(), at);
                self.wait = Wait::Click;
            }
            0x03 => {
                if let Some(n) = code.name(a(1)) {
                    self.play("bgm", &n, true, 0.6);
                    self.bgm = Some(n);
                }
            }
            0x04 => {
                let s = if a(1) == u32::MAX { 1.0 } else { f(1) };
                self.stop("bgm", s * 1000.0);
                self.bgm = None;
            }
            0x05 => {
                if let Some(n) = code.name(a(0)) {
                    self.play("se", &n, false, 0.8);
                }
            }
            0x1d => self.stop("se", 0.0),
            0x16 => {
                if let Some(n) = code.name(a(1)) {
                    self.play(&format!("loop{}", a(0)), &n, true, 0.8);
                    self.loops.insert(a(0), n);
                }
            }
            0x17 => {
                let chans: Vec<u32> = if a(0) == u32::MAX {
                    self.loops.keys().copied().collect()
                } else {
                    vec![a(0)]
                };
                for c in chans {
                    self.stop(&format!("loop{c}"), 500.0);
                    self.loops.remove(&c);
                }
            }
            0x06 => {
                // a new scene: the window and the other layers go too
                let instant = a(1) == 1;
                self.window = false;
                let shown: Vec<i32> = self
                    .layers
                    .iter()
                    .filter(|(k, l)| **k != BG && l.cur.is_some())
                    .map(|(k, _)| *k)
                    .collect();
                for k in shown {
                    self.set_layer(k, None, instant);
                }
                self.set_layer(BG, code.name(a(0)), instant);
            }
            0x15 => self.set_layer(BG, code.name(a(0)), false),
            0x8e => self.set_layer(a(0) as i32, code.name(a(1)), false),
            0x95 | 0x96 => self.move_layer(a(0), f(1), i.op == 0x95),
            0x94 => self.wait = Wait::Moves,
            0x98 => self.wait = Wait::Fades,
            0x10 | 0x1c => self.wait = Wait::Frames((f(0).max(0.0) * 60.0) as u32),
            0x14 => {
                if a(1) == 5 {
                    self.vars.insert(a(0), f(2));
                } else {
                    self.log(&format!("14: mode {}", a(1)));
                }
            }
            0x0d => {
                let v = self.vars.get(&a(0)).copied().unwrap_or(0.0);
                let x = f(2);
                let jump = match a(1) {
                    9 => v == x,
                    0xa => v != x,
                    c => {
                        self.log(&format!("0d: comparison {c}"));
                        false
                    }
                };
                if jump {
                    self.jump(code.target(a(3)));
                }
            }
            0x0a => self.jump(code.target(a(0))),
            0x0b => {
                self.stack.push((self.script.clone(), self.pc));
                self.jump(code.target(a(0)));
            }
            0x0c => self.ret(),
            0x09 => {
                let n = a(1) as usize;
                self.choices = (0..n)
                    .map(|k| (code.tex(a(2 + 2 * k)), a(3 + 2 * k)))
                    .collect();
                self.msg_at = (self.script.clone(), at);
                self.wait = Wait::Choice;
            }
            0x25 => self.hidden[0] = a(0) != 0,
            0x35 => self.hidden[1] = a(0) != 0,
            0x37 => {
                self.window = false;
                self.text.clear();
                self.name = None;
            }
            0x43 => {
                self.window = false;
                self.wait = Wait::Title;
            }
            _ => {}
        }
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    fn advance(&mut self) {
        if self.wait == Wait::Click {
            self.wait = Wait::None;
            self.text.clear();
            self.name = None;
        }
    }

    fn pick(&mut self, i: usize) {
        let Some(&(_, t)) = self.choices.get(i) else {
            return;
        };
        self.choices.clear();
        self.wait = Wait::None;
        let t = self.code.target(t);
        self.jump(t);
    }

    fn act(&mut self, a: Act) {
        match a {
            Act::New => {
                self.wait = Wait::None;
                self.skip = false;
            }
            Act::Save => self.view = View::Slots { save: true },
            Act::Load => self.view = View::Slots { save: false },
            Act::QSave => self.save(0),
            Act::QLoad => self.load(0),
            Act::Skip => self.skip = !self.skip,
            Act::Quit => self.wait = Wait::Ended,
            Act::None => {}
        }
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if self.backlog.is_open() {
            self.backlog.close();
            return;
        }
        self.pointer = (x, y);
        if let View::Slots { save } = self.view {
            match self.slot_rects().into_iter().position(|r| inside(r, x, y)) {
                Some(i) if save => {
                    self.save(i);
                    self.view = View::Game;
                }
                Some(i) => self.load(i),
                None => self.view = View::Game,
            }
            return;
        }
        match self.wait {
            Wait::Title => {
                let hit = self
                    .title_buttons()
                    .into_iter()
                    .position(|(_, r)| inside(r, x, y));
                if let Some(a) = hit.and_then(|i| self.skin.title_acts.get(i)) {
                    self.act(*a);
                }
            }
            Wait::Choice => {
                if let Some(i) = self
                    .choice_rects()
                    .into_iter()
                    .position(|r| inside(r, x, y))
                {
                    self.pick(i);
                }
            }
            Wait::Ended => {}
            w => {
                // the window's buttons work while it shows, also during a fade or a wait
                let shown = self.window && self.hidden == [false; 2];
                let hit = if shown {
                    self.window_buttons()
                        .into_iter()
                        .position(|(_, r)| inside(r, x, y))
                } else {
                    None
                };
                match hit.and_then(|i| self.skin.buttons.get(i)) {
                    Some(a) => self.act(*a),
                    None if w == Wait::Click => {
                        self.skip = false;
                        self.advance();
                    }
                    None => {}
                }
            }
        }
    }

    pub fn cancel(&mut self) {
        if self.backlog.is_open() {
            self.backlog.close();
            return;
        }
        if self.view != View::Game {
            self.view = View::Game;
        } else if self.wait == Wait::Click {
            self.view = View::Slots { save: true };
        }
    }

    /// 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip (toggles)
    pub fn key(&mut self, k: usize) {
        match k {
            4 => match self.wait {
                Wait::Title => self.act(Act::New),
                Wait::Choice => self.pick(0),
                _ => self.advance(),
            },
            5 => self.cancel(),
            6 => self.skip = !self.skip,
            _ => {}
        }
    }

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
        if !up {
            self.advance();
        }
    }

    // ---- drawing ----

    fn text_img(&mut self, s: &str, size: i32) -> Rc<Image> {
        let key = (s.to_string(), size);
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let img = match &self.font {
            Some(f) => Rc::new(Font::outline(f.clone(), size).render(
                &nexas::sjis::encode(s),
                [255, 255, 255],
                2,
            )),
            None => Rc::new(Image::new(1, 1)),
        };
        if self.text_cache.len() > 300 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, img.clone());
        img
    }

    fn solid(rgba: [u8; 4], alpha: f32, r: [i32; 4]) -> Draw {
        let mut img = Image::new(1, 1);
        img.px.copy_from_slice(&rgba);
        Draw {
            image: Rc::new(img),
            src: [0, 0, 1, 1],
            dst: r.map(|v| v as f32),
            place: place(alpha),
        }
    }

    fn draw(img: &Rc<Image>, src: [i32; 4], x: f32, y: f32, alpha: f32) -> Draw {
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

    fn pic_draw(p: &Pic, dx: f32, dy: f32, alpha: f32) -> Draw {
        let src = [0, 0, p.img.w as i32, p.img.h as i32];
        Self::draw(&p.img, src, p.x as f32 + dx, p.y as f32 + dy, alpha)
    }

    fn text_draw(&mut self, s: &str, size: i32, x: i32, y: i32, out: &mut Vec<Draw>) {
        let img = self.text_img(s, size);
        let src = [0, 0, img.w as i32, img.h as i32];
        out.push(Self::draw(&img, src, x as f32, y as f32, 1.0));
    }

    /// A picture's buttons on the screen: (the part of the picture, the screen rect).
    fn buttons_of(&mut self, name: &str, gap: i32) -> Vec<([i32; 4], [i32; 4])> {
        let Some(p) = self.pic(name) else {
            return Vec::new();
        };
        let across = p.img.w > p.img.h;
        runs(&p.img, across, gap)
            .into_iter()
            .map(|(a, b)| {
                let src = if across {
                    [a, 0, b, p.img.h as i32]
                } else {
                    [0, a, p.img.w as i32, b]
                };
                let r = [src[0] + p.x, src[1] + p.y, src[2] + p.x, src[3] + p.y];
                (src, r)
            })
            .collect()
    }

    fn title_buttons(&mut self) -> Vec<([i32; 4], [i32; 4])> {
        self.buttons_of(self.skin.title_buttons, 0)
    }

    fn window_buttons(&mut self) -> Vec<([i32; 4], [i32; 4])> {
        self.buttons_of("FRAME_BUTTON_NORMAL", self.skin.gap)
    }

    /// Buttons, the hovered one from the `hover` picture (which may sit a little elsewhere).
    fn buttons_draws(&mut self, normal: &str, hover: &str, gap: i32, out: &mut Vec<Draw>) {
        let (Some(n), h) = (self.pic(normal), self.pic(hover)) else {
            return;
        };
        let (px, py) = self.pointer;
        for (src, r) in self.buttons_of(normal, gap) {
            match &h {
                Some(h) if inside(r, px, py) => {
                    let across = n.img.w > n.img.h;
                    let s = if across {
                        [r[0] - h.x, 0, r[2] - h.x, h.img.h as i32]
                    } else {
                        [0, r[1] - h.y, h.img.w as i32, r[3] - h.y]
                    };
                    let (x, y) = if across { (r[0], h.y) } else { (h.x, r[1]) };
                    out.push(Self::draw(&h.img, s, x as f32, y as f32, 1.0));
                }
                _ => out.push(Self::draw(&n.img, src, r[0] as f32, r[1] as f32, 1.0)),
            }
        }
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
        let (name, text) = (self.name.clone().unwrap_or_default(), self.text.clone());
        self.backlog.push(name.trim(), &text);
    }

    fn draws_inner(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        for (k, l) in &self.layers {
            let a = if l.dur == 0 {
                1.0
            } else {
                l.t as f32 / l.dur as f32
            };
            if let Some(o) = &l.old {
                let oa = if *k == BG { 1.0 } else { 1.0 - a };
                out.push(Self::pic_draw(o, l.dx, l.dy, oa));
            }
            if let Some(c) = &l.cur {
                out.push(Self::pic_draw(c, l.dx, l.dy, a));
            }
        }
        if self.wait == Wait::Title {
            if let Some(p) = self.pic(self.skin.title) {
                out.push(Self::pic_draw(&p, 0.0, 0.0, 1.0));
            }
            self.buttons_draws(self.skin.title_buttons, self.skin.title_hover, 0, &mut out);
        } else if self.window && self.hidden == [false; 2] {
            self.window_draws(&mut out);
        }
        if self.wait == Wait::Choice {
            self.choice_draws(&mut out);
        }
        if let View::Slots { save } = self.view {
            self.slot_draws(save, &mut out);
        }
        out
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let k = self.skin;
        if let Some(p) = self.pic("FRAME_BASE") {
            out.push(Self::pic_draw(&p, 0.0, 0.0, 1.0));
        }
        if let Some(n) = self.name.clone()
            && let Some(p) = self.pic("FRAME_NAMEPLATE")
        {
            out.push(Self::pic_draw(&p, 0.0, 0.0, 1.0));
            let y = p.y + (p.img.h as i32 - k.size) / 2 - 2;
            self.text_draw(&n, k.size, p.x + 40, y, out);
        }
        self.buttons_draws("FRAME_BUTTON_NORMAL", "FRAME_BUTTON_ROLLOVER", k.gap, out);
        let mut y = k.text.1;
        for line in wrap(&self.text.clone(), k.wrap) {
            self.text_draw(&line, k.size, k.text.0, y, out);
            y += k.line;
        }
    }

    fn choice_rects(&mut self) -> Vec<[i32; 4]> {
        let (x, w, h) = match self.pic("SELECT_BASE") {
            Some(p) => (p.x, p.img.w as i32, p.img.h as i32),
            None => (240, 800, 50),
        };
        let n = self.choices.len() as i32;
        let top = 300 - n * (h + 20) / 2;
        (0..n)
            .map(|i| [x, top + i * (h + 20), x + w, top + i * (h + 20) + h])
            .collect()
    }

    fn choice_draws(&mut self, out: &mut Vec<Draw>) {
        let (px, py) = self.pointer;
        let base = self.pic("SELECT_BASE");
        let hi = self.pic("SELECT_HIGHLIGHT");
        let labels: Vec<String> = self.choices.iter().map(|c| c.0.clone()).collect();
        for (r, label) in self.choice_rects().into_iter().zip(labels) {
            let p = if inside(r, px, py) { &hi } else { &base };
            match p {
                Some(p) => {
                    let src = [0, 0, p.img.w as i32, p.img.h as i32];
                    out.push(Self::draw(&p.img, src, r[0] as f32, r[1] as f32, 1.0));
                }
                None => out.push(Self::solid([40, 20, 40, 255], 0.85, r)),
            }
            let img = self.text_img(&label, 28);
            let x = (r[0] + r[2]) / 2 - img.w as i32 / 2;
            let y = (r[1] + r[3]) / 2 - img.h as i32 / 2;
            let src = [0, 0, img.w as i32, img.h as i32];
            out.push(Self::draw(&img, src, x as f32, y as f32, 1.0));
        }
    }

    // ---- saves ----

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        (0..SLOTS as i32)
            .map(|i| {
                let (c, r) = (i % 2, i / 2);
                let (x, y) = (90 + c * 560, 130 + r * 76);
                [x, y, x + 540, y + 66]
            })
            .collect()
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        match self.pic(if save { "SAVE_BASE" } else { "LOAD_BASE" }) {
            Some(p) => out.push(Self::pic_draw(&p, 0.0, 0.0, 1.0)),
            None => out.push(Self::solid(
                [10, 0, 20, 255],
                0.85,
                [0, 0, W as i32, H as i32],
            )),
        }
        let (px, py) = self.pointer;
        for (i, r) in self.slot_rects().into_iter().enumerate() {
            let c = if inside(r, px, py) {
                [230, 90, 160, 255]
            } else {
                [60, 30, 60, 255]
            };
            out.push(Self::solid(c, 0.8, r));
            let label = match self.read_slot(i) {
                Some(s) => format!("{:02}  {}", i + 1, s.label),
                None => format!("{:02}  ----", i + 1),
            };
            self.text_draw(&label, 24, r[0] + 16, r[1] + 18, out);
        }
    }

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&format!("slot{i:02}.sav"))?;
        Slot::parse(&String::from_utf8(b).ok()?)
    }

    fn save(&mut self, i: usize) {
        if self.msg_at.0.is_empty() {
            return;
        }
        let t: String = self
            .text
            .chars()
            .filter(|c| !c.is_control())
            .take(12)
            .collect();
        let s = Slot {
            label: format!("{} {t}", self.msg_at.0.to_uppercase()),
            at: self.msg_at.clone(),
            stack: self.stack.clone(),
            vars: self.vars.clone(),
            layers: self
                .layers
                .iter()
                .filter_map(|(k, l)| {
                    let n = l.cur.as_ref()?.name.clone();
                    Some((*k, n, l.dx, l.dy))
                })
                .collect(),
            bgm: self.bgm.clone(),
            loops: self.loops.clone(),
        };
        self.files
            .write_save(&format!("slot{i:02}.sav"), s.write().as_bytes());
    }

    fn load(&mut self, i: usize) {
        let Some(s) = self.read_slot(i) else {
            return;
        };
        if !self.goto_script(&s.at.0) {
            return;
        }
        self.pc = s.at.1;
        self.stack = s.stack;
        self.vars = s.vars;
        self.layers.clear();
        for (k, n, dx, dy) in s.layers {
            self.set_layer(k, Some(n), true);
            let l = self.layers.entry(k).or_default();
            l.dx = dx;
            l.dy = dy;
        }
        for c in std::mem::take(&mut self.loops).into_keys() {
            self.stop(&format!("loop{c}"), 0.0);
        }
        for (c, n) in s.loops {
            self.play(&format!("loop{c}"), &n, true, 0.8);
            self.loops.insert(c, n);
        }
        self.stop("bgm", 0.0);
        self.bgm = s.bgm;
        if let Some(b) = self.bgm.clone() {
            self.play("bgm", &b, true, 0.6);
        }
        self.text.clear();
        self.name = None;
        self.choices.clear();
        self.window = false;
        self.hidden = [false; 2];
        self.skip = false;
        self.wait = Wait::None;
        self.view = View::Game;
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
}

/// Splits text into lines of at most `n` characters, at its own line breaks too.
fn wrap(s: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in s.split('\n') {
        let chars: Vec<char> = para.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let mut end = (i + n).min(chars.len());
            // keep closing marks with the line before
            while end < chars.len() && "」』）。、！？…ー〜".contains(chars[end]) && end - i < n + 4
            {
                end += 1;
            }
            out.push(chars[i..end].iter().collect());
            i = end;
        }
    }
    out
}

/// A save, as lines of `key value`.
struct Slot {
    label: String,
    at: (String, usize),
    stack: Vec<(String, usize)>,
    vars: BTreeMap<u32, f32>,
    layers: Vec<(i32, String, f32, f32)>,
    bgm: Option<String>,
    loops: BTreeMap<u32, String>,
}

impl Slot {
    fn write(&self) -> String {
        let mut s = String::from("IVORYSAV1\n");
        s += &format!("label {}\n", self.label.replace('\n', " "));
        s += &format!("at {} {}\n", self.at.0, self.at.1);
        for (n, pc) in &self.stack {
            s += &format!("call {n} {pc}\n");
        }
        for (k, v) in &self.vars {
            s += &format!("var {k} {v}\n");
        }
        for (k, n, dx, dy) in &self.layers {
            s += &format!("layer {k} {dx} {dy} {n}\n");
        }
        if let Some(b) = &self.bgm {
            s += &format!("bgm {b}\n");
        }
        for (c, n) in &self.loops {
            s += &format!("loop {c} {n}\n");
        }
        s
    }

    fn parse(t: &str) -> Option<Slot> {
        let mut lines = t.lines();
        if lines.next()? != "IVORYSAV1" {
            return None;
        }
        let mut s = Slot {
            label: String::new(),
            at: (String::new(), 0),
            stack: Vec::new(),
            vars: BTreeMap::new(),
            layers: Vec::new(),
            bgm: None,
            loops: BTreeMap::new(),
        };
        for l in lines {
            let (k, rest) = l.split_once(' ').unwrap_or((l, ""));
            let w: Vec<&str> = rest.splitn(4, ' ').collect();
            match k {
                "label" => s.label = rest.to_string(),
                "at" => s.at = (w[0].to_string(), w.get(1)?.parse().ok()?),
                "call" => s.stack.push((w[0].to_string(), w.get(1)?.parse().ok()?)),
                "var" => {
                    s.vars.insert(w[0].parse().ok()?, w.get(1)?.parse().ok()?);
                }
                "layer" => s.layers.push((
                    w[0].parse().ok()?,
                    w.get(3)?.to_string(),
                    w.get(1)?.parse().ok()?,
                    w.get(2)?.parse().ok()?,
                )),
                "bgm" => s.bgm = Some(rest.to_string()),
                "loop" => {
                    let (c, n) = rest.split_once(' ')?;
                    s.loops.insert(c.parse().ok()?, n.to_string());
                }
                _ => {}
            }
        }
        (!s.at.0.is_empty()).then_some(s)
    }
}
