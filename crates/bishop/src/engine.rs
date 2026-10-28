//! Runs a Bishop (BSXX) game: the script of bsxx.dat from its `boot` label.
//!
//! The script is register code: values come from immediates, strings (by number), system and
//! global variables, locals (the stack above the stack pointer) and registers; register 0
//! holds the last comparison. A command (one byte) takes its arguments from the locals 1, 2,
//! ... that the code before it set. Pictures are "layers" in order: a background (kind 1,
//! which clears the others), pictures added on top by name (kind 2) and bust-ups (kind 3, one
//! per character, a body with a face on it); they show on the screen when a change (fade,
//! mask) presents them. Messages wait for a click. Saves are our own (ten slots from the
//! right-click menu).

use std::collections::HashMap;
use std::rc::Rc;

use nexas::files::Files;
use nexas::gfx::{Draw, Image, Place};

use crate::bsa;
use crate::bsg::{self, Bsg};
use crate::cfg::Cfg;
use crate::script::Script;

/// A sound to start or stop.
pub struct Sound {
    pub data: Vec<u8>,
    pub volume: f32,
    pub channel: i32,
    pub stop: bool,
    pub fade_ms: u32,
    pub looped: bool,
}

const FRAME_MS: f64 = 1000.0 / 60.0;
const SLOTS: usize = 10;
const STEPS: usize = 200_000;
const BGM: i32 = 0;
const VOICE: i32 = 1;
const BGV: i32 = 2;
const SE: i32 = 100;
/// a Rev.6 message (its own layout), past the 2.0 opcodes
const REV6_MESSAGE: u8 = 0xf0;

/// BSScript Rev.6 opcodes as their BSXScript 2.0 counterparts.
fn rev6_op(op: u8) -> u8 {
    match op {
        0x00..=0x07 => op,
        // call, return, clear the stack
        0x08..=0x0a => op + 1,
        // comparisons, assignment, arithmetic, negation
        0x0b..=0x19 => op + 1,
        0x1a => REV6_MESSAGE,
        // load a picture (and at x, y)
        0x1b | 0x1c => op + 3,
        // replace, show or hide a layer
        0x1e | 0x1f => op + 3,
        // window, change, masked change
        0x23..=0x25 => op + 3,
        // flash, BGM, BGM stop
        0x26 => 0x2b,
        0x27 => 0x2c,
        0x28 => 0x2d,
        // looped voice and its stop, a sound
        0x29 => 0x30,
        0x2a => 0x31,
        0x2b => 0x2e,
        0x2c => 0x33,
        0x2d => 0x34,
        0x2f => 0x35,
        0x30 => 0x37,
        0x31 => 0x38,
        // commands without a counterpart: nothing
        _ => 0x23,
    }
}

/// The Rev.6 layout (in the program there): an 800x600 screen, the message window at the
/// bottom and the title's buttons on the right.
const REV6_CFG: &str = "[main window]
width = 800
height = 600
[default font]
height = 22
line length = 30
[message window]
base window = message
bg color = 0xa0000000
text = 48, 52
name = 48, 26
name color = 0xffff8855
[title phase]
base = title_base
op = title_menu_op, 2, 624, 208
start = title_menu_begin, 2, 672, 256
load = title_menu_load, 2, 672, 304
museum = title_menu_museum, 2, 696, 352
config = title_menu_setting, 2, 720, 400
quit = title_menu_quit, 2, 720, 448
";

