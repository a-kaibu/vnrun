//! The runtime of the LINK engine (2015, ハラミタマ): the [SCR-Ver5.1] machine, its scenes
//! (each text names the whole scene it is shown over, a [LAYER] state of its script), the
//! message window, the choices and the title menu (vrm.arc's own pictures).
//!
//! The instructions, as read from the trial's scripts (u16 op, u16 size, operands):
//!
//! | op | operands | |
//! |----|----------|-|
//! | 01 | u8 kinds, u8 op, u16 var, i32 a, i32 b | var = b (op 0), a + b, a - b, a * b, a / b |
//! | 04 | | return (the end of a script) |
//! | 05 | u8 flags, u16 frames | wait (flags 0x80: not) |
//! | 06 | i32 scene, i32 -1, u8 kind, i32 parameter | change the scene: 0 at once, 1 fade, 4 rule (a resource) |
//! | 07 | i32 line, i32 scene, i32 bgm, .., i32 looped sound, .. | a message |
//! | 08 | u8, i32 n, .. | a choice of message.dat's choices; the answer goes to var 0 |
//! | 09 | u8 channel, i32 sound | 2 looped, 0xff stop all, else once; -1 stops |
//! | 0a | i32 bgm | -1 stops |
//! | 0b | i32 offset | goto |
//! | 0c | u8 kind, i32 script | jump to a script (kind 4: the resource, else a var holds it) |
//! | 0d | u8 kinds, u8 op, u16 var, i32 a, i32 b | var = a ==, !=, <, <=, >, >=, &&, || b (a a var; b a var when kinds is 0x2_) |
//! | 0f | u8, u16 var, i32 offset | goto when var is 0 |
//! | 14 | u8 1, u8 call | the program's screens: 2 the map (where to go, var 0x0c), 0x0e the map's second question (others: not known) |
//! | 15 | u8 length, cp932 | the chapter's title |
//! | 1a | u8 kind, i32 script | call a script |

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nexas::files::Files;
use nexas::gfx::{Draw, Image, Place};

use crate::engine::{SoundEvent, over, text};
use crate::link::arc::{LinkArc, Pic, picture, stem};
use crate::link::data::{Messages, Params, Script};

const LETTER_MS: f64 = 25.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Title,
    Run,
    Ended,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Wait {
    None,
    Click,
    Until(f64),
    Trans,
    Choice,
}

struct Trans {
    from: Rc<Image>,
    to: Rc<Image>,
    rule: Option<Rc<Image>>,
    start: f64,
    ms: f64,
}

struct Message {
    name: Option<String>,
    text: String,
    start: f64,
}

type Placed = (Rc<Image>, i32, i32);

struct Button {
    normal: Option<Placed>,
    hover: Option<Placed>,
    rect: [i32; 4],
    action: &'static str,
}

#[derive(Default)]
struct Ui {
    title: Option<Placed>,
    buttons: Vec<Button>,
    window: Option<Placed>,
    choice: Option<Placed>,
    choice_hover: Option<Placed>,
}

pub struct Engine {
    files: Box<dyn Files>,
    arcs: HashMap<String, Option<LinkArc>>,
    p: Params,
    m: Messages,
    pub width: u32,
    pub height: u32,
    font: Option<fontdue::Font>,
    pub log: bool,
    pub now: f64,
    mode: Mode,
    scripts: HashMap<u32, Rc<Script>>,
    script: Option<Rc<Script>>,
    pc: usize,
    stack: Vec<(Rc<Script>, usize)>,
    vars: Vec<i32>,
    /// the scene shown (of the running script)
    scene: Option<(String, usize)>,
    screen: Rc<Image>,
    trans: Option<Trans>,
    wait: Wait,
    msg: Option<Message>,
    window_img: Option<(usize, Rc<Image>)>,
    /// the choices: their words and the value each sets, and the variable
    choices: Vec<(String, i32)>,
    choice_var: usize,
    choice_imgs: Vec<Placed>,
    hover: Option<usize>,
    bgm: i32,
    se_loop: i32,
    sounds: Vec<SoundEvent>,
    skip: bool,
    clicked: bool,
    pics: HashMap<u32, Rc<Pic>>,
    ui: Ui,
    warned: HashSet<String>,
    pub said: Vec<String>,
}

