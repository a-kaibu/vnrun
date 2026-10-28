//! Runs the scripts a frame at a time, from `_START` (the logo, then the title of STITLE with
//! its click map, then SMAIN, which calls the story's scripts).
//!
//! Script ops (the first byte of a record; u16 s = a string of the script), as read from the
//! command switch of the game's script compiler:
//!
//! | op | args | |
//! |----|------|-|
//! | 0 | | end of a text line |
//! | 1 | s | text |
//! | 2, 3, 7, 9 | | notes, blank lines, label marks, block ends |
//! | 4 | | wait for a click, then clear the text |
//! | 6 | var `=` expr | set a variable |
//! | 8 | flag, place | after IF / ELSE: where the false branch goes; after SELECT etc.: the block's end |
//! | 10 | s, expr | a case of SELECT (s: its text), CLICKMAP or SWITCH (expr: its condition) |
//! | 13 | s | the speaker's name |
//! | 16 | mode, s | background (a `#name` of _BGSET or a picture); mode 0x80 shows it with a fade (0x80 | n: a wipe by BG_FADEnn) |
//! | 18 | mode, n, s.. | standing pictures (`#name`s of _SPRSET) |
//! | 19 | mode | clear the standing pictures |
//! | 20 / 21 | s | voice / sound |
//! | 22 / 23 | s, ch / ch | looping sound / stop it |
//! | 25 | i16 | music Mnn (0 stops it, below 0 fades it out) |
//! | 26 | [flag,] s | jump to `script`, `script:label` or `label` |
//! | 27 / 108 | s | call `script` or `script:label` |
//! | 28 | s | go to a label of this script |
//! | 29 | expr | IF: with an 8 after it a block, else the next record only |
//! | 30 | | ELSE |
//! | 31 | | return |
//! | 33 | ffff, n, places.., end | SELECT: the player picks a case |
//! | 34 | s, n, places.., end | CLICKMAP: the player clicks a place of the map s of _MAP |
//! | 37 | | close the text window |
//! | 38 | ms | wait |
//! | 42 | n, places.., end | SWITCH: the first case whose condition holds |
//! | 44 | s | the title of a save |
//! | 50 / 51 | | quit / the load screen |
//! | 56 | mode, s | a picture over everything (ffff: none) |
//! | 79 | mode | show the changes |
//! | 83 | | wait for a click |
//! | 106 | s | a background voice ("OFF" stops it) |

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place, Rule};

use crate::arc::Archives;
use crate::script::{self, Rec, Script, Tok};

const FRAME_MS: f64 = 1000.0 / 60.0;
const SLOTS: usize = 10;
const FADE_MS: f64 = 500.0;

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

/// A picture of the game: PRS ("YB"), else JPEG / PNG / BMP.
pub fn decode_picture(b: &[u8]) -> Result<Image, String> {
    if b.starts_with(b"YB") {
        tmrhiro::marble::prs::decode(b)
    } else {
        Image::decode(b)
    }
}

#[derive(Clone, PartialEq, Debug)]
enum Wait {
    None,
    Click,
    Until(f64),
    Fade,
    /// a SELECT: its cases (records) and their texts
    Select(Vec<(usize, String)>),
    /// a CLICKMAP: the map and its cases
    Map(String, Vec<usize>),
    /// the load screen of the title (51)
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
    /// a mode 0x80 | n (n > 0): the old picture goes where the rule BG_FADEnn is dark first
    rule: Option<Rc<Image>>,
}

/// How soft a rule wipe's edge is (part of the rule's brightness range).
const RULE_VAGUE: f32 = 0.25;

/// One running script and its next record.
#[derive(Clone, Debug)]
struct Frame {
    script: String,
    pc: usize,
}

/// A standing picture: its `#name`, the body and the face (at x, y on the body).
#[derive(Clone)]
struct Sprite {
    name: String,
    body: Rc<Image>,
    face: Option<(Rc<Image>, i32, i32)>,
}

/// A face picture and its place on the body.
type Face = (String, i32, i32);

/// A character of _SPRSET: `PREFIX:name,picture` then `BODY:name,suffix` and
/// `FACE:name,suffix,x,y` lines.
struct SprDef {
    prefix: String,
    base: String,
    bodies: Vec<(String, String)>,
    faces: Vec<(String, String, i32, i32)>,
}

/// A click map of _MAP: its focus picture and places (rectangle, picture, focus picture).
#[derive(Clone, Default)]
struct ClickMap {
    focus: String,
    items: Vec<([i32; 4], String, String)>,
}