#[derive(Clone, Debug, PartialEq)]
struct Layer {
    /// what finds the layer again: the picture's name, or "buNN" for a bust-up
    key: String,
    kind: i32,
    /// the pictures (a bust-up: body, then face)
    parts: Vec<String>,
    x: i32,
    y: i32,
    visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Wait {
    None,
    Click,
    Until(u64),
    Title,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Menu {
    Main,
    Save,
    Load,
    /// the map's places (see `Engine::map_choices`)
    Map,
}

#[derive(Clone)]
struct Fade {
    from: Vec<Draw>,
    start: u64,
    ms: u64,
}

#[derive(Clone, Default)]
struct Msg {
    name: String,
    lines: Vec<String>,
}

pub struct Engine {
    files: Box<dyn Files>,
    pub screen: (u32, u32),
    s: Rc<Script>,
    cfg: Cfg,
    pc: usize,
    stack: Vec<i32>,
    sp: usize,
    regs: Vec<i32>,
    globals: Vec<i32>,
    sys: Vec<i32>,
    /// picture name (lower case, no folder or extension) -> (archive, offset, size)
    pics: HashMap<String, (String, u64, usize)>,
    /// "bgm/name", "se/name", "voice/name" -> (archive, offset, size)
    audio: HashMap<String, (String, u64, usize)>,
    cache: HashMap<String, Option<Rc<Bsg>>>,
    layers: Vec<Layer>,
    shown: Vec<Layer>,
    fade: Option<Fade>,
    windows: [bool; 4],
    msg: Option<Msg>,
    msg_pc: usize,
    wait: Wait,
    menu: Option<Menu>,
    sounds: Vec<Sound>,
    bgm: Option<String>,
    bgv: Option<String>,
    now: u64,
    pointer: (i32, i32),
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<(String, u32), Rc<Image>>,
    /// the message window's picture, filled with its colour: (name, picture, x, y)
    window_pic: Option<(String, Rc<Image>, i32, i32)>,
    /// the map's global and its choices (value, label)
    map_choices: (usize, Vec<(i32, String)>),
    /// the hero's family and given names (`\A`, `\B`)
    hero: (String, String),
    ended: bool,
    pub log: bool,
}

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Rc<Image> {
    let mut i = Image::new(w, h);
    i.fill(rgba);
    Rc::new(i)
}

impl Engine {
    pub fn new(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let raw = files
            .read("bsxx.dat")
            .or_else(|| files.read("bsx.dat"))
            .ok_or("no bsxx.dat")?;
        let s = Rc::new(Script::parse(&raw)?);
        let cfg = match files.read("bsxx.cfg") {
            Some(b) => Cfg::parse(&nexas::sjis::decode(&b)),
            // Rev.6 has its layout in the program
            None if s.major == 1 => Cfg::parse(REV6_CFG),
            None => Cfg::default(),
        };
        let w = cfg
            .nums("main window", "width")
            .first()
            .copied()
            .unwrap_or(800);
        let h = cfg
            .nums("main window", "height")
            .first()
            .copied()
            .unwrap_or(600);
        let hero = match files.read("bsxx.ini") {
            Some(b) => hero_names(&b),
            None => files
                .read("bsx.ini")
                .map(|b| hero_names_rev6(&b))
                .unwrap_or_default(),
        };
        let mut e = Engine {
            files,
            screen: (w as u32, h as u32),
            stack: vec![0; s.stack],
            regs: vec![0; s.regs.max(8)],
            globals: vec![0; s.globals],
            sys: vec![0; s.sys.max(1)],
            s,
            cfg,
            pc: 0,
            sp: 0,
            pics: HashMap::new(),
            audio: HashMap::new(),
            cache: HashMap::new(),
            layers: Vec::new(),
            shown: Vec::new(),
            fade: None,
            windows: [false; 4],
            msg: None,
            msg_pc: 0,
            wait: Wait::None,
            menu: None,
            sounds: Vec::new(),
            bgm: None,
            bgv: None,
            now: 0,
            pointer: (0, 0),
            font: None,
            text_cache: HashMap::new(),
            window_pic: None,
            map_choices: (0, Vec::new()),
            hero,
            ended: false,
            log: false,
        };
        e.index_archives();
        e.pc = e.s.label("boot").unwrap_or(0);
        Ok(e)
    }

    fn index_archives(&mut self) {
        for name in self.files.list() {
            let lower = name.to_lowercase();
            let Some(stem) = lower.strip_suffix(".bsa") else {
                continue;
            };
            let Some(head) = self.files.read_range(&name, 0, 16) else {
                continue;
            };
            let Some((ver, count, at)) = bsa::header(&head) else {
                continue;
            };
            let size = self.files.size(&name).unwrap_or(0);
            let Some(idx) = self
                .files
                .read_range(&name, at, size.saturating_sub(at) as usize)
            else {
                continue;
            };
            let index = bsa::Index::parse(ver, count, &idx);
            for (path, (off, len)) in index.entries {
                let base = path.rsplit('/').next().unwrap_or(&path);
                let (base, ext) = base.rsplit_once('.').unwrap_or((base, ""));
                let entry = (name.clone(), off, len);
                if ext == "bsg" || ext == "bsc" {
                    self.pics.insert(base.to_string(), entry);
                } else {
                    let kind = if stem.starts_with("voice") {
                        "voice"
                    } else if stem.starts_with("bgm") {
                        "bgm"
                    } else {
                        "se"
                    };
                    self.audio.insert(format!("{kind}/{base}"), entry);
                }
            }
        }
    }

    pub fn set_font(&mut self, b: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(b, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        self.font = Some(Rc::new(f));
        Ok(())
    }

    fn trace(&self, s: &str) {
        if self.log {
            eprintln!("{s}");
        }
    }

    fn picture(&mut self, name: &str) -> Option<Rc<Bsg>> {
        let key = name.to_lowercase();
        if let Some(p) = self.cache.get(&key) {
            return p.clone();
        }
        let pic = match key.as_str() {
            "black" | "white" => {
                let v = if key == "black" { 0 } else { 255 };
                let mut img = Image::new(self.screen.0, self.screen.1);
                img.fill([v, v, v, 255]);
                Some(Rc::new(Bsg {
                    image: img,
                    x: 0,
                    y: 0,
                }))
            }
            _ => {
                let (arc, off, len) = self.pics.get(&key).cloned()?;
                let b = self.files.read_range(&arc, off, len)?;
                match bsg::decode(&b) {
                    Ok(p) => Some(Rc::new(p)),
                    Err(e) => {
                        self.trace(&format!("{name}: {e}"));
                        None
                    }
                }
            }
        };
        if self.cache.len() > 64 {
            self.cache.clear();
        }
        self.cache.insert(key, pic.clone());
        pic
    }

    // ---- running ----

    pub fn frame(&mut self) -> Result<(), String> {
        self.now = (self.now as f64 + FRAME_MS) as u64;
        if let Some(f) = &self.fade
            && self.now >= f.start + f.ms
        {
            self.fade = None;
        }
        if self.menu.is_some() || self.ended {
            return Ok(());
        }
        if let Wait::Until(t) = self.wait
            && self.now >= t
        {
            self.wait = Wait::None;
        }
        if self.wait == Wait::None {
            self.run()?;
        }
        Ok(())
    }

    fn arg(&self, i: usize) -> i32 {
        self.stack.get(self.sp + i).copied().unwrap_or(0)
    }

    fn sarg(&self, i: usize) -> String {
        self.s.string(self.arg(i)).to_string()
    }

    fn word(&self, at: usize) -> u32 {
        self.s
            .code
            .get(at..at + 4)
            .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
    }

    fn get(&self, t: u32, v: u32) -> i32 {
        let v = v as i32;
        let at = |a: &Vec<i32>, i: i32| a.get(i.max(0) as usize).copied().unwrap_or(0);
        match t {
            0x100 | 0x101 => v,
            0x102 => at(&self.sys, v),
            0x103 => at(&self.globals, v),
            0x104 => at(&self.stack, self.sp as i32 + v),
            0x105 => at(&self.regs, v),
            _ => 0,
        }
    }

    fn set(&mut self, t: u32, v: u32, val: i32) {
        let v = v as i32;
        let slot = match t {
            0x102 => self.sys.get_mut(v.max(0) as usize),
            0x103 => self.globals.get_mut(v.max(0) as usize),
            0x104 => self.stack.get_mut((self.sp as i32 + v).max(0) as usize),
            0x105 => self.regs.get_mut(v.max(0) as usize),
            _ => None,
        };
        if let Some(s) = slot {
            *s = val;
        }
    }

    fn set_reg(&mut self, r: u32, val: i32) {
        if let Some(s) = self.regs.get_mut(r as usize) {
            *s = val;
        }
    }

    fn jump_label(&mut self, n: u32) {
        self.pc = self.s.labels.get(n as usize).copied().unwrap_or(0) as usize;
    }

    fn push(&mut self, v: i32) {
        if self.sp < self.stack.len() {
            self.stack[self.sp] = v;
        }
        self.sp += 1;
    }

    fn run(&mut self) -> Result<(), String> {
        for _ in 0..STEPS {
            if self.wait != Wait::None || self.menu.is_some() || self.ended {
                return Ok(());
            }
            self.step()?;
        }
        Err(format!(
            "script ran {STEPS} steps without waiting (at {:x})",
            self.pc
        ))
    }

    fn step(&mut self) -> Result<(), String> {
        let pc = self.pc;
        let Some(&op) = self.s.code.get(pc) else {
            self.trace("ran off the end of the code");
            self.ended = true;
            return Ok(());
        };
        let op = if self.s.major == 1 { rev6_op(op) } else { op };
        let script = self.s.clone();
        let w = |k: usize| {
            script
                .code
                .get(pc + k..pc + k + 4)
                .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
        };
        let h = |k: usize| {
            script
                .code
                .get(pc + k..pc + k + 2)
                .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as u32)
        };
        match op {
            0x00 => {
                // the end: back to the title
                self.trace(&format!("[{pc:x}] end"));
                self.stop_all(500);
                self.reset_vm();
                self.pc = self.s.label("title").unwrap_or(0);
            }
            0x01 | 0x02 => {
                self.trace(&format!("[{pc:x}] quit"));
                self.ended = true;
            }
            0x03 => self.jump_label(w(1)),
            0x04 => self.pc = w(1) as usize,
            0x05 | 0x06 => {
                let c = self.regs[0] != 0;
                self.pc = if c == (op == 0x05) {
                    w(1) as usize
                } else {
                    pc + 5
                };
            }
            0x07 | 0x08 | 0x34 | 0x36 | 0x38 => self.pc = pc + 5,
            0x09 => {
                let target = w(1);
                self.push((pc + 5) as i32);
                self.push(0);
                self.jump_label(target);
            }
            0x0a => {
                if self.sp < 2 {
                    self.trace(&format!("[{pc:x}] return with an empty stack"));
                    self.pc = pc + 1;
                } else {
                    self.sp -= 2;
                    self.pc = self.stack[self.sp] as usize;
                }
            }
            0x0b => {
                self.sp = 0;
                self.pc = pc + 1;
            }
            0x0c..=0x11 | 0x13..=0x19 | 0x3a..=0x3c => {
                let a = self.get(h(6), w(8));
                let b = self.get(h(12), w(14));
                let v = match op {
                    0x0c => (a > b) as i32,
                    0x0d => (a < b) as i32,
                    0x0e => (a >= b) as i32,
                    0x0f => (a <= b) as i32,
                    0x10 => (a != b) as i32,
                    0x11 => (a == b) as i32,
                    0x13 => a | b,
                    0x14 => a & b,
                    0x15 => a.wrapping_add(b),
                    0x16 => a.wrapping_sub(b),
                    0x17 => a.wrapping_mul(b),
                    0x18 => a.checked_div(b).unwrap_or(0),
                    0x19 => a.checked_rem(b).unwrap_or(0),
                    0x3a => a ^ b,
                    0x3b => a.wrapping_shl(b as u32 & 31),
                    _ => a.wrapping_shr(b as u32 & 31),
                };
                self.set_reg(w(2), v);
                self.pc = pc + 18;
            }
            0x12 => {
                let v = self.get(h(12), w(14));
                self.set(h(6), w(8), v);
                self.set_reg(w(2), v);
                self.pc = pc + 18;
            }
            0x1a | 0x3d => {
                let a = self.get(h(6), w(8));
                self.set_reg(w(2), if op == 0x1a { a.wrapping_neg() } else { !a });
                self.pc = pc + 12;
            }
            0x1b | 0x1c => {
                let a = self.get(h(6), w(8)) + if op == 0x1b { 1 } else { -1 };
                self.set(h(6), w(8), a);
                self.set_reg(w(2), a);
                self.pc = pc + 12;
            }
            REV6_MESSAGE => {
                // kind, message, then a name, a voice and one more word by kind
                let kind = self.s.code.get(pc + 1).copied().unwrap_or(0) as usize;
                self.pc = pc + 6 + kind * 4;
                self.message(pc, kind.min(1));
                if kind >= 2 {
                    let v = self.s.string(self.word(pc + 10) as i32).to_string();
                    self.stop(VOICE, 0);
                    self.play("voice", &v, VOICE, false, 1.0);
                }
            }
            0x1d => {
                let kind = self.s.code.get(pc + 1).copied().unwrap_or(0) as usize;
                let mut len = 6 + kind * 4;
                if kind > 1 {
                    len += w(10) as usize * 4;
                }
                self.pc = pc + len;
                self.message(pc, kind);
            }
            0x33 => self.pc = pc + 13,
            0x3e => {
                let n = w(1) as usize;
                self.pc = pc + 5 + n * 4;
            }
            0x1e..=0x32 | 0x35 | 0x37 | 0x39 | 0x3f => {
                self.pc = pc + 1;
                self.command(op);
            }
            _ => return Err(format!("unknown op {op:02x} at {pc:x}")),
        }
        Ok(())
    }

    fn reset_vm(&mut self) {
        self.sp = 0;
        self.stack.iter_mut().for_each(|v| *v = 0);
        self.regs.iter_mut().for_each(|v| *v = 0);
    }

    fn message(&mut self, pc: usize, kind: usize) {
        let idx = self.word(pc + 2) as usize;
        let text = self.s.messages.get(idx).cloned().unwrap_or_default();
        let name = if kind >= 1 {
            let n = self.word(pc + 6) as usize;
            self.s.names.get(n).cloned().unwrap_or_default()
        } else {
            String::new()
        };
        let mut voices = Vec::new();
        if kind >= 2 {
            for i in 0..self.word(pc + 10) as usize {
                voices.push(self.s.string(self.word(pc + 14 + i * 4) as i32).to_string());
            }
        }
        let text = self.subst(&text);
        let name = self.subst(&name);
        self.trace(&format!("[{pc:x}] {name} {text}"));
        let per = self
            .cfg
            .nums("default font", "line length")
            .first()
            .copied()
            .unwrap_or(22) as usize;
        self.msg = Some(Msg {
            name,
            lines: crate::text::wrap(&text, per.max(4)),
        });
        self.msg_pc = pc;
        if !voices.is_empty() {
            self.stop(VOICE, 0);
        }
        for v in voices {
            self.play("voice", &v, VOICE, false, 1.0);
        }
        // pictures changed without a change command show at once
        if self.layers != self.shown {
            self.shown = self.layers.clone();
        }
        self.wait = Wait::Click;
    }

    /// The hero's names in place of `\A` and `\B`; size codes (`\b`, `\s`) dropped.
    fn subst(&self, t: &str) -> String {
        let mut out = String::new();
        let mut it = t.chars();
        while let Some(c) = it.next() {
            if c != '\\' {
                out.push(match c {
                    '\u{e000}' => '♥',
                    c => c,
                });
                continue;
            }
            match it.next() {
                Some('A') => out.push_str(&self.hero.0),
                Some('B') => out.push_str(&self.hero.1),
                Some('n') => out.push('\n'),
                _ => {}
            }
        }
        out
    }

    fn command(&mut self, op: u8) {
        let args: Vec<i32> = (0..6).map(|i| self.arg(i)).collect();
        let a = |i: usize| args[i];
        if self.log {
            eprintln!(
                "[{:x}] cmd {op:02x} ({}, {}, {}, {})",
                self.pc - 1,
                a(1),
                a(2),
                a(3),
                a(4)
            );
        }
        match op {
            0x1e | 0x1f => {
                let name = self.sarg(1);
                let (x, y) = if op == 0x1f { (a(3), a(4)) } else { (0, 0) };
                self.load(&name, a(2), x, y);
            }
            0x21 => {
                let name = self.sarg(2);
                if let Some(l) = self.layers.get_mut(a(1).max(0) as usize) {
                    l.parts = vec![name];
                }
            }
            0x22 => {
                let i = a(1).max(0) as usize;
                if i < self.layers.len() {
                    match a(2) {
                        -1 => {
                            self.layers.remove(i);
                        }
                        0 => self.layers[i].visible = true,
                        1 => self.layers[i].visible = false,
                        _ => {}
                    }
                }
            }
            0x26 => {
                if let Some(w) = self.windows.get_mut(a(1).max(0) as usize) {
                    *w = a(2) != 0;
                }
                if a(1) == 0 && a(2) == 0 {
                    self.msg = None;
                }
            }
            0x27 => self.present(a(2).max(0) as u64 * 100),
            0x28 => self.present(a(2).max(0) as u64 * 100),
            0x2c => {
                let name = self.sarg(2);
                if self.bgm.as_deref() != Some(name.as_str()) {
                    self.bgm = Some(name.clone());
                    self.play("bgm", &name, BGM, true, 0.6);
                }
            }
            0x2d => {
                self.bgm = None;
                self.stop(BGM, a(1).max(0) as u32 * 100);
            }
            0x2e | 0x32 => {
                let name = self.sarg(1);
                self.play("se", &name, SE, false, 0.8);
            }
            0x2f => self.stop(SE, 0),
            0x30 => {
                let name = self.sarg(1);
                self.bgv = Some(name.clone());
                self.play("voice", &name, BGV, true, 0.7);
            }
            0x31 => {
                self.bgv = None;
                self.stop(BGV, 0);
            }
            0x39 => {
                // a picture at a depth (a(1)) of a kind (a(3)); 3.x adds an x offset (the
                // close-up bust-ups sit off to the right and come with -663)
                let name = self.sarg(2);
                if name.eq_ignore_ascii_case("reserved") {
                    return;
                }
                let x = if self.s.major >= 3 { a(4) } else { 0 };
                self.load(&name, a(3), x, 0);
            }
            0x35 => {
                // wait a(2) tenths of a second (mode 2: for a click)
                if a(1) == 2 {
                    self.wait = Wait::Click;
                } else {
                    self.wait = Wait::Until(self.now + a(2).max(0) as u64 * 100);
                }
            }
            0x37 => {
                let s = self.sarg(1);
                if s.eq_ignore_ascii_case("title") {
                    self.start_title();
                } else if s.eq_ignore_ascii_case("map") {
                    self.start_map();
                } else {
                    self.trace(&format!("system {s}"));
                }
            }
            _ => {}
        }
    }

    fn load(&mut self, name: &str, kind: i32, x: i32, y: i32) {
        if name.is_empty() || name.starts_with("**") {
            return;
        }
        let lower = name.to_lowercase();
        match kind {
            1 | 0x81 => {
                self.layers.clear();
                self.layers.push(Layer {
                    key: lower.clone(),
                    kind,
                    parts: vec![lower],
                    x,
                    y,
                    visible: true,
                });
            }
            3 => {
                // Rev.6 names a bust-up buNNNNNFF (body NNNNN, face FF)
                let n = if self.s.major == 1 { 7 } else { 4 };
                let key: String = lower.chars().take(n).collect();
                let parts = self.bustup(&lower);
                self.trace(&format!("bust-up {lower}: {parts:?}"));
                let layer = Layer {
                    key: key.clone(),
                    kind,
                    parts,
                    x,
                    y,
                    visible: true,
                };
                match self.layers.iter_mut().find(|l| l.key == key) {
                    Some(l) => *l = layer,
                    None => self.layers.push(layer),
                }
            }
            _ => {
                let layer = Layer {
                    key: lower.clone(),
                    kind,
                    parts: vec![lower.clone()],
                    x,
                    y,
                    visible: true,
                };
                match self.layers.iter_mut().find(|l| l.key == lower) {
                    Some(l) => *l = layer,
                    None => self.layers.push(layer),
                }
            }
        }
    }

    /// The pictures of a bust-up `buCCsDDDDpEE` (character, size, dress, pose, face): the
    /// body `buCCsDDDDp` (or another dress in the same pose) and the face `buCCs000?pEE`.
    fn bustup(&mut self, name: &str) -> Vec<String> {
        if self.s.major == 1 && name.len() == 9 && name.is_ascii() {
            let body = format!("{}00", &name[..7]);
            return [body, name.to_string()]
                .into_iter()
                .filter(|p| self.pics.contains_key(p))
                .collect();
        }
        if self.pics.contains_key(name) && name.len() <= 10 {
            return vec![name.to_string()];
        }
        let c: Vec<char> = name.chars().collect();
        if c.len() < 12 {
            return vec![name.to_string()];
        }
        let head: String = c[..5].iter().collect();
        let dress: String = c[5..9].iter().collect();
        let pose = c[9];
        let face: String = c[10..].iter().collect();
        let mut parts = Vec::new();
        let body = format!("{head}{dress}{pose}");
        if self.pics.contains_key(&body) {
            parts.push(body);
        } else {
            // a dress the trial has, in the same pose
            let mut other: Vec<&String> = self
                .pics
                .keys()
                .filter(|k| k.len() == 10 && k.starts_with(&head) && k.ends_with(pose))
                .collect();
            other.sort();
            if let Some(b) = other.first() {
                parts.push((*b).clone());
            }
        }
        let face_name = format!("{head}000{}{pose}{face}", c[8]);
        if self.pics.contains_key(&face_name) {
            parts.push(face_name);
        } else if self.pics.contains_key(name) {
            parts.push(name.to_string());
        } else {
            let alt = format!("{head}0001{pose}{face}");
            if self.pics.contains_key(&alt) {
                parts.push(alt);
            }
        }
        parts
    }

    fn present(&mut self, ms: u64) {
        let from = self.scene_of(&self.shown.clone());
        self.shown = self.layers.clone();
        if ms > 0 {
            self.fade = Some(Fade {
                from,
                start: self.now,
                ms,
            });
            self.wait = Wait::Until(self.now + ms);
        } else {
            self.fade = None;
        }
    }

    fn start_title(&mut self) {
        self.msg = None;
        self.layers.clear();
        if let Some(base) = self.cfg.get("title phase", "base").map(str::to_string) {
            let pos = self.cfg.nums("title phase", "base pos");
            let (x, y) = (pos.first().copied(), pos.get(1).copied());
            self.load(&base, 1, x.unwrap_or(0) as i32, y.unwrap_or(0) as i32);
        }
        self.present(500);
        if let Some(bgm) = self.cfg.get("title phase", "bgm name").map(str::to_string) {
            self.bgm = Some(bgm.clone());
            self.play("bgm", &bgm, BGM, true, 0.6);
        }
        self.windows = [false; 4];
        self.wait = Wait::Title;
    }

    /// The title's buttons: (what, picture, frames, rect).
    fn title_buttons(&mut self) -> Vec<(String, Rc<Bsg>, i32, [i32; 4])> {
        let mut out = Vec::new();
        for what in ["op", "start", "load", "museum", "config", "quit"] {
            let v = self.cfg.list("title phase", what);
            if v.len() < 4 {
                continue;
            }
            let (Some(n), Some(x), Some(y)) = (
                crate::cfg::num(&v[1]),
                crate::cfg::num(&v[2]),
                crate::cfg::num(&v[3]),
            ) else {
                continue;
            };
            let Some(p) = self.picture(&v[0]) else {
                continue;
            };
            let n = n.max(1) as i32;
            let (w, h) = (p.image.w as i32, p.image.h as i32 / n);
            // Rev.6 games have the place either in the picture (2008) or in the program
            // (2007, our REV6_CFG)
            let (x, y) = if self.s.major == 1 && (p.x, p.y) != (0, 0) {
                (p.x as i64, p.y as i64)
            } else {
                (x, y)
            };
            out.push((
                what.to_string(),
                p,
                n,
                [x as i32, y as i32, x as i32 + w, y as i32 + h],
            ));
        }
        out
    }

    // ---- sound ----

    fn play(&mut self, kind: &str, name: &str, channel: i32, looped: bool, volume: f32) {
        let key = format!("{kind}/{}", name.to_lowercase());
        let Some((arc, off, len)) = self.audio.get(&key).cloned() else {
            self.trace(&format!("no sound {key}"));
            return;
        };
        let Some(data) = self.files.read_range(&arc, off, len) else {
            return;
        };
        self.sounds.push(Sound {
            data,
            volume,
            channel,
            stop: false,
            fade_ms: 0,
            looped,
        });
    }

    fn stop(&mut self, ch: i32, fade_ms: u32) {
        self.sounds.push(Sound {
            data: Vec::new(),
            volume: 0.0,
            channel: ch,
            stop: true,
            fade_ms,
            looped: false,
        });
    }

    fn stop_all(&mut self, fade_ms: u32) {
        self.bgm = None;
        self.bgv = None;
        self.stop(-1, fade_ms);
    }

    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    // ---- drawing ----

    fn scene_of(&mut self, layers: &[Layer]) -> Vec<Draw> {
        let mut out = Vec::new();
        for l in layers {
            if !l.visible {
                continue;
            }
            for p in &l.parts {
                if let Some(b) = self.picture(p) {
                    let img = Rc::new(b.image.clone());
                    out.push(draw(&img, (l.x + b.x) as f32, (l.y + b.y) as f32, 1.0));
                }
            }
        }
        out
    }

    fn text(&mut self, s: &str, rgb: [u8; 3], size: u32) -> Rc<Image> {
        let key = (format!("{s}\u{0}{rgb:?}"), size);
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let img = match &self.font {
            Some(f) => Rc::new(crate::text::render(f, s, size as f32, rgb)),
            None => Rc::new(Image::new(1, 1)),
        };
        if self.text_cache.len() > 200 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, img.clone());
        img
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let Some(m) = self.msg.clone() else { return };
        if !self.windows[0] && self.windows.iter().any(|&w| w) {
            // the game hid the message window
        }
        let sec = "message window";
        let pos = self.cfg.nums(sec, "base pos");
        let base = self.cfg.get(sec, "base window").map(str::to_string);
        let wx = pos.first().copied().unwrap_or(0) as i32;
        // without a position the window sits at the bottom
        let wy = match pos.get(1) {
            Some(&y) => y as i32,
            None => base
                .clone()
                .and_then(|b| self.picture(&b))
                .map_or(448, |b| self.screen.1 as i32 - b.y - b.image.h as i32),
        };
        if let Some((name, img, x, y)) = &self.window_pic
            && base.as_deref() == Some(name.as_str())
        {
            out.push(draw(img, (wx + x) as f32, (wy + y) as f32, 1.0));
        } else {
            match base.clone().and_then(|b| self.picture(&b)) {
                Some(b) => {
                    // a white or grey picture is a shape that `bg color` (ARGB) fills (2008's
                    // window has its own colours)
                    let grey = self.s.major != 1
                        || b.image
                            .px
                            .chunks_exact(4)
                            .all(|p| p[3] == 0 || (p[0] == p[1] && p[1] == p[2]));
                    let c = self
                        .cfg
                        .nums(sec, "bg color")
                        .first()
                        .copied()
                        .unwrap_or(0x80000000) as u32;
                    let mut img = b.image.clone();
                    for p in img.px.chunks_exact_mut(4).filter(|_| grey) {
                        p[0] = (c >> 16) as u8;
                        p[1] = (c >> 8) as u8;
                        p[2] = c as u8;
                        p[3] = (p[3] as u32 * (c >> 24) / 255) as u8;
                    }
                    img.renew();
                    let img = Rc::new(img);
                    out.push(draw(&img, (wx + b.x) as f32, (wy + b.y) as f32, 1.0));
                    self.window_pic = Some((base.clone().unwrap_or_default(), img, b.x, b.y));
                }
                None => {
                    let mut d = draw(&solid(1, 1, [0, 0, 0, 160]), 0.0, 0.0, 1.0);
                    d.dst = [0.0, wy as f32, self.screen.0 as f32, self.screen.1 as f32];
                    out.push(d);
                }
            }
        }
        let size = self
            .cfg
            .nums("default font", "height")
            .first()
            .copied()
            .unwrap_or(20)
            .clamp(10, 64) as u32;
        let t = self.cfg.nums(sec, "text");
        let (tx, ty) = (
            wx + t.first().copied().unwrap_or(174) as i32,
            wy + t.get(1).copied().unwrap_or(28) as i32,
        );
        if !m.name.is_empty() {
            let n = self.cfg.nums(sec, "name");
            let (nx, ny) = (
                wx + n.first().copied().unwrap_or(174) as i32,
                wy + n.get(1).copied().unwrap_or(8) as i32,
            );
            let c = self
                .cfg
                .nums(sec, "name color")
                .first()
                .copied()
                .unwrap_or(0xffff8855) as u32;
            let rgb = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
            let img = self.text(&m.name, rgb, size);
            out.push(draw(
                &img,
                (nx - crate::text::PAD) as f32,
                (ny - crate::text::PAD) as f32,
                1.0,
            ));
        }
        let lh = size as i32 + 6;
        for (i, l) in m.lines.iter().enumerate() {
            let img = self.text(l, [255, 255, 255], size);
            out.push(draw(
                &img,
                (tx - crate::text::PAD) as f32,
                (ty + i as i32 * lh - crate::text::PAD) as f32,
                1.0,
            ));
        }
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        if let Some(m) = self.menu {
            return self.menu_draws(m);
        }
        let shown = self.shown.clone();
        let mut out = self.scene_of(&shown);
        if let Some(f) = &self.fade {
            let k = (self.now.saturating_sub(f.start) as f32 / f.ms.max(1) as f32).min(1.0);
            // the new scene fades in over the old one
            let mut from = f.from.clone();
            for d in &mut out {
                d.place.alpha *= k;
            }
            from.extend(out);
            out = from;
        }
        if self.wait == Wait::Title {
            let p = self.pointer;
            for (_, img, n, r) in self.title_buttons() {
                let lit = inside(r, p.0, p.1);
                let h = r[3] - r[1];
                let state = if lit { (n - 1).min(1) } else { 0 };
                let image = Rc::new(img.image.clone());
                out.push(part(
                    &image,
                    [0, state * h, r[2] - r[0], h],
                    r[0] as f32,
                    r[1] as f32,
                ));
            }
        } else {
            self.window_draws(&mut out);
        }
        out
    }

    pub fn render(&mut self) -> Image {
        let mut img = Image::new(self.screen.0, self.screen.1);
        img.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if x >= 0 {
            self.pointer = (x, y);
        }
        if let Some(m) = self.menu {
            self.menu_click(m, x, y);
            return;
        }
        match self.wait {
            Wait::Title => {
                let buttons = self.title_buttons();
                // without the buttons' pictures any click starts
                let hit = if buttons.is_empty() {
                    Some("start".to_string())
                } else {
                    buttons.into_iter().find(|b| inside(b.3, x, y)).map(|b| b.0)
                };
                match hit.as_deref() {
                    Some("start") => {
                        self.stop(BGM, 500);
                        self.bgm = None;
                        self.wait = Wait::None;
                    }
                    Some("load") => self.menu = Some(Menu::Load),
                    Some("quit") => self.ended = true,
                    _ => {}
                }
            }
            Wait::Click => {
                self.fade = None;
                self.wait = Wait::None;
            }
            Wait::Until(_) => {
                self.fade = None;
                self.wait = Wait::None;
            }
            Wait::None => {}
        }
    }

    pub fn cancel(&mut self) {
        self.menu = match self.menu {
            None if self.wait == Wait::Click => Some(Menu::Main),
            None => None,
            // the map waits for a place
            Some(Menu::Map) => Some(Menu::Map),
            Some(_) => None,
        };
    }

    pub fn wheel(&mut self, _up: bool) {}

    pub fn ended(&self) -> bool {
        self.ended
    }

    // ---- menu and saves ----

    fn menu_items(&self) -> Vec<(&'static str, [i32; 4])> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        ["セーブ", "ロード", "タイトルへ", "閉じる"]
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                let y = h / 2 - 130 + i as i32 * 66;
                (n, [w / 2 - 140, y, w / 2 + 140, y + 52])
            })
            .collect()
    }

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        let top = (h - SLOTS as i32 * 50) / 2;
        (0..SLOTS as i32)
            .map(|i| [w / 2 - 300, top + i * 50, w / 2 + 300, top + i * 50 + 44])
            .collect()
    }

    fn button(&mut self, label: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let lit = inside(r, self.pointer.0, self.pointer.1);
        let mut d = draw(
            &solid(
                1,
                1,
                if lit {
                    [90, 60, 120, 230]
                } else {
                    [30, 30, 40, 220]
                },
            ),
            0.0,
            0.0,
            1.0,
        );
        d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        out.push(d);
        let t = self.text(label, [255, 255, 255], 22);
        let mut d = draw(
            &t,
            (r[0] + 14) as f32,
            (r[1] + (r[3] - r[1] - 26) / 2) as f32,
            1.0,
        );
        d.place.clip = [r[0] as f32, r[1] as f32, (r[2] - 6) as f32, r[3] as f32];
        out.push(d);
    }

    /// The map phase (Rev.6): the code after the call compares a global with numbers and jumps
    /// to an event for each; the player picks one of them from a list.
    fn start_map(&mut self) {
        let code = self.s.code.clone();
        let w = |at: usize| {
            code.get(at..at + 4)
                .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
        };
        let h = |at: usize| {
            code.get(at..at + 2)
                .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as u32)
        };
        let norm = |op: u8| if self.s.major == 1 { rev6_op(op) } else { op };
        let mut choices = Vec::new();
        let mut var = None;
        let mut at = self.pc;
        while let Some(&raw) = code.get(at)
            && at < self.pc + 4000
        {
            let op = norm(raw);
            match op {
                0x00 => break,
                0x11 if h(at + 6) == 0x103 && h(at + 12) == 0x100 => {
                    let v = w(at + 8);
                    if var.is_none_or(|x| x == v) {
                        var = Some(v);
                        let n = w(at + 14) as i32;
                        let next = at + 18;
                        let target = (code.get(next).map(|&o| norm(o)) == Some(0x06)
                            && code.get(next + 5).map(|&o| norm(o)) == Some(0x03))
                        .then(|| w(next + 6) as usize)
                        .and_then(|l| self.s.label_names.get(l).cloned());
                        choices.push((n, target.unwrap_or_else(|| "次へ".to_string())));
                    }
                }
                _ => {}
            }
            at += match op {
                0x03..=0x09 | 0x34 | 0x36 | 0x38 => 5,
                0x0c..=0x19 | 0x3a..=0x3c => 18,
                0x1a..=0x1c | 0x3d => 12,
                0x33 => 13,
                0x3e => 5 + w(at + 1) as usize * 4,
                REV6_MESSAGE => 6 + code.get(at + 1).copied().unwrap_or(0) as usize * 4,
                0x1d => {
                    let k = code.get(at + 1).copied().unwrap_or(0) as usize;
                    6 + k * 4 + if k > 1 { w(at + 10) as usize * 4 } else { 0 }
                }
                _ => 1,
            };
        }
        self.trace(&format!("map: {choices:?}"));
        if let (Some(v), false) = (var, choices.is_empty()) {
            self.map_choices = (v as usize, choices);
            self.menu = Some(Menu::Map);
        }
    }

    fn map_rects(&self) -> Vec<[i32; 4]> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        let n = self.map_choices.1.len() as i32;
        let top = (h - n * 60) / 2;
        (0..n)
            .map(|i| [w / 2 - 200, top + i * 60, w / 2 + 200, top + i * 60 + 50])
            .collect()
    }

    fn menu_draws(&mut self, m: Menu) -> Vec<Draw> {
        let shown = self.shown.clone();
        let mut out = self.scene_of(&shown);
        let mut d = draw(&solid(1, 1, [0, 0, 0, 150]), 0.0, 0.0, 1.0);
        d.dst = [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32];
        out.push(d);
        match m {
            Menu::Main => {
                for (label, r) in self.menu_items() {
                    self.button(label, r, &mut out);
                }
            }
            Menu::Map => {
                let labels: Vec<String> = self.map_choices.1.iter().map(|c| c.1.clone()).collect();
                for (label, r) in labels.iter().zip(self.map_rects()) {
                    self.button(label, r, &mut out);
                }
            }
            Menu::Save | Menu::Load => {
                for (i, r) in self.slot_rects().into_iter().enumerate() {
                    let label = match self.read_slot(i) {
                        Some(s) => format!("{:02}  {}", i + 1, slot_title(&s)),
                        None => format!("{:02}  ----", i + 1),
                    };
                    self.button(&label, r, &mut out);
                }
            }
        }
        out
    }

    fn menu_click(&mut self, m: Menu, x: i32, y: i32) {
        match m {
            Menu::Map => {
                if let Some(i) = self.map_rects().into_iter().position(|r| inside(r, x, y)) {
                    let (var, choices) = &self.map_choices;
                    let n = choices[i].0;
                    if let Some(g) = self.globals.get_mut(*var) {
                        *g = n;
                    }
                    self.menu = None;
                }
            }
            Menu::Main => {
                let hit = self
                    .menu_items()
                    .into_iter()
                    .position(|(_, r)| inside(r, x, y));
                match hit {
                    Some(0) => self.menu = Some(Menu::Save),
                    Some(1) => self.menu = Some(Menu::Load),
                    Some(2) => {
                        self.menu = None;
                        self.stop_all(300);
                        self.reset_vm();
                        self.pc = self.s.label("title").unwrap_or(0);
                        self.wait = Wait::None;
                    }
                    Some(_) => self.menu = None,
                    None => {}
                }
            }
            Menu::Save | Menu::Load => {
                if let Some(i) = self.slot_rects().into_iter().position(|r| inside(r, x, y)) {
                    if m == Menu::Save {
                        self.save(i);
                        self.menu = None;
                    } else if self.read_slot(i).is_some() {
                        self.menu = None;
                        self.load_slot(i);
                    }
                }
            }
        }
    }

    fn read_slot(&mut self, i: usize) -> Option<String> {
        let b = self.files.read_save(&format!("bsx{:02}.sav", i + 1))?;
        String::from_utf8(b).ok()
    }

    fn save(&mut self, i: usize) {
        let mut s = String::new();
        let text = self
            .msg
            .as_ref()
            .map(|m| m.lines.concat())
            .unwrap_or_default();
        s.push_str(&format!("title {}\n", esc(&text)));
        s.push_str(&format!("pc {}\n", self.msg_pc));
        s.push_str(&format!("sp {}\n", self.sp));
        s.push_str(&format!("stack {}\n", join(&self.stack)));
        s.push_str(&format!("regs {}\n", join(&self.regs)));
        s.push_str(&format!("globals {}\n", join(&self.globals)));
        s.push_str(&format!("sys {}\n", join(&self.sys)));
        s.push_str(&format!(
            "windows {}\n",
            join(&self.windows.map(|w| w as i32))
        ));
        for l in &self.layers {
            s.push_str(&format!(
                "layer {} {} {} {} {} {}\n",
                esc(&l.key),
                l.kind,
                l.x,
                l.y,
                l.visible as i32,
                l.parts.iter().map(|p| esc(p)).collect::<Vec<_>>().join(",")
            ));
        }
        if let Some(b) = &self.bgm {
            s.push_str(&format!("bgm {}\n", esc(b)));
        }
        if let Some(b) = &self.bgv {
            s.push_str(&format!("bgv {}\n", esc(b)));
        }
        self.files
            .write_save(&format!("bsx{:02}.sav", i + 1), s.as_bytes());
    }

    fn load_slot(&mut self, i: usize) {
        let Some(t) = self.read_slot(i) else { return };
        self.stop_all(200);
        self.layers.clear();
        let mut bgm = None;
        let mut bgv = None;
        for line in t.lines() {
            let (k, v) = line.split_once(' ').unwrap_or((line, ""));
            match k {
                "pc" => self.pc = v.parse().unwrap_or(0),
                "sp" => self.sp = v.parse().unwrap_or(0),
                "stack" => fill(&mut self.stack, v),
                "regs" => fill(&mut self.regs, v),
                "globals" => fill(&mut self.globals, v),
                "sys" => fill(&mut self.sys, v),
                "windows" => {
                    for (w, x) in self.windows.iter_mut().zip(split::<i32>(v)) {
                        *w = x != 0;
                    }
                }
                "layer" => {
                    let f: Vec<&str> = v.split(' ').collect();
                    if f.len() >= 6 {
                        self.layers.push(Layer {
                            key: unesc(f[0]),
                            kind: f[1].parse().unwrap_or(2),
                            x: f[2].parse().unwrap_or(0),
                            y: f[3].parse().unwrap_or(0),
                            visible: f[4] != "0",
                            parts: f[5].split(',').map(unesc).collect(),
                        });
                    }
                }
                "bgm" => bgm = Some(unesc(v)),
                "bgv" => bgv = Some(unesc(v)),
                _ => {}
            }
        }
        self.shown = self.layers.clone();
        self.fade = None;
        self.msg = None;
        if let Some(b) = bgm {
            self.play("bgm", &b, BGM, true, 0.6);
            self.bgm = Some(b);
        }
        if let Some(b) = bgv {
            self.play("voice", &b, BGV, true, 0.7);
            self.bgv = Some(b);
        }
        self.wait = Wait::None;
    }

    // ---- for tests and the native runner ----

    pub fn shown_text(&self) -> String {
        self.msg
            .as_ref()
            .map(|m| {
                if m.name.is_empty() {
                    m.lines.concat()
                } else {
                    format!("{}「{}", m.name, m.lines.concat().trim_start_matches('「'))
                }
            })
            .unwrap_or_default()
    }

    pub fn position(&self) -> String {
        format!("pc {:x} sp {} wait {:?}", self.pc, self.sp, self.wait)
    }

    pub fn mode_name(&self) -> &'static str {
        match (&self.menu, &self.wait) {
            (Some(_), _) => "menu",
            (None, Wait::Title) => "title",
            _ => "game",
        }
    }
}

