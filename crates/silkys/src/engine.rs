//! Runs the `.mes` scripts a frame at a time from `START.MES` and carries out their system
//! calls (numbers as the exe's `CFunc*` classes: 1 Select, 2 MesWnd, 3 Shake, 10 Font,
//! 11 Value, 12 Mouse, 13 Anime, 14 Flag, 15 Music, 16 Effect, 17 HEffect, 18 Voice, 19 Sprite,
//! 20 Screen, 21 Script, 22 BackLog, 23 Movie, 24 OneMes, 25 XML (settings), 26 UI, 27 Save,
//! 28 Wait, 29 the game's own calls).
//!
//! Pictures live in numbered sprites, surfaces the scripts copy between (`19/3`, a plain copy
//! of all four channels or of the colour only); sprite 0 is the screen and the others are
//! shown above it by their layer when visible. Characters (`29/13`) are pictures of their own,
//! scaled and placed by the slot (centre, left, right). The title menu (`29/18`), the message
//! window, the choices and the save slots are drawn here from the game's pictures and areas
//! (`TITLE_T.AREA`, `MES.AREA`, and a [`Skin`] per window picture: `MESSAGE_P.AKB`,
//! `MESWAKU.AKB`).

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::akb;
use crate::arc::Archives;
use crate::mes::{Mes, Op};
use crate::vm::{Stop, Val, Vm};

pub const W: u32 = 1280;
pub const H: u32 = 720;
const SLOTS: usize = 12;
/// What a window button does.
#[derive(Clone, Copy, PartialEq)]
enum Act {
    Save,
    Load,
    QSave,
    QLoad,
    Skip,
    Next,
}

/// Each game draws its message window in its own way: the picture, the part of it under the
/// text (its alpha, in the colour of the settings), and where each `MES.AREA` button's
/// pictures are (the hovered one `hover` lower).
struct Skin {
    pic: &'static str,
    /// the window's top on the screen
    y: i32,
    /// the window's background in the picture, and its top under the window's
    bg: [i32; 4],
    bg_y: i32,
    rgb: [u8; 3],
    /// buttons: area id, picture rect, hover offset, action
    buttons: &'static [(i32, [i32; 4], i32, Act)],
    name: (i32, i32),
    text: (i32, i32),
    size: i32,
    line: i32,
    wrap: usize,
}

const fn b(
    id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    hover: i32,
    act: Act,
) -> (i32, [i32; 4], i32, Act) {
    (id, [x, y, x + w, y + h], hover, act)
}

/// ママのおっぱい (MESSAGE_P.AKB)
const SKIN_P: Skin = Skin {
    pic: "MESSAGE_P.AKB",
    y: 500,
    bg: [0, 12, 1280, 236],
    bg_y: 12,
    rgb: [0, 0, 0],
    buttons: &[
        b(0, 3 * 86, 728, 86, 30, 30, Act::Next),
        b(1, 86, 728, 86, 30, 30, Act::Skip),
        b(2, 7 * 86, 728, 86, 30, 30, Act::Next),
        b(3, 0, 728, 86, 30, 30, Act::Next),
        b(4, 4 * 86, 728, 86, 30, 30, Act::Next),
        b(5, 5 * 86, 728, 86, 30, 30, Act::Next),
        b(6, 2 * 86, 728, 86, 30, 30, Act::Next),
        b(7, 6 * 86, 728, 86, 30, 30, Act::Next),
        b(8, 680, 500, 89, 16, 16, Act::Save),
        b(9, 680 + 89, 500, 89, 16, 16, Act::QSave),
        b(10, 680 + 2 * 89, 500, 89, 16, 16, Act::Load),
        b(11, 680 + 3 * 89, 500, 89, 16, 16, Act::QLoad),
    ],
    name: (232, 30),
    text: (232, 72),
    size: 28,
    line: 40,
    wrap: 29,
};

/// なないろリンカネーション (MESWAKU.AKB)
const SKIN_WAKU: Skin = Skin {
    pic: "MESWAKU.AKB",
    y: 521,
    bg: [0, 0, 1280, 199],
    bg_y: 0,
    rgb: [24, 56, 104],
    buttons: &[
        b(0, 0, 434, 77, 22, 44, Act::Save),
        b(1, 77, 434, 77, 22, 44, Act::Load),
        b(2, 2 * 77, 434, 77, 22, 44, Act::QSave),
        b(3, 3 * 77, 434, 77, 22, 44, Act::QLoad),
        b(4, 4 * 77, 434, 77, 22, 44, Act::Next),
        b(5, 512 + 3 * 45, 434, 45, 44, 88, Act::Next),
        b(6, 1002, 200, 45, 44, 88, Act::Next),
        b(7, 1002 + 45, 200, 45, 44, 88, Act::Skip),
        b(8, 1002 + 2 * 45, 200, 45, 44, 88, Act::Next),
        b(9, 1002 + 3 * 45, 200, 45, 44, 88, Act::Skip),
        b(10, 512, 434, 45, 44, 88, Act::Next),
        b(11, 512 + 45, 434, 45, 44, 88, Act::Next),
        b(12, 512 + 2 * 45, 434, 45, 44, 88, Act::Next),
    ],
    name: (262, 58),
    text: (262, 90),
    size: 26,
    line: 36,
    wrap: 30,
};

/// a game whose window picture is unknown: text on a dark band
const SKIN_NONE: Skin = Skin {
    pic: "",
    y: 500,
    bg: [0, 0, 0, 0],
    bg_y: 0,
    rgb: [0, 0, 0],
    buttons: &[],
    name: (160, 30),
    text: (160, 72),
    size: 28,
    line: 40,
    wrap: 34,
};
/// how many values of g4 a scene record keeps
const RECORD: usize = 8;

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

#[derive(Clone)]
struct Spr {
    img: Rc<Image>,
    /// the layer it is shown on (-1: never shown)
    z: i32,
    visible: bool,
    alpha: f32,
    /// a fade of the alpha: from, to, frames, frames done
    fade: Option<(f32, f32, u32, u32)>,
    /// the fade set up by 19/21 and 19/17: from, to (0..255), frames
    next_fade: (i32, i32, i32),
    /// the picture last loaded into it
    name: String,
}

