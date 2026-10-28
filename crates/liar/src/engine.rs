//! Runs the GSC scripts a frame at a time, from 0000.gsc (which goes to the title, 0001.gsc).
//!
//! The ops, as read from the game's interpreter (arguments are looked up in the variables as
//! [`crate::gsc`] says; `t` is a place in the code, `s` a string of the script):
//!
//! | op | args | |
//! |----|------|-|
//! | 03 / 04 / 05 | t | jump if temporary 0 is 0 / is not 0 / always |
//! | 08 | | the end |
//! | 09 | h | a random number into temporary h |
//! | 0A | | wait for a click |
//! | 0C | n, s | go on in script n (from its start) |
//! | 0D | n | wait n tenths of a second |
//! | 0E | h n, s question, t x5, s x5, .. | a choice of n |
//! | 0F | n, s, a x10 | call script n with a0..a9 in the variables 10..19 |
//! | 10 | v | return (variable 0 = v) |
//! | 14 | pic, ? | the background (layer 0) |
//! | 15 | ? | no background |
//! | 1A / 1B | | hold the screen / show the changes at once |
//! | 1C | kind, time, time | show the changes with a fade (kind NN: by the mask efNN.msk) |
//! | 1E | layer, pic, x, y, fade, ? | a picture on a layer |
//! | 24 | layer, fade | clear a layer (0: all, above 100: a group) |
//! | 25 | layer, on | show / hide a layer |
//! | 26 | layer, cx, cy, clamp | place the layer's pictures by their centre (x, y), keep them on screen |
//! | 28 | layer, group | |
//! | 2A | window, on | show / hide the text window |
//! | 3C / 3D | track / ?, time | music Track%02d of bgm.xfl / stop it |
//! | 3E / 3F / 40 | ch, n / ch, count / ch | load %04d of wav.xfl / play it (999: for ever) / stop |
//! | 46 / 47 | layer, value | a button: its value goes to variable 0 / the game's own (2 load) |
//! | 48 | layer | not a button any more |
//! | 49 | | wait for a button |
//! | 4B | layer, pic, x, y | the button's picture under the pointer |
//! | 51 | ?, voice, ?, ?, s name, s text, ? | a text, then a click |
//! | 52 | ?, ?, ?, ?, s text, ? | more text, then a click |
//! | 5A / 5B / 60 / 64 / 68 | window, .. | where the window, text and click mark go; size, colour |
//! | 79 | layer, s | the archive of the layer's pictures (0: of all) |
//! | C8 | t, a x10 | call t of this script with a0..a9 in the variables 10..19 |
//!
//! Pictures are `%04d.wcg` of the layer's archive (else of any `grp*.xfl`); voices are
//! `%04d.wav` of voice.xfl, `n.xfl/%04d.wav` for the voice n * 10000 + k.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};
use vncommon::rule::Rule;

use crate::gsc::{Arg, Rec, Script};
use crate::wcg;
use crate::xfl::{self, Archives};

const FRAME_MS: f64 = 1000.0 / 60.0;
/// one unit of the fades' times
const UNIT_MS: f64 = 25.0;
const SLOTS: usize = 10;
const LAYERS: usize = 100;
const MAX_STACK: usize = 16;

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

#[derive(Clone, PartialEq, Debug)]
enum Wait {
    None,
    Click,
    Until(f64),
    Fade,
    /// a choice: its places and texts
    Choice(Vec<(u32, String)>),
    Buttons,
    /// the load screen opened by a button of the title
    Load,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Game,
    Menu,
    Slots { save: bool },
}

#[derive(Clone)]
struct Fade {
    from: Vec<Draw>,
    start: f64,
    ms: f64,
    /// 1C of a kind with a grps:efNN.msk: the old picture goes where the mask is dark first
    rule: Option<Rc<Image>>,
}

/// How soft a mask wipe's edge is (part of the mask's brightness range).
const RULE_VAGUE: f32 = 0.2;

#[derive(Clone, Debug, PartialEq)]
struct Frame {
    script: i32,
    /// the next record
    pc: usize,
}

#[derive(Clone, Default)]
struct Layer {
    pic: i32,
    dir: String,
    x: i32,
    y: i32,
    centre: (bool, bool),
    clamp: bool,
    group: i32,
    hidden: bool,
}

#[derive(Clone)]
struct Button {
    value: i32,
    /// 0x47: one of the game's own (2: the load screen)
    system: bool,
    hover: Option<(i32, i32, i32)>,
}

/// Where the text window goes and its parts (from ops 5A..6C).
#[derive(Clone)]
struct Window {
    at: (i32, i32),
    text: [i32; 4],
    size: i32,
    rgb: [u8; 3],
    mark: (i32, i32),
}

pub struct Engine {
    files: Box<dyn Files>,
    arcs: Archives,
    pub width: u32,
    pub height: u32,
    pub log: bool,
    font: Option<Rc<fontdue::Font>>,
    scripts: HashMap<i32, Option<Rc<Script>>>,
    stack: Vec<Frame>,
    vars: Vec<i16>,
    temps: [i16; 64],
    layers: Vec<Layer>,
    bg_dir: String,
    buttons: BTreeMap<usize, Button>,
    win: Window,
    window: bool,
    name: Option<String>,
    lines: Vec<String>,
    text: String,
    wait: Wait,
    view: View,
    frozen: Option<Vec<Draw>>,
    fade: Option<Fade>,
    rules: Vec<(usize, Rule)>,
    now: f64,
    bgm: Option<i32>,
    /// sound channel: the sound loaded, and whether it plays for ever
    se: BTreeMap<i32, (i32, bool)>,
    sounds: Vec<SoundEvent>,
    images: HashMap<String, Option<Rc<Image>>>,
    parts: HashMap<String, Option<Rc<Image>>>,
    text_cache: HashMap<(String, [u8; 3], i32), Rc<Image>>,
    pointer: (i32, i32),
    skip: bool,
    ended: bool,
    /// where a save made now starts again
    saved_at: Option<Vec<Frame>>,
    rng: u32,
    unknown: Vec<u16>,
}

fn solid_image(rgba: [u8; 4]) -> Rc<Image> {
    let mut i = Image::new(1, 1);
    i.fill(rgba);
    Rc::new(i)
}