fn black(w: u32, h: u32) -> Rc<Image> {
    let mut img = Image::new(w, h);
    img.fill([0, 0, 0, 255]);
    Rc::new(img)
}

/// Whether the game folder holds a LINK game (params.dat and message.dat).
pub fn is_link(files: &mut dyn Files) -> bool {
    files.size("params.dat").is_some() && files.size("message.dat").is_some()
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let p = Params::parse(&files.read("params.dat").ok_or("no params.dat")?)?;
        let m = Messages::parse(&files.read("message.dat").ok_or("no message.dat")?)?;
        let (width, height) = (p.width, p.height);
        let mut e = Engine {
            files,
            arcs: HashMap::new(),
            p,
            m,
            width,
            height,
            font: None,
            log: false,
            now: 0.0,
            mode: Mode::Title,
            scripts: HashMap::new(),
            script: None,
            pc: 0,
            stack: Vec::new(),
            vars: vec![0; 0x10000],
            scene: None,
            screen: black(width, height),
            trans: None,
            wait: Wait::None,
            msg: None,
            window_img: None,
            choices: Vec::new(),
            choice_var: 0,
            choice_imgs: Vec::new(),
            hover: None,
            bgm: -1,
            se_loop: -1,
            sounds: Vec::new(),
            skip: false,
            clicked: false,
            pics: HashMap::new(),
            ui: Ui::default(),
            warned: HashSet::new(),
            said: Vec::new(),
        };
        if e.arc("scr").is_none() {
            return Err("no scr.arc".into());
        }
        e.ui = e.make_ui();
        if e.ui.title.is_none() {
            e.start()?;
        }
        Ok(e)
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        self.font = Some(
            fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }

    fn warn(&mut self, msg: String) {
        if self.log && self.warned.insert(msg.clone()) {
            eprintln!("kaguya: {msg}");
        }
    }

    fn arc(&mut self, name: &str) -> Option<&LinkArc> {
        if !self.arcs.contains_key(name) {
            let a = LinkArc::open(self.files.as_mut(), &format!("{name}.arc")).ok();
            self.arcs.insert(name.to_string(), a);
        }
        self.arcs.get(name)?.as_ref()
    }

    fn read(&mut self, arc: &str, stem: &str) -> Option<Vec<u8>> {
        self.arc(arc)?;
        let a = self.arcs.get(arc)?.as_ref()?;
        a.read(self.files.as_mut(), stem, &self.p.key)
    }

    /// A resource by number: its archive is its extension ("X.spd": spd.arc's X).
    fn resource(&mut self, id: u32) -> Option<Vec<u8>> {
        let name = self.p.names.get(id as usize)?.clone();
        let (s, ext) = name.rsplit_once('.')?;
        let data = self.read(ext, s);
        if data.is_none() {
            self.warn(format!("no {name}"));
        }
        data
    }

    fn pic(&mut self, id: u32) -> Option<Rc<Pic>> {
        if let Some(p) = self.pics.get(&id) {
            return Some(p.clone());
        }
        let b = self.resource(id)?;
        let p = match picture(&b) {
            Ok(p) => Rc::new(p),
            Err(err) => {
                let n = self.p.names[id as usize].clone();
                self.warn(format!("{n}: {err}"));
                return None;
            }
        };
        if self.pics.len() > 48 {
            self.pics.clear();
        }
        self.pics.insert(id, p.clone());
        Some(p)
    }

    fn ui_pic(&mut self, name: &str) -> Option<Placed> {
        let b = self.read("vrm", name)?;
        let p = picture(&b).ok()?;
        Some((Rc::new(p.img), p.x, p.y))
    }

    fn make_ui(&mut self) -> Ui {
        let mut ui = Ui {
            title: self.ui_pic("タイトル画面"),
            window: self.ui_pic("メッセ枠"),
            choice: self.ui_pic("選択肢"),
            choice_hover: self.ui_pic("選択肢ターゲット"),
            ..Ui::default()
        };
        for (word, action) in [
            ("はじめから", "start"),
            ("つづきから", ""),
            ("CGモード", ""),
            ("回想モード", ""),
            ("音楽鑑賞", ""),
            ("ゲームを終了", "quit"),
        ] {
            let normal = self.ui_pic(&format!("タイトル画面{word}オフ"));
            let hover = self.ui_pic(&format!("タイトル画面{word}ターゲット"));
            let Some((img, x, y)) = normal.clone().or(hover.clone()) else {
                continue;
            };
            ui.buttons.push(Button {
                normal,
                hover,
                rect: [x, y, x + img.w as i32, y + img.h as i32],
                action,
            });
        }
        ui
    }

    /// はじめから: the start script ("スタート.scr", else the first).
    fn start(&mut self) -> Result<(), String> {
        let id = self
            .p
            .find("スタート.scr")
            .or_else(|| {
                self.p
                    .names
                    .iter()
                    .position(|n| n.ends_with(".scr"))
                    .map(|i| i as u32)
            })
            .ok_or("no script")?;
        self.mode = Mode::Run;
        self.trans = None;
        self.goto_script(id)?;
        Ok(())
    }

    fn load(&mut self, id: u32) -> Result<Rc<Script>, String> {
        if let Some(s) = self.scripts.get(&id) {
            return Ok(s.clone());
        }
        let name = self
            .p
            .names
            .get(id as usize)
            .cloned()
            .ok_or_else(|| format!("no resource {id}"))?;
        let b = self
            .read("scr", stem(&name))
            .ok_or_else(|| format!("no script {name}"))?;
        let s = Rc::new(Script::parse(&name, b)?);
        self.scripts.insert(id, s.clone());
        Ok(s)
    }

    fn goto_script(&mut self, id: u32) -> Result<(), String> {
        let s = self.load(id)?;
        if self.log {
            eprintln!("kaguya: script {}", s.name);
        }
        self.script = Some(s);
        self.pc = 16;
        Ok(())
    }

    fn var(&self, i: i32) -> i32 {
        self.vars.get(i as u32 as usize).copied().unwrap_or(0)
    }

    fn set(&mut self, i: usize, v: i32) {
        if let Some(x) = self.vars.get_mut(i) {
            *x = v;
        }
    }

    fn sound(&mut self, channel: &str, data: Option<Vec<u8>>, repeat: bool, fade_ms: f32) {
        self.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data,
            volume: 1.0,
            repeat,
            fade_ms,
        });
    }

    fn play_bgm(&mut self, id: i32) {
        if id == self.bgm {
            return;
        }
        self.bgm = id;
        let data = if id < 0 {
            None
        } else {
            self.resource(id as u32)
        };
        self.sound("bgm", data, true, 500.0);
    }

    fn play_loop(&mut self, id: i32) {
        if id == self.se_loop {
            return;
        }
        self.se_loop = id;
        let data = if id < 0 {
            None
        } else {
            self.resource(id as u32)
        };
        self.sound("se_loop", data, true, 300.0);
    }

    /// The picture of scene `k` of the running script.
    fn compose(&mut self, k: usize) -> Rc<Image> {
        let Some(s) = self.script.clone() else {
            return black(self.width, self.height);
        };
        let mut img = Image::new(self.width, self.height);
        img.fill([0, 0, 0, 255]);
        for l in s.scenes.get(k).cloned().unwrap_or_default() {
            let ids = self
                .p
                .pictures
                .get(l.picture as usize)
                .cloned()
                .unwrap_or_default();
            for id in ids.into_iter().filter(|&i| i != 0) {
                if let Some(p) = self.pic(id) {
                    over(&mut img, &p.img, l.x + p.x, l.y + p.y, None);
                }
            }
        }
        Rc::new(img)
    }

    /// Shows scene `k`: kind 0 at once, 1 a fade, 4 a wipe by the rule `param`.
    fn show_scene(&mut self, k: usize, kind: u8, param: i32) {
        let name = self
            .script
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        self.scene = Some((name, k));
        self.finish_trans();
        let to = self.compose(k);
        let from = self.screen.clone();
        self.screen = to.clone();
        if self.skip || kind == 0 {
            return;
        }
        let (rule, ms) = match kind {
            4 => (
                self.pic(param as u32).map(|p| Rc::new(p.img.clone())),
                800.0,
            ),
            1 if (1..0x100).contains(&param) => (None, param as f64 * 1000.0 / 60.0),
            _ => (None, 500.0),
        };
        self.trans = Some(Trans {
            from,
            to,
            rule,
            start: self.now,
            ms,
        });
        self.wait = Wait::Trans;
    }

    fn finish_trans(&mut self) {
        self.trans = None;
        if self.wait == Wait::Trans {
            self.wait = Wait::None;
        }
    }

    fn say(&mut self, line: usize) {
        let Some(l) = self.m.lines.get(line) else {
            self.warn(format!("no line {line}"));
            return;
        };
        let name = l.speaker.and_then(|s| {
            if s == 0 {
                Some(self.p.hero.clone())
            } else {
                self.m.names.get(s).cloned()
            }
        });
        let text = l.text.clone();
        let voice = l.voice.clone();
        if self.log {
            match &name {
                Some(n) => eprintln!("kaguya: 【{n}】{}", text.replace('\n', "")),
                None => eprintln!("kaguya: {}", text.replace('\n', "")),
            }
        }
        self.said.push(text.clone());
        self.msg = Some(Message {
            name: name.filter(|n| !n.is_empty()),
            text,
            start: self.now,
        });
        self.window_img = None;
        if let Some(v) = voice
            && !self.skip
        {
            match self.read("voice", &v) {
                Some(d) => self.sound("voice", Some(d), false, 0.0),
                None => self.warn(format!("no voice {v}")),
            }
        }
        self.wait = Wait::Click;
    }

    fn ask(&mut self, choices: Vec<(String, i32)>, var: usize) {
        if self.log {
            let words: Vec<&str> = choices.iter().map(|c| c.0.as_str()).collect();
            eprintln!("kaguya: choose {words:?}");
        }
        self.choices = choices;
        self.choice_var = var;
        self.choice_imgs.clear();
        self.hover = None;
        self.wait = Wait::Choice;
    }

    fn answer(&mut self, k: usize) {
        let Some((_, v)) = self.choices.get(k).cloned() else {
            return;
        };
        if self.log {
            eprintln!("kaguya: choice {k}");
        }
        self.set(self.choice_var, v);
        self.choices.clear();
        self.choice_imgs.clear();
        self.hover = None;
        self.wait = Wait::None;
    }

    fn ret(&mut self) -> Result<(), String> {
        match self.stack.pop() {
            Some((s, pc)) => {
                self.script = Some(s);
                self.pc = pc;
            }
            None => self.end(),
        }
        Ok(())
    }

    fn end(&mut self) {
        if self.log {
            eprintln!("kaguya: the end");
        }
        self.mode = Mode::Ended;
        self.msg = None;
        self.sound("bgm", None, false, 1000.0);
        self.sound("se_loop", None, false, 300.0);
    }

    /// The map: the routes, from the scripts named "X0000　name　マップルート①".
    fn map_choices(&self) -> Vec<(String, i32)> {
        let mut out = Vec::new();
        for n in &self.p.names {
            if n.contains("マップルート①") {
                let who = n.split('　').nth(1).unwrap_or(n).to_string();
                out.push((who, out.len() as i32));
            }
        }
        if out.is_empty() {
            out.push(("→".to_string(), 0));
        }
        out
    }

    fn run(&mut self) -> Result<(), String> {
        for _ in 0..100_000 {
            if self.mode != Mode::Run || self.wait != Wait::None {
                return Ok(());
            }
            let Some(s) = self.script.clone() else {
                return Ok(());
            };
            let Some(i) = s.ins(self.pc) else {
                self.warn(format!("{}: no instruction at {:#x}", s.name, self.pc));
                self.ret()?;
                continue;
            };
            self.pc = i.next;
            match i.op {
                0x01 => {
                    let (kinds, op, dst) = (i.u8(0), i.u8(1), i.u16(2) as usize);
                    let a = self.var(i.i32(4));
                    let b = if kinds & 0x0f == 1 {
                        self.var(i.i32(8))
                    } else {
                        i.i32(8)
                    };
                    let v = match op {
                        0 => b,
                        1 => a.wrapping_add(b),
                        2 => a.wrapping_sub(b),
                        3 => a.wrapping_mul(b),
                        4 => a.checked_div(b).unwrap_or(0),
                        5 => a.checked_rem(b).unwrap_or(0),
                        _ => {
                            self.warn(format!("op 01 {op}"));
                            b
                        }
                    };
                    self.set(dst, v);
                }
                0x04 => self.ret()?,
                0x05 => {
                    let (flags, frames) = (i.u8(0), i.u16(1));
                    if flags & 0x80 == 0 && !self.skip {
                        self.wait = Wait::Until(self.now + frames as f64 * 1000.0 / 60.0);
                    }
                }
                0x06 => {
                    let k = i.i32(0);
                    if k >= 0 {
                        self.show_scene(k as usize, i.u8(8), i.i32(9));
                    }
                }
                0x07 => {
                    let (line, k, bgm, se) = (i.i32(0), i.i32(4), i.i32(8), i.i32(20));
                    let name = s.name.clone();
                    if k >= 0 && self.scene != Some((name, k as usize)) {
                        self.show_scene(k as usize, 0, 0);
                    }
                    self.play_bgm(bgm);
                    self.play_loop(se);
                    if line >= 0 {
                        self.say(line as usize);
                    }
                }
                0x08 => {
                    let n = (i.i32(1).max(0) as usize).min(self.m.choices.len());
                    let c = (0..n)
                        .map(|k| (self.m.choices[k].clone(), k as i32))
                        .collect();
                    self.ask(c, 0);
                }
                0x09 => {
                    let (ch, id) = (i.u8(0), i.i32(1));
                    match ch {
                        2 => self.play_loop(id),
                        0xff => {
                            self.play_loop(-1);
                            self.sound("se", None, false, 0.0);
                        }
                        _ if id < 0 => self.sound("se", None, false, 0.0),
                        _ => {
                            if !self.skip {
                                let d = self.resource(id as u32);
                                self.sound("se", d, false, 0.0);
                            }
                        }
                    }
                }
                0x0a => self.play_bgm(i.i32(0)),
                0x0b => self.pc = i.i32(0) as usize,
                0x0c | 0x1a => {
                    let id = if i.u8(0) == 4 {
                        i.i32(1)
                    } else {
                        self.var(i.i32(1))
                    };
                    if i.op == 0x1a {
                        self.stack.push((s.clone(), self.pc));
                    }
                    if let Err(err) = self.goto_script(id as u32) {
                        self.warn(err);
                        if i.op == 0x1a {
                            self.ret()?;
                        } else {
                            self.end();
                        }
                    }
                }
                0x0d => {
                    let (kinds, op, dst) = (i.u8(0), i.u8(1), i.u16(2) as usize);
                    let a = self.var(i.i32(4));
                    let b = if kinds >> 4 == 2 {
                        self.var(i.i32(8))
                    } else {
                        i.i32(8)
                    };
                    let v = match op {
                        0 => a == b,
                        1 => a != b,
                        2 => a < b,
                        3 => a <= b,
                        4 => a > b,
                        5 => a >= b,
                        6 => a != 0 && b != 0,
                        7 => a != 0 || b != 0,
                        _ => {
                            self.warn(format!("op 0d {op}"));
                            false
                        }
                    };
                    self.set(dst, v as i32);
                }
                0x0f => {
                    if self.var(i.u16(1) as i32) == 0 {
                        self.pc = i.i32(3) as usize;
                    }
                }
                0x14 => match i.u8(1) {
                    2 => {
                        let c = self.map_choices();
                        self.ask(c, 0x0c);
                    }
                    0x0e => self.ask(vec![("人間".into(), 1), ("妖怪".into(), 2)], 0x0c),
                    // 9: the trial's end (its script then shows its last scene and returns)
                    c => self.warn(format!("op 14 {c}")),
                },
                0x15 => {
                    if self.log {
                        let n = i.u8(0) as usize;
                        let raw = &i.args[1..(1 + n).min(i.args.len())];
                        let t = encoding_rs::SHIFT_JIS.decode(raw).0;
                        eprintln!("kaguya: [{t}]");
                    }
                }
                0x16 | 0x18 | 0x19 | 0x1b => {}
                op => self.warn(format!("op {op:02x}")),
            }
        }
        Ok(())
    }

    fn message_done(&self) -> bool {
        self.msg.as_ref().is_none_or(|m| {
            self.skip || (self.now - m.start) / LETTER_MS >= m.text.chars().count() as f64
        })
    }

    /// Runs one frame (1/60 s).
    pub fn frame(&mut self) -> Result<(), String> {
        self.now += 1000.0 / 60.0;
        let clicked = std::mem::take(&mut self.clicked);
        if let Some(t) = &self.trans
            && (self.now >= t.start + t.ms || clicked || self.skip)
        {
            self.finish_trans();
        }
        self.wait = match self.wait {
            Wait::Until(t) if self.now < t && !clicked && !self.skip => Wait::Until(t),
            Wait::Trans if self.trans.is_some() => Wait::Trans,
            Wait::Choice => Wait::Choice,
            Wait::Click => {
                if self.message_done() {
                    if clicked || self.skip {
                        self.sound("voice", None, false, 0.0);
                        self.msg = None;
                        Wait::None
                    } else {
                        Wait::Click
                    }
                } else {
                    if clicked && let Some(m) = &mut self.msg {
                        m.start = -1e9;
                    }
                    Wait::Click
                }
            }
            _ => Wait::None,
        };
        if self.mode == Mode::Run {
            self.run()?;
        }
        Ok(())
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    fn menu_at(&self, x: i32, y: i32) -> Option<usize> {
        self.ui
            .buttons
            .iter()
            .position(|b| x >= b.rect[0] && y >= b.rect[1] && x < b.rect[2] && y < b.rect[3])
    }

    pub fn point(&mut self, x: i32, y: i32) {
        if self.mode == Mode::Title {
            self.hover = self.menu_at(x, y);
        } else if self.wait == Wait::Choice {
            self.hover = self.choice_at(x, y);
        }
    }

    fn choose(&mut self, k: usize) {
        match self.ui.buttons.get(k).map(|b| b.action) {
            Some("start") => {
                if let Err(err) = self.start() {
                    self.warn(err);
                    self.mode = Mode::Ended;
                }
            }
            Some("quit") => self.mode = Mode::Ended,
            _ => {}
        }
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.point(x, y);
        match self.mode {
            Mode::Title => {
                if let Some(k) = self.menu_at(x, y) {
                    self.choose(k);
                }
            }
            Mode::Run if self.wait == Wait::Choice => {
                if let Some(k) = self.choice_at(x, y) {
                    self.answer(k);
                }
            }
            Mode::Run => self.clicked = true,
            Mode::Ended => {}
        }
    }

    pub fn cancel(&mut self) {
        self.skip = false;
    }

    /// 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip.
    pub fn key(&mut self, k: usize) {
        let n = match self.mode {
            Mode::Title => self.ui.buttons.len(),
            _ => self.choices.len(),
        }
        .max(1);
        let menu = self.mode == Mode::Title || self.wait == Wait::Choice;
        match k {
            0 if menu => self.hover = Some(self.hover.map_or(0, |h| (h + n - 1) % n)),
            1 if menu => self.hover = Some(self.hover.map_or(0, |h| (h + 1) % n)),
            4 if menu => {
                if let Some(h) = self.hover {
                    if self.mode == Mode::Title {
                        self.choose(h);
                    } else {
                        self.answer(h);
                    }
                }
            }
            4 => self.clicked = true,
            6 if self.mode == Mode::Run => self.skip = !self.skip,
            5 => self.skip = false,
            _ => {}
        }
    }

    pub fn wheel(&mut self, up: bool) {
        if !up && self.mode == Mode::Run {
            self.clicked = true;
        }
    }

    pub fn choosing(&self) -> bool {
        self.wait == Wait::Choice
    }

    pub fn ended(&self) -> bool {
        self.mode == Mode::Ended
    }

    fn screen_now(&self) -> Vec<(Rc<Image>, f32)> {
        let Some(t) = &self.trans else {
            return vec![(self.screen.clone(), 1.0)];
        };
        let p = ((self.now - t.start) / t.ms).clamp(0.0, 1.0) as f32;
        let Some(rule) = &t.rule else {
            return vec![(t.from.clone(), 1.0), (t.to.clone(), p)];
        };
        let mut out = (*t.from).clone();
        out.renew();
        let edge = 32.0;
        let level = p * (255.0 + edge);
        let (w, h) = (out.w as usize, out.h as usize);
        for y in 0..h {
            for x in 0..w {
                let r = if (x as u32) < rule.w && (y as u32) < rule.h {
                    rule.px[(y * rule.w as usize + x) * 4] as f32
                } else {
                    0.0
                };
                let a = ((level - r) / edge).clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let i = (y * w + x) * 4;
                for k in 0..3 {
                    let (f, n) = (out.px[i + k] as f32, t.to.px[i + k] as f32);
                    out.px[i + k] = (f + (n - f) * a) as u8;
                }
            }
        }
        vec![(Rc::new(out), 1.0)]
    }

    /// The name and the letters shown, on a clear picture from the window's top.
    fn message_picture(&mut self) -> Option<(Rc<Image>, i32)> {
        let m = self.msg.as_ref()?;
        let shown = if self.message_done() {
            usize::MAX
        } else {
            ((self.now - m.start) / LETTER_MS) as usize
        };
        let top = self
            .ui
            .window
            .as_ref()
            .map_or(self.height as i32 - 188, |w| w.2);
        if let Some((n, img)) = &self.window_img
            && *n == shown
        {
            return Some((img.clone(), top));
        }
        let mut img = Image::new(self.width, (self.height as i32 - top).max(1) as u32);
        img.fill([0, 0, 0, 0]);
        if let Some(font) = &self.font {
            if let Some(n) = &m.name {
                text(&mut img, font, n, 56.0, 42.0, 22.0, usize::MAX);
            }
            text(&mut img, font, &m.text, 96.0, 72.0, 24.0, shown);
        }
        let img = Rc::new(img);
        self.window_img = Some((shown, img.clone()));
        Some((img, top))
    }

    fn choice_pictures(&mut self) -> Vec<Placed> {
        if self.choice_imgs.len() == self.choices.len() {
            return self.choice_imgs.clone();
        }
        let base = self.ui.choice.clone();
        let (w, h) = base.as_ref().map_or((self.width as i32 * 3 / 5, 60), |b| {
            (b.0.w as i32, b.0.h as i32)
        });
        let gap = 12;
        let total = self.choices.len() as i32 * (h + gap) - gap;
        let mut out = Vec::new();
        for (k, (c, _)) in self.choices.iter().enumerate() {
            let mut img = Image::new(w as u32, h as u32);
            img.fill([0, 0, 0, 0]);
            if base.is_none() {
                img.fill([20, 20, 40, 200]);
            }
            if let Some(font) = &self.font {
                let size = 24.0;
                let width: f32 = c
                    .chars()
                    .map(|ch| font.metrics(ch, size).advance_width)
                    .sum();
                text(
                    &mut img,
                    font,
                    c,
                    (w as f32 - width) / 2.0,
                    (h as f32 - size) / 2.0 - 2.0,
                    size,
                    usize::MAX,
                );
            }
            let x = base.as_ref().map_or((self.width as i32 - w) / 2, |b| b.1);
            let y = (self.height as i32 - total) / 2 - 60 + k as i32 * (h + gap);
            out.push((Rc::new(img), x, y));
        }
        self.choice_imgs = out.clone();
        out
    }

    fn choice_at(&mut self, x: i32, y: i32) -> Option<usize> {
        self.choice_pictures().iter().position(|(img, cx, cy)| {
            x >= *cx && y >= *cy && x < cx + img.w as i32 && y < cy + img.h as i32
        })
    }

    /// The pictures to draw, bottom first.
    pub fn draws(&mut self) -> Vec<Draw> {
        let full = [0.0, 0.0, self.width as f32, self.height as f32];
        let place = |x: f32, y: f32, alpha: f32| Place {
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
            clip: full,
        };
        let whole = |image: Rc<Image>, x: i32, y: i32, alpha: f32| {
            let (w, h) = (image.w as i32, image.h as i32);
            let (x, y) = (x as f32, y as f32);
            Draw {
                src: [0, 0, w, h],
                dst: [x, y, x + w as f32, y + h as f32],
                image,
                place: place(x, y, alpha),
            }
        };
        let mut out = Vec::new();
        if self.mode == Mode::Title {
            if let Some((img, x, y)) = self.ui.title.clone() {
                out.push(whole(img, x, y, 1.0));
            }
            for (k, b) in self.ui.buttons.iter().enumerate() {
                let p = if self.hover == Some(k) {
                    b.hover.clone().or(b.normal.clone())
                } else {
                    b.normal.clone()
                };
                if let Some((img, x, y)) = p {
                    out.push(whole(img, x, y, 1.0));
                }
            }
            return out;
        }
        for (img, alpha) in self.screen_now() {
            out.push(whole(img, 0, 0, alpha));
        }
        if self.mode == Mode::Run && self.msg.is_some() && self.trans.is_none() {
            if let Some((img, x, y)) = self.ui.window.clone() {
                out.push(whole(img, x, y, 1.0));
            }
            if let Some((img, top)) = self.message_picture() {
                out.push(whole(img, 0, top, 1.0));
            }
        }
        if self.wait == Wait::Choice {
            let (base, lit) = (self.ui.choice.clone(), self.ui.choice_hover.clone());
            for (k, (img, x, y)) in self.choice_pictures().into_iter().enumerate() {
                let back = if self.hover == Some(k) {
                    lit.clone().or(base.clone())
                } else {
                    base.clone()
                };
                if let Some((b, _, _)) = &back {
                    out.push(whole(b.clone(), x, y, 1.0));
                }
                let a = if back.is_none() && self.hover != Some(k) {
                    0.75
                } else {
                    1.0
                };
                out.push(whole(img, x, y, a));
            }
        }
        out
    }
}