/// The hero's names from bsx.ini.
fn hero_names_rev6(b: &[u8]) -> (String, String) {
    // Shift-JIS fields padded with spaces: id, title, its reading, two files, given name,
    // family name
    let text = nexas::sjis::decode(b);
    let f: Vec<&str> = text
        .split([' ', '\0', '\u{3000}'])
        .filter(|s| !s.is_empty())
        .collect();
    let at = |i: usize| f.get(i).map_or(String::new(), |s| s.to_string());
    (at(6), at(5))
}

/// The hero's names from bsxx.ini (UTF-16: id, title, given name, family name).
fn hero_names(b: &[u8]) -> (String, String) {
    let u: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let parts: Vec<String> = u
        .split(|&c| c == 0)
        .map(String::from_utf16_lossy)
        .filter(|s| !s.trim().is_empty() && s.chars().all(|c| !c.is_control()))
        .collect();
    let given = parts.get(2).cloned().unwrap_or_default();
    let family = parts.get(3).cloned().unwrap_or_default();
    (family, given)
}

fn slot_title(t: &str) -> String {
    let first = t.lines().next().unwrap_or("");
    unesc(first.strip_prefix("title ").unwrap_or(""))
        .chars()
        .take(24)
        .collect()
}

fn join<T: ToString>(v: &[T]) -> String {
    v.iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn split<T: std::str::FromStr>(s: &str) -> Vec<T> {
    s.split(',').filter_map(|x| x.parse().ok()).collect()
}

fn fill(dst: &mut [i32], v: &str) {
    for (d, x) in dst.iter_mut().zip(split::<i32>(v)) {
        *d = x;
    }
}

fn esc(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '%' | ' ' | ',' | '\n' => out.push_str(&format!("%{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    if out.is_empty() {
        out.push_str("%00");
    }
    out
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '%' {
            let h: String = it.by_ref().take(2).collect();
            if let Ok(v) = u32::from_str_radix(&h, 16)
                && v != 0
                && let Some(c) = char::from_u32(v)
            {
                out.push(c);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
}

/// Part of a picture (`src`: x, y, w, h) at (x, y).
fn part(image: &Rc<Image>, src: [i32; 4], x: f32, y: f32) -> Draw {
    let mut d = draw(image, x - src[0] as f32, y - src[1] as f32, 1.0);
    d.place.clip = [x, y, x + src[2] as f32, y + src[3] as f32];
    d
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_escapes() {
        assert_eq!(unesc(&esc("a b,c%d")), "a b,c%d");
        assert_eq!(unesc(&esc("")), "");
    }

    #[test]
    fn hero_from_ini() {
        let mut b = Vec::new();
        for s in ["UK@BISHOP", "裏教師", "怜司", "杉沢"] {
            for u in s.encode_utf16() {
                b.extend(u.to_le_bytes());
            }
            b.extend([0, 0]);
        }
        assert_eq!(hero_names(&b), ("杉沢".to_string(), "怜司".to_string()));
    }
}