/// Text split into lines of at most `width` pixels (`^n` breaks a line).
fn wrap(s: &str, size: i32, width: i32) -> Vec<String> {
    let mut out = Vec::new();
    for para in s.split("^n") {
        let mut line = String::new();
        let mut w = 0;
        for c in para.chars() {
            let cw = if c.is_ascii() { size / 2 } else { size };
            // a closing mark stays on its line
            let closing = "、。」』）！？…ー".contains(c);
            if w + cw > width && !line.is_empty() && !closing {
                out.push(std::mem::take(&mut line));
                w = 0;
            }
            line.push(c);
            w += cw;
        }
        out.push(line);
    }
    out
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arcs = Archives::open(files.as_mut());
        if !arcs.packs.contains_key("scr") {
            return Err("no scr.xfl in the game folder".into());
        }
        let mut e = Engine {
            files,
            arcs,
            width: 1280,
            height: 720,
            log: false,
            font: None,
            scripts: HashMap::new(),
            stack: Vec::new(),
            vars: vec![0; 0x10000],
            temps: [0; 64],
            layers: vec![Layer::default(); LAYERS],
            bg_dir: "grpe".into(),
            buttons: BTreeMap::new(),
            win: Window {
                at: (0, 503),
                text: [160, 34, 980, 150],
                size: 30,
                rgb: [255, 255, 255],
                mark: (1180, 115),
            },
            window: false,
            name: None,
            lines: Vec::new(),
            text: String::new(),
            wait: Wait::None,
            view: View::Game,
            frozen: None,
            fade: None,
            rules: Vec::new(),
            now: 0.0,
            bgm: None,
            se: BTreeMap::new(),
            sounds: Vec::new(),
            images: HashMap::new(),
            parts: HashMap::new(),
            text_cache: HashMap::new(),
            pointer: (0, 0),
            skip: false,
            ended: false,
            saved_at: None,
            rng: 0x2545_f491,
            unknown: Vec::new(),
        };
        // the screen is as big as the title's background
        for (p, n) in [("grpe", "9102"), ("grpe", "9999"), ("grpe", "9901")] {
            if let Some(i) = e.picture(p, n.parse().unwrap()) {
                e.width = i.w;
                e.height = i.h;
                break;
            }
        }
        e.win.at.1 = e.height as i32 - 217;
        e.start();
        Ok(e)
    }

    fn start(&mut self) {
        self.stack = vec![Frame { script: 0, pc: 0 }];
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
            eprintln!("liar: {s}");
        }
    }

    fn script(&mut self, n: i32) -> Option<Rc<Script>> {
        if let Some(s) = self.scripts.get(&n) {
            return s.clone();
        }
        let b = self
            .arcs
            .read(self.files.as_mut(), "scr", &format!("{n:04}.gsc"));
        let s = match b.map(|b| Script::parse(&b)) {
            Some(Ok(s)) => Some(Rc::new(s)),
            Some(Err(e)) => {
                self.trace(&format!("{n:04}.gsc: {e}"));
                None
            }
            None => {
                self.trace(&format!("no script {n:04}.gsc"));
                None
            }
        };
        self.scripts.insert(n, s.clone());
        s
    }

    // ---- values ----

    fn var(&self, i: i32) -> i16 {
        self.vars[i as u16 as usize]
    }

    fn set_var(&mut self, i: i32, v: i32) {
        self.vars[i as u16 as usize] = v as i16;
    }

    fn temp(&self, i: i32) -> i16 {
        self.temps[(i as usize) & 63]
    }

    /// An i32 argument: its low 16 bits looked up as often as its high 16 bits say.
    fn val(&self, a: Arg) -> i32 {
        match a {
            Arg::I(v) => {
                let mut x = v as u16 as i16;
                for _ in 0..((v >> 16) as u16).min(8) {
                    x = self.var(x as i32);
                }
                x as i32
            }
            Arg::H(v) => v as i32,
        }
    }

    fn arg(&self, r: &Rec, i: usize) -> i32 {
        r.args.get(i).map_or(0, |&a| self.val(a))
    }

    /// An operand of a computing op: a number, a temporary or a variable, then looked up
    /// `more` times.
    fn operand(&self, mode: u16, more: u16, v: i32) -> i32 {
        let mut x = match mode {
            0 => v as i16,
            1 => self.temp(v),
            _ => self.var(v),
        };
        for _ in 0..more {
            x = self.var(x as i32);
        }
        x as i32
    }

    fn compute(&mut self, r: &Rec) {
        let h = |i: usize| r.args.get(i).map_or(0, |a| a.raw());
        let op = r.op;
        if op & 0xf000 == 0xf000 {
            let b = self.operand((op >> 8) & 3, op & 0xf, h(1));
            self.temps[(h(0) as usize) & 63] = b as i16;
            return;
        }
        let (dst, a, b) = (h(0), h(1), h(2));
        let (am, amore) = ((op >> 10) & 3, (op >> 4) & 0xf);
        let bv = self.operand((op >> 8) & 3, op & 0xf, b);
        let v = if op >> 12 == 1 {
            // assignment to the operand a
            if am == 1 {
                self.temps[(a as usize) & 63] = bv as i16;
            } else {
                let mut i = a as i16;
                for _ in 0..amore {
                    i = self.var(i as i32);
                }
                self.set_var(i as i32, bv);
            }
            bv
        } else {
            let av = self.operand(am, amore, a);
            match op >> 12 {
                2 => (av != 0 || bv != 0) as i32,
                3 => (av != 0 && bv != 0) as i32,
                4 => (av == bv) as i32,
                5 => (av >= bv) as i32,
                6 => (av > bv) as i32,
                7 => (av <= bv) as i32,
                8 => (av < bv) as i32,
                9 => (av != bv) as i32,
                0xa => av.wrapping_add(bv),
                0xb => av.wrapping_sub(bv),
                0xc => av.wrapping_mul(bv),
                0xd => av.checked_div(bv).unwrap_or(0),
                0xe => av.checked_rem(bv).unwrap_or(0),
                _ => 0,
            }
        };
        self.temps[(dst as usize) & 63] = v as i16;
    }

    fn random(&mut self) -> i32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 1 & 0x7fff) as i32
    }

    // ---- running ----

    fn jump(&mut self, s: &Script, off: i32) {
        let pc = s.at(off as u32);
        if let Some(f) = self.stack.last_mut() {
            f.pc = pc;
        }
    }

    /// Calls (a place of) a script with ten values in the variables 10..19.
    fn call(&mut self, script: i32, pc: usize, params: &[i32]) {
        if self.stack.len() >= MAX_STACK {
            self.trace("calls nested too deep");
            return;
        }
        if self.script(script).is_none() {
            return;
        }
        for (k, &p) in params.iter().take(10).enumerate() {
            self.set_var(10 + k as i32, p);
        }
        self.stack.push(Frame { script, pc });
    }

    fn ret(&mut self, v: i32) {
        self.stack.pop();
        self.set_var(0, v);
        if self.stack.is_empty() {
            self.trace("returned from the first script");
            self.back_to_title();
        }
    }

    pub fn frame(&mut self) -> Result<(), String> {
        self.now += FRAME_MS;
        if let Some(f) = &self.fade
            && self.now - f.start >= f.ms
        {
            self.fade = None;
        }
        match self.wait {
            Wait::Fade if self.fade.is_none() => self.wait = Wait::None,
            Wait::Until(t) if self.now >= t || self.skip => self.wait = Wait::None,
            Wait::Click if self.skip && self.view == View::Game => self.advance(),
            _ => {}
        }
        if self.view == View::Game {
            self.run();
        }
        Ok(())
    }

    fn run(&mut self) {
        let mut budget = 200_000;
        while self.wait == Wait::None && !self.ended && budget > 0 {
            budget -= 1;
            let Some(f) = self.stack.last().cloned() else {
                self.back_to_title();
                return;
            };
            let Some(s) = self.script(f.script) else {
                self.ret(0);
                continue;
            };
            let Some(r) = s.recs.get(f.pc).cloned() else {
                // past the end
                self.ret(0);
                continue;
            };
            if let Some(top) = self.stack.last_mut() {
                top.pc += 1;
            }
            self.exec(&s, &r);
        }
    }

    fn exec(&mut self, s: &Script, r: &Rec) {
        let a = |i: usize| self.arg(r, i);
        if r.op >= 0x100 {
            self.compute(r);
            return;
        }
        if self.log && !matches!(r.op, 3 | 5) {
            let args: Vec<i32> = (0..r.args.len()).map(a).collect();
            self.trace(&format!(
                "{:04} {:#x} {:#x} {args:?}",
                self.stack.last().map_or(0, |f| f.script),
                r.off,
                r.op
            ));
        }
        match r.op {
            3 => {
                if self.temps[0] == 0 {
                    self.jump(s, r.args[0].raw());
                }
            }
            4 => {
                if self.temps[0] != 0 {
                    self.jump(s, r.args[0].raw());
                }
            }
            5 => self.jump(s, r.args[0].raw()),
            8 => {
                self.trace("the end");
                self.back_to_title();
            }
            9 => {
                let v = self.random();
                self.temps[(r.args[0].raw() as usize) & 63] = v as i16;
            }
            0xa => {
                self.saved_at = None;
                self.wait = Wait::Click;
            }
            0xc => {
                let n = a(0);
                if self.script(n).is_some()
                    && let Some(f) = self.stack.last_mut()
                {
                    *f = Frame { script: n, pc: 0 };
                }
            }
            0xd => {
                let ms = a(0).max(0) as f64 * 100.0;
                self.wait = Wait::Until(self.now + ms);
            }
            0xe => self.choice(s, r),
            0xf => {
                let params: Vec<i32> = (2..12).map(a).collect();
                self.call(a(0), 0, &params);
            }
            0x10 => self.ret(a(0)),
            0x14 => {
                let pic = a(0);
                if (1..1000).contains(&pic) {
                    self.set_var(9000 + pic, 1);
                }
                let dir = self.bg_dir.clone();
                self.change(0, |l| {
                    l.pic = pic.max(0);
                    l.dir = dir;
                    l.x = 0;
                    l.y = 0;
                });
            }
            0x15 => self.change(0, |l| l.pic = 0),
            0x1a => {
                if self.frozen.is_none() {
                    self.frozen = Some(self.stage_draws());
                }
            }
            0x1b => self.frozen = None,
            0x1c => {
                let ms = a(1).max(0) as f64 * UNIT_MS;
                let rule = self.rule(a(0));
                self.present_ruled(ms, rule);
            }
            0x1e => {
                let (slot, pic, x, y, fade) = (a(0), a(1), a(2), a(3), a(4));
                self.place(slot, pic, x, y, fade);
            }
            0x24 => {
                let (slot, fade) = (a(0), a(1));
                let slots = self.slots(slot);
                let old = self.unfrozen_snapshot(fade);
                for i in slots {
                    if i > 0 {
                        self.layers[i].pic = 0;
                        self.buttons.remove(&i);
                    }
                }
                self.after_change(old, fade);
            }
            0x25 => {
                let (slot, on) = (a(0), a(1));
                for i in self.slots(slot) {
                    self.layers[i].hidden = on == 0;
                }
            }
            0x26 => {
                let (slot, cx, cy, clamp) = (a(0), a(1), a(2), a(3));
                for i in self.slots(slot) {
                    let l = &mut self.layers[i];
                    l.centre = (cx != 0, cy != 0);
                    l.clamp = clamp != 0;
                }
            }
            0x28 => {
                let (slot, g) = (a(0), a(1));
                if let Ok(i) = usize::try_from(slot)
                    && (1..LAYERS).contains(&i)
                {
                    self.layers[i].group = g;
                }
            }
            0x2a => {
                self.window = a(1) != 0;
                if !self.window {
                    self.clear_text();
                }
            }
            0x3c => self.play_music(a(0)),
            0x3d => {
                let ms = (a(1).max(0) as f32 * 10.0).min(3000.0);
                self.stop_music(ms);
            }
            0x3e => {
                let (ch, n) = (a(0), a(1));
                self.se.insert(ch, (n, false));
            }
            0x3f => {
                let (ch, count) = (a(0), a(1));
                if let Some(&(n, _)) = self.se.get(&ch) {
                    let forever = count >= 999 || count == 0;
                    self.se.insert(ch, (n, forever));
                    self.play_se(ch, n, forever);
                }
            }
            0x40 => {
                let ch = a(0);
                self.se.remove(&ch);
                self.push_sound(&format!("se{ch}"), None, 0.0, false, 200.0);
            }
            0x46 | 0x47 => {
                if let Ok(i) = usize::try_from(a(0))
                    && i < LAYERS
                {
                    let hover = self.buttons.get(&i).and_then(|b| b.hover);
                    self.buttons.insert(
                        i,
                        Button {
                            value: a(1),
                            system: r.op == 0x47,
                            hover,
                        },
                    );
                }
            }
            0x48 => {
                if let Ok(i) = usize::try_from(a(0)) {
                    self.buttons.remove(&i);
                }
            }
            0x49 => {
                if self.buttons.values().any(|b| !b.system || b.value == 2) {
                    self.saved_at = None;
                    self.wait = Wait::Buttons;
                } else {
                    self.set_var(0, 0);
                }
            }
            0x4b => {
                if let Ok(i) = usize::try_from(a(0))
                    && i < LAYERS
                {
                    let hover = Some((a(1), a(2), a(3)));
                    self.buttons
                        .entry(i)
                        .or_insert(Button {
                            value: 0,
                            system: true,
                            hover: None,
                        })
                        .hover = hover;
                }
            }
            0x51 => {
                let voice = a(1);
                let name = s.s(a(4)).to_string();
                let text = s.s(a(5)).to_string();
                self.message(voice, &name, &text, false);
            }
            0x52 => {
                let text = s.s(a(4)).to_string();
                self.message(0, "", &text, true);
            }
            0x53 => self.clear_text(),
            0x5a => self.win.at = (a(1), a(2)),
            0x5b => self.win.text = [a(1), a(2), a(3), a(4)],
            0x60 => {
                let n = a(1);
                if (12..=64).contains(&n) {
                    self.win.size = n - 2;
                }
            }
            0x64 => self.win.mark = (a(1), a(2)),
            0x68 => self.win.rgb = [a(1) as u8, a(2) as u8, a(3) as u8],
            0x79 => {
                let (slot, d) = (a(0), s.s(a(1)).to_lowercase());
                if slot == 0 {
                    for l in self.layers.iter_mut().skip(1) {
                        l.dir = d.clone();
                    }
                } else if let Ok(i) = usize::try_from(slot)
                    && i < LAYERS
                {
                    self.layers[i].dir = d;
                }
            }
            0xc8 => {
                let params: Vec<i32> = (1..11).map(a).collect();
                let pc = s.at(r.args[0].raw() as u32);
                let script = self.stack.last().map_or(0, |f| f.script);
                self.call(script, pc, &params);
            }
            op => {
                if !self.unknown.contains(&op) {
                    self.unknown.push(op);
                    self.trace(&format!("op {op:#x} left out"));
                }
            }
        }
    }

    /// The layers an op names: 0 all of 1..99, above 100 the ones of group n % 100.
    fn slots(&self, slot: i32) -> Vec<usize> {
        match slot {
            0 => (1..LAYERS).collect(),
            1..=99 => vec![slot as usize],
            s if s > 100 => {
                let g = s % 100;
                (1..LAYERS).filter(|&i| self.layers[i].group == g).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Before a change of the pictures outside a held screen: the screen as it is, for a
    /// fade of `fade` units.
    fn unfrozen_snapshot(&mut self, fade: i32) -> Option<Vec<Draw>> {
        (self.frozen.is_none() && fade > 0 && !self.skip).then(|| self.stage_draws())
    }

    fn after_change(&mut self, old: Option<Vec<Draw>>, fade: i32) {
        if let Some(from) = old {
            self.frozen = Some(from);
            self.present(fade as f64 * UNIT_MS);
        }
    }

    fn change(&mut self, slot: usize, f: impl FnOnce(&mut Layer)) {
        f(&mut self.layers[slot]);
    }

    fn place(&mut self, slot: i32, pic: i32, x: i32, y: i32, fade: i32) {
        let Ok(i) = usize::try_from(slot) else {
            return;
        };
        if !(1..LAYERS).contains(&i) {
            return;
        }
        let old = self.unfrozen_snapshot(fade);
        let l = &mut self.layers[i];
        l.pic = pic.max(0);
        l.x = x;
        l.y = y;
        l.hidden = false;
        if l.dir.is_empty() {
            l.dir = "grpo".into();
        }
        self.after_change(old, fade);
    }

    fn message(&mut self, voice: i32, name: &str, text: &str, append: bool) {
        if !append {
            self.clear_text();
            if !name.is_empty() {
                self.name = Some(name.to_string());
            }
        }
        self.text.push_str(text);
        let w = self.win.text[2].max(200);
        self.lines = wrap(&self.text, self.win.size, w);
        if voice > 0 {
            let path = if voice >= 10000 {
                format!("{}.xfl/{:04}.wav", voice / 10000, voice % 10000)
            } else {
                format!("{voice:04}.wav")
            };
            let d = self.arcs.read(self.files.as_mut(), "voice", &path);
            if d.is_none() {
                self.trace(&format!("no voice {path}"));
            }
            if d.is_some() && !self.skip {
                self.push_sound("voice", d.map(xfl::unwrap_ogg), 1.0, false, 0.0);
            }
        }
        self.window = true;
        self.saved_at = Some(self.stack.clone());
        self.wait = Wait::Click;
    }

    fn choice(&mut self, s: &Script, r: &Rec) {
        let n = (r.args[0].raw().clamp(0, 5)) as usize;
        let items = (0..n)
            .map(|k| {
                let t = r.args.get(2 + k).map_or(0, |a| a.raw()) as u32;
                let text = s.s(self.arg(r, 7 + k)).to_string();
                (t, text)
            })
            .collect();
        let q = s.s(self.arg(r, 1)).to_string();
        self.clear_text();
        if !q.is_empty() {
            self.text = q.clone();
            self.lines = wrap(&q, self.win.size, self.win.text[2].max(200));
            self.window = true;
        }
        self.skip = false;
        self.saved_at = None;
        self.wait = Wait::Choice(items);
    }

    fn clear_text(&mut self) {
        self.lines.clear();
        self.text.clear();
        self.name = None;
    }

    // ---- pictures ----

    /// `%04d.wcg` of a pack, else of any picture pack.
    fn picture(&mut self, dir: &str, pic: i32) -> Option<Rc<Image>> {
        let key = format!("{dir}/{pic}");
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let name = format!("{pic:04}.wcg");
        let mut b = self.arcs.read(self.files.as_mut(), dir, &name);
        if b.is_none() {
            let mut packs: Vec<String> = self
                .arcs
                .packs
                .keys()
                .filter(|p| p.starts_with("grp") && self.arcs.has(p, &name))
                .cloned()
                .collect();
            packs.sort();
            if let Some(p) = packs.first() {
                b = self.arcs.read(self.files.as_mut(), p, &name);
            }
        }
        let img = match b.map(|b| wcg::decode(&b)) {
            Some(Ok(i)) => Some(Rc::new(i)),
            Some(Err(e)) => {
                self.trace(&format!("{dir}/{name}: {e}"));
                None
            }
            None => {
                self.trace(&format!("no picture {dir}/{name}"));
                None
            }
        };
        if self.images.len() > 48 {
            self.images.clear();
        }
        self.images.insert(key, img.clone());
        img
    }

    /// A picture of an LWG set of grps.xfl, and where it goes in the set.
    fn part(&mut self, set: &str, name: &str) -> Option<(Rc<Image>, i32, i32)> {
        let key = format!("{set}:{name}");
        let b = self.arcs.read(self.files.as_mut(), "grps", set)?;
        let l = wcg::Lwg::parse(&b)?;
        let (p, data) = l.part(&b, name)?;
        let (x, y) = (p.x, p.y);
        if let Some(i) = self.parts.get(&key) {
            return i.clone().map(|i| (i, x, y));
        }
        let img = wcg::decode(data).ok().map(Rc::new);
        self.parts.insert(key, img.clone());
        img.map(|i| (i, x, y))
    }

    /// The rule picture of a 1C kind: grps:efNN.msk (a BMP), for the kinds that have one.
    fn rule(&mut self, kind: i32) -> Option<Rc<Image>> {
        if !(10..100).contains(&kind) || self.skip {
            return None;
        }
        let key = format!("rule/{kind}");
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let name = format!("ef{kind:02}.msk");
        let img = self
            .arcs
            .read(self.files.as_mut(), "grps", &name)
            .and_then(|b| vncommon::bmp::decode(&b))
            .map(Rc::new);
        if img.is_none() {
            self.trace(&format!("no mask grps/{name}"));
        }
        if self.images.len() > 48 {
            self.images.clear();
        }
        self.images.insert(key, img.clone());
        img
    }

    fn present(&mut self, ms: f64) {
        self.present_ruled(ms, None);
    }

    fn present_ruled(&mut self, ms: f64, rule: Option<Rc<Image>>) {
        match self.frozen.take() {
            Some(from) if ms > 0.0 && !self.skip => {
                self.fade = Some(Fade {
                    from,
                    start: self.now,
                    ms,
                    rule,
                });
                if self.wait == Wait::None {
                    self.wait = Wait::Fade;
                }
            }
            _ => {}
        }
    }

    fn solid(&self, rgba: [u8; 4]) -> Draw {
        let img = solid_image(rgba);
        let mut d = draw(&img, 0.0, 0.0, 1.0);
        d.dst = [0.0, 0.0, self.width as f32, self.height as f32];
        d
    }

    /// Where a layer's picture goes.
    fn layer_rect(&self, l: &Layer, img: &Image) -> (i32, i32) {
        let (w, h) = (img.w as i32, img.h as i32);
        let mut x = l.x - if l.centre.0 { w / 2 } else { 0 };
        let mut y = l.y - if l.centre.1 { h / 2 } else { 0 };
        if l.clamp {
            x = x.clamp((self.width as i32 - w).min(0), 0.max(self.width as i32 - w));
            y = y.clamp(
                (self.height as i32 - h).min(0),
                0.max(self.height as i32 - h),
            );
        }
        (x, y)
    }

    /// Reads the pictures of the layers that are not read yet.
    fn load_layers(&mut self) {
        for i in 0..LAYERS {
            let l = &self.layers[i];
            if l.pic <= 0 || l.hidden {
                continue;
            }
            let dir = if i == 0 {
                self.bg_dir.clone()
            } else {
                l.dir.clone()
            };
            let pic = l.pic;
            if !self.images.contains_key(&format!("{dir}/{pic}")) {
                self.picture(&dir, pic);
            }
        }
    }

    /// The screen: the layers, then the text window.
    fn stage_draws(&mut self) -> Vec<Draw> {
        self.load_layers();
        let mut out = vec![self.solid([0, 0, 0, 255])];
        for (i, l) in self.layers.iter().enumerate() {
            if l.pic <= 0 || l.hidden {
                continue;
            }
            let dir = if i == 0 { &self.bg_dir } else { &l.dir };
            let Some(Some(img)) = self.images.get(&format!("{dir}/{}", l.pic)) else {
                continue;
            };
            let (x, y) = self.layer_rect(l, img);
            out.push(draw(img, x as f32, y as f32, 1.0));
        }
        if self.window {
            self.window_draws(&mut out);
        }
        out
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = match &self.frozen {
            Some(f) => f.clone(),
            None => self.stage_draws(),
        };
        self.rules.clear();
        if let Some(f) = &self.fade {
            let p = ((self.now - f.start) / f.ms.max(1.0)).clamp(0.0, 1.0) as f32;
            for d in &f.from {
                let mut d = d.clone();
                match &f.rule {
                    Some(r) => self.rules.push((
                        out.len(),
                        Rule {
                            image: r.clone(),
                            progress: p,
                            vague: RULE_VAGUE,
                            appear: false,
                        },
                    )),
                    None => d.place.alpha *= 1.0 - p,
                }
                out.push(d);
            }
        }
        match self.wait.clone() {
            Wait::Choice(items) => self.choice_draws(&items, &mut out),
            Wait::Buttons => self.button_draws(&mut out),
            Wait::Click if self.window && self.frozen.is_none() && self.fade.is_none() => {
                self.mark_draws(&mut out)
            }
            _ => {}
        }
        match self.view {
            View::Menu => self.menu_draws(&mut out),
            View::Slots { save } => self.slot_draws(save, &mut out),
            View::Game => {}
        }
        out
    }
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
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

impl Engine {
    fn label(&mut self, s: &str, rgb: [u8; 3], size: i32) -> Rc<Image> {
        let key = (s.to_string(), rgb, size);
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let img = match &self.font {
            Some(f) => {
                Rc::new(Font::outline(f.clone(), size).render(&nexas::sjis::encode(s), rgb, 2))
            }
            None => Rc::new(Image::new(1, 1)),
        };
        if self.text_cache.len() > 200 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, img.clone());
        img
    }

    /// The text window (tbox00.lwg of grps.xfl), the speaker's name and the text.
    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let (wx, wy) = self.win.at;
        match self.part("tbox00.lwg", "back") {
            Some((img, x, y)) => out.push(draw(&img, (wx + x) as f32, (wy + y) as f32, 1.0)),
            None => {
                let mut d = self.solid([0, 0, 0, 150]);
                d.dst = [0.0, wy as f32, self.width as f32, self.height as f32];
                out.push(d);
            }
        }
        let size = self.win.size;
        let [tx, ty, _, _] = self.win.text;
        if let Some(n) = self.name.clone() {
            let img = self.label(&format!("【{n}】"), [255, 236, 170], size);
            out.push(draw(
                &img,
                (wx + tx - size / 2) as f32,
                (wy + 2) as f32,
                1.0,
            ));
        }
        let step = size + size / 4;
        let rgb = self.win.rgb;
        for (k, l) in self.lines.clone().iter().enumerate() {
            let img = self.label(l, rgb, size);
            out.push(draw(
                &img,
                (wx + tx - 2) as f32,
                (wy + ty + k as i32 * step - 2) as f32,
                1.0,
            ));
        }
    }

    /// The click mark (wait00.lwg) while a text waits.
    fn mark_draws(&mut self, out: &mut Vec<Draw>) {
        let (wx, wy) = self.win.at;
        let (mx, my) = self.win.mark;
        if let Some((img, x, y)) = self.part("wait00.lwg", "body") {
            let a = 0.6 + 0.4 * ((self.now / 300.0).sin() as f32).abs();
            out.push(draw(
                &img,
                (wx + mx + x - 30) as f32,
                (wy + my + y - 30) as f32,
                a,
            ));
        }
    }

    fn choice_rects(&mut self, n: usize) -> Vec<[i32; 4]> {
        let (w, h) = (self.width as i32, self.height as i32);
        let (bw, bh) = self
            .part("sel_a00.lwg", "body")
            .map_or((720, 50), |(i, _, _)| (i.w as i32, i.h as i32));
        let gap = bh / 2;
        let top = (h - (bh + gap) * n as i32) / 2 - h / 10;
        (0..n as i32)
            .map(|i| {
                let y = top + i * (bh + gap);
                [(w - bw) / 2, y, (w + bw) / 2, y + bh]
            })
            .collect()
    }

    fn choice_draws(&mut self, items: &[(u32, String)], out: &mut Vec<Draw>) {
        let rects = self.choice_rects(items.len());
        let size = self.win.size;
        for ((_, t), r) in items.iter().zip(rects) {
            let over = inside(r, self.pointer.0, self.pointer.1);
            match self.part("sel_a00.lwg", "body") {
                Some((img, _, _)) => out.push(draw(
                    &img,
                    r[0] as f32,
                    r[1] as f32,
                    if over { 1.0 } else { 0.8 },
                )),
                None => {
                    let mut d = self.solid([30, 40, 60, 210]);
                    d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
                    out.push(d);
                }
            }
            let rgb = if over {
                [255, 120, 60]
            } else {
                [255, 255, 255]
            };
            let img = self.label(t, rgb, size);
            let x = (r[0] + r[2]) as f32 / 2.0 - img.w as f32 / 2.0;
            let y = (r[1] + r[3]) as f32 / 2.0 - img.h as f32 / 2.0;
            out.push(draw(&img, x, y, 1.0));
        }
    }

    /// The buttons that answer a click and where they are.
    fn button_rects(&mut self) -> Vec<(usize, [i32; 4])> {
        let mut out = Vec::new();
        let slots: Vec<usize> = self.buttons.keys().copied().collect();
        for i in slots {
            let b = &self.buttons[&i];
            if b.system && b.value != 2 {
                continue;
            }
            let l = self.layers[i].clone();
            if l.pic <= 0 || l.hidden {
                continue;
            }
            let Some(img) = self.picture(&l.dir, l.pic) else {
                continue;
            };
            let (x, y) = self.layer_rect(&l, &img);
            out.push((i, [x, y, x + img.w as i32, y + img.h as i32]));
        }
        out
    }

    fn button_draws(&mut self, out: &mut Vec<Draw>) {
        let (px, py) = self.pointer;
        for (i, r) in self.button_rects() {
            if !inside(r, px, py) {
                continue;
            }
            let Some((pic, x, y)) = self.buttons.get(&i).and_then(|b| b.hover) else {
                continue;
            };
            let dir = self.layers[i].dir.clone();
            if let Some(img) = self.picture(&dir, pic) {
                let l = Layer {
                    x,
                    y,
                    ..self.layers[i].clone()
                };
                let (x, y) = self.layer_rect(&l, &img);
                out.push(draw(&img, x as f32, y as f32, 1.0));
            }
        }
    }

    // ---- menus ----

    fn button(&mut self, t: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let over = inside(r, self.pointer.0, self.pointer.1);
        let mut d = self.solid(if over {
            [60, 110, 160, 230]
        } else {
            [20, 30, 50, 210]
        });
        d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        out.push(d);
        let img = self.label(t, [255, 255, 255], 24);
        let x = (r[0] + r[2]) as f32 / 2.0 - img.w as f32 / 2.0;
        let y = (r[1] + r[3]) as f32 / 2.0 - img.h as f32 / 2.0;
        out.push(draw(&img, x, y, 1.0));
    }

    fn menu_items(&self) -> Vec<(&'static str, [i32; 4])> {
        let (w, h) = (self.width as i32, self.height as i32);
        let names = ["セーブ", "ロード", "タイトルへ戻る", "ゲームに戻る"];
        let (bw, bh, gap) = (360, 56, 20);
        let top = (h - (bh * 4 + gap * 3)) / 2;
        names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let y = top + i as i32 * (bh + gap);
                (*n, [(w - bw) / 2, y, (w + bw) / 2, y + bh])
            })
            .collect()
    }

    fn menu_draws(&mut self, out: &mut Vec<Draw>) {
        out.push(self.solid([0, 0, 0, 150]));
        for (t, r) in self.menu_items() {
            self.button(t, r, out);
        }
    }

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        let (w, h) = (self.width as i32, self.height as i32);
        let step = ((h - 60) / (SLOTS as i32 + 1)).min(58);
        let (bw, bh, gap) = (w - 240, step - 10, 10);
        let top = (h - (bh + gap) * (SLOTS as i32 + 1)) / 2 + 16;
        (0..=SLOTS as i32)
            .map(|i| {
                let y = top + i * (bh + gap);
                [120, y, 120 + bw, y + bh]
            })
            .collect()
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        out.push(self.solid([0, 0, 0, 200]));
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
        let head = self.label(if save { "セーブ" } else { "ロード" }, [255, 255, 255], 30);
        out.push(draw(&head, 120.0, (rects[0][1] - 44) as f32, 1.0));
    }

    /// The frame as one picture (for the native runner).
    pub fn render(&mut self) -> Image {
        let ds = self.draws();
        vncommon::rule::render_ruled(self.width, self.height, &ds, &self.rules)
    }

    /// The mask wipes of the last [`Engine::draws`]: (draw index, rule).
    pub fn rules(&self) -> &[(usize, Rule)] {
        &self.rules
    }

    // ---- sounds ----

    fn push_sound(
        &mut self,
        channel: &str,
        data: Option<Vec<u8>>,
        volume: f32,
        repeat: bool,
        fade_ms: f32,
    ) {
        self.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data,
            volume,
            repeat,
            fade_ms,
        });
    }

    fn play_music(&mut self, n: i32) {
        if self.bgm == Some(n) {
            return;
        }
        let d = self
            .arcs
            .read(self.files.as_mut(), "bgm", &format!("track{n:02}.wav"));
        if d.is_none() {
            self.trace(&format!("no music Track{n:02}"));
        }
        self.bgm = Some(n);
        self.push_sound("bgm", d.map(xfl::unwrap_ogg), 1.0, true, 0.0);
    }

    fn stop_music(&mut self, ms: f32) {
        self.bgm = None;
        self.push_sound("bgm", None, 0.0, false, ms);
    }

    fn play_se(&mut self, ch: i32, n: i32, forever: bool) {
        let d = self
            .arcs
            .read(self.files.as_mut(), "wav", &format!("{n:04}.wav"));
        if d.is_none() {
            self.trace(&format!("no sound {n:04}"));
        }
        if !forever && self.skip {
            return;
        }
        self.push_sound(
            &format!("se{ch}"),
            d.map(xfl::unwrap_ogg),
            1.0,
            forever,
            0.0,
        );
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    fn advance(&mut self) {
        if self.wait == Wait::Click {
            self.wait = Wait::None;
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
                    Some(0) => self.view = View::Slots { save: true },
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
        match self.wait.clone() {
            Wait::Click => {
                if self.fade.is_some() {
                    self.fade = None;
                    return;
                }
                self.push_sound("voice", None, 0.0, false, 0.0);
                self.advance();
            }
            Wait::Until(_) => self.wait = Wait::None,
            Wait::Fade => {
                self.fade = None;
                self.wait = Wait::None;
            }
            Wait::Choice(items) => {
                let hit = self
                    .choice_rects(items.len())
                    .into_iter()
                    .position(|r| inside(r, x, y));
                if let Some(i) = hit {
                    self.wait = Wait::None;
                    self.clear_text();
                    let f = self.stack.last().cloned();
                    if let Some(f) = f
                        && let Some(s) = self.script(f.script)
                    {
                        self.jump(&s, items[i].0 as i32);
                    }
                }
            }
            Wait::Buttons => {
                let hit = self
                    .button_rects()
                    .into_iter()
                    .find(|(_, r)| inside(*r, x, y));
                if let Some((i, _)) = hit {
                    let b = self.buttons[&i].clone();
                    if b.system {
                        // the load screen
                        self.wait = Wait::Load;
                        self.view = View::Slots { save: false };
                    } else {
                        self.set_var(0, b.value);
                        self.wait = Wait::None;
                    }
                }
            }
            Wait::Load | Wait::None => {}
        }
    }

    fn close_slots(&mut self) {
        if self.wait == Wait::Load {
            self.wait = Wait::Buttons;
            self.view = View::Game;
        } else {
            self.view = View::Menu;
        }
    }

    pub fn cancel(&mut self) {
        match self.view {
            View::Menu => self.view = View::Game,
            View::Slots { .. } => self.close_slots(),
            View::Game => {
                self.skip = false;
                if self.wait == Wait::Click && self.saved_at.is_some() {
                    self.view = View::Menu;
                }
            }
        }
    }

    /// 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip.
    pub fn key(&mut self, k: usize) {
        match k {
            4 if self.view == View::Game && self.wait == Wait::Click => self.advance(),
            5 => self.cancel(),
            6 => self.skip = !self.skip,
            _ => {}
        }
    }

    pub fn wheel(&mut self, up: bool) {
        if !up && self.view == View::Game && self.wait == Wait::Click {
            self.advance();
        }
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    fn reset(&mut self) {
        self.stack.clear();
        for l in self.layers.iter_mut() {
            *l = Layer::default();
        }
        self.buttons.clear();
        self.window = false;
        self.clear_text();
        self.view = View::Game;
        self.frozen = None;
        self.fade = None;
        self.skip = false;
        self.saved_at = None;
        self.wait = Wait::None;
        self.temps = [0; 64];
        for ch in self.se.keys().copied().collect::<Vec<_>>() {
            self.push_sound(&format!("se{ch}"), None, 0.0, false, 200.0);
        }
        self.se.clear();
        self.push_sound("voice", None, 0.0, false, 0.0);
    }

    fn back_to_title(&mut self) {
        self.reset();
        self.stop_music(300.0);
        self.vars.iter_mut().for_each(|v| *v = 0);
        self.start();
    }

    // ---- saves ----

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&format!("slot{i:02}"))?;
        Slot::parse(&String::from_utf8_lossy(&b))
    }

    fn save(&mut self, i: usize) {
        let Some(at) = self.saved_at.clone() else {
            return;
        };
        let mut s = String::new();
        let title = format!(
            "{}{}",
            self.name
                .as_ref()
                .map_or(String::new(), |n| format!("【{n}】")),
            self.text.replace("^n", "")
        );
        let title: String = title.chars().take(40).collect();
        s += &format!("title {title}\n");
        for f in &at {
            s += &format!("frame {} {}\n", f.script, f.pc);
        }
        let vars: Vec<String> = self
            .vars
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0)
            .map(|(i, v)| format!("{i}:{v}"))
            .collect();
        s += &format!("vars {}\n", vars.join(" "));
        let temps: Vec<String> = self.temps.iter().map(|v| v.to_string()).collect();
        s += &format!("temps {}\n", temps.join(" "));
        for (i, l) in self.layers.iter().enumerate() {
            s += &format!(
                "layer {i} {} {} {} {} {} {} {} {} {} {}\n",
                l.pic,
                l.x,
                l.y,
                l.centre.0 as u8,
                l.centre.1 as u8,
                l.clamp as u8,
                l.group,
                l.hidden as u8,
                if l.dir.is_empty() { "-" } else { &l.dir },
                0
            );
        }
        if let Some(b) = self.bgm {
            s += &format!("bgm {b}\n");
        }
        for (c, (n, forever)) in &self.se {
            if *forever {
                s += &format!("se {c} {n}\n");
            }
        }
        if let Some(n) = &self.name {
            s += &format!("name {n}\n");
        }
        s += &format!("text {}\n", self.text.replace('\n', "^n"));
        self.files.write_save(&format!("slot{i:02}"), s.as_bytes());
        self.view = View::Game;
    }

    fn load_slot(&mut self, i: usize) {
        let Some(slot) = self.read_slot(i) else {
            return;
        };
        self.reset();
        self.stop_music(0.0);
        self.stack = slot.frames;
        self.vars.iter_mut().for_each(|v| *v = 0);
        for (i, v) in slot.vars {
            self.vars[i] = v;
        }
        self.temps = slot.temps;
        for (i, l) in slot.layers {
            if i < LAYERS {
                self.layers[i] = l;
            }
        }
        if let Some(b) = slot.bgm {
            self.play_music(b);
        }
        for (c, n) in slot.se {
            self.se.insert(c, (n, true));
            self.play_se(c, n, true);
        }
        self.window = true;
        self.name = slot.name;
        self.text = slot.text;
        self.lines = wrap(&self.text, self.win.size, self.win.text[2].max(200));
        self.saved_at = Some(self.stack.clone());
        self.wait = Wait::Click;
        self.view = View::Game;
        self.frozen = Some(vec![self.solid([0, 0, 0, 255])]);
        self.present(400.0);
    }
}

