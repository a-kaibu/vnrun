//! The runtime: the script machine, the picture layers and their transitions, the message
//! window and the title menu (the last two are drawn by the program itself, from the parts in
//! vram01/vram02).

use std::collections::HashMap;
use std::rc::Rc;

use nexas::files::Files;
use nexas::gfx::{Draw, Image, Place};

use crate::arc::Arc;
use crate::scr::{Ins, Script};
use crate::text::Text;

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

const W: u32 = 800;
const H: u32 = 600;
/// how long a letter takes to appear
const LETTER_MS: f64 = 25.0;
/// the message window: its part of vram02 and where it goes
const WINDOW: [i32; 4] = [0, 0, 800, 128];
const WINDOW_Y: i32 = 600 - 128;
/// the title menu: (part of vram01's rows, what it does)
const MENU: [(i32, Choice); 2] = [(602, Choice::New), (782, Choice::Exit)];
const MENU_X: i32 = 580;
const MENU_Y: i32 = 470;
const MENU_STEP: i32 = 36;
const BUTTON: (i32, i32) = (196, 30);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Choice {
    New,
    Exit,
}

#[derive(Clone, Debug, PartialEq)]
enum Wait {
    None,
    /// a message: until it has appeared and been clicked
    Click,
    Until(f64, bool),
    /// a transition or fade
    Shown,
    /// the moving pictures (op 0x64)
    Objects,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Run,
    Title,
    Ended,
}

/// What a scene is made of; layers by the load type of op 0x15.
#[derive(Clone, Default, PartialEq)]
struct Scene {
    bg: Option<String>,
    cg: Option<String>,
    /// load types 2..=5
    chars: [Option<String>; 4],
    /// colour multipliers by layer (0 background, 2..=5 characters, 7 event picture)
    tint: [Option<[u8; 3]>; 8],
}

#[derive(Clone)]
enum TransKind {
    Cross,
    Rule(Rc<Image>),
    /// from a colour to the new picture
    In([u8; 3]),
    /// from the old picture to a colour (the new screen is that colour)
    Out([u8; 3]),
}

struct Trans {
    from: Rc<Image>,
    to: Rc<Image>,
    kind: TransKind,
    start: f64,
    ms: f64,
}

/// A movement: from, to, start, ms.
type Move = ((f32, f32), (f32, f32), f64, f64);

/// A picture moving over the scene (ops 0x62..0x66): a cut-in.
#[derive(Clone)]
struct Obj {
    image: Rc<Image>,
    x: f32,
    y: f32,
    alpha: f32,
    /// moving: from, to, start, ms
    mv: Option<Move>,
    fade: Option<(f32, f32, f64, f64)>,
    remove_at: Option<f64>,
}

struct Message {
    name: Option<String>,
    text: String,
    start: f64,
}

pub struct Engine {
    files: Box<dyn Files>,
    arcs: HashMap<&'static str, Arc>,
    text: Text,
    pub width: u32,
    pub height: u32,
    font: Option<fontdue::Font>,
    pub log: bool,
    pub now: f64,
    mode: Mode,
    script: Script,
    pc: usize,
    vars: Vec<i32>,
    wait: Wait,
    back: Scene,
    screen: Rc<Image>,
    trans: Option<Trans>,
    rule: Option<Rc<Image>>,
    flash: Option<([u8; 3], f64)>,
    shake: Option<f64>,
    objs: [Option<Obj>; 3],
    window: bool,
    msg: Option<Message>,
    /// the window as drawn: (letters shown, picture)
    window_img: Option<(usize, Rc<Image>)>,
    pics: HashMap<String, Rc<Image>>,
    sounds: Vec<SoundEvent>,
    se_next: usize,
    skip: bool,
    clicked: bool,
    pointer: (i32, i32),
    hover: Option<usize>,
    warned: std::collections::HashSet<String>,
    /// every message shown, for the log
    pub said: Vec<String>,
}

