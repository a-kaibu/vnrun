//! Runs the scripts a frame at a time, from `start.scr`: it carries out lines until one waits
//! (for a click, a time, a fade or the script's buttons) and says what to draw and play.
//!
//! The scene is a background (`bgon` / `cgon`), numbered sprites (`chon`) and the script's
//! buttons (`btnset`, picked with `btnenable`, the answer in `%reg`). Changes are gathered
//! and shown together by `ud` (0 at once, 1 a fade of `ms`, 2 a wipe by a mask (.msk), a
//! rule wipe). Text lines wait for a click in the message window set up by `wnd(0, ...)`.
//!
//! Commands carried out: bgon bgoff cgon cgoff chon choff ch ud wndoff mono wait pause
//! btnset btnon btnoff btnenable btnreset rand seon seoff cvon bgmon bgmoff envon envoff float
//! (2: the load screen) exit backtitle global var wnd; the rest are skipped.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};
use vncommon::rule::Rule;

use crate::arc::Archives;
use crate::pic;
use crate::script::{self, Line, Script, Val};

const FRAME_MS: f64 = 1000.0 / 60.0;
const SLOTS: usize = 10;

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

#[derive(Clone, Debug)]
struct Sprite {
    name: String,
    x: i32,
    y: i32,
}

#[derive(Clone, Debug)]
struct Button {
    pic: String,
    x: i32,
    y: i32,
    text: Option<(String, i32, i32)>,
}

/// The message window, from `wnd(0, ...)`.
#[derive(Clone)]
struct WindowLook {
    frame: Option<String>,
    x: i32,
    y: i32,
    w: i32,
    text_x: i32,
    text_y: i32,
    /// the font size (`fnt`), the space between lines and the letters per line (`wnd TXT`)
    size: i32,
    spacing: i32,
    per_line: i32,
    prompt: Option<(String, i32, i32)>,
    /// (picture, macro, x, y)
    buttons: Vec<(String, String, i32, i32)>,
}

#[derive(Clone, PartialEq)]
enum Wait {
    None,
    Click,
    Pause,
    Until(f64),
    Fade,
    Buttons,
    /// the options gathered from `/` lines
    Choice,
    Load,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Game,
    Menu,
    Slots { save: bool },
}

/// (macro, rect, picture, picture under the pointer)
type WindowButton = (String, [i32; 4], Rc<Image>, Rc<Image>);

struct Fade {
    from: Vec<Draw>,
    start: f64,
    ms: f64,
    /// `ud(2, mask, ms)`: the old picture goes where the mask (.msk) is dark first
    rule: Option<Rc<Image>>,
}

/// How soft a mask wipe's edge is (part of the mask's brightness range).
const RULE_VAGUE: f32 = 0.2;

pub struct Engine {
    files: Box<dyn Files>,
    arcs: Archives,
    pub width: u32,
    pub height: u32,
    pub log: bool,
    font: Option<Rc<fontdue::Font>>,
    scripts: HashMap<String, Rc<Script>>,

    cur: String,
    pc: usize,
    stack: Vec<(String, usize)>,
    vars: HashMap<String, Val>,
    bg: Option<String>,
    sprites: BTreeMap<i32, Sprite>,
    centred: HashMap<i32, bool>,
    /// `ch(n, SYS, k)`: 1 a part of the message window (shown with it), 2 above the buttons
    system: HashMap<i32, i64>,
    buttons: BTreeMap<i32, Button>,
    buttons_on: bool,
    mono: bool,
    look: WindowLook,
    window: bool,
    name: Option<String>,
    text: Option<String>,
    choices: Vec<String>,
    wait: Wait,
    view: View,
    frozen: Option<Vec<Draw>>,
    fade: Option<Fade>,
    rules: Vec<(usize, Rule)>,
    now: f64,
    bgm: Option<String>,
    env: Option<String>,
    sounds: Vec<SoundEvent>,
    pic_cache: HashMap<(String, bool), Rc<Image>>,
    state_cache: HashMap<(String, usize), Rc<Image>>,
    text_cache: HashMap<(String, i32), Rc<Image>>,
    pointer: (i32, i32),
    skip: bool,
    ended: bool,
    seed: u32,
    instead_of_title: Option<String>,
    /// the line on screen, for saves: (script, line)
    saved_at: Option<(String, usize)>,
}

fn white(rgba: [u8; 4]) -> Rc<Image> {
    let mut i = Image::new(1, 1);
    i.fill(rgba);
    Rc::new(i)
}

fn sepia(img: &Image) -> Image {
    let mut out = img.clone();
    out.renew();
    for p in out.px.chunks_exact_mut(4) {
        let l = (p[0] as f32 * 0.3 + p[1] as f32 * 0.59 + p[2] as f32 * 0.11).min(255.0);
        p[0] = (l * 1.07).min(255.0) as u8;
        p[1] = (l * 0.95) as u8;
        p[2] = (l * 0.78) as u8;
    }
    out
}