struct Slot {
    title: String,
    frames: Vec<Frame>,
    vars: Vec<(usize, i16)>,
    temps: [i16; 64],
    layers: Vec<(usize, Layer)>,
    bgm: Option<i32>,
    se: Vec<(i32, i32)>,
    name: Option<String>,
    text: String,
}

impl Slot {
    fn parse(s: &str) -> Option<Slot> {
        let mut slot = Slot {
            title: String::new(),
            frames: Vec::new(),
            vars: Vec::new(),
            temps: [0; 64],
            layers: Vec::new(),
            bgm: None,
            se: Vec::new(),
            name: None,
            text: String::new(),
        };
        for line in s.lines() {
            let (k, rest) = line.split_once(' ').unwrap_or((line, ""));
            let nums = || -> Vec<i32> { rest.split(' ').filter_map(|x| x.parse().ok()).collect() };
            match k {
                "title" => slot.title = rest.to_string(),
                "frame" => {
                    let v = nums();
                    if v.len() == 2 {
                        slot.frames.push(Frame {
                            script: v[0],
                            pc: v[1].max(0) as usize,
                        });
                    }
                }
                "vars" => {
                    for p in rest.split(' ') {
                        if let Some((i, v)) = p.split_once(':')
                            && let (Ok(i), Ok(v)) = (i.parse::<usize>(), v.parse::<i16>())
                            && i < 0x10000
                        {
                            slot.vars.push((i, v));
                        }
                    }
                }
                "temps" => {
                    for (k, v) in nums().into_iter().take(64).enumerate() {
                        slot.temps[k] = v as i16;
                    }
                }
                "layer" => {
                    let f: Vec<&str> = rest.split(' ').collect();
                    let n = |i: usize| f.get(i).and_then(|x| x.parse::<i32>().ok()).unwrap_or(0);
                    if f.len() >= 10 {
                        let dir = if f[9] == "-" {
                            String::new()
                        } else {
                            f[9].to_string()
                        };
                        slot.layers.push((
                            n(0).max(0) as usize,
                            Layer {
                                pic: n(1),
                                x: n(2),
                                y: n(3),
                                centre: (n(4) != 0, n(5) != 0),
                                clamp: n(6) != 0,
                                group: n(7),
                                hidden: n(8) != 0,
                                dir,
                            },
                        ));
                    }
                }
                "bgm" => slot.bgm = rest.parse().ok(),
                "se" => {
                    let v = nums();
                    if v.len() == 2 {
                        slot.se.push((v[0], v[1]));
                    }
                }
                "name" => slot.name = Some(rest.to_string()),
                "text" => slot.text = rest.to_string(),
                _ => {}
            }
        }
        (!slot.frames.is_empty()).then_some(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::wrap;

    #[test]
    fn wrapping() {
        assert_eq!(wrap("あいう^nえお", 10, 100), vec!["あいう", "えお"]);
        // 3 full-width characters a line; the closing mark stays on its line
        assert_eq!(wrap("あいうえ。", 10, 30), vec!["あいう", "え。"]);
        assert_eq!(wrap("あいう。え", 10, 30), vec!["あいう。", "え"]);
    }
}