fn solid(rgb: [u8; 3]) -> Rc<Image> {
    let mut img = Image::new(1, 1);
    img.fill([rgb[0], rgb[1], rgb[2], 255]);
    Rc::new(img)
}

fn black_screen() -> Rc<Image> {
    let mut img = Image::new(W, H);
    img.fill([0, 0, 0, 255]);
    Rc::new(img)
}

/// Lays `src` over `dst` (both the screen's size, or `src` smaller at the top left).
fn over(dst: &mut Image, src: &Image, tint: Option<[u8; 3]>) {
    let (w, h) = (src.w.min(dst.w) as usize, src.h.min(dst.h) as usize);
    for y in 0..h {
        let s = &src.px[y * src.w as usize * 4..][..w * 4];
        let d = &mut dst.px[y * dst.w as usize * 4..][..w * 4];
        for (d, s) in d.chunks_exact_mut(4).zip(s.chunks_exact(4)) {
            let a = s[3] as u32;
            if a == 0 {
                continue;
            }
            for k in 0..3 {
                let c = match tint {
                    Some(t) => s[k] as u32 * t[k] as u32 / 255,
                    None => s[k] as u32,
                };
                d[k] = ((c * a + d[k] as u32 * (255 - a)) / 255) as u8;
            }
            d[3] = 255;
        }
    }
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let mut arcs = HashMap::new();
        for name in [
            "scr", "bg_", "sp_", "cg_", "cgw", "bgm", "wav", "vo1", "vo2",
        ] {
            let file = format!("{name}.arc");
            if files.size(&file).is_some() {
                arcs.insert(name, Arc::open(files.as_mut(), &file)?);
            }
        }
        if !arcs.contains_key("scr") {
            return Err("no scr.arc".into());
        }
        let text = Text::new(
            files.read("tblstr.arc").ok_or("no tblstr.arc")?,
            files.read("tblstr.ari").ok_or("no tblstr.ari")?,
        );
        let mut e = Engine {
            files,
            arcs,
            text,
            width: W,
            height: H,
            font: None,
            log: false,
            now: 0.0,
            mode: Mode::Run,
            script: Script {
                name: String::new(),
                code: Vec::new(),
            },
            pc: 0,
            vars: vec![0; 4096],
            wait: Wait::None,
            back: Scene::default(),
            screen: black_screen(),
            trans: None,
            rule: None,
            flash: None,
            shake: None,
            objs: [None, None, None],
            window: false,
            msg: None,
            window_img: None,
            pics: HashMap::new(),
            sounds: Vec::new(),
            se_next: 0,
            skip: false,
            clicked: false,
            pointer: (0, 0),
            hover: None,
            warned: Default::default(),
            said: Vec::new(),
        };
        // set by the program before the logo; otherwise the logo goes to a debug script
        e.vars[2] = 1;
        e.load_script("logo")?;
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

    fn read(&mut self, arc: &str, name: &str) -> Option<Vec<u8>> {
        let a = self.arcs.get(arc)?;
        let e = a.find(name)?.clone();
        a.read(self.files.as_mut(), &e)
    }

    fn load_script(&mut self, name: &str) -> Result<(), String> {
        let b = self
            .read("scr", name)
            .ok_or_else(|| format!("no script {name}"))?;
        self.script = Script::parse(name, &b)?;
        self.pc = 0;
        if self.log {
            eprintln!("kaguya: script {name}");
        }
        Ok(())
    }

    fn pic(&mut self, arc: &str, name: &str) -> Option<Rc<Image>> {
        let k = format!("{arc}/{name}");
        if let Some(p) = self.pics.get(&k) {
            return Some(p.clone());
        }
        let Some(b) = self.read(arc, name) else {
            self.warn(format!("no picture {k}"));
            return None;
        };
        match crate::pic::decode(&b) {
            Ok(img) => {
                if self.pics.len() > 40 {
                    self.pics.clear();
                }
                let img = Rc::new(img);
                self.pics.insert(k, img.clone());
                Some(img)
            }
            Err(err) => {
                self.warn(format!("{k}: {err}"));
                None
            }
        }
    }

    fn has(&self, arc: &str, name: &str) -> bool {
        self.arcs.get(arc).is_some_and(|a| a.find(name).is_some())
    }

    /// A character: "nata13" is the body "nata00" with the face "nat013" over it. Without
    /// such a body the name is a whole picture, or the face goes on the plain body "nat000".
    fn character(&mut self, name: &str) -> Vec<Rc<Image>> {
        let c: Vec<char> = name.chars().collect();
        if c.len() != 6 {
            return self.pic("sp_", name).into_iter().collect();
        }
        let base: String = c[..3].iter().collect();
        let mut body = format!("{base}{}00", c[3]);
        let face = format!("{base}0{}{}", c[4], c[5]);
        if !self.has("sp_", &body) {
            if self.has("sp_", name) {
                return self.pic("sp_", name).into_iter().collect();
            } else {
                body = format!("{base}000");
            }
        }
        let mut out = Vec::new();
        if self.has("sp_", &body)
            && let Some(b) = self.pic("sp_", &body)
        {
            out.push(b);
        }
        if face != body
            && self.has("sp_", &face)
            && let Some(f) = self.pic("sp_", &face)
        {
            out.push(f);
        }
        if out.is_empty() {
            self.warn(format!("no character {name}"));
        }
        out
    }

    fn compose(&mut self) -> Rc<Image> {
        let s = self.back.clone();
        let mut out = Image::new(W, H);
        out.fill([0, 0, 0, 255]);
        if let Some(bg) = &s.bg
            && let Some(p) = self.pic("bg_", bg)
        {
            over(&mut out, &p, s.tint[0]);
        }
        if let Some(cg) = &s.cg
            && let Some(p) = self.pic("cg_", cg).or_else(|| self.pic("bg_", cg))
        {
            over(&mut out, &p, s.tint[7]);
        }
        // the load types 3, 4, 5 and 2 are the program's slots 1 to 4
        for t in [3usize, 4, 5, 2] {
            if let Some(name) = &s.chars[t - 2] {
                for p in self.character(name) {
                    over(&mut out, &p, s.tint[t]);
                }
            }
        }
        Rc::new(out)
    }

    /// Shows the scene being made, with a transition.
    fn show(&mut self, kind: Option<TransKind>, ms: f64) {
        let to = match &kind {
            Some(TransKind::Out(c)) => {
                let mut img = Image::new(W, H);
                img.fill([c[0], c[1], c[2], 255]);
                Rc::new(img)
            }
            _ => self.compose(),
        };
        self.finish_trans();
        match kind {
            Some(kind) if !self.skip && ms > 0.0 => {
                self.trans = Some(Trans {
                    from: self.screen.clone(),
                    to,
                    kind,
                    start: self.now,
                    ms,
                });
                self.wait = Wait::Shown;
            }
            _ => self.screen = to,
        }
    }

    fn finish_trans(&mut self) {
        if let Some(t) = self.trans.take() {
            self.screen = t.to;
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

    fn voice(&mut self, name: &str) {
        let data = self.read("vo1", name).or_else(|| self.read("vo2", name));
        if data.is_none() {
            self.warn(format!("no voice {name}"));
            return;
        }
        if !self.skip {
            self.sound("voice", data, false, 0.0);
        }
    }

    /// Runs the script until it waits.
    fn run(&mut self) -> Result<(), String> {
        for _ in 0..100_000 {
            if self.mode != Mode::Run || self.wait != Wait::None {
                return Ok(());
            }
            let Some(ins) = self.script.decode(self.pc) else {
                self.warn(format!("{}: bad code at {:#x}", self.script.name, self.pc));
                self.back_to_title();
                return Ok(());
            };
            self.pc = ins.next;
            self.exec(&ins)?;
        }
        Err("the script does not wait".into())
    }

    fn val(&self, v: i32, constant: bool) -> i32 {
        if constant {
            v
        } else {
            self.vars.get(v as usize).copied().unwrap_or(0)
        }
    }

    fn exec(&mut self, i: &Ins) -> Result<(), String> {
        match i.op {
            0x00 => {
                let v = i.u16(2);
                if let Some(x) = self.vars.get_mut(v) {
                    *x = i.i32(4);
                }
            }
            0x01 => {
                let v = i.u16(2);
                if let Some(x) = self.vars.get_mut(v) {
                    *x = x.wrapping_add(i.i32(4));
                }
            }
            0x5b => {
                let v = self.val(i.u16(6) as i32, false);
                if let Some(x) = self.vars.get_mut(i.u16(4)) {
                    *x = v;
                }
            }
            0x02 => self.pc = i.i32(4) as usize,
            0x03..=0x08 => {
                let flags = i.u8(2);
                let a = self.val(i.i32(4), flags & 1 != 0);
                let b = self.val(i.i32(8), flags & 2 != 0);
                let jump = match i.op {
                    0x03 => a == b,
                    0x04 => a != b,
                    0x05 => a > b,
                    0x06 => a < b,
                    0x07 => a >= b,
                    _ => a <= b,
                };
                if jump {
                    self.pc = i.i32(12) as usize;
                }
            }
            0x0c => {
                let name = i.s(0).to_string();
                if self.load_script(&name).is_err() {
                    self.warn(format!("no script {name}"));
                    self.back_to_title();
                }
            }
            0x27 => self.back_to_title(),
            // the title: the program's own menu (game mode 0x15)
            0x2a => self.enter_title(),
            0x13 => self.message(i),
            0x14 | 0x3f => {
                self.window = false;
                self.msg = None;
            }
            0x15 => self.load(i.u8(3), i.s(0)),
            0x17 => match i.u8(2) {
                0 => {
                    self.back = Scene::default();
                }
                t @ 2..=5 => {
                    self.back.chars[t as usize - 2] = None;
                    self.back.tint[t as usize] = None;
                }
                7 => {
                    self.back.cg = None;
                    self.back.tint[7] = None;
                }
                _ => {}
            },
            0x58 => {
                let l = i.u8(2) as usize;
                if l < 8 {
                    self.back.tint[l] = Some(i.rgb(3));
                }
            }
            0x16 => {
                let kind = match self.rule.take() {
                    Some(r) => TransKind::Rule(r),
                    None => TransKind::Cross,
                };
                let ms = if matches!(kind, TransKind::Rule(_)) {
                    700.0
                } else {
                    400.0
                };
                self.show(Some(kind), ms);
            }
            0x1d => self.show(None, 0.0),
            0x19 => self.show(Some(TransKind::In([0; 3])), 1000.0),
            0x1a => self.show(Some(TransKind::Out([0; 3])), 1000.0),
            0x1f => self.show(Some(TransKind::In([0; 3])), 500.0),
            0x20 => self.show(Some(TransKind::Out([0; 3])), 500.0),
            0x1b => self.show(Some(TransKind::In([255; 3])), 500.0),
            0x1c => self.show(Some(TransKind::Out([255; 3])), 500.0),
            0x21 => self.flash([255; 3]),
            0x5f => self.flash(i.rgb(2)),
            0x22 => {
                if !self.skip {
                    self.shake = Some(self.now);
                    self.wait = Wait::Until(self.now + 300.0, true);
                }
            }
            0x18 | 0x57 => {
                let ms = i.i32(4) as f64 * 100.0;
                if !self.skip || i.op == 0x18 {
                    self.wait = Wait::Until(self.now + ms, i.op == 0x57);
                }
            }
            0x50 => {
                let name = i.s(0).to_string();
                match i.u8(2) {
                    2 => {
                        let data = self.read("bgm", &name);
                        if data.is_none() {
                            self.warn(format!("no music {name}"));
                        }
                        self.sound("bgm", data, i.u8(4) == 0, 0.0);
                    }
                    k => self.warn(format!("music {name} of kind {k}")),
                }
            }
            0x53 => match i.u8(3) {
                0 => self.sound("bgm", None, false, 1000.0),
                _ => self.stop_se(),
            },
            0x52 | 0x0f => {
                let name = i.s(0).to_string();
                let data = self.read("wav", &name);
                if data.is_none() {
                    self.warn(format!("no sound {name}"));
                }
                let repeat = i.op == 0x52 && i.u8(2) == 2;
                if repeat {
                    self.sound("seloop", data, true, 0.0);
                } else if !self.skip {
                    let ch = format!("se{}", self.se_next);
                    self.se_next = (self.se_next + 1) % 4;
                    self.sound(&ch, data, false, 0.0);
                }
            }
            0x55 => self.stop_se(),
            0x2c => {
                let name = i.s(0).to_string();
                self.voice(&name);
            }
            0x62 => {
                let slot = i.u8(2) as usize;
                let name = i.s(0).to_string();
                let arc = i.s(1).to_string();
                let image = self.pic(&arc, &name).or_else(|| self.pic("bg_", &name));
                if slot < 3
                    && let Some(image) = image
                {
                    self.objs[slot] = Some(Obj {
                        image,
                        x: i.i16(4) as f32,
                        y: i.i16(6) as f32,
                        alpha: 1.0,
                        mv: None,
                        fade: None,
                        remove_at: None,
                    });
                }
            }
            0x63 => {
                let now = self.now;
                if let Some(Some(o)) = self.objs.get_mut(i.u8(2) as usize) {
                    let to = (i.i16(6) as f32, i.i16(8) as f32);
                    o.mv = Some(((o.x, o.y), to, now, i.i16(4).max(0) as f64));
                }
            }
            0x66 => {
                let now = self.now;
                if let Some(Some(o)) = self.objs.get_mut(i.u8(2) as usize) {
                    o.alpha = 0.0;
                    o.fade = Some((0.0, 1.0, now, i.i16(4).max(0) as f64));
                }
            }
            0x64 => {
                if !self.skip {
                    self.wait = Wait::Objects;
                }
            }
            0x65 => {
                let now = self.now;
                if let Some(Some(o)) = self.objs.get_mut(i.u8(2) as usize) {
                    o.fade = Some((o.alpha, 0.0, now, 200.0));
                    o.remove_at = Some(now + 200.0);
                }
            }
            // the rest: settings, the scene index, chapter marks, MIDI, ...
            0x0a | 0x0b | 0x0d | 0x0e | 0x10 | 0x12 | 0x23 | 0x29 | 0x36 | 0x37 | 0x3d | 0x47
            | 0x48 | 0x54 | 0x5c | 0x5d | 0x60 => {}
            op => self.warn(format!("op {op:#04x} {:02x?}", i.f)),
        }
        Ok(())
    }

    fn stop_se(&mut self) {
        self.sound("seloop", None, false, 300.0);
    }

    fn flash(&mut self, rgb: [u8; 3]) {
        if !self.skip {
            self.flash = Some((rgb, self.now));
            self.wait = Wait::Until(self.now + 250.0, true);
        }
    }

    fn load(&mut self, kind: u8, file: &str) {
        let name = Some(file.to_string());
        match kind {
            0 => {
                self.back = Scene {
                    bg: name,
                    ..Scene::default()
                };
            }
            1 | 6 => {
                self.back.bg = name;
                self.back.tint[0] = None;
            }
            2..=5 => {
                self.back.chars[kind as usize - 2] = name;
                self.back.tint[kind as usize] = None;
            }
            7 => {
                self.back.cg = name;
                self.back.tint[7] = None;
            }
            8 => {
                self.rule = self.pic("bg_", file);
            }
            k => self.warn(format!("picture {name:?} of kind {k}")),
        }
    }

    fn message(&mut self, i: &Ins) {
        let name = (i.i32(4) >= 0)
            .then(|| self.text.at(i.i32(4) as usize))
            .flatten();
        let text = self.text.line(i.i32(8) as usize).unwrap_or_default();
        if self.log {
            match &name {
                Some(n) => eprintln!("【{n}】{}", text.replace('\n', "")),
                None => eprintln!("{}", text.replace('\n', "")),
            }
        }
        self.said.push(text.clone());
        let voice = i.s(0).to_string();
        self.sound("voice", None, false, 0.0);
        if !voice.is_empty() {
            self.voice(&voice);
        }
        self.window = true;
        self.window_img = None;
        self.msg = Some(Message {
            name,
            text,
            start: self.now,
        });
        self.wait = Wait::Click;
    }

    fn back_to_title(&mut self) {
        self.sound("voice", None, false, 0.0);
        self.stop_se();
        if self.load_script("title").is_err() {
            self.mode = Mode::Ended;
        }
        self.wait = Wait::None;
        self.window = false;
        self.msg = None;
        self.objs = [None, None, None];
        self.skip = false;
    }

    fn enter_title(&mut self) {
        self.mode = Mode::Title;
        self.back = Scene {
            bg: Some("タイトル画面".into()),
            ..Scene::default()
        };
        self.show(Some(TransKind::In([0; 3])), 600.0);
        let data = self.read("bgm", "タイトル");
        self.sound("bgm", data, true, 0.0);
    }

    fn menu_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..MENU.len()).find(|&k| {
            let top = MENU_Y + k as i32 * MENU_STEP;
            x >= MENU_X && x < MENU_X + BUTTON.0 && y >= top && y < top + BUTTON.1
        })
    }

    fn choose(&mut self, k: usize) {
        match MENU[k].1 {
            Choice::New => {
                self.sound("bgm", None, false, 500.0);
                self.vars = vec![0; 4096];
                self.vars[2] = 1;
                self.mode = Mode::Run;
                self.back = Scene::default();
                self.show(Some(TransKind::Out([0; 3])), 500.0);
                if self.load_script("init").is_err() {
                    self.mode = Mode::Ended;
                }
            }
            Choice::Exit => self.mode = Mode::Ended,
        }
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
            && (self.now >= t.start + t.ms || clicked)
        {
            self.finish_trans();
        }
        let now = self.now;
        for slot in self.objs.iter_mut() {
            let Some(o) = slot else { continue };
            if let Some((a, b, s, ms)) = o.mv {
                let t = if ms <= 0.0 {
                    1.0
                } else {
                    ((now - s) / ms).clamp(0.0, 1.0)
                } as f32;
                o.x = a.0 + (b.0 - a.0) * t;
                o.y = a.1 + (b.1 - a.1) * t;
                if t >= 1.0 {
                    o.mv = None;
                }
            }
            if let Some((a, b, s, ms)) = o.fade {
                let t = if ms <= 0.0 {
                    1.0
                } else {
                    ((now - s) / ms).clamp(0.0, 1.0)
                } as f32;
                o.alpha = a + (b - a) * t;
                if t >= 1.0 {
                    o.fade = None;
                }
            }
            if o.remove_at.is_some_and(|r| now >= r) {
                *slot = None;
            }
        }
        if self.flash.is_some_and(|(_, s)| now > s + 250.0) {
            self.flash = None;
        }
        if self.shake.is_some_and(|s| now > s + 300.0) {
            self.shake = None;
        }
        self.wait = match std::mem::replace(&mut self.wait, Wait::None) {
            Wait::Shown if self.trans.is_some() => Wait::Shown,
            Wait::Until(t, skippable) if now < t && !(skippable && (clicked || self.skip)) => {
                Wait::Until(t, skippable)
            }
            Wait::Objects
                if self
                    .objs
                    .iter()
                    .flatten()
                    .any(|o| o.mv.is_some() || o.fade.is_some()) =>
            {
                Wait::Objects
            }
            Wait::Click => {
                if self.message_done() {
                    if clicked || self.skip {
                        self.sound("voice", None, false, 0.0);
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

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
        if self.mode == Mode::Title {
            self.hover = self.menu_at(x, y);
        }
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.point(x, y);
        if self.mode == Mode::Title {
            if self.trans.is_some() {
                self.finish_trans();
            } else if let Some(k) = self.menu_at(x, y) {
                self.choose(k);
            }
            return;
        }
        self.clicked = true;
    }

    pub fn cancel(&mut self) {
        self.skip = false;
    }

    /// 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip.
    pub fn key(&mut self, k: usize) {
        match (self.mode, k) {
            (Mode::Title, 0) => {
                self.hover = Some(self.hover.map_or(0, |h| (h + MENU.len() - 1) % MENU.len()))
            }
            (Mode::Title, 1) => self.hover = Some(self.hover.map_or(0, |h| (h + 1) % MENU.len())),
            (Mode::Title, 4) => {
                if let Some(h) = self.hover {
                    self.choose(h);
                }
            }
            (Mode::Run, 4) => self.clicked = true,
            (Mode::Run, 6) => self.skip = !self.skip,
            (_, 5) => self.skip = false,
            _ => {}
        }
    }

    pub fn wheel(&mut self, up: bool) {
        if !up {
            self.clicked = true;
        }
    }

    pub fn ended(&self) -> bool {
        self.mode == Mode::Ended
    }

    /// Where the scene is now (a rule transition is worked out here).
    fn screen_now(&self) -> Vec<(Rc<Image>, f32)> {
        let Some(t) = &self.trans else {
            return vec![(self.screen.clone(), 1.0)];
        };
        let p = ((self.now - t.start) / t.ms).clamp(0.0, 1.0) as f32;
        match &t.kind {
            TransKind::Cross => vec![(t.from.clone(), 1.0), (t.to.clone(), p)],
            TransKind::In(c) => vec![(solid(*c), 1.0), (t.to.clone(), p)],
            TransKind::Out(c) => vec![(t.from.clone(), 1.0), (solid(*c), p)],
            TransKind::Rule(rule) => {
                let mut out = (*t.from).clone();
                out.renew();
                // dark parts of the rule first, with a soft edge
                let edge = 32.0;
                let level = p * (255.0 + edge);
                let (w, h) = (W as usize, H as usize);
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
        }
    }

    fn window_picture(&mut self) -> Option<Rc<Image>> {
        let m = self.msg.as_ref()?;
        let shown = if self.message_done() {
            usize::MAX
        } else {
            ((self.now - m.start) / LETTER_MS) as usize
        };
        if let Some((n, img)) = &self.window_img
            && *n == shown
        {
            return Some(img.clone());
        }
        let parts = self.pic("bg_", "vram02")?;
        let (ww, wh) = (
            (WINDOW[2] - WINDOW[0]) as u32,
            (WINDOW[3] - WINDOW[1]) as u32,
        );
        let mut img = Image::new(ww, wh);
        for y in 0..wh as usize {
            let s = &parts.px
                [((WINDOW[1] as usize + y) * parts.w as usize + WINDOW[0] as usize) * 4..]
                [..ww as usize * 4];
            img.px[y * ww as usize * 4..][..ww as usize * 4].copy_from_slice(s);
        }
        let m = self.msg.as_ref()?;
        if let Some(font) = &self.font {
            if let Some(n) = &m.name {
                let width: f32 = n.chars().map(|c| font.metrics(c, 20.0).advance_width).sum();
                let x = 122.0 + (268.0 - width) / 2.0;
                text(&mut img, font, n, x, 9.0, 20.0, usize::MAX);
            }
            text(&mut img, font, &m.text, 120.0, 46.0, 22.0, shown);
        }
        let img = Rc::new(img);
        self.window_img = Some((shown, img.clone()));
        Some(img)
    }

    /// The pictures to draw, bottom first.
    pub fn draws(&mut self) -> Vec<Draw> {
        let full = [0.0, 0.0, W as f32, H as f32];
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
        let whole = |image: Rc<Image>, x: f32, y: f32, alpha: f32| {
            let (w, h) = (image.w as i32, image.h as i32);
            Draw {
                src: [0, 0, w, h],
                dst: [x, y, x + w as f32, y + h as f32],
                image,
                place: place(x, y, alpha),
            }
        };
        let mut out = Vec::new();
        let (sx, sy) = match self.shake {
            Some(s) => {
                let k = ((self.now - s) / 30.0) as i32;
                (
                    [-8.0, 8.0, -5.0, 5.0][k as usize % 4],
                    [4.0, -4.0, 6.0, -6.0][k as usize % 4],
                )
            }
            None => (0.0, 0.0),
        };
        for (img, alpha) in self.screen_now() {
            if img.w == 1 {
                out.push(Draw {
                    image: img,
                    src: [0, 0, 1, 1],
                    dst: full,
                    place: place(0.0, 0.0, alpha),
                });
            } else {
                out.push(whole(img, sx, sy, alpha));
            }
        }
        for o in self.objs.iter().flatten() {
            out.push(whole(o.image.clone(), o.x, o.y, o.alpha));
        }
        if let Some((c, s)) = self.flash {
            let a = 1.0 - ((self.now - s) / 250.0).clamp(0.0, 1.0) as f32;
            out.push(Draw {
                image: solid(c),
                src: [0, 0, 1, 1],
                dst: full,
                place: place(0.0, 0.0, a),
            });
        }
        if self.window
            && self.mode == Mode::Run
            && let Some(w) = self.window_picture()
        {
            out.push(whole(w, 0.0, WINDOW_Y as f32, 1.0));
        }
        if self.mode == Mode::Title
            && self.trans.is_none()
            && let Some(parts) = self.pic("bg_", "vram01")
        {
            for (k, (row, _)) in MENU.iter().enumerate() {
                let x0 = if self.hover == Some(k) { BUTTON.0 } else { 0 };
                let (x, y) = (MENU_X as f32, (MENU_Y + k as i32 * MENU_STEP) as f32);
                out.push(Draw {
                    image: parts.clone(),
                    src: [x0, *row, x0 + BUTTON.0, row + BUTTON.1],
                    dst: [x, y, x + BUTTON.0 as f32, y + BUTTON.1 as f32],
                    place: place(x, y, 1.0),
                });
            }
        }
        out
    }
}

/// Writes `s` (its first `n` letters) in white with a dark shadow; '\n' starts a line.
fn text(img: &mut Image, font: &fontdue::Font, s: &str, x: f32, y: f32, size: f32, n: usize) {
    let ascent = font
        .horizontal_line_metrics(size)
        .map_or(size * 0.88, |m| m.ascent);
    let step = size + 6.0;
    let (mut pen, mut line) = (x, y);
    for c in s.chars().take(n) {
        if c == '\n' {
            pen = x;
            line += step;
            continue;
        }
        let (m, bmp) = font.rasterize(c, size);
        let gx = pen as i32 + m.xmin;
        let gy = (line + ascent - m.height as f32 - m.ymin as f32).round() as i32;
        for (ox, oy, rgb) in [(2, 2, [0u8, 0, 0]), (0, 0, [255, 255, 255])] {
            for by in 0..m.height {
                for bx in 0..m.width {
                    let a = bmp[by * m.width + bx] as u32;
                    if a == 0 {
                        continue;
                    }
                    let (px, py) = (gx + bx as i32 + ox, gy + by as i32 + oy);
                    if px < 0 || py < 0 || px >= img.w as i32 || py >= img.h as i32 {
                        continue;
                    }
                    let i = ((py as u32 * img.w + px as u32) * 4) as usize;
                    let d = &mut img.px[i..i + 4];
                    for k in 0..3 {
                        d[k] = ((rgb[k] as u32 * a + d[k] as u32 * (255 - a)) / 255) as u8;
                    }
                    d[3] = (a + d[3] as u32 * (255 - a) / 255) as u8;
                }
            }
        }
        pen += m.advance_width.max(size * 0.5);
    }
}