/// `"name"` -> name
fn unquote(s: &str) -> String {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(s)
        .to_string()
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arcs = Archives::open(files.as_mut())?;
        if arcs.entry("start.scr").is_none() {
            return Err("no start.scr".into());
        }
        let mut e = Engine {
            files,
            arcs,
            width: 1280,
            height: 720,
            log: false,
            font: None,
            scripts: HashMap::new(),
            cur: "start".into(),
            pc: 0,
            stack: Vec::new(),
            vars: HashMap::new(),
            bg: None,
            sprites: BTreeMap::new(),
            centred: HashMap::new(),
            system: HashMap::new(),
            buttons: BTreeMap::new(),
            buttons_on: false,
            mono: false,
            look: WindowLook {
                frame: None,
                x: 0,
                y: 504,
                w: 1280,
                text_x: 220,
                text_y: 84,
                size: 26,
                spacing: 8,
                per_line: 0,
                prompt: None,
                buttons: Vec::new(),
            },
            window: false,
            name: None,
            text: None,
            choices: Vec::new(),
            wait: Wait::None,
            view: View::Game,
            frozen: None,
            fade: None,
            rules: Vec::new(),
            now: 0.0,
            bgm: None,
            env: None,
            sounds: Vec::new(),
            pic_cache: HashMap::new(),
            state_cache: HashMap::new(),
            text_cache: HashMap::new(),
            pointer: (0, 0),
            skip: false,
            ended: false,
            seed: 0x1234_5678,
            instead_of_title: None,
            saved_at: None,
        };
        // the screen: the size of the first full-screen background
        for n in ["black", "white", "bk_title"] {
            if let Some(img) = e.picture(n, false)
                && img.w >= 320
                && img.h >= 240
            {
                (e.width, e.height) = (img.w, img.h);
                break;
            }
        }
        e.script("start").ok_or("start.scr unreadable")?;
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

    /// Goes to a script instead of the title (for the native runner), after the start
    /// script's settings.
    pub fn start_at(&mut self, name: &str) {
        self.instead_of_title = Some(name.to_lowercase());
    }

    fn trace(&self, s: &str) {
        if self.log {
            eprintln!("lilim: {s}");
        }
    }

    fn script(&mut self, name: &str) -> Option<Rc<Script>> {
        let key = name.to_lowercase();
        if let Some(s) = self.scripts.get(&key) {
            return Some(s.clone());
        }
        let e = self.arcs.entry(&format!("{key}.scr"))?.clone();
        let b = self.arcs.read(self.files.as_mut(), &e)?;
        let s = Rc::new(script::parse(&b));
        self.scripts.insert(key, s.clone());
        Some(s)
    }

    fn get(&self, v: &str) -> Val {
        self.vars
            .get(&v.to_lowercase())
            .cloned()
            .unwrap_or_else(|| {
                if v.starts_with('$') {
                    Val::Str(String::new())
                } else {
                    Val::Int(0)
                }
            })
    }

    fn eval(&self, s: &str) -> Val {
        script::eval(s, &|v| self.get(v))
    }

    fn num(&self, a: &[String], i: usize) -> i64 {
        a.get(i).map_or(0, |s| self.eval(s).int())
    }

    fn word(&self, a: &[String], i: usize) -> String {
        a.get(i).map_or(String::new(), |s| {
            let t = s.trim();
            if t.starts_with('"') {
                unquote(t)
            } else if t.starts_with('$') || t.starts_with('%') {
                self.get(t).text()
            } else {
                t.to_string()
            }
        })
    }

    // ---- running ----

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
            Wait::Click | Wait::Pause if self.skip && self.view == View::Game => self.advance(),
            _ => {}
        }
        if self.view == View::Game {
            self.run();
        }
        Ok(())
    }

    fn run(&mut self) {
        for _ in 0..20000 {
            if self.wait != Wait::None || self.ended || self.view != View::Game {
                return;
            }
            let Some(s) = self.script(&self.cur.clone()) else {
                self.trace(&format!("no script {}", self.cur));
                self.ret();
                continue;
            };
            let Some(line) = s.lines.get(self.pc).cloned() else {
                self.ret();
                continue;
            };
            self.pc += 1;
            self.exec(line);
        }
    }

    fn ret(&mut self) {
        match self.stack.pop() {
            Some((s, pc)) => {
                self.cur = s;
                self.pc = pc;
            }
            None => {
                self.trace("the first script ended");
                self.ended = true;
            }
        }
    }

    fn exec(&mut self, line: Line) {
        match line {
            Line::Label(_) => {}
            Line::End => self.ret(),
            Line::Assign(v, e) => {
                let mut x = self.eval(&e);
                if v.starts_with('%') {
                    x = Val::Int(x.int());
                } else {
                    x = Val::Str(x.text());
                }
                self.vars.insert(v.to_lowercase(), x);
            }
            Line::Text(name, t) => self.say(name, t),
            Line::Choice(t, last) => {
                self.choices.push(t);
                if last {
                    self.present(0.0);
                    self.wait = Wait::Choice;
                }
            }
            Line::Ctl(c, a) => self.control(&c, &a),
            Line::Cmd(c, a) => self.command(&c, &a),
        }
    }

    /// Moves past the matching line: `stop` says whether a line at depth 0 ends the search
    /// (and is carried out when it says so); `open` / `close` are the nesting words.
    fn scan(&mut self, open: &str, close: &str, stop: &dyn Fn(&Engine, &Line) -> bool) {
        let Some(s) = self.script(&self.cur.clone()) else {
            return;
        };
        let mut depth = 0;
        while let Some(l) = s.lines.get(self.pc) {
            self.pc += 1;
            if let Line::Ctl(c, _) = l {
                if c == open {
                    depth += 1;
                    continue;
                }
                if c == close && depth > 0 {
                    depth -= 1;
                    continue;
                }
            }
            if depth == 0 && stop(self, l) {
                return;
            }
        }
    }

    fn jump_label(&mut self, label: &str) -> bool {
        let label = label.to_uppercase();
        if let Some(s) = self.script(&self.cur.clone())
            && let Some(&at) = s.labels.get(&label)
        {
            self.pc = at;
            return true;
        }
        false
    }

    fn control(&mut self, c: &str, a: &[String]) {
        if self.log {
            self.trace(&format!("{}:{} ^{c} {a:?}", self.cur, self.pc - 1));
        }
        let mut arg = a.first().map(|s| unquote(s)).unwrap_or_default();
        if c == "go"
            && arg.eq_ignore_ascii_case("title")
            && let Some(n) = self.instead_of_title.take()
        {
            arg = n;
        }
        match c {
            "gsb" => {
                if self.script(&arg).is_some() {
                    self.stack.push((self.cur.clone(), self.pc));
                    self.cur = arg.to_lowercase();
                    self.pc = 0;
                } else {
                    let back = (self.cur.clone(), self.pc);
                    if self.jump_label(&arg) {
                        self.stack.push(back);
                    } else {
                        self.trace(&format!("no script or label {arg}"));
                    }
                }
            }
            "go" => {
                if self.script(&arg).is_some() {
                    self.cur = arg.to_lowercase();
                    self.pc = 0;
                } else if !self.jump_label(&arg) {
                    self.trace(&format!("no script or label {arg}"));
                }
            }
            "jp" => {
                if !self.jump_label(&arg) {
                    if self.script(&arg).is_some() {
                        self.cur = arg.to_lowercase();
                        self.pc = 0;
                    } else {
                        self.trace(&format!("no label {arg}"));
                    }
                }
            }
            "ret" => self.ret(),
            "if" => {
                if self.eval(a.first().map_or("0", |s| s)).int() == 0 {
                    self.scan("if", "endi", &|e, l| match l {
                        Line::Ctl(c, a) if c == "elif" => {
                            e.eval(a.first().map_or("0", |s| s)).int() != 0
                        }
                        Line::Ctl(c, _) => c == "else" || c == "endi",
                        _ => false,
                    });
                }
            }
            "elif" | "else" => self.scan(
                "if",
                "endi",
                &|_, l| matches!(l, Line::Ctl(c, _) if c == "endi"),
            ),
            "sw" => {
                let v = self.eval(a.first().map_or("0", |s| s));
                self.scan("sw", "ends", &|e, l| match l {
                    Line::Ctl(c, a) if c == "case" => {
                        let x = e.eval(a.first().map_or("0", |s| s));
                        x.int() == v.int() && x.text() == v.text()
                    }
                    Line::Ctl(c, _) => c == "df" || c == "ends",
                    _ => false,
                });
            }
            "case" | "df" => self.scan(
                "sw",
                "ends",
                &|_, l| matches!(l, Line::Ctl(c, _) if c == "ends"),
            ),
            "macro" => self.scan(
                "macro",
                "endm",
                &|_, l| matches!(l, Line::Ctl(c, _) if c == "endm"),
            ),
            _ => {}
        }
    }

    fn say(&mut self, name: Option<String>, t: String) {
        if self.log {
            self.trace(&format!("{}:{} text {name:?} {t}", self.cur, self.pc - 1));
        }
        self.present(0.0);
        self.window = true;
        self.name = name;
        self.text = Some(t);
        self.saved_at = Some((self.cur.clone(), self.pc - 1));
        self.wait = Wait::Click;
    }

    fn sprite_place(&mut self, n: i32, name: &str, a: &[String]) -> (i32, i32) {
        let img = self.picture(name, false);
        let (w, h) = img.map_or((0, 0), |i| (i.w as i32, i.h as i32));
        let (sw, sh) = (self.width as i32, self.height as i32);
        let first = a
            .get(2)
            .map(|s| s.trim().to_uppercase())
            .unwrap_or_default();
        if first == "CENTER" {
            return ((sw - w) / 2, (sh - h) / 2);
        }
        if first == "NON" || a.len() < 3 {
            if let Some(s) = self.sprites.get(&n) {
                // the same place for a picture of the same size
                return (s.x, s.y);
            }
            return ((sw - w) / 2, 0);
        }
        let (x, y) = (self.num(a, 2) as i32, self.num(a, 3) as i32);
        if *self.centred.get(&n).unwrap_or(&false) {
            (x - w / 2, y)
        } else {
            (x, y)
        }
    }

    fn command(&mut self, c: &str, a: &[String]) {
        if self.log {
            self.trace(&format!("{}:{} {c} {a:?}", self.cur, self.pc - 1));
        }
        match c {
            "bgon" | "cgon" => {
                self.freeze();
                self.bg = Some(self.word(a, 0));
            }
            "bgoff" | "cgoff" => {
                self.freeze();
                self.bg = None;
            }
            "chon" => {
                let n = self.num(a, 0) as i32;
                let name = self.word(a, 1);
                let (x, y) = self.sprite_place(n, &name, a);
                self.freeze();
                self.sprites.insert(n, Sprite { name, x, y });
            }
            "choff" => {
                let n = self.num(a, 0) as i32;
                self.freeze();
                if n < 0 {
                    let system = self.system.clone();
                    self.sprites
                        .retain(|k, _| system.get(k).copied().unwrap_or(0) > 0);
                } else {
                    self.sprites.remove(&n);
                }
            }
            "ch" => {
                let n = self.num(a, 0) as i32;
                match self.word(a, 1).to_uppercase().as_str() {
                    "PNT" => {
                        self.centred.insert(n, self.num(a, 2) == 1);
                    }
                    "SYS" => {
                        self.system.insert(n, self.num(a, 2));
                    }
                    _ => {}
                }
            }
            "ud" => {
                let kind = self.num(a, 0);
                let ms = match kind {
                    0 => 0.0,
                    2 => self.num(a, 2) as f64,
                    _ => self.num(a, 1) as f64,
                };
                if self.frozen.is_some() {
                    let rule = if kind == 2 {
                        let m = self.word(a, 1);
                        self.picture(&m, false)
                    } else {
                        None
                    };
                    self.present_ruled(ms, rule);
                } else if ms > 0.0 && !self.skip {
                    self.wait = Wait::Until(self.now + ms);
                }
            }
            "wndoff" => {
                self.window = false;
                self.text = None;
                self.name = None;
            }
            "mono" => {
                self.freeze();
                self.mono = self.num(a, 0) > 0;
            }
            "wait" => {
                if !self.skip {
                    self.wait = Wait::Until(self.now + self.num(a, 0) as f64);
                }
            }
            "pause" => {
                self.present(0.0);
                self.wait = Wait::Pause;
            }
            "btnset" => {
                let id = self.num(a, 0) as i32;
                let text = (a.len() >= 5).then(|| {
                    (
                        self.word(a, 4),
                        self.num(a, 5) as i32,
                        self.num(a, 6) as i32,
                    )
                });
                let b = Button {
                    pic: self.word(a, 1),
                    x: self.num(a, 2) as i32,
                    y: self.num(a, 3) as i32,
                    text,
                };
                self.buttons.insert(id, b);
            }
            "btnon" => self.buttons_on = true,
            "btnoff" => self.buttons_on = false,
            "btnenable" => {
                self.present(0.0);
                self.buttons_on = true;
                self.wait = Wait::Buttons;
            }
            "btnreset" => {
                let w = self.word(a, 0);
                if w.eq_ignore_ascii_case("ALL") || a.is_empty() {
                    self.buttons.clear();
                } else {
                    self.buttons.remove(&(self.num(a, 0) as i32));
                }
            }
            "rand" => {
                let n = self.num(a, 0).max(1);
                self.seed ^= self.seed << 13;
                self.seed ^= self.seed >> 17;
                self.seed ^= self.seed << 5;
                self.vars
                    .insert("%reg".into(), Val::Int(self.seed as i64 % n));
            }
            "seon" => {
                let n = self.word(a, 0);
                if !self.skip {
                    let d = self.sound(&n);
                    self.push_sound("se", d, 0.8, false, 0.0);
                }
            }
            "seoff" => self.push_sound("se", None, 0.0, false, 200.0),
            "cvon" => {
                let n = self.word(a, 0);
                if !self.skip && !n.is_empty() {
                    let d = self.sound(&n);
                    if d.is_none() {
                        self.trace(&format!("no voice {n}"));
                    }
                    self.push_sound("voice", d, 1.0, false, 0.0);
                }
            }
            "bgmon" => {
                let n = self.word(a, 0);
                self.play_music(&n);
            }
            "bgmoff" => {
                let ms = self.num(a, 1);
                self.stop_music(ms as f32);
            }
            "envon" => {
                let n = self.word(a, 0);
                self.env = Some(n.clone());
                let d = self.sound(&n);
                self.push_sound("env", d, 0.7, true, 0.0);
            }
            "envoff" => {
                self.env = None;
                let ms = if self.num(a, 0) == 0 {
                    self.num(a, 1)
                } else {
                    0
                };
                self.push_sound("env", None, 0.0, false, ms as f32);
            }
            "float" => {
                if self.num(a, 0) == 2 {
                    self.view = View::Slots { save: false };
                    self.wait = Wait::Load;
                }
            }
            "exit" => {
                self.trace("exit");
                self.ended = true;
            }
            "backtitle" => self.back_to_title(),
            "global" => {
                if let Some(v) = a.first() {
                    let x = Val::Int(self.num(a, 1));
                    self.vars.insert(v.trim().to_lowercase(), x);
                }
            }
            "var" => {
                for v in a {
                    let v = v.trim().to_lowercase();
                    let x = if v.starts_with('$') {
                        Val::Str(String::new())
                    } else {
                        Val::Int(0)
                    };
                    self.vars.entry(v).or_insert(x);
                }
            }
            "wnd" => self.window_setting(a),
            "fnt" => {
                let size = self.num(a, 0) as i32;
                if (10..=64).contains(&size) {
                    self.look.size = size;
                    self.text_cache.clear();
                }
            }
            _ => {}
        }
    }

    /// `wnd(0, POS, x, y)`, `wnd(0, TXT, x, y, ...)`, `wnd(0, FRM, "picture")`,
    /// `wnd(0, FP, "prompt", x, y)`, `wnd(0, BTN, "picture", "macro", x, y, ...)`.
    fn window_setting(&mut self, a: &[String]) {
        if self.num(a, 0) != 0 {
            return;
        }
        match self.word(a, 1).to_uppercase().as_str() {
            "POS" => (self.look.x, self.look.y) = (self.num(a, 2) as i32, self.num(a, 3) as i32),
            "SIZE" => self.look.w = self.num(a, 2) as i32,
            "TXT" => {
                // x, y, lines, -, spacing, letters per line
                (self.look.text_x, self.look.text_y) =
                    (self.num(a, 2) as i32, self.num(a, 3) as i32);
                self.look.spacing = self.num(a, 6).clamp(0, 40) as i32;
                self.look.per_line = self.num(a, 7).clamp(0, 200) as i32;
            }
            "FRM" => self.look.frame = Some(self.word(a, 2)).filter(|s| !s.is_empty()),
            "FP" => {
                self.look.prompt = Some((
                    self.word(a, 2),
                    self.num(a, 3) as i32,
                    self.num(a, 4) as i32,
                ))
            }
            "BTN" => {
                if a.len() < 6 {
                    self.look.buttons.clear();
                } else {
                    let b = (
                        self.word(a, 2),
                        self.word(a, 3),
                        self.num(a, 4) as i32,
                        self.num(a, 5) as i32,
                    );
                    self.look.buttons.push(b);
                }
            }
            _ => {}
        }
    }

    // ---- pictures ----

    fn picture(&mut self, name: &str, mono: bool) -> Option<Rc<Image>> {
        let key = (name.to_lowercase(), mono);
        if let Some(p) = self.pic_cache.get(&key) {
            return Some(p.clone());
        }
        let img = if mono {
            Rc::new(sepia(self.picture(name, false)?.as_ref()))
        } else {
            let e = self
                .arcs
                .find(name, &[".cmp", ".abm", ".bmp", ".msk", ".img"])?
                .clone();
            let b = self.arcs.read(self.files.as_mut(), &e)?;
            match pic::decode(&b) {
                Ok(i) => Rc::new(i),
                Err(err) => {
                    self.trace(&format!("{name}: {err}"));
                    return None;
                }
            }
        };
        if self.pic_cache.len() > 40 {
            self.pic_cache.clear();
        }
        self.pic_cache.insert(key, img.clone());
        Some(img)
    }

    /// A button picture's state (three stacked: normal, under the pointer, pressed).
    fn button_state(&mut self, name: &str, k: usize) -> Option<Rc<Image>> {
        let key = (name.to_lowercase(), k);
        if let Some(p) = self.state_cache.get(&key) {
            return Some(p.clone());
        }
        let img = self.picture(name, false)?;
        let n = if img.h % 3 == 0 { 3 } else { 1 };
        let s = Rc::new(pic::state(&img, k, n));
        if self.state_cache.len() > 80 {
            self.state_cache.clear();
        }
        self.state_cache.insert(key, s.clone());
        Some(s)
    }

    // ---- drawing ----

    fn freeze(&mut self) {
        if self.frozen.is_none() {
            self.fade = None;
            self.frozen = Some(self.stage_draws());
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
        let mut d = draw(&white(rgba), 0.0, 0.0, 1.0);
        d.dst = [0.0, 0.0, self.width as f32, self.height as f32];
        d
    }

    fn button_rects(&mut self) -> Vec<(i32, [i32; 4])> {
        let list: Vec<(i32, Button)> = self.buttons.iter().map(|(k, b)| (*k, b.clone())).collect();
        let mut out = Vec::new();
        for (k, b) in list {
            if let Some(img) = self.button_state(&b.pic, 0) {
                out.push((k, [b.x, b.y, b.x + img.w as i32, b.y + img.h as i32]));
            }
        }
        out
    }

    fn stage_draws(&mut self) -> Vec<Draw> {
        let mut out = vec![self.solid([0, 0, 0, 255])];
        let mono = self.mono;
        if let Some(bg) = self.bg.clone()
            && let Some(img) = self.picture(&bg, mono)
        {
            out.push(draw(&img, 0.0, 0.0, 1.0));
        }
        let sprites: Vec<(i32, Sprite)> =
            self.sprites.iter().map(|(k, s)| (*k, s.clone())).collect();
        for (k, s) in &sprites {
            if self.system.get(k).copied().unwrap_or(0) != 0 {
                continue;
            }
            if let Some(img) = self.picture(&s.name, mono) {
                out.push(draw(&img, s.x as f32, s.y as f32, 1.0));
            }
        }
        if self.buttons_on {
            let (px, py) = self.pointer;
            let list: Vec<Button> = self.buttons.values().cloned().collect();
            let live = self.wait == Wait::Buttons;
            for b in list {
                let Some(n) = self.button_state(&b.pic, 0) else {
                    continue;
                };
                let over = live && inside([b.x, b.y, b.x + n.w as i32, b.y + n.h as i32], px, py);
                let img = if over {
                    self.button_state(&b.pic, 1).unwrap_or(n)
                } else {
                    n
                };
                out.push(draw(&img, b.x as f32, b.y as f32, 1.0));
                if let Some((t, tx, ty)) = &b.text {
                    let size = self.look.size;
                    let ti = self.text(t, size);
                    out.push(draw(&ti, (b.x + tx) as f32, (b.y + ty) as f32, 1.0));
                }
            }
        }
        for (k, s) in &sprites {
            if self.system.get(k).copied().unwrap_or(0) < 2 {
                continue;
            }
            if let Some(img) = self.picture(&s.name, false) {
                out.push(draw(&img, s.x as f32, s.y as f32, 1.0));
            }
        }
        out
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = match self.frozen.clone() {
            Some(f) => f,
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
        if self.window && self.fade.is_none() && self.frozen.is_none() {
            self.window_draws(&mut out);
        }
        if self.wait == Wait::Choice {
            let boxes = self.choice_boxes();
            for (t, r) in self.choices.clone().iter().zip(boxes) {
                self.button(t, r, &mut out);
            }
        }
        match self.view {
            View::Menu => self.menu_draws(&mut out),
            View::Slots { save } => self.slot_draws(save, &mut out),
            View::Game => {}
        }
        out
    }

    fn text(&mut self, s: &str, size: i32) -> Rc<Image> {
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
        if self.text_cache.len() > 200 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, img.clone());
        img
    }

    fn window_buttons(&mut self) -> Vec<WindowButton> {
        let (wx, wy) = (self.look.x, self.look.y);
        let list = self.look.buttons.clone();
        let mut out = Vec::new();
        for (p, m, x, y) in list {
            let (Some(n), Some(h)) = (self.button_state(&p, 0), self.button_state(&p, 1)) else {
                continue;
            };
            let r = [wx + x, wy + y, wx + x + n.w as i32, wy + y + n.h as i32];
            out.push((m, r, n, h));
        }
        out
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let (wx, wy) = (self.look.x, self.look.y);
        // the sprites that are parts of the window
        let parts: Vec<Sprite> = self
            .sprites
            .iter()
            .filter(|(k, _)| self.system.get(k) == Some(&1))
            .map(|(_, s)| s.clone())
            .collect();
        for s in parts {
            if let Some(img) = self.picture(&s.name, false) {
                out.push(draw(&img, s.x as f32, s.y as f32, 1.0));
            }
        }
        match self
            .look
            .frame
            .clone()
            .and_then(|f| self.picture(&f, false))
        {
            Some(img) => out.push(draw(&img, wx as f32, wy as f32, 1.0)),
            None => {
                let mut d = self.solid([0, 0, 0, 160]);
                d.dst = [0.0, wy as f32, self.width as f32, self.height as f32];
                out.push(d);
            }
        }
        let (px, py) = self.pointer;
        for (_, r, n, h) in self.window_buttons() {
            let img = if inside(r, px, py) { h } else { n };
            out.push(draw(&img, r[0] as f32, r[1] as f32, 1.0));
        }
        let size = self.look.size;
        let (tx, ty) = (wx + self.look.text_x, wy + self.look.text_y);
        let step = size + self.look.spacing.max(4);
        if let Some(n) = self.name.clone() {
            let img = self.text(&n, size);
            if self.look.text_x >= 200 && self.look.text_x >= img.w as i32 + 60 {
                // left of the text
                let nx = (tx - 24 - img.w as i32).max(wx + 40);
                out.push(draw(&img, nx as f32, ty as f32, 1.0));
            } else {
                // a line above it
                let ni = self.text(&format!("【{n}】"), size);
                out.push(draw(&ni, tx as f32, (ty - step - size / 2) as f32, 1.0));
            }
        }
        if let Some(t) = self.text.clone() {
            let fit = (wx + self.look.w - tx - 24) / size.max(1);
            let per_line = match self.look.per_line {
                0 => fit,
                n => n.min(fit + 2),
            }
            .max(10) as usize;
            let mut y = ty;
            for piece in wrap(&t, per_line) {
                let img = self.text(&piece, size);
                out.push(draw(&img, tx as f32, y as f32, 1.0));
                y += step;
            }
        }
        if self.wait == Wait::Click
            && let Some((p, x, y)) = self.look.prompt.clone()
            && let Some(img) = self.picture(&p, false)
        {
            // square frames stacked: one every 60 ms
            let side = img.w.max(1);
            let n = (img.h / side).max(1) as usize;
            let k = (self.now / 60.0) as usize % n;
            let mut d = draw(&img, (wx + x) as f32, (wy + y) as f32, 1.0);
            d.src = [
                0,
                (k as u32 * side) as i32,
                side as i32,
                ((k as u32 + 1) * side) as i32,
            ];
            d.dst = [
                d.dst[0],
                d.dst[1],
                d.dst[0] + side as f32,
                d.dst[1] + side as f32,
            ];
            out.push(d);
        }
    }

    fn button(&mut self, t: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let over = inside(r, self.pointer.0, self.pointer.1);
        let mut d = self.solid(if over {
            [70, 110, 190, 230]
        } else {
            [20, 30, 60, 210]
        });
        d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        out.push(d);
        let size = (self.look.size + 2).clamp(18, 26);
        let img = self.text(t, size);
        let x = (r[0] + r[2]) as f32 / 2.0 - img.w as f32 / 2.0;
        let y = (r[1] + r[3]) as f32 / 2.0 - img.h as f32 / 2.0;
        out.push(draw(&img, x, y, 1.0));
    }

    fn choice_boxes(&self) -> Vec<[i32; 4]> {
        let (w, h) = (self.width as i32, self.height as i32);
        let n = self.choices.len() as i32;
        let (bw, bh, gap) = (w * 3 / 4, 44, 14);
        let total = n * bh + (n - 1).max(0) * gap;
        let top = ((self.look.y.min(h) - total) / 2).max(8);
        (0..n)
            .map(|i| {
                let y = top + i * (bh + gap);
                [(w - bw) / 2, y, (w + bw) / 2, y + bh]
            })
            .collect()
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
        let (bw, bh, gap) = (w - 240, 48, 10);
        let top = (h - (bh + gap) * (SLOTS as i32 + 1)) / 2;
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
        let head = self.text(if save { "セーブ" } else { "ロード" }, 30);
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

    /// A sound from the archives, or (older games) a loose file in bgm/, se/ or the top folder.
    fn sound(&mut self, name: &str) -> Option<Vec<u8>> {
        if let Some(e) = self.arcs.find(name, &[".ogg", ".wav"]).cloned() {
            return self.arcs.read(self.files.as_mut(), &e);
        }
        for dir in ["bgm/", "se/", "cv/", ""] {
            for ext in [".ogg", ".wav"] {
                let path = format!("{dir}{name}{ext}");
                if let Some(b) = self.files.read(&path) {
                    return Some(b);
                }
            }
        }
        None
    }

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

    fn play_music(&mut self, name: &str) {
        if self.bgm.as_deref() == Some(name) {
            return;
        }
        self.bgm = Some(name.to_string());
        let d = self.sound(name);
        if d.is_none() {
            self.trace(&format!("no music {name}"));
        }
        self.push_sound("bgm", d, 0.7, true, 0.0);
    }

    fn stop_music(&mut self, ms: f32) {
        self.bgm = None;
        self.push_sound("bgm", None, 0.0, false, ms.max(0.0));
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    fn advance(&mut self) {
        match self.wait {
            Wait::Click => {
                self.wait = Wait::None;
                self.text = None;
                self.name = None;
            }
            Wait::Pause => self.wait = Wait::None,
            _ => {}
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
                            self.load(i);
                        }
                    }
                    Some(_) => self.close_slots(),
                    None => {}
                }
                return;
            }
            View::Game => {}
        }
        if self.fade.is_some() && matches!(self.wait, Wait::Click | Wait::Pause) {
            self.fade = None;
            return;
        }
        match self.wait {
            Wait::Choice => {
                if let Some(i) = self.choice_boxes().iter().position(|r| inside(*r, x, y)) {
                    self.vars.insert("%reg".into(), Val::Int(i as i64));
                    self.choices.clear();
                    self.wait = Wait::None;
                }
            }
            Wait::Buttons => {
                if let Some((id, _)) = self
                    .button_rects()
                    .into_iter()
                    .find(|(_, r)| inside(*r, x, y))
                {
                    self.vars.insert("%reg".into(), Val::Int(id as i64));
                    self.wait = Wait::None;
                }
            }
            Wait::Click | Wait::Pause => {
                if self.window && self.wait == Wait::Click {
                    let hit = self
                        .window_buttons()
                        .into_iter()
                        .find(|b| inside(b.1, x, y))
                        .map(|b| b.0);
                    if let Some(m) = hit {
                        match m.as_str() {
                            "savemenu" | "quicksave" => self.view = View::Slots { save: true },
                            "loadmenu" | "quickload" => self.view = View::Slots { save: false },
                            "startskip" => self.skip = !self.skip,
                            _ => self.view = View::Menu,
                        }
                        return;
                    }
                }
                self.push_sound("voice", None, 0.0, false, 0.0);
                self.advance();
            }
            _ => {}
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
                if matches!(self.wait, Wait::Click | Wait::Pause) && self.saved_at.is_some() {
                    self.view = View::Menu;
                }
            }
        }
    }

    pub fn key(&mut self, k: usize) {
        match k {
            4 => {
                let (x, y) = self.pointer;
                if matches!(self.wait, Wait::Click | Wait::Pause) || self.view != View::Game {
                    self.click(x, y);
                }
            }
            5 => self.cancel(),
            6 => self.skip = !self.skip,
            _ => {}
        }
    }

    pub fn wheel(&mut self, up: bool) {
        if !up && matches!(self.wait, Wait::Click | Wait::Pause) && self.view == View::Game {
            self.push_sound("voice", None, 0.0, false, 0.0);
            self.advance();
        }
    }

    pub fn choosing(&self) -> bool {
        matches!(self.wait, Wait::Buttons | Wait::Choice)
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    fn reset_scene(&mut self) {
        self.stack.clear();
        self.bg = None;
        self.sprites.clear();
        self.buttons.clear();
        self.buttons_on = false;
        self.mono = false;
        self.window = false;
        self.text = None;
        self.name = None;
        self.choices.clear();
        self.wait = Wait::None;
        self.view = View::Game;
        self.frozen = None;
        self.fade = None;
        self.skip = false;
        self.stop_music(300.0);
        self.env = None;
        for ch in ["env", "se", "voice"] {
            self.push_sound(ch, None, 0.0, false, 300.0);
        }
    }

    fn back_to_title(&mut self) {
        self.reset_scene();
        self.saved_at = None;
        self.cur = "title".into();
        self.pc = 0;
    }

    // ---- saves ----

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&format!("slot{i:02}"))?;
        Slot::parse(&String::from_utf8_lossy(&b))
    }

    fn save(&mut self, i: usize) {
        let Some((cur, pc)) = self.saved_at.clone() else {
            return;
        };
        let mut s = String::new();
        let first = self.text.clone().unwrap_or_default();
        let title = match &self.name {
            Some(n) => format!("{n} {first}"),
            None => first,
        };
        s += &format!("title {}\n", title.replace('\n', " "));
        s += &format!("at {pc} {cur}\n");
        for (a, b) in &self.stack {
            s += &format!("call {b} {a}\n");
        }
        for (k, v) in &self.vars {
            match v {
                Val::Int(n) => s += &format!("int {k} {n}\n"),
                Val::Str(t) => s += &format!("str {k} {t}\n"),
            }
        }
        if let Some(b) = &self.bg {
            s += &format!("bg {b}\n");
        }
        for (k, sp) in &self.sprites {
            s += &format!("sprite {k} {} {} {}\n", sp.x, sp.y, sp.name);
        }
        if let Some(b) = &self.bgm {
            s += &format!("bgm {b}\n");
        }
        if let Some(b) = &self.env {
            s += &format!("env {b}\n");
        }
        s += &format!("mono {}\n", self.mono as u8);
        self.files.write_save(&format!("slot{i:02}"), s.as_bytes());
        self.view = View::Game;
    }

    fn load(&mut self, i: usize) {
        let Some(slot) = self.read_slot(i) else {
            return;
        };
        self.reset_scene();
        self.cur = slot.at.0.clone();
        self.pc = slot.at.1;
        self.stack = slot.calls;
        self.vars = slot.vars;
        self.bg = slot.bg;
        self.sprites = slot.sprites;
        self.mono = slot.mono;
        if let Some(b) = slot.bgm {
            self.bgm = None;
            self.play_music(&b);
        }
        if let Some(e) = slot.env {
            let d = self.sound(&e);
            self.env = Some(e);
            self.push_sound("env", d, 0.7, true, 0.0);
        }
        self.saved_at = Some(slot.at);
        self.freeze();
        self.frozen = Some(vec![self.solid([0, 0, 0, 255])]);
        self.present(400.0);
    }
}