pub struct Engine {
    files: Box<dyn Files>,
    arcs: Archives,
    pub width: u32,
    pub height: u32,
    pub log: bool,
    font: Option<Rc<fontdue::Font>>,
    scripts: HashMap<String, Rc<Script>>,
    bgset: HashMap<String, String>,
    sprset: Vec<SprDef>,
    maps: HashMap<String, ClickMap>,
    stack: Vec<Frame>,
    vars: BTreeMap<(u8, u8), i32>,
    bg: Option<(String, Rc<Image>)>,
    sprites: Vec<Sprite>,
    over: Option<(String, Rc<Image>)>,
    window: bool,
    name: Option<String>,
    lines: Vec<String>,
    line: String,
    save_title: String,
    wait: Wait,
    view: View,
    frozen: Option<Vec<Draw>>,
    fade: Option<Fade>,
    rules: Vec<(usize, Rule)>,
    now: f64,
    bgm: Option<i32>,
    loops: BTreeMap<u8, String>,
    sounds: Vec<SoundEvent>,
    images: HashMap<String, Option<Rc<Image>>>,
    text_cache: HashMap<(String, [u8; 3], i32), Rc<Image>>,
    pointer: (i32, i32),
    skip: bool,
    ended: bool,
    /// where a save made now starts again: the stack after the shown text
    saved_at: Option<Vec<Frame>>,
    rng: u32,
}