#[derive(Clone)]
struct Chara {
    slot: i32,
    body: String,
    face: Option<String>,
    /// percent
    scale: i32,
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Wait {
    None,
    Frames(u32),
    /// a message: until a click
    Click,
    /// a timed wait that a click ends (26/0)
    Pause(u32),
    /// a sprite fade running
    Fade(i32),
    /// the screen's colour fading
    Screen,
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
    arcs: Archives,
    vm: Vm,
    scripts: HashMap<String, Rc<Mes>>,
    pics: HashMap<String, Rc<Image>>,
    sprites: BTreeMap<i32, Spr>,
    chars: Vec<Chara>,
    chars_shown: Vec<Chara>,
    text: Vec<u8>,
    name: Option<String>,
    window: bool,
    wait: Wait,
    view: View,
    /// the screen colour over everything: rgb, alpha now, alpha wanted, per frame
    screen: ([u8; 3], f32, f32, f32),
    /// the title's buttons: picture rect (in sprite 5) and screen rect
    title: Vec<([i32; 4], [i32; 4])>,
    title_logo: Option<([i32; 4], [i32; 2])>,
    choices: Vec<(i32, String)>,
    pointer: (i32, i32),
    sounds: Vec<SoundEvent>,
    bgm: Option<String>,
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<(String, i32, [u8; 3]), Rc<Image>>,
    skip: bool,
    /// the title's picture scrolling through sprite 2 onto the screen: x, y, to x, to y, step
    scroll: Option<(i32, i32, i32, i32, i32)>,
    /// the results the system call being handled pushed
    rets: usize,
    /// what the scene holds, by kind and number (27/4): the first values of g4 when recorded;
    /// a load's restore (lib 45) reads them back with 27/2
    records: BTreeMap<(i32, i32), Vec<Val>>,
    /// a load waiting for the script's `27/10`
    restore_msg: Option<i32>,
    skin: &'static Skin,
    /// the window's background in the settings' colour
    win_bg: Option<Rc<Image>>,
    pub trace: bool,
    pub frames: u64,
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

fn place(x: f32, y: f32, alpha: f32) -> Place {
    Place {
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
        clip: [0.0, 0.0, W as f32, H as f32],
    }
}

fn str_of(v: &Val) -> String {
    nexas::sjis::decode(&v.bytes())
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arcs = Archives::open(files.as_mut());
        let mut vm = Vm::new();
        vm.trace = std::env::var("SILKYS_TRACE").is_ok();
        let mut e = Engine {
            backlog: Default::default(),
            files,
            arcs,
            vm,
            scripts: HashMap::new(),
            pics: HashMap::new(),
            sprites: BTreeMap::new(),
            chars: Vec::new(),
            chars_shown: Vec::new(),
            text: Vec::new(),
            name: None,
            window: false,
            wait: Wait::None,
            view: View::Game,
            screen: ([0, 0, 0], 0.0, 0.0, 0.0),
            title: Vec::new(),
            title_logo: None,
            choices: Vec::new(),
            pointer: (0, 0),
            sounds: Vec::new(),
            bgm: None,
            font: None,
            text_cache: HashMap::new(),
            skip: false,
            scroll: None,
            rets: 0,
            records: BTreeMap::new(),
            restore_msg: None,
            skin: &SKIN_NONE,
            win_bg: None,
            trace: std::env::var("SILKYS_LOG").is_ok(),
            frames: 0,
        };
        e.skin = [&SKIN_P, &SKIN_WAKU]
            .into_iter()
            .find(|k| e.arcs.entry(k.pic).is_some())
            .unwrap_or(&SKIN_NONE);
        let start = e.script("start.mes").ok_or("START.MES not found")?;
        e.vm.start("start.mes", start);
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

    fn script(&mut self, name: &str) -> Option<Rc<Mes>> {
        let key = name.to_lowercase();
        if let Some(m) = self.scripts.get(&key) {
            return Some(m.clone());
        }
        let b = self.arcs.read(self.files.as_mut(), name)?;
        let m = Rc::new(Mes::parse(&b).ok()?);
        self.scripts.insert(key, m.clone());
        Some(m)
    }

    fn pic(&mut self, name: &str) -> Option<Rc<Image>> {
        let key = name.to_lowercase();
        if let Some(p) = self.pics.get(&key) {
            return Some(p.clone());
        }
        let b = self.arcs.read(self.files.as_mut(), name)?;
        let base = match akb::header(&b) {
            Ok((a, ..)) => a.base.and_then(|n| self.pic(&n)),
            Err(_) => None,
        };
        let img = match akb::decode(&b, base.as_deref()) {
            Ok(i) => Rc::new(i),
            Err(e) => {
                self.log(&format!("{name}: {e}"));
                return None;
            }
        };
        if self.pics.len() > 24 {
            self.pics.clear();
        }
        self.pics.insert(key, img.clone());
        Some(img)
    }

    fn area(&mut self, name: &str) -> Vec<(i32, [i32; 4])> {
        let Some(b) = self.arcs.read(self.files.as_mut(), name) else {
            return Vec::new();
        };
        let i = |k: usize| {
            b.get(k..k + 4)
                .map_or(0, |s| i32::from_le_bytes(s.try_into().unwrap()))
        };
        (0..i(0).max(0) as usize)
            .map(|n| {
                let o = 4 + n * 20;
                (i(o), [i(o + 4), i(o + 8), i(o + 12), i(o + 16)])
            })
            .collect()
    }

    // ---- the frame ----

    pub fn ended(&self) -> bool {
        self.wait == Wait::Ended
    }

    pub fn frame(&mut self) -> Result<(), String> {
        self.log_page();
        self.frames += 1;
        self.step_fades();
        match self.wait {
            Wait::Frames(n) | Wait::Pause(n) => {
                let n = if self.skip { 0 } else { n.saturating_sub(1) };
                self.wait = if n == 0 {
                    Wait::None
                } else if matches!(self.wait, Wait::Pause(_)) {
                    Wait::Pause(n)
                } else {
                    Wait::Frames(n)
                };
            }
            Wait::Fade(id) => {
                if self.sprites.get(&id).is_none_or(|s| s.fade.is_none()) {
                    self.wait = Wait::None;
                }
            }
            Wait::Screen => {
                if self.screen.3 == 0.0 {
                    self.wait = Wait::None;
                }
            }
            Wait::Click if self.skip && self.view == View::Game => self.advance(),
            _ => {}
        }
        if self.wait == Wait::None && self.view == View::Game {
            self.run()?;
        }
        Ok(())
    }

    fn step_fades(&mut self) {
        let skip = self.skip;
        for s in self.sprites.values_mut() {
            if let Some((a, b, n, done)) = s.fade {
                let done = if skip { n } else { done + 1 };
                s.alpha = a + (b - a) * (done as f32 / n.max(1) as f32);
                s.fade = (done < n).then_some((a, b, n, done));
            }
        }
        if let Some((x, y, tx, ty, step)) = self.scroll {
            let to = |a: i32, b: i32| {
                if skip {
                    b
                } else if a < b {
                    (a + step).min(b)
                } else {
                    (a - step).max(b)
                }
            };
            let (x, y) = (to(x, tx), to(y, ty));
            self.blit(0, 0, 0, W as i32, H as i32, 2, x, y, false);
            self.scroll = ((x, y) != (tx, ty)).then_some((x, y, tx, ty, step));
        }
        let (_, now, want, speed) = self.screen;
        if speed > 0.0 {
            let now = if skip {
                want
            } else if now < want {
                (now + speed).min(want)
            } else {
                (now - speed).max(want)
            };
            self.screen.1 = now;
            if now == want {
                self.screen.3 = 0.0;
            }
        }
    }

    fn run(&mut self) -> Result<(), String> {
        let mut n = 0;
        while self.wait == Wait::None && self.view == View::Game {
            n += 1;
            if n > 20000 {
                break;
            }
            match self.vm.run()? {
                Stop::Text(s) => self.text.extend(s),
                Stop::Syscall(id) => {
                    // a call takes everything its statement pushed, however many of those
                    // arguments its handler reads, and leaves only its results
                    let base = self.vm.mark.min(self.vm.stack.len());
                    self.rets = 0;
                    self.syscall(id)?;
                    let keep = self.rets.min(self.vm.stack.len());
                    let results = self.vm.stack.split_off(self.vm.stack.len() - keep);
                    self.vm.stack.truncate(base.min(self.vm.stack.len()));
                    self.vm.stack.extend(results);
                    self.vm.mark = self.vm.stack.len();
                }
                Stop::End => {
                    self.log("the script ended");
                    self.wait = Wait::Ended;
                }
            }
        }
        Ok(())
    }

    // ---- system calls ----

    fn pi(&mut self) -> i32 {
        self.vm.pop_int()
    }

    fn ps(&mut self) -> String {
        let v = self.vm.pop();
        str_of(&v)
    }

    fn ret(&mut self, v: i32) {
        self.rets += 1;
        self.vm.push(Val::Int(v));
    }

    fn syscall(&mut self, id: i32) -> Result<(), String> {
        let sub = self.pi();
        if self.trace {
            let top: Vec<String> = self
                .vm
                .stack
                .iter()
                .rev()
                .take(6)
                .map(|v| match v {
                    Val::Int(i) => i.to_string(),
                    Val::Str(s) => format!("{:?}", nexas::sjis::decode(s)),
                })
                .collect();
            let f = self
                .vm
                .frames
                .last()
                .map_or(String::new(), |f| format!("{}:{:x}", f.name, f.pc));
            self.log(&format!("sys {id}/{sub} {top:?} @{f}"));
        }
        match id {
            1 => self.select(sub),
            2 => self.mes_wnd(sub),
            3 => {
                // shake: a duration and strength
                if sub == 1 {
                    self.pi();
                    self.pi();
                    self.pi();
                }
            }
            10 => {
                if sub == 0 {
                    self.pi();
                    self.pi();
                }
            }
            13 | 23 | 12 | 11 => {}
            14 => self.flag(sub),
            15..=18 => self.sound_call(id, sub),
            19 => self.sprite(sub),
            20 => self.screen_call(sub),
            21 => self.script_call(sub)?,
            22 => {
                if sub == 0 {
                    self.pi();
                }
                if sub == 2 {
                    // a new message
                    self.text.clear();
                    self.name = None;
                }
            }
            24 => {
                if sub == 0 {
                    self.pi();
                }
            }
            25 => {
                // settings: 0 reads one, 1 writes one
                let _key = self.ps();
                let v = self.pi();
                if sub == 0 {
                    self.ret(0);
                } else {
                    let _ = v;
                }
            }
            26 => self.ui(sub),
            27 => self.save_call(sub),
            29 => self.game(sub),
            _ => self.log(&format!("unknown system call {id}/{sub}")),
        }
        Ok(())
    }

    fn select(&mut self, sub: i32) {
        match sub {
            0 => {
                self.pi();
                // the choices follow: Int(n) Choice(next) text... 0x00, each
                self.choices.clear();
                let Some(f) = self.vm.frames.last() else {
                    return;
                };
                let code = f.code.clone();
                let mut pc = f.pc;
                while let Ok((Op::Int(n), p)) = code.op(pc) {
                    let Ok((Op::Choice(next), mut q)) = code.op(p) else {
                        break;
                    };
                    let mut label = Vec::new();
                    while let Ok((Op::Text(s), r)) = code.op(q) {
                        label.extend(s);
                        q = r;
                    }
                    self.choices.push((n, nexas::sjis::decode(&label)));
                    pc = next as usize;
                }
                self.vm.frames.last_mut().unwrap().pc = pc;
            }
            2 => {
                // waits for the pick, which goes to g3[18] as its number + 1
                if self.choices.is_empty() {
                    self.vm.store(0x0e, 18, Val::Int(-1));
                } else {
                    self.wait = Wait::Choice;
                }
            }
            _ => {}
        }
    }

    fn mes_wnd(&mut self, sub: i32) {
        match sub {
            2 => self.window = self.pi() == 1,
            3 => {
                self.pi();
                self.pi();
                // the answer comes when the click does
                self.ret(0);
                self.window = true;
                self.wait = Wait::Click;
            }
            _ => {}
        }
    }

    fn flag(&mut self, sub: i32) {
        match sub {
            0 => {
                for _ in 0..4 {
                    self.pi();
                }
            }
            2 | 11 => {
                self.pi();
            }
            3 | 6 => {
                self.pi();
                self.pi();
                self.pi();
            }
            _ => {}
        }
    }

    fn sound(&mut self, name: &str) -> Option<Vec<u8>> {
        self.arcs.read(self.files.as_mut(), name)
    }

    fn push_sound(
        &mut self,
        ch: &str,
        data: Option<Vec<u8>>,
        volume: f32,
        repeat: bool,
        fade: f32,
    ) {
        self.sounds.push(SoundEvent {
            channel: ch.into(),
            data,
            volume,
            repeat,
            fade_ms: fade,
        });
    }

    fn sound_call(&mut self, id: i32, sub: i32) {
        let kind = match id {
            15 => "bgm",
            16 => "se",
            17 => "hse",
            _ => "voice",
        };
        match sub {
            0 => {
                self.pi();
            }
            1 | 5 => {
                let name = self.ps();
                let ch = self.pi();
                if self.skip && kind != "bgm" {
                    return;
                }
                if kind == "bgm" {
                    if self.bgm.as_deref() == Some(name.as_str()) {
                        return;
                    }
                    self.bgm = Some(name.clone());
                }
                let d = self.sound(&name);
                if d.is_none() {
                    self.log(&format!("no sound {name}"));
                }
                let repeat = kind == "bgm" || kind == "hse";
                let volume = if kind == "bgm" { 0.6 } else { 0.9 };
                self.push_sound(&format!("{kind}{ch}"), d, volume, repeat, 0.0);
            }
            2 => {
                let ch = self.pi();
                if kind == "bgm" {
                    self.bgm = None;
                }
                self.push_sound(&format!("{kind}{ch}"), None, 0.0, false, 0.0);
            }
            3 => {
                let ms = self.pi();
                let ch = self.pi();
                if kind == "bgm" {
                    self.bgm = None;
                }
                self.push_sound(
                    &format!("{kind}{ch}"),
                    None,
                    0.0,
                    false,
                    ms.max(0) as f32 * 16.0,
                );
            }
            4 => {
                self.pi();
                self.pi();
            }
            7 => {
                self.pi();
            }
            _ => {}
        }
    }

    fn spr(&mut self, id: i32) -> &mut Spr {
        self.sprites.entry(id).or_insert_with(|| Spr {
            img: Rc::new(Image::new(W, H)),
            z: -1,
            visible: false,
            alpha: 1.0,
            fade: None,
            next_fade: (0, 255, 300),
            name: String::new(),
        })
    }

    fn sprite(&mut self, sub: i32) {
        match sub {
            0 => {
                let id = self.pi();
                let w = self.pi().clamp(1, 4096) as u32;
                let h = self.pi().clamp(1, 4096) as u32;
                let z = self.pi();
                let _kind = self.pi();
                let mut img = Image::new(w, h);
                if id == 0 {
                    img.fill([0, 0, 0, 255]);
                }
                self.sprites.insert(
                    id,
                    Spr {
                        img: Rc::new(img),
                        z: if id == 0 { 0 } else { z },
                        visible: id == 0,
                        alpha: 1.0,
                        fade: None,
                        next_fade: (0, 255, 300),
                        name: String::new(),
                    },
                );
            }
            1 => {
                let name = self.ps();
                let id = self.pi();
                match self.pic(&name) {
                    Some(p) => {
                        let s = self.spr(id);
                        let mut img = (*p).clone();
                        img.renew();
                        s.img = Rc::new(img);
                        s.name = name;
                    }
                    None => self.log(&format!("no picture {name}")),
                }
            }
            2 => {
                let id = self.pi();
                let s = self.spr(id);
                let img = Rc::make_mut(&mut s.img);
                img.fill([0, 0, 0, 255]);
                img.renew();
                s.visible = true;
            }
            3 => {
                let dx = self.pi();
                let dy = self.pi();
                let w = self.pi();
                let h = self.pi();
                let to = self.pi();
                let sx = self.pi();
                let sy = self.pi();
                let from = self.pi();
                let mode = self.pi();
                self.blit(to, dx, dy, w, h, from, sx, sy, mode == 1);
            }
            6 => {
                let x = self.pi();
                let y = self.pi();
                let w = self.pi();
                let h = self.pi();
                let id = self.pi();
                let color = self.pi() as u32;
                let mode = self.pi();
                self.fill(id, x, y, w, h, color, mode == 1);
            }
            9 => {
                for _ in 0..6 {
                    self.pi();
                }
            }
            10 => {
                let id = self.pi();
                self.spr(id).visible = true;
            }
            11 => {
                let id = self.pi();
                let s = self.spr(id);
                s.visible = false;
                s.fade = None;
            }
            12 => {
                let id = self.pi();
                let v = self.sprites.get(&id).is_some_and(|s| s.visible) as i32;
                self.ret(v);
            }
            16 => {
                let id = self.pi();
                self.spr(id).next_fade = (0, 255, 300);
            }
            17 => {
                let id = self.pi();
                let ms = self.pi();
                self.spr(id).next_fade.2 = ms;
            }
            18 => {
                self.pi();
                self.pi();
            }
            21 => {
                let id = self.pi();
                let a = self.pi();
                let b = self.pi();
                let s = self.spr(id);
                s.next_fade.0 = a;
                s.next_fade.1 = b;
            }
            24 => {
                for _ in 0..4 {
                    self.pi();
                }
            }
            25 => self.ret(1),
            26 => {
                // the effect takes the sprite: kept as the effect number's sprite
                let _effect = self.pi();
                let id = self.pi();
                self.vm.g5_effect = id;
            }
            27 => {
                self.pi();
                self.pi();
            }
            28 => {
                self.pi();
                let id = self.vm.g5_effect;
                let skip = self.skip;
                let s = self.spr(id);
                let (a, b, ms) = s.next_fade;
                let n = if skip { 1 } else { ms.max(1) as u32 };
                s.visible = true;
                s.alpha = a as f32 / 255.0;
                s.fade = Some((a as f32 / 255.0, b as f32 / 255.0, n, 0));
                self.wait = Wait::Fade(id);
            }
            _ => self.log(&format!("sprite {sub} skipped")),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn blit(
        &mut self,
        to: i32,
        dx: i32,
        dy: i32,
        w: i32,
        h: i32,
        from: i32,
        sx: i32,
        sy: i32,
        all: bool,
    ) {
        let Some(src) = self.sprites.get(&from).map(|s| s.img.clone()) else {
            return;
        };
        let dst = self.spr(to);
        let d = Rc::make_mut(&mut dst.img);
        for y in 0..h.max(0) {
            let (ty, fy) = (dy + y, sy + y);
            if ty < 0 || fy < 0 || ty >= d.h as i32 || fy >= src.h as i32 {
                continue;
            }
            for x in 0..w.max(0) {
                let (tx, fx) = (dx + x, sx + x);
                if tx < 0 || fx < 0 || tx >= d.w as i32 || fx >= src.w as i32 {
                    continue;
                }
                let t = ((ty as u32 * d.w + tx as u32) * 4) as usize;
                let f = ((fy as u32 * src.w + fx as u32) * 4) as usize;
                let n = if all { 4 } else { 3 };
                d.px[t..t + n].copy_from_slice(&src.px[f..f + n]);
            }
        }
        d.renew();
    }

    #[allow(clippy::too_many_arguments)]
    fn fill(&mut self, id: i32, x: i32, y: i32, w: i32, h: i32, color: u32, all: bool) {
        let s = self.spr(id);
        let d = Rc::make_mut(&mut s.img);
        let c = [
            (color >> 16) as u8,
            (color >> 8) as u8,
            color as u8,
            (color >> 24) as u8,
        ];
        for ty in y.max(0)..(y + h).min(d.h as i32) {
            for tx in x.max(0)..(x + w).min(d.w as i32) {
                let t = ((ty as u32 * d.w + tx as u32) * 4) as usize;
                let n = if all { 4 } else { 3 };
                d.px[t..t + n].copy_from_slice(&c[..n]);
            }
        }
        d.renew();
    }

    fn screen_call(&mut self, sub: i32) {
        match sub {
            0 => {
                // to a colour over `ms` frames (the times of the fades are in frames)
                let color = self.pi() as u32;
                let ms = self.pi();
                self.fade_screen(Some(color), 1.0, ms);
            }
            1 => {
                let ms = self.pi();
                self.fade_screen(None, 0.0, ms);
            }
            2 => {
                self.pi();
                self.pi();
                self.pi();
            }
            3 => {
                self.pi();
                self.pi();
            }
            4 => {
                let v = (self.screen.1 > 0.0 || self.screen.2 > 0.0) as i32;
                self.ret(v);
            }
            _ => {}
        }
    }

    fn fade_screen(&mut self, color: Option<u32>, to: f32, ms: i32) {
        if let Some(c) = color {
            self.screen.0 = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
        }
        let frames = ms.max(1) as f32;
        self.screen.2 = to;
        if self.skip || ms <= 0 {
            self.screen.1 = to;
            self.screen.3 = 0.0;
            return;
        }
        self.screen.3 = 1.0 / frames;
        self.wait = Wait::Screen;
    }

    fn script_call(&mut self, sub: i32) -> Result<(), String> {
        let name = self.ps();
        match sub {
            0 | 1 => {
                let Some(code) = self.script(&name) else {
                    self.log(&format!("no script {name}"));
                    return Ok(());
                };
                if sub == 0 {
                    // a jump: the script replaces the one running (and any routine in it)
                    while let Some(f) = self.vm.frames.pop() {
                        if !f.name.starts_with("lib") {
                            break;
                        }
                    }
                }
                self.vm.stack.clear();
                let base = 0;
                self.vm.frames.push(crate::vm::Frame {
                    code,
                    pc: 0,
                    name: name.to_lowercase(),
                    args: Vec::new(),
                    base,
                });
                if sub == 0 && self.vm.frames.len() == 1 {
                    self.vm.last_msg = -1;
                }
                self.log(&format!("script {name}"));
            }
            2 => {
                let b = self
                    .arcs
                    .read(self.files.as_mut(), &name)
                    .ok_or(format!("no {name}"))?;
                let lib = Mes::parse(&b)?;
                self.vm.load_lib(lib)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn ui(&mut self, sub: i32) {
        match sub {
            0 => {
                // a wait of `ms` that a click ends (0: until a click)
                let ms = self.pi();
                if self.skip {
                    return;
                }
                self.wait = if ms <= 0 {
                    Wait::Click
                } else {
                    Wait::Pause((ms as u32 * 60 / 1000).max(1))
                };
            }
            1 => {
                // whether a key is down
                self.pi();
                self.ret(0);
            }
            4 => {
                self.pi();
            }
            _ => {}
        }
    }

    fn save_call(&mut self, sub: i32) {
        match sub {
            0 | 3 => {
                self.pi();
            }
            1 => {
                self.pi();
            }
            2 => {
                // is (kind, i) recorded? its values back into g4 if so
                self.pi();
                let kind = self.pi();
                let i = self.pi();
                let found = self.records.get(&(kind, i)).cloned();
                if let Some(v) = &found {
                    for (k, x) in v.iter().enumerate() {
                        self.vm.store(0x0f, k as i32, x.clone());
                    }
                }
                self.ret(found.is_some() as i32);
            }
            4 => {
                // records (1) or forgets (0) what g4 holds now as (kind, i)
                self.pi();
                let on = self.pi();
                let kind = self.pi();
                let i = self.pi();
                if on != 0 {
                    let v = self.vm.g4[..RECORD].to_vec();
                    self.records.insert((kind, i), v);
                } else {
                    self.records.remove(&(kind, i));
                }
            }
            5 => {
                self.pi();
                self.pi();
            }
            6 => {
                // forgets every record of a kind
                let kind = self.pi();
                self.records.retain(|k, _| k.0 != kind);
            }
            10 => {
                // after a load the scene is back: on to the saved message
                if let Some(m) = self.restore_msg.take()
                    && let Some(f) = self.vm.frames.first_mut()
                    && let Some(&at) = f.code.marks.get(m.max(0) as usize)
                {
                    self.vm.frames.truncate(1);
                    self.vm.stack.clear();
                    self.vm.frames[0].pc = at as usize;
                }
            }
            _ => {}
        }
    }

    fn game(&mut self, sub: i32) {
        match sub {
            // the game ends
            3 => self.wait = Wait::Ended,
            12 => {
                let a = self.pi();
                if a == 4 || a == 6 {
                    self.pi();
                }
            }
            13 => self.chara(),
            14 => {
                let s = self.pi();
                if matches!(s, 4..=9) {
                    // the background (sprite 2) onto the screen: 8 and 9 from a point in it
                    let (x, y) = if s >= 8 {
                        (self.pi(), self.pi())
                    } else {
                        (0, 0)
                    };
                    if s == 7 {
                        self.pi();
                    }
                    self.blit(0, 0, 0, W as i32, H as i32, 2, x, y, false);
                    return;
                }
                let name = self.ps();
                if s == 14 {
                    // the background picture into sprite 2
                    if let Some(p) = self.pic(&name) {
                        let spr = self.spr(2);
                        let mut img = (*p).clone();
                        img.renew();
                        spr.img = Rc::new(img);
                    }
                }
            }
            15 => {
                let n = self.ps();
                self.name = (!n.is_empty()).then_some(n);
            }
            18 => {
                let _variant = self.pi();
                self.open_title();
            }
            19 => {
                self.pi();
                self.pi();
                self.pi();
            }
            22 => {
                let a = self.pi();
                if a == 3 || a == 1 {
                    self.pi();
                }
            }
            23 => {
                // scrolls the view of sprite 2 on the screen from (x, y) to (tx, ty)
                let x = self.pi();
                let y = self.pi();
                let tx = self.pi();
                let ty = self.pi();
                let step = self.pi().max(1);
                self.pi();
                self.scroll = Some((x, y, tx, ty, step));
            }
            26 => {
                // the choices
                if self.choices.is_empty() {
                    self.vm.store(0x0e, 18, Val::Int(-1));
                } else {
                    self.wait = Wait::Choice;
                }
            }
            1006 | 1008 | 1009 | 2012 | 810 | 2029 | 27 | 29 | 33 | 40 | 41 => {
                let n = match sub {
                    1008 => 1,
                    1009 => 3,
                    2012 => 2,
                    29 | 33 | 40 | 41 => 1,
                    _ => 0,
                };
                for _ in 0..n {
                    self.pi();
                }
                if sub == 810 {
                    let _ = self.pi();
                }
            }
            _ => self.log(&format!("game call {sub} skipped")),
        }
    }

    fn chara(&mut self) {
        let sub = self.pi();
        match sub {
            0 => {
                let slot = self.pi();
                let body = self.ps();
                let scale = self.pi();
                let _z = self.pi();
                let x = (1280 - (scale * 2048) / 100) / 2
                    + match slot {
                        1 => -324,
                        2 => 324,
                        _ => 0,
                    };
                let y = (scale as f32 / 100.0 * -100.0) as i32
                    - ((100 - scale) as f32 / 100.0 * -200.0) as i32;
                self.chars.retain(|c| c.slot != slot);
                self.chars.push(Chara {
                    slot,
                    body,
                    face: None,
                    scale,
                    x,
                    y,
                });
            }
            1 => {
                let slot = self.pi();
                self.pi();
                self.chars.retain(|c| c.slot != slot);
            }
            2 | 14 | 16 => {
                // show the characters as set
                self.chars_shown = self.chars.clone();
            }
            5 => {
                let face = self.ps();
                let slot = self.pi();
                self.pi();
                if let Some(c) = self.chars.iter_mut().find(|c| c.slot == slot) {
                    c.face = Some(face);
                }
                self.chars_shown = self.chars.clone();
            }
            6 | 8 | 9 => {
                self.pi();
                self.pi();
            }
            7 | 10 | 11 => {
                let slot = self.pi();
                if sub == 7 {
                    // every character away
                    let _ = slot;
                    self.chars.clear();
                }
            }
            12 => {
                for _ in 0..3 {
                    self.pi();
                }
            }
            13 | 15 => {
                for _ in 0..4 {
                    self.pi();
                }
            }
            17 => {
                for _ in 0..3 {
                    self.pi();
                }
            }
            _ => {}
        }
    }

    // ---- the title ----

    fn open_title(&mut self) {
        self.title.clear();
        self.title_logo = None;
        let Some((p, name)) = self
            .sprites
            .get(&5)
            .map(|s| (s.img.clone(), s.name.clone()))
        else {
            self.ret(1);
            return;
        };
        let mut rects = self.area("TITLE_T.AREA");
        if rects.is_empty() {
            rects = self.area("TITLE.AREA");
        }
        if name.eq_ignore_ascii_case("trial_Title_p.akb") {
            // ママのおっぱい: three columns of 400x42 under the logo, four states each
            let cells = [(0, 0), (1, 0), (2, 0), (2, 2), (0, 1), (1, 1), (2, 1)];
            for (&(c, g), (_, r)) in cells.iter().zip(&rects) {
                let src = [c * 400, 224 + g * 168, c * 400 + 400, 224 + g * 168 + 42];
                self.title.push((src, *r));
            }
            self.title_logo = Some(([0, 564, 580, 816], [700, 468]));
        } else {
            // the buttons' pictures packed in the sheet in the areas' order: each goes on the
            // first band with room for it, a band as tall as four states of its first button
            let width = self
                .arcs
                .read(self.files.as_mut(), &name)
                .and_then(|b| akb::header(&b).ok().map(|h| h.0.x1))
                .unwrap_or(p.w as i32);
            let mut bands: Vec<(i32, i32, i32)> = Vec::new(); // top, height, used width
            for (_, r) in &rects {
                let (w, h) = (r[2] - r[0], r[3] - r[1]);
                let at = match bands.iter_mut().find(|b| b.2 + w <= width) {
                    Some(b) => {
                        b.2 += w;
                        (b.2 - w, b.0)
                    }
                    None => {
                        let top = bands.last().map_or(0, |b| b.0 + b.1);
                        bands.push((top, 4 * h, w));
                        (0, top)
                    }
                };
                self.title.push(([at.0, at.1, at.0 + w, at.1 + h], *r));
            }
        }
        self.wait = Wait::Title;
    }

    fn title_pick(&mut self, i: usize) {
        self.scroll = None;
        // the second and third buttons load (the last save, or one picked); the others go to
        // the script as their number + 1, which ignores the settings
        match i {
            1 => self.load_latest(),
            2 => self.view = View::Slots { save: false },
            6 if self.title.len() == 7 => self.wait = Wait::Ended,
            _ => {
                self.ret(i as i32 + 1);
                self.title.clear();
                self.wait = Wait::None;
            }
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
        if let Wait::Pause(_) = self.wait {
            self.wait = Wait::None;
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
                Some(i) => {
                    if self.read_slot(i).is_some() {
                        self.load(i);
                    }
                }
                None => self.view = View::Game,
            }
            return;
        }
        match self.wait {
            Wait::Title => {
                if let Some(i) = self.title.iter().position(|(_, r)| inside(*r, x, y)) {
                    self.title_pick(i);
                }
            }
            Wait::Choice => {
                if let Some(i) = self
                    .choice_rects()
                    .into_iter()
                    .position(|r| inside(r, x, y))
                {
                    let v = self.choices[i].0 + 1;
                    self.vm.store(0x0e, 18, Val::Int(v));
                    self.choices.clear();
                    self.wait = Wait::None;
                }
            }
            Wait::Click => {
                if let Some(b) = self.window_button(x, y) {
                    self.window_action(b);
                } else {
                    self.advance();
                }
            }
            _ => self.advance(),
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

    /// 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip (held: toggles)
    pub fn key(&mut self, k: usize) {
        match k {
            4 => {
                if self.wait == Wait::Title && !self.title.is_empty() {
                    self.title_pick(0);
                } else if self.wait == Wait::Choice && !self.choices.is_empty() {
                    let v = self.choices[0].0 + 1;
                    self.vm.store(0x0e, 18, Val::Int(v));
                    self.choices.clear();
                    self.wait = Wait::None;
                } else {
                    self.advance();
                }
            }
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

    // ---- the message window ----

    /// The window's buttons: the picture's rect, the hover offset, the screen rect, the action.
    fn window_buttons(&mut self) -> Vec<([i32; 4], i32, [i32; 4], Act)> {
        let rects = self.area("MES.AREA");
        let y = self.skin.y;
        let mut out = Vec::new();
        for (id, r) in rects {
            if let Some(&(_, src, hover, act)) = self.skin.buttons.iter().find(|k| k.0 == id) {
                out.push((src, hover, [r[0], r[1] + y, r[2], r[3] + y], act));
            }
        }
        out
    }

    fn window_button(&mut self, x: i32, y: i32) -> Option<Act> {
        if !self.window {
            return None;
        }
        self.window_buttons()
            .into_iter()
            .find(|b| inside(b.2, x, y))
            .map(|b| b.3)
    }

    fn window_action(&mut self, act: Act) {
        match act {
            Act::Skip => self.skip = !self.skip,
            Act::Save => self.view = View::Slots { save: true },
            Act::QSave => self.save(0),
            Act::Load => self.view = View::Slots { save: false },
            Act::QLoad => {
                if self.read_slot(0).is_some() {
                    self.load(0);
                }
            }
            Act::Next => self.advance(),
        }
    }

    // ---- drawing ----

    fn text_img(&mut self, s: &str, size: i32, rgb: [u8; 3]) -> Rc<Image> {
        let key = (s.to_string(), size, rgb);
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let img = match &self.font {
            Some(f) => {
                Rc::new(Font::outline(f.clone(), size).render(&nexas::sjis::encode(s), rgb, 2))
            }
            None => Rc::new(Image::new(1, 1)),
        };
        if self.text_cache.len() > 300 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, img.clone());
        img
    }

    fn solid(rgba: [u8; 4], alpha: f32) -> Draw {
        let mut img = Image::new(1, 1);
        img.px.copy_from_slice(&rgba);
        Draw {
            image: Rc::new(img),
            src: [0, 0, 1, 1],
            dst: [0.0, 0.0, W as f32, H as f32],
            place: place(0.0, 0.0, alpha),
        }
    }

    fn sprite_draw(img: &Rc<Image>, src: [i32; 4], x: f32, y: f32, alpha: f32) -> Draw {
        Draw {
            image: img.clone(),
            src,
            dst: [
                x,
                y,
                x + (src[2] - src[0]) as f32,
                y + (src[3] - src[1]) as f32,
            ],
            place: place(0.0, 0.0, alpha),
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
                self.text_img(s, size, rgb)
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
        let (name, text) = (
            self.name.clone().unwrap_or_default(),
            nexas::sjis::decode(&self.text).replace('＠', "♥"),
        );
        self.backlog.push(name.trim(), &text);
    }

    fn draws_inner(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        let mut layers: Vec<(i32, i32)> = self
            .sprites
            .iter()
            .filter(|(id, s)| **id == 0 || (s.visible && s.z >= 0))
            .map(|(id, s)| (if *id == 0 { -1 } else { s.z }, *id))
            .collect();
        layers.sort();
        let chars = self.chars_shown.clone();
        let mut chars_done = false;
        for (z, id) in layers {
            if z >= 12 && !chars_done {
                self.chara_draws(&chars, &mut out);
                chars_done = true;
            }
            let s = &self.sprites[&id];
            let src = [0, 0, s.img.w as i32, s.img.h as i32];
            out.push(Self::sprite_draw(
                &s.img,
                src,
                0.0,
                0.0,
                s.alpha.clamp(0.0, 1.0),
            ));
        }
        if !chars_done {
            self.chara_draws(&chars, &mut out);
        }
        if self.window && self.wait != Wait::Title {
            self.window_draws(&mut out);
        }
        if self.wait == Wait::Choice {
            self.choice_draws(&mut out);
        }
        if self.wait == Wait::Title {
            self.title_draws(&mut out);
        }
        let (c, a, ..) = self.screen;
        if a > 0.0 {
            out.push(Self::solid([c[0], c[1], c[2], 255], a));
        }
        if let View::Slots { save } = self.view {
            self.slot_draws(save, &mut out);
        }
        out
    }

    fn chara_draws(&mut self, chars: &[Chara], out: &mut Vec<Draw>) {
        for c in chars {
            let k = c.scale as f32 / 100.0;
            let mut layers = vec![c.body.clone()];
            layers.extend(c.face.clone());
            for name in layers {
                let Some(p) = self.pic(&name) else { continue };
                let (w, h) = (p.w as f32 * k, p.h as f32 * k);
                out.push(Draw {
                    image: p.clone(),
                    src: [0, 0, p.w as i32, p.h as i32],
                    dst: [c.x as f32, c.y as f32, c.x as f32 + w, c.y as f32 + h],
                    place: place(0.0, 0.0, 1.0),
                });
            }
        }
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let k = self.skin;
        if self.win_bg.is_none()
            && let Some(p) = self.pic(k.pic)
        {
            // the background's alpha in the window's colour
            let [x0, y0, x1, y1] = k.bg;
            let mut img = Image::new((x1 - x0) as u32, (y1 - y0) as u32);
            for y in 0..img.h {
                for x in 0..img.w {
                    let (sx, sy) = (x + x0 as u32, y + y0 as u32);
                    if sx < p.w && sy < p.h {
                        let a = p.px[((sy * p.w + sx) * 4 + 3) as usize];
                        let o = ((y * img.w + x) * 4) as usize;
                        img.px[o..o + 4].copy_from_slice(&[k.rgb[0], k.rgb[1], k.rgb[2], a]);
                    }
                }
            }
            img.renew();
            self.win_bg = Some(Rc::new(img));
        }
        if let Some(bg) = self.win_bg.clone() {
            let src = [0, 0, bg.w as i32, bg.h as i32];
            out.push(Self::sprite_draw(
                &bg,
                src,
                0.0,
                (k.y + k.bg_y) as f32,
                0.85,
            ));
        } else if k.pic.is_empty() {
            let band = Rc::new({
                let mut i = Image::new(1, 1);
                i.fill([0, 0, 0, 160]);
                i
            });
            let mut d = Self::sprite_draw(&band, [0, 0, 1, 1], 0.0, k.y as f32, 1.0);
            d.dst = [0.0, k.y as f32, W as f32, H as f32];
            out.push(d);
        }
        if let Some(p) = self.pic(k.pic) {
            let (px, py) = self.pointer;
            for (src, hover, r, _) in self.window_buttons() {
                let dy = if inside(r, px, py) { hover } else { 0 };
                out.push(Self::sprite_draw(
                    &p,
                    [src[0], src[1] + dy, src[2], src[3] + dy],
                    r[0] as f32,
                    r[1] as f32,
                    1.0,
                ));
            }
        }
        if let Some(n) = self.name.clone() {
            let img = self.text_img(&n, k.size, [255, 230, 240]);
            out.push(Self::sprite_draw(
                &img,
                [0, 0, img.w as i32, img.h as i32],
                k.name.0 as f32,
                (k.y + k.name.1) as f32,
                1.0,
            ));
        }
        // the scripts write a heart as a full-width @
        let text = nexas::sjis::decode(&self.text).replace('＠', "♥");
        let mut y = k.y + k.text.1;
        for line in wrap(&text, k.wrap) {
            let img = self.text_img(&line, k.size, [255, 255, 255]);
            out.push(Self::sprite_draw(
                &img,
                [0, 0, img.w as i32, img.h as i32],
                k.text.0 as f32,
                y as f32,
                1.0,
            ));
            y += k.line;
        }
    }

    fn choice_rects(&self) -> Vec<[i32; 4]> {
        let n = self.choices.len() as i32;
        let top = 300 - n * 35;
        (0..n)
            .map(|i| [340, top + i * 70, 940, top + i * 70 + 56])
            .collect()
    }

    fn choice_draws(&mut self, out: &mut Vec<Draw>) {
        let (px, py) = self.pointer;
        let labels: Vec<String> = self.choices.iter().map(|c| c.1.clone()).collect();
        for (r, label) in self.choice_rects().into_iter().zip(labels) {
            let mut d = Self::solid(
                if inside(r, px, py) {
                    [230, 90, 160, 255]
                } else {
                    [40, 20, 40, 255]
                },
                0.85,
            );
            d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
            out.push(d);
            let img = self.text_img(&label, 30, [255, 255, 255]);
            let x = (r[0] + r[2]) as f32 / 2.0 - img.w as f32 / 2.0;
            out.push(Self::sprite_draw(
                &img,
                [0, 0, img.w as i32, img.h as i32],
                x,
                r[1] as f32 + 10.0,
                1.0,
            ));
        }
    }

    fn title_draws(&mut self, out: &mut Vec<Draw>) {
        let Some(p) = self.sprites.get(&5).map(|s| s.img.clone()) else {
            return;
        };
        if let Some((src, at)) = self.title_logo {
            out.push(Self::sprite_draw(&p, src, at[0] as f32, at[1] as f32, 1.0));
        }
        let (px, py) = self.pointer;
        for (src, r) in &self.title {
            let dy = if inside(*r, px, py) {
                src[3] - src[1]
            } else {
                0
            };
            out.push(Self::sprite_draw(
                &p,
                [src[0], src[1] + dy, src[2], src[3] + dy],
                r[0] as f32,
                r[1] as f32,
                1.0,
            ));
        }
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

    // ---- saves ----

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        (0..SLOTS as i32)
            .map(|i| {
                let (c, r) = (i % 2, i / 2);
                [
                    140 + c * 520,
                    90 + r * 95,
                    140 + c * 520 + 480,
                    90 + r * 95 + 80,
                ]
            })
            .collect()
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        out.push(Self::solid([10, 0, 20, 255], 0.85));
        let title = if save { "セーブ" } else { "ロード" };
        let img = self.text_img(title, 34, [255, 200, 230]);
        out.push(Self::sprite_draw(
            &img,
            [0, 0, img.w as i32, img.h as i32],
            140.0,
            30.0,
            1.0,
        ));
        let (px, py) = self.pointer;
        for (i, r) in self.slot_rects().into_iter().enumerate() {
            let mut d = Self::solid(
                if inside(r, px, py) {
                    [200, 80, 150, 255]
                } else {
                    [60, 30, 60, 255]
                },
                0.9,
            );
            d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
            out.push(d);
            let label = match self.read_slot(i) {
                Some(s) => format!("{:02}  {}", i + 1, s.label),
                None => format!("{:02}  ----", i + 1),
            };
            let img = self.text_img(&label, 24, [255, 255, 255]);
            out.push(Self::sprite_draw(
                &img,
                [0, 0, img.w as i32, img.h as i32],
                r[0] as f32 + 16.0,
                r[1] as f32 + 24.0,
                1.0,
            ));
        }
    }

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&format!("slot{i:02}.sav"))?;
        Slot::parse(&b)
    }

    fn save(&mut self, i: usize) {
        let Some(f) = self.vm.frames.first() else {
            return;
        };
        let label = {
            let t = nexas::sjis::decode(&self.text);
            let t: String = t.chars().filter(|c| !c.is_control()).take(18).collect();
            format!("{} {}", f.name, t)
        };
        let s = Slot {
            label,
            script: f.name.clone(),
            msg: self.vm.last_msg,
            g1: self.vm.g1.clone(),
            g2: self.vm.g2.clone(),
            g3: self.vm.g3.clone(),
            g4: self.vm.g4.clone(),
            g5: self.vm.g5.clone(),
            records: self.records.clone(),
        };
        let b = s.write();
        self.files.write_save(&format!("slot{i:02}.sav"), &b);
    }

    fn load_latest(&mut self) {
        if self.read_slot(0).is_some() {
            self.load(0);
        }
    }

    fn load(&mut self, i: usize) {
        let Some(s) = self.read_slot(i) else {
            return;
        };
        let Some(code) = self.script(&s.script) else {
            return;
        };
        self.vm.g1 = s.g1;
        self.vm.g2 = s.g2;
        self.vm.g3 = s.g3;
        self.vm.g4 = s.g4;
        self.vm.g5 = s.g5;
        self.records = s.records;
        // the script's own restore: bit 0x1000 of system variable 55 runs it (lib 45), then
        // 27/10 goes on to the message
        let v = self.vm.g3i(55) | 0x1000;
        self.vm.store(0x0e, 55, Val::Int(v));
        self.restore_msg = Some(s.msg);
        self.vm.start(&s.script, code);
        self.vm.last_msg = s.msg;
        self.text.clear();
        self.name = None;
        self.chars.clear();
        self.chars_shown.clear();
        self.choices.clear();
        self.title.clear();
        self.window = false;
        self.wait = Wait::None;
        self.view = View::Game;
        self.screen = ([0, 0, 0], 0.0, 0.0, 0.0);
        for (id, s) in self.sprites.iter_mut() {
            if *id != 0 {
                s.visible = false;
                s.fade = None;
            }
        }
    }
}

/// Splits text into lines of at most `n` characters, at its own line breaks too.
fn wrap(s: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in s.split('\n') {
        let chars: Vec<char> = para.chars().collect();
        if chars.is_empty() {
            continue;
        }
        let mut i = 0;
        while i < chars.len() {
            let mut end = (i + n).min(chars.len());
            // keep closing marks with the line before
            while end < chars.len() && "」』）。、！？…ー〜".contains(chars[end]) && end - i < n + 2
            {
                end += 1;
            }
            out.push(chars[i..end].iter().collect());
            i = end;
        }
    }
    out
}

struct Slot {
    label: String,
    script: String,
    msg: i32,
    g1: Vec<u8>,
    g2: Vec<i16>,
    g3: Vec<Val>,
    g4: Vec<Val>,
    g5: Vec<u8>,
    records: BTreeMap<(i32, i32), Vec<Val>>,
}

fn put_vals(d: &mut Vec<u8>, g: &[Val]) {
    for v in g {
        match v {
            Val::Int(i) => {
                d.push(0);
                d.extend(i.to_le_bytes());
            }
            Val::Str(s) => {
                d.push(1);
                d.extend((s.len() as u32).to_le_bytes());
                d.extend(s);
            }
        }
    }
}

fn get_vals(d: &[u8]) -> Option<Vec<Val>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < d.len() {
        let k = d[i];
        let n = u32::from_le_bytes(d.get(i + 1..i + 5)?.try_into().ok()?);
        if k == 0 {
            out.push(Val::Int(n as i32));
            i += 5;
        } else {
            let n = n as usize;
            out.push(Val::Str(d.get(i + 5..i + 5 + n)?.to_vec()));
            i += 5 + n;
        }
    }
    Some(out)
}

impl Slot {
    fn write(&self) -> Vec<u8> {
        let mut b = b"SILKYSAV1\n".to_vec();
        let put = |b: &mut Vec<u8>, d: &[u8]| {
            b.extend((d.len() as u32).to_le_bytes());
            b.extend(d);
        };
        put(&mut b, self.label.as_bytes());
        put(&mut b, self.script.as_bytes());
        put(&mut b, &self.msg.to_le_bytes());
        put(&mut b, &self.g1);
        let g2: Vec<u8> = self.g2.iter().flat_map(|v| v.to_le_bytes()).collect();
        put(&mut b, &g2);
        for g in [&self.g3, &self.g4] {
            let mut d = Vec::new();
            put_vals(&mut d, g);
            put(&mut b, &d);
        }
        put(&mut b, &self.g5);
        // the records: kind, number, then their values
        let mut d = Vec::new();
        for ((kind, i), v) in &self.records {
            put_vals(&mut d, &[Val::Int(*kind), Val::Int(*i)]);
            put_vals(&mut d, v);
        }
        put(&mut b, &d);
        b
    }

    fn parse(b: &[u8]) -> Option<Slot> {
        let mut p = b.strip_prefix(b"SILKYSAV1\n")?;
        let mut get = || -> Option<Vec<u8>> {
            let n = u32::from_le_bytes(p.get(..4)?.try_into().ok()?) as usize;
            let d = p.get(4..4 + n)?.to_vec();
            p = &p[4 + n..];
            Some(d)
        };
        let label = String::from_utf8(get()?).ok()?;
        let script = String::from_utf8(get()?).ok()?;
        let msg = i32::from_le_bytes(get()?.get(..4)?.try_into().ok()?);
        let g1 = get()?;
        let g2 = get()?
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        let g3 = get_vals(&get()?)?;
        let g4 = get_vals(&get()?)?;
        let g5 = get()?;
        let flat = get_vals(&get()?)?;
        let records = flat
            .chunks_exact(2 + RECORD)
            .map(|c| ((c[0].int(), c[1].int()), c[2..].to_vec()))
            .collect();
        Some(Slot {
            label,
            script,
            msg,
            g1,
            g2,
            g3,
            g4,
            g5,
            records,
        })
    }
}