struct Slot {
    title: String,
    at: (String, usize),
    calls: Vec<(String, usize)>,
    vars: HashMap<String, Val>,
    bg: Option<String>,
    sprites: BTreeMap<i32, Sprite>,
    bgm: Option<String>,
    env: Option<String>,
    mono: bool,
}

impl Slot {
    fn parse(s: &str) -> Option<Slot> {
        let mut slot = Slot {
            title: String::new(),
            at: (String::new(), 0),
            calls: Vec::new(),
            vars: HashMap::new(),
            bg: None,
            sprites: BTreeMap::new(),
            bgm: None,
            env: None,
            mono: false,
        };
        let mut seen = false;
        for line in s.lines() {
            let (k, rest) = line.split_once(' ').unwrap_or((line, ""));
            let mut it = rest.splitn(2, ' ');
            let (a, b) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
            match k {
                "title" => slot.title = rest.to_string(),
                "at" => {
                    slot.at = (b.to_string(), a.parse().ok()?);
                    seen = true;
                }
                "call" => slot.calls.push((b.to_string(), a.parse().unwrap_or(0))),
                "int" => {
                    slot.vars
                        .insert(a.to_string(), Val::Int(b.parse().unwrap_or(0)));
                }
                "str" => {
                    slot.vars.insert(a.to_string(), Val::Str(b.to_string()));
                }
                "bg" => slot.bg = Some(rest.to_string()),
                "sprite" => {
                    let p: Vec<&str> = rest.splitn(4, ' ').collect();
                    if p.len() == 4 {
                        slot.sprites.insert(
                            p[0].parse().unwrap_or(0),
                            Sprite {
                                name: p[3].to_string(),
                                x: p[1].parse().unwrap_or(0),
                                y: p[2].parse().unwrap_or(0),
                            },
                        );
                    }
                }
                "bgm" => slot.bgm = Some(rest.to_string()),
                "env" => slot.env = Some(rest.to_string()),
                "mono" => slot.mono = rest == "1",
                _ => {}
            }
        }
        seen.then_some(slot)
    }
}

/// Breaks a line into pieces of about `n` characters; closing marks stay on the line before.
fn wrap(s: &str, n: usize) -> Vec<String> {
    const CLOSING: &str = "」』）】〉》、。，．・ー…！？!?)ぁぃぅぇぉっゃゅょァィゥェォッャュョ";
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let mut end = (i + n.max(1)).min(chars.len());
        // up to two closing marks hang on the end of the line
        let mut k = 0;
        while end < chars.len() && k < 2 && CLOSING.contains(chars[end]) {
            end += 1;
            k += 1;
        }
        out.push(chars[i..end].iter().collect());
        i = end;
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
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

#[cfg(test)]
mod tests {
    #[test]
    fn wrap_keeps_closing_marks() {
        assert_eq!(super::wrap("あいう」", 3), vec!["あいう」"]);
        assert_eq!(super::wrap("あいうえお", 3), vec!["あいう", "えお"]);
    }
}