fn solid_image(rgba: [u8; 4]) -> Rc<Image> {
    let mut i = Image::new(1, 1);
    i.fill(rgba);
    Rc::new(i)
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arcs = Archives::open(files.as_mut());
        if !arcs.packs.contains_key("scr") {
            return Err("no md_scr.med in the game folder".into());
        }
        let mut e = Engine {
            files,
            arcs,
            width: 1280,
            height: 720,
            log: false,
            font: None,
            scripts: HashMap::new(),
            bgset: HashMap::new(),
            sprset: Vec::new(),
            maps: HashMap::new(),
            stack: Vec::new(),
            vars: BTreeMap::new(),
            bg: None,
            sprites: Vec::new(),
            over: None,
            window: false,
            name: None,
            lines: Vec::new(),
            line: String::new(),
            save_title: String::new(),
            wait: Wait::None,
            view: View::Game,
            frozen: None,
            fade: None,
            rules: Vec::new(),
            now: 0.0,
            bgm: None,
            loops: BTreeMap::new(),
            sounds: Vec::new(),
            images: HashMap::new(),
            text_cache: HashMap::new(),
            pointer: (0, 0),
            skip: false,
            ended: false,
            saved_at: None,
            rng: 0x2545_f491,
        };
        // the screen is as big as the black background
        for n in ["BG_BLACK", "TITLE01", "BG_WHITE"] {
            if let Some(i) = e.image(n) {
                e.width = i.w;
                e.height = i.h;
                break;
            }
        }
        e.load_tables();
        e.call("_START");
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
            eprintln!("med: {s}");
        }
    }

    fn load_tables(&mut self) {
        if let Some(s) = self.script("_BGSET") {
            // "#name" lines, each followed by its picture (a "#DUMMY" has none)
            for w in s.strs.windows(2) {
                if w[0].starts_with('#') && !w[1].starts_with('#') {
                    self.bgset
                        .insert(w[0][1..].trim().to_string(), w[1].trim().to_string());
                }
            }
        }
        if let Some(s) = self.script("_SPRSET") {
            for line in &s.strs {
                let (k, v) = line.split_once(':').unwrap_or((line, ""));
                let f: Vec<&str> = v.split(',').map(str::trim).collect();
                match k.trim() {
                    "PREFIX" if f.len() >= 2 => self.sprset.push(SprDef {
                        prefix: f[0].trim_start_matches('#').to_string(),
                        base: f[1].to_string(),
                        bodies: Vec::new(),
                        faces: Vec::new(),
                    }),
                    "BODY" if f.len() >= 2 => {
                        if let Some(d) = self.sprset.last_mut() {
                            d.bodies.push((f[0].to_string(), f[1].to_string()));
                        }
                    }
                    "FACE" if f.len() >= 2 => {
                        let n = |i: usize| f.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
                        if let Some(d) = self.sprset.last_mut() {
                            d.faces
                                .push((f[0].to_string(), f[1].to_string(), n(2), n(3)));
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(s) = self.script("_MAP") {
            let mut cur: Option<(String, ClickMap)> = None;
            for line in &s.strs {
                let line = line.trim();
                if let Some(name) = line.strip_prefix(':') {
                    if let Some((n, m)) = cur.take() {
                        self.maps.insert(n, m);
                    }
                    cur = Some((name.to_uppercase(), ClickMap::default()));
                    continue;
                }
                if line.starts_with('@') {
                    if let Some((n, m)) = cur.take() {
                        self.maps.insert(n, m);
                    }
                    continue;
                }
                let Some((_, m)) = cur.as_mut() else {
                    continue;
                };
                if let Some(g) = line.strip_prefix("GRA:") {
                    m.focus = g.split(',').nth(1).unwrap_or("").trim().to_string();
                    continue;
                }
                if line.contains(':') {
                    continue;
                }
                let f: Vec<&str> = line.split(',').map(str::trim).collect();
                let n = |i: usize| f.get(i).and_then(|v| v.parse::<i32>().ok());
                if f.len() == 5
                    && let (Some(x0), Some(y0), Some(x1), Some(y1)) = (n(1), n(2), n(3), n(4))
                {
                    m.items
                        .push(([x0, y0, x1, y1], String::new(), String::new()));
                } else if f.len() >= 4
                    && let (Some(x), Some(y)) = (n(0), n(1))
                {
                    m.items
                        .push(([x, y, x, y], f[2].to_string(), f[3].to_string()));
                }
            }
            if let Some((n, m)) = cur.take() {
                self.maps.insert(n, m);
            }
        }
    }

    fn script(&mut self, name: &str) -> Option<Rc<Script>> {
        let key = name.to_uppercase();
        if let Some(s) = self.scripts.get(&key) {
            return Some(s.clone());
        }
        let b = self.arcs.read(self.files.as_mut(), "scr", name)?;
        match Script::parse(&b) {
            Ok(s) => {
                let s = Rc::new(s);
                self.scripts.insert(key, s.clone());
                Some(s)
            }
            Err(e) => {
                self.trace(&format!("{name}: {e}"));
                None
            }
        }
    }

    /// `script`, `script:label` or (with `here`) a label of the running script: the frame
    /// that starts there.
    fn target(&mut self, to: &str) -> Option<Frame> {
        let (s, label) = match to.split_once(':') {
            Some((s, l)) => (s.trim().to_string(), Some(l.trim())),
            None => {
                let here = self.stack.last().map(|f| f.script.clone());
                // a label of this script, else a script
                if let Some(h) = &here
                    && let Some(sc) = self.script(h)
                    && let Some(pc) = sc.label(to.trim())
                {
                    return Some(Frame {
                        script: h.clone(),
                        pc,
                    });
                }
                (to.trim().to_string(), None)
            }
        };
        let sc = self.script(&s)?;
        let pc = match label {
            Some(l) => sc.label(l)?,
            None => 0,
        };
        Some(Frame { script: s, pc })
    }

    fn call(&mut self, to: &str) {
        match self.target(to) {
            Some(f) => self.stack.push(f),
            None => self.trace(&format!("no script {to}")),
        }
    }

    fn jump(&mut self, to: &str) {
        match self.target(to) {
            Some(f) => {
                self.stack.pop();
                self.stack.push(f);
            }
            None => self.trace(&format!("no script {to}")),
        }
    }

    fn set_pc(&mut self, pc: usize) {
        if let Some(f) = self.stack.last_mut() {
            f.pc = pc;
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
            Wait::Until(t) if self.now >= t => self.wait = Wait::None,
            Wait::Click if self.skip && self.view == View::Game => self.advance(),
            _ => {}
        }
        if self.view == View::Game {
            self.run();
        }
        Ok(())
    }

    fn run(&mut self) {
        for _ in 0..20000 {
            if self.wait != Wait::None || self.ended {
                return;
            }
            let Some(top) = self.stack.last().cloned() else {
                self.trace("the story ended");
                self.back_to_title();
                return;
            };
            let Some(s) = self.script(&top.script) else {
                self.stack.pop();
                continue;
            };
            let Some(rec) = s.recs.get(top.pc).cloned() else {
                self.stack.pop();
                continue;
            };
            self.set_pc(top.pc + 1);
            self.exec(&s, top.pc, &rec);
        }
    }

    fn var(&mut self, c: u8, i: u8) -> i32 {
        if c == 0xd7 {
            // the engine's values; 0x45 / 0x47: random numbers
            if i == 0x45 || i == 0x47 {
                self.rng ^= self.rng << 13;
                self.rng ^= self.rng >> 17;
                self.rng ^= self.rng << 5;
                return (self.rng % 10) as i32;
            }
        }
        self.vars.get(&(c, i)).copied().unwrap_or(0)
    }

    fn eval(&mut self, t: &[Tok]) -> i32 {
        let mut vals: HashMap<(u8, u8), i32> = HashMap::new();
        for tok in t {
            if let Tok::Var(c, i) = *tok {
                let v = self.var(c, i);
                vals.insert((c, i), v);
            }
        }
        script::eval(t, &mut |c, i| vals.get(&(c, i)).copied().unwrap_or(0))
    }

    fn exec(&mut self, s: &Rc<Script>, pc: usize, r: &Rec) {
        if self.log && !matches!(r.op, 0..=4 | 7 | 9) {
            self.trace(&format!(
                "{}:{:x} op{} {:02x?}",
                self.stack.last().map_or("", |f| f.script.as_str()),
                r.off,
                r.op,
                r.args
            ));
        }
        let str_at = |at: usize| s.s(r.u16(at)).to_string();
        match r.op {
            0 => {
                let l = std::mem::take(&mut self.line);
                self.lines.push(l);
            }
            1 => {
                let t = str_at(0);
                self.line.push_str(&t);
            }
            4 => self.text_wait(),
            6 => {
                let (t, _) = script::tokens(&r.args);
                if let Some(Tok::Var(c, i)) = t.first().copied() {
                    let rhs = if t.get(1) == Some(&Tok::Op(0xf5)) {
                        &t[2..]
                    } else {
                        &t[1..]
                    };
                    let v = self.eval(rhs);
                    self.vars.insert((c, i), v);
                }
            }
            10 => {
                // a case reached from the one before: the block ends
                if let Some(&exit) = s.case_exit.get(&pc) {
                    self.set_pc(exit);
                }
            }
            13 => {
                let n = str_at(0);
                self.name = Some(n);
            }
            16 => {
                let mode = r.u8(0);
                let name = str_at(1);
                self.background(&name, mode);
            }
            18 => {
                let mode = r.u8(0);
                let n = r.u8(1) as usize;
                let names: Vec<String> = (0..n).map(|k| str_at(2 + 2 * k)).collect();
                self.freeze();
                self.sprites.clear();
                for name in names {
                    match self.sprite(&name) {
                        Some(sp) => self.sprites.push(sp),
                        None => self.trace(&format!("no standing picture {name}")),
                    }
                }
                self.show(mode);
            }
            19 => {
                self.freeze();
                self.sprites.clear();
                self.show(r.u8(0));
            }
            20 => {
                if !self.skip {
                    let f = str_at(0);
                    let d = self.sound_data("cv", &f);
                    self.push_sound("voice", d, 1.0, false, 0.0);
                }
            }
            21 => {
                if !self.skip {
                    let f = str_at(0);
                    let d = self.sound_data("se", &f);
                    self.push_sound("se", d, 1.0, false, 0.0);
                }
            }
            22 => {
                let f = str_at(0);
                let ch = r.u8(2);
                self.play_loop(ch, &f);
            }
            23 => {
                let ch = r.u8(0);
                if ch == 0xff {
                    for c in self.loops.keys().copied().collect::<Vec<_>>() {
                        self.stop_loop(c);
                    }
                } else {
                    self.stop_loop(ch);
                }
            }
            25 => {
                let n = r.i16(0) as i32;
                if n > 0 {
                    self.play_music(n);
                } else {
                    self.stop_music(if n < 0 { (-n * 20) as f32 } else { 0.0 });
                }
            }
            26 => {
                let to = if r.args.len() >= 3 {
                    str_at(1)
                } else {
                    str_at(0)
                };
                self.jump(&to);
            }
            27 | 108 => {
                let to = str_at(0);
                self.call(&to);
            }
            28 => {
                let to = str_at(0);
                let here = self
                    .stack
                    .last()
                    .map(|f| f.script.clone())
                    .unwrap_or_default();
                self.jump(&format!("{here}:{to}"));
            }
            29 => {
                let (t, _) = script::tokens(&r.args);
                let ok = self.eval(&t) != 0;
                match s.recs.get(pc + 1) {
                    Some(next) if next.op == 8 && next.u8(0) == 1 => {
                        if ok {
                            self.set_pc(pc + 2);
                        } else {
                            let to = s.at(next.u16(1) as u32);
                            // into the ELSE branch, past the ELSE and its jump
                            let to = match s.recs.get(to) {
                                Some(e) if e.op == 30 => {
                                    to + 1 + s.recs.get(to + 1).is_some_and(|x| x.op == 8) as usize
                                }
                                _ => to,
                            };
                            self.set_pc(to);
                        }
                    }
                    _ => {
                        if !ok {
                            self.set_pc(pc + 2);
                        }
                    }
                }
            }
            30 => {
                // the end of the IF branch: past the ELSE branch
                if let Some(next) = s.recs.get(pc + 1).filter(|x| x.op == 8) {
                    let to = s.at(next.u16(1) as u32);
                    self.set_pc(to);
                }
            }
            31 => {
                self.stack.pop();
            }
            33 => {
                if let Some((cases, _)) = s.block(pc) {
                    let items = cases
                        .iter()
                        .map(|&c| {
                            let t = s.recs.get(c).map_or(0xffff, |x| x.u16(0));
                            (c, s.s(t).to_string())
                        })
                        .collect();
                    self.flush();
                    self.window = false;
                    self.wait = Wait::Select(items);
                }
            }
            34 => {
                if let Some((cases, _)) = s.block(pc) {
                    let map = str_at(0).to_uppercase();
                    self.flush();
                    self.window = false;
                    self.wait = Wait::Map(map, cases);
                }
            }
            37 => {
                self.flush();
                self.window = false;
            }
            38 => {
                self.flush();
                if !self.skip {
                    self.wait = Wait::Until(self.now + r.u16(0) as f64);
                }
            }
            42 => {
                if let Some((cases, end)) = s.block(pc) {
                    let mut to = end;
                    for c in cases {
                        let Some(cr) = s.recs.get(c) else { continue };
                        let (t, _) = script::tokens(&cr.args[2.min(cr.args.len())..]);
                        if self.eval(&t) != 0 {
                            to = c + 1;
                            break;
                        }
                    }
                    self.set_pc(to);
                }
            }
            44 => self.save_title = str_at(0).replace("\\N", " "),
            50 => self.ended = true,
            51 => {
                self.flush();
                self.wait = Wait::Load;
                self.view = View::Slots { save: false };
            }
            56 => {
                let mode = r.u8(0);
                let name = if r.u16(1) == 0xffff {
                    String::new()
                } else {
                    str_at(1)
                };
                self.freeze();
                self.over = if name.is_empty() {
                    None
                } else {
                    self.image(&name).map(|i| (name, i))
                };
                self.show(mode);
            }
            79 => self.show(r.u8(0) | 0x80),
            83 => {
                self.flush();
                self.wait = Wait::Click;
            }
            106 => {
                let f = str_at(0);
                if f.eq_ignore_ascii_case("OFF") {
                    self.push_sound("bgcv", None, 0.0, false, 300.0);
                } else {
                    let d = self.sound_data("cv", &f);
                    self.push_sound("bgcv", d, 0.6, true, 0.0);
                }
            }
            _ => {}
        }
    }

    fn text_wait(&mut self) {
        if !self.line.is_empty() {
            let l = std::mem::take(&mut self.line);
            self.lines.push(l);
        }
        self.flush();
        self.window = true;
        self.saved_at = Some(self.stack.clone());
        self.wait = Wait::Click;
    }

    /// After the click on a text: the next text starts empty.
    fn clear_text(&mut self) {
        self.lines.clear();
        self.line.clear();
        self.name = None;
    }

    // ---- pictures ----

    fn image(&mut self, name: &str) -> Option<Rc<Image>> {
        let key = name.to_uppercase();
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let b = ["gra", "gra2", "jpg"]
            .iter()
            .find_map(|p| self.arcs.read(self.files.as_mut(), p, name));
        let img = match b.map(|b| decode_picture(&b)) {
            Some(Ok(i)) => Some(Rc::new(i)),
            Some(Err(e)) => {
                self.trace(&format!("{name}: {e}"));
                None
            }
            None => None,
        };
        if self.images.len() > 64 {
            self.images.clear();
        }
        self.images.insert(key, img.clone());
        img
    }

    fn background(&mut self, name: &str, mode: u8) {
        let pic = match name.strip_prefix('#') {
            Some(n) => self.bgset.get(n).cloned().unwrap_or_else(|| n.to_string()),
            None => name.to_string(),
        };
        // a name that is no picture ("SGGG", "S000"): black
        let img = self.image(&pic);
        if img.is_none() {
            self.trace(&format!("no background {name}: black"));
        }
        self.freeze();
        self.bg = img.map(|i| (name.to_string(), i));
        if mode & 0x40 == 0 {
            self.sprites.clear();
        }
        self.over = None;
        self.show(mode);
    }

    /// `#name` of _SPRSET: the PREFIX it starts with, then a BODY and a FACE name.
    fn sprite(&mut self, name: &str) -> Option<Sprite> {
        let n = name.trim_start_matches('#');
        let mut found = None;
        for d in &self.sprset {
            let Some(rest) = n.strip_prefix(d.prefix.as_str()) else {
                continue;
            };
            let mut best: Option<(String, Option<Face>)> = None;
            for (b, bs) in &d.bodies {
                let Some(r2) = rest.strip_prefix(b.as_str()) else {
                    continue;
                };
                if r2.is_empty() {
                    best = Some((format!("{}{bs}", d.base), None));
                    break;
                }
                if let Some((_, fs, x, y)) = d.faces.iter().find(|f| f.0 == r2) {
                    best = Some((
                        format!("{}{bs}", d.base),
                        Some((format!("{}{fs}", d.base), *x, *y)),
                    ));
                    break;
                }
            }
            if best.is_none() && rest.is_empty() {
                best = Some((d.base.clone(), None));
            }
            if best.is_some() {
                found = best;
                break;
            }
        }
        let (body, face) = found?;
        let body_img = self.image(&body)?;
        let face = face.and_then(|(f, x, y)| self.image(&f).map(|i| (i, x, y)));
        Some(Sprite {
            name: name.to_string(),
            body: body_img,
            face,
        })
    }

    /// The changes since the last showing come in: at once, or with a fade for mode 0x80 (a
    /// wipe by the rule picture BG_FADEnn for 0x80 | n).
    fn show(&mut self, mode: u8) {
        if mode & 0x80 != 0 {
            let n = mode & 0x7f;
            let rule = if n > 0 && self.frozen.is_some() && !self.skip {
                self.image(&format!("BG_FADE{n:02}"))
            } else {
                None
            };
            self.present_ruled(FADE_MS, rule);
        }
    }

    /// Keeps the screen as it is until the next showing, a text or a wait.
    fn freeze(&mut self) {
        if self.frozen.is_none() {
            self.fade = None;
            self.frozen = Some(self.stage_draws());
        }
    }

    fn flush(&mut self) {
        if self.frozen.is_some() {
            self.present(0.0);
        }
    }

    fn present(&mut self, ms: f64) {
        self.present_ruled(ms, None);
    }

    fn present_ruled(&mut self, ms: f64, rule: Option<Rc<Image>>) {
        if let Some(from) = self.frozen.take()
            && ms > 0.0
            && !self.skip
        {
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
    }

    fn solid(&self, rgba: [u8; 4]) -> Draw {
        let img = solid_image(rgba);
        let mut d = draw(&img, 0.0, 0.0, 1.0);
        d.dst = [0.0, 0.0, self.width as f32, self.height as f32];
        d
    }

    fn stage_draws(&self) -> Vec<Draw> {
        let mut out = vec![self.solid([0, 0, 0, 255])];
        if let Some((_, img)) = &self.bg {
            out.push(draw(img, 0.0, 0.0, 1.0));
        }
        let n = self.sprites.len() as f32;
        for (k, s) in self.sprites.iter().enumerate() {
            // side by side: the centres at (k + 1) / (n + 1) of the width
            let cx = self.width as f32 * (k as f32 + 1.0) / (n + 1.0);
            let x = (cx - s.body.w as f32 / 2.0).round();
            let y = self.height as f32 - s.body.h as f32;
            out.push(draw(&s.body, x, y, 1.0));
            if let Some((f, fx, fy)) = &s.face {
                out.push(draw(f, x + *fx as f32, y + *fy as f32, 1.0));
            }
        }
        if let Some((_, img)) = &self.over {
            out.push(draw(img, 0.0, 0.0, 1.0));
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
            Wait::Map(m, _) => self.map_draws(&m, &mut out),
            Wait::Select(items) => self.select_draws(&items, &mut out),
            _ => {
                if self.window && self.fade.is_none() && self.frozen.is_none() {
                    self.window_draws(&mut out);
                }
            }
        }
        match self.view {
            View::Menu => self.menu_draws(&mut out),
            View::Slots { save } => self.slot_draws(save, &mut out),
            View::Game => {}
        }
        out
    }

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

    /// The text window (SYS_TW) at the bottom centre: where its picture goes and the part
    /// that holds the text (below its see-through top, inside a side margin).
    fn window_rect(&mut self) -> ([i32; 2], [i32; 4]) {
        let (w, h) = (self.width as i32, self.height as i32);
        match self.image("SYS_TW") {
            Some(i) => {
                let (iw, ih) = (i.w as i32, i.h as i32);
                let full = iw >= w;
                let x = (w - iw) / 2;
                let y = h - ih - if full { 0 } else { h / 48 };
                // the first row that is mostly opaque in the middle
                let mid = (iw / 2) as usize;
                let top = (0..ih as usize)
                    .find(|&r| i.px[(r * iw as usize + mid) * 4 + 3] >= 160)
                    .unwrap_or(0) as i32;
                let inset = if full { w / 7 } else { 0 };
                ([x, y], [x + inset, y + top, x + iw - inset / 2, y + ih])
            }
            None => ([0, 0], [w / 8, h - h / 4, w - w / 8, h - 12]),
        }
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let (at, r) = self.window_rect();
        match self.image("SYS_TW") {
            Some(img) => out.push(draw(&img, at[0] as f32, at[1] as f32, 1.0)),
            None => {
                let mut d = self.solid([0, 0, 0, 160]);
                d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
                out.push(d);
            }
        }
        let size = (self.height as i32 / 28).max(18);
        let step = size + size / 3;
        let (tx, mut ty) = (r[0] + size * 2, r[1] + size + size / 2);
        if let Some(n) = self.name.clone() {
            let img = self.label(&n, [255, 230, 160], size);
            out.push(draw(
                &img,
                (r[0] + size) as f32,
                (r[1] + size / 4) as f32,
                1.0,
            ));
        }
        for l in self.lines.clone() {
            let img = self.label(&l, [255, 255, 255], size);
            out.push(draw(&img, tx as f32 - 2.0, ty as f32 - 2.0, 1.0));
            ty += step;
        }
        if self.wait == Wait::Click
            && let Some(img) = self.image("SYS_CLICKW")
        {
            // a strip of square frames
            let fw = img.h as i32;
            let frames = (img.w as i32 / fw.max(1)).max(1);
            let i = (self.now / 100.0) as i32 % frames;
            let (x, y) = (r[2] - fw - size, r[3] - fw - size / 2);
            let mut d = draw(&img, x as f32, y as f32, 1.0);
            d.src = [i * fw, 0, (i + 1) * fw, img.h as i32];
            d.dst = [x as f32, y as f32, (x + fw) as f32, (y + fw) as f32];
            out.push(d);
        }
    }

    /// The places of a click map: (rectangle, picture, focus picture).
    fn map_items(&mut self, m: &str) -> Vec<([i32; 4], String, String)> {
        let Some(map) = self.maps.get(m).cloned() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (r, pic, focus) in map.items {
            if pic.is_empty() {
                out.push((r, map.focus.clone(), String::new()));
            } else {
                let (w, h) = self
                    .image(&pic)
                    .map_or((200, 100), |i| (i.w as i32, i.h as i32));
                out.push(([r[0], r[1], r[0] + w, r[1] + h], pic, focus));
            }
        }
        out
    }

    fn map_draws(&mut self, m: &str, out: &mut Vec<Draw>) {
        let (px, py) = self.pointer;
        for (r, a, b) in self.map_items(m) {
            let over = inside(r, px, py);
            if b.is_empty() {
                // a part of the focus picture over the background
                if over && let Some(img) = self.image(&a) {
                    let mut d = draw(&img, r[0] as f32, r[1] as f32, 1.0);
                    d.src = r;
                    d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
                    out.push(d);
                }
            } else if let Some(img) = self.image(if over { &b } else { &a }) {
                out.push(draw(&img, r[0] as f32, r[1] as f32, 1.0));
            }
        }
    }

    fn select_rects(&mut self, n: usize) -> Vec<[i32; 4]> {
        let (w, h) = (self.width as i32, self.height as i32);
        let (bw, bh) = self
            .image("SYS_SEL")
            .map_or((w / 2, 56), |i| (i.w as i32, i.h as i32));
        let gap = bh / 3;
        let top = (h - (bh + gap) * n as i32) / 2 - h / 10;
        (0..n as i32)
            .map(|i| {
                let y = top + i * (bh + gap);
                [(w - bw) / 2, y, (w + bw) / 2, y + bh]
            })
            .collect()
    }

    fn select_draws(&mut self, items: &[(usize, String)], out: &mut Vec<Draw>) {
        let rects = self.select_rects(items.len());
        let size = (self.height as i32 / 28).max(18);
        for ((_, t), r) in items.iter().zip(rects) {
            let over = inside(r, self.pointer.0, self.pointer.1);
            match self.image(if over { "SYS_SEL2" } else { "SYS_SEL" }) {
                Some(img) => out.push(draw(&img, r[0] as f32, r[1] as f32, 1.0)),
                None => {
                    let mut d = self.solid(if over {
                        [120, 60, 150, 230]
                    } else {
                        [30, 20, 40, 210]
                    });
                    d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
                    out.push(d);
                }
            }
            let img = self.label(t, [255, 255, 255], size);
            let x = (r[0] + r[2]) as f32 / 2.0 - img.w as f32 / 2.0;
            let y = (r[1] + r[3]) as f32 / 2.0 - img.h as f32 / 2.0;
            out.push(draw(&img, x, y, 1.0));
        }
    }

    // ---- menus ----

    fn button(&mut self, t: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let over = inside(r, self.pointer.0, self.pointer.1);
        let mut d = self.solid(if over {
            [120, 60, 150, 230]
        } else {
            [30, 20, 40, 210]
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
        nexas::gfx::render_ruled(self.width, self.height, &ds, &self.rules)
    }

    /// The rule wipes of the last [`Engine::draws`]: (draw index, rule).
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

    /// A sound of a pack, WADY made into WAV.
    fn sound_data(&mut self, pack: &str, name: &str) -> Option<Vec<u8>> {
        let d = self.arcs.read(self.files.as_mut(), pack, name);
        if d.is_none() {
            self.trace(&format!("no sound {pack}:{name}"));
        }
        let d = d?;
        Some(tmrhiro::marble::wady::to_wav(&d).unwrap_or(d))
    }

    fn play_music(&mut self, n: i32) {
        if self.bgm == Some(n) {
            return;
        }
        let d = self.sound_data("bgm", &format!("M{n:02}"));
        self.bgm = Some(n);
        self.push_sound("bgm", d, 1.0, true, 0.0);
    }

    fn stop_music(&mut self, ms: f32) {
        self.bgm = None;
        self.push_sound("bgm", None, 0.0, false, ms);
    }

    fn play_loop(&mut self, ch: u8, f: &str) {
        let d = self.sound_data("se", f);
        self.loops.insert(ch, f.to_string());
        self.push_sound(&format!("loop{ch}"), d, 1.0, true, 0.0);
    }

    fn stop_loop(&mut self, ch: u8) {
        self.loops.remove(&ch);
        self.push_sound(&format!("loop{ch}"), None, 0.0, false, 300.0);
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
            self.clear_text();
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
            Wait::Select(items) => {
                let hit = self
                    .select_rects(items.len())
                    .into_iter()
                    .position(|r| inside(r, x, y));
                if let Some(i) = hit {
                    self.wait = Wait::None;
                    self.set_pc(items[i].0 + 1);
                }
            }
            Wait::Map(m, cases) => {
                let hit = self
                    .map_items(&m)
                    .into_iter()
                    .position(|(r, _, _)| inside(r, x, y));
                if let Some(i) = hit
                    && let Some(&c) = cases.get(i)
                {
                    self.wait = Wait::None;
                    self.set_pc(c + 1);
                }
            }
            Wait::Load | Wait::None => {}
        }
    }

    fn close_slots(&mut self) {
        if self.wait == Wait::Load {
            self.wait = Wait::None;
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
        self.sprites.clear();
        self.bg = None;
        self.over = None;
        self.window = false;
        self.clear_text();
        self.view = View::Game;
        self.frozen = None;
        self.fade = None;
        self.skip = false;
        self.saved_at = None;
        self.wait = Wait::None;
        for ch in self.loops.keys().copied().collect::<Vec<_>>() {
            self.stop_loop(ch);
        }
        self.push_sound("voice", None, 0.0, false, 0.0);
        self.push_sound("bgcv", None, 0.0, false, 0.0);
    }

    /// Back to the title of STITLE (or the start).
    fn back_to_title(&mut self) {
        self.reset();
        self.stop_music(300.0);
        self.vars.clear();
        let to = if self
            .script("STITLE")
            .is_some_and(|s| s.label("TITLE_SETUP").is_some())
        {
            "STITLE:TITLE_SETUP"
        } else {
            "_START"
        };
        self.call(to);
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
        let first = self.lines.first().cloned().unwrap_or_default();
        let title = if self.save_title.is_empty() {
            first
        } else {
            format!("{}  {first}", self.save_title)
        };
        s += &format!("title {}\n", title.replace('\n', " "));
        s += &format!("chapter {}\n", self.save_title);
        for f in &at {
            s += &format!("frame {} {}\n", f.pc, f.script);
        }
        for ((c, i), v) in &self.vars {
            s += &format!("var {c} {i} {v}\n");
        }
        if let Some((n, _)) = &self.bg {
            s += &format!("bg {n}\n");
        }
        for sp in &self.sprites {
            s += &format!("sprite {}\n", sp.name);
        }
        if let Some((n, _)) = &self.over {
            s += &format!("over {n}\n");
        }
        if let Some(b) = self.bgm {
            s += &format!("bgm {b}\n");
        }
        for (c, f) in &self.loops {
            s += &format!("loop {c} {f}\n");
        }
        if let Some(n) = &self.name {
            s += &format!("name {n}\n");
        }
        for l in &self.lines {
            s += &format!("line {l}\n");
        }
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
        self.vars = slot.vars;
        self.save_title = slot.chapter;
        if let Some(b) = slot.bg {
            let pic = match b.strip_prefix('#') {
                Some(n) => self.bgset.get(n).cloned().unwrap_or_else(|| n.to_string()),
                None => b.clone(),
            };
            self.bg = self.image(&pic).map(|i| (b, i));
        }
        for n in slot.sprites {
            if let Some(sp) = self.sprite(&n) {
                self.sprites.push(sp);
            }
        }
        if let Some(o) = slot.over {
            self.over = self.image(&o).map(|i| (o, i));
        }
        if let Some(b) = slot.bgm {
            self.play_music(b);
        }
        for (c, f) in slot.loops {
            self.play_loop(c, &f);
        }
        self.window = true;
        self.name = slot.name;
        self.lines = slot.lines;
        self.saved_at = Some(self.stack.clone());
        self.wait = Wait::Click;
        self.view = View::Game;
        self.frozen = Some(vec![self.solid([0, 0, 0, 255])]);
        self.present(400.0);
    }
}

struct Slot {
    title: String,
    chapter: String,
    frames: Vec<Frame>,
    vars: BTreeMap<(u8, u8), i32>,
    bg: Option<String>,
    sprites: Vec<String>,
    over: Option<String>,
    bgm: Option<i32>,
    loops: Vec<(u8, String)>,
    name: Option<String>,
    lines: Vec<String>,
}

impl Slot {
    fn parse(s: &str) -> Option<Slot> {
        let mut slot = Slot {
            title: String::new(),
            chapter: String::new(),
            frames: Vec::new(),
            vars: BTreeMap::new(),
            bg: None,
            sprites: Vec::new(),
            over: None,
            bgm: None,
            loops: Vec::new(),
            name: None,
            lines: Vec::new(),
        };
        for line in s.lines() {
            let (k, rest) = line.split_once(' ').unwrap_or((line, ""));
            match k {
                "title" => slot.title = rest.to_string(),
                "chapter" => slot.chapter = rest.to_string(),
                "frame" => {
                    let (n, script) = rest.split_once(' ')?;
                    slot.frames.push(Frame {
                        script: script.to_string(),
                        pc: n.parse().ok()?,
                    });
                }
                "var" => {
                    let v: Vec<i64> = rest.split(' ').filter_map(|x| x.parse().ok()).collect();
                    if v.len() == 3 {
                        slot.vars.insert((v[0] as u8, v[1] as u8), v[2] as i32);
                    }
                }
                "bg" => slot.bg = Some(rest.to_string()),
                "sprite" => slot.sprites.push(rest.to_string()),
                "over" => slot.over = Some(rest.to_string()),
                "bgm" => slot.bgm = rest.parse().ok(),
                "loop" => {
                    if let Some((c, f)) = rest.split_once(' ') {
                        slot.loops.push((c.parse().unwrap_or(0), f.to_string()));
                    }
                }
                "name" => slot.name = Some(rest.to_string()),
                "line" => slot.lines.push(rest.to_string()),
                _ => {}
            }
        }
        (!slot.frames.is_empty()).then_some(slot)
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
