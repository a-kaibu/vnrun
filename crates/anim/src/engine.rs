//! Runs the scenario a frame at a time: it carries out instructions until one waits (for a
//! click, a time, a transition or a choice), and says what to draw and play.
//!
//! The menus are in the game's program, so the title screen (from p_trialmenu.cwp: the
//! picture with its buttons, and the lit buttons below it), the message window (from
//! p_message.cwp), the choices (the bars of p_select.cwp) and the save screen are drawn here.
//!
//! The instructions, as the 2008 to 2010 releases use them (S: a string number):
//!
//! ```text
//! 0b J / 0c J / 0d     goto, call, return            19..1e, 22  var += -= *= /= %= =, random
//! 24..29 V n J         goto J if var == != < > <= >= n   2a..2f  the same, as a call
//! 30..35 V V           var op= var                   37..3c V V J  goto if var op var
//! 46 S                 background                    48 slot x S / 49 slot S  a character
//! 4c slot              takes a character away        4d n  shows the changes (n: the effect)
//! 4e r g b - - - ms    a flash                       56 r g b - -  a plain screen
//! 5a n / 5b / 5c ms    music bgm/n.eog, stop, fade   5d S  a sound   60 ch S / 61 ch  a loop
//! 64 ms                a pause                       93 n S, 96 e  a full screen picture
//! c8 S..  / c9 n       a choice, and where answer n goes on
//! e6 voice name text / e7 name text / e8 voice text / e9 text   messages
//! f0                   back to the title
//! ```

use std::collections::HashMap;
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::data::{self, Pck};
use crate::script::{Arg, Program};

/// A sound to start or stop.
pub struct Sound {
    pub data: Vec<u8>,
    pub volume: f32,
    pub bgm: bool,
    pub voice: bool,
    /// the effect channel (`Some(-1)`: all of them)
    pub effect_channel: Option<i32>,
    pub stop: bool,
    pub fade_ms: u32,
    pub looped: bool,
}

const FRAME_MS: f64 = 1000.0 / 60.0;
const SLOTS: usize = 10;
const TEXT_SIZE: i32 = 22;
const LINE_STEP: f32 = 30.0;
/// The center x of each character slot.
const SLOT_X: [i32; 5] = [133, 200, 400, 600, 667];

/// What a title button does.
#[derive(Clone, Copy, PartialEq)]
enum TitleAction {
    Start,
    Load,
    Exit,
    /// a setting the trial does not have here (sound volume, configuration)
    Nothing,
}

#[derive(Clone)]
struct Chara {
    name: String,
    image: Rc<Image>,
    x: i32,
}

struct Fade {
    from: Vec<Draw>,
    start: u64,
    ms: u64,
}

enum Wait {
    None,
    Click,
    Until(u64),
    Fade,
    Choice(Vec<String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Title,
    Game,
    Menu,
    Slots { save: bool, from_title: bool },
}

struct Message {
    name: Option<Rc<Image>>,
    lines: Vec<Rc<Image>>,
    first: String,
}

#[derive(Default, Clone)]
struct Slot {
    text: String,
}

pub struct Engine {
    files: Box<dyn Files>,
    pub screen: (u32, u32),
    /// where the pictures start: the 2009 release shows its 1024x576 pictures between bands
    /// on a 1024x768 screen
    stage_y: f32,
    pub log: bool,
    prog: Rc<Program>,
    pc: usize,
    stack: Vec<usize>,
    vars: HashMap<(u8, u16), i64>,
    now: u64,
    frames: u64,
    wait: Wait,
    view: Screen,
    bg: Option<Rc<Image>>,
    bg_name: String,
    bg_color: [u8; 4],
    charas: [Option<Chara>; SLOTS],
    layer: Option<(String, Rc<Image>)>,
    frozen: Option<Vec<Draw>>,
    fade: Option<Fade>,
    window: bool,
    message: Option<Message>,
    message_pc: usize,
    choice: i64,
    bgm: String,
    loops: HashMap<i32, String>,
    sounds: Vec<Sound>,
    font: Option<Rc<fontdue::Font>>,
    pointer: (i32, i32),
    cache: HashMap<String, Rc<Image>>,
    text_cache: HashMap<(String, [u8; 3], i32), Rc<Image>>,
    slots: Vec<Option<Slot>>,
    wave: Option<(String, Pck)>,
    title: Option<Option<Rc<Image>>>,
    ended: bool,
}

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Rc<Image> {
    let mut img = Image::new(w, h);
    img.fill(rgba);
    Rc::new(img)
}

impl Engine {
    pub fn new(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let names = files.list();
        let sce = names
            .iter()
            .find(|n| n.to_lowercase().ends_with("_sce.dat"))
            .ok_or("Crowd: no <game>_sce.dat in the game folder")?
            .clone();
        let raw = files
            .read(&sce)
            .ok_or_else(|| format!("{sce}: unreadable"))?;
        let plain =
            data::gax_decrypt(&raw).ok_or_else(|| format!("{sce}: not encrypted as expected"))?;
        let prog = Program::parse(&plain).map_err(|e| format!("{sce}: {e}"))?;
        let stem = sce[..sce.len() - "_sce.dat".len()].to_string();
        let wave_name = format!("voice/{stem}_wave.pck");
        let wave = files
            .read_range(&wave_name, 0, 1 << 20)
            .and_then(|h| {
                // the index may run past the first megabyte
                let n = u32::from_le_bytes(h.get(..4)?.try_into().ok()?) as usize;
                let need = 4 + 12 * n + 64 * n;
                if need > h.len() {
                    files.read_range(&wave_name, 0, need)
                } else {
                    Some(h)
                }
            })
            .and_then(|h| Pck::parse(&h))
            .map(|p| (wave_name, p));
        let mut e = Engine {
            files,
            screen: (800, 600),
            stage_y: 0.0,
            log: false,
            prog: Rc::new(prog),
            pc: 0,
            stack: Vec::new(),
            vars: HashMap::new(),
            now: 0,
            frames: 0,
            wait: Wait::None,
            view: Screen::Title,
            bg: None,
            bg_name: String::new(),
            bg_color: [0, 0, 0, 255],
            charas: Default::default(),
            layer: None,
            frozen: None,
            fade: None,
            window: false,
            message: None,
            message_pc: 0,
            choice: 0,
            bgm: String::new(),
            loops: HashMap::new(),
            sounds: Vec::new(),
            font: None,
            pointer: (-1, -1),
            cache: HashMap::new(),
            text_cache: HashMap::new(),
            slots: Vec::new(),
            wave,
            title: None,
            ended: false,
        };
        e.slots = (0..SLOTS).map(|i| e.read_slot(i)).collect();
        e.read_globals();
        // the screen is as wide as the title (1024 in 2009, whose pictures are 1024x576)
        if e.title_picture().is_some_and(|t| t.w == 1024) {
            e.screen = (1024, 768);
            e.stage_y = 96.0;
        }
        Ok(e)
    }

    pub fn set_font(&mut self, b: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(b, fontdue::FontSettings::default())?;
        self.font = Some(Rc::new(f));
        self.text_cache.clear();
        Ok(())
    }

    fn trace(&self, s: &str) {
        if self.log {
            let at = self
                .prog
                .code
                .get(self.pc.saturating_sub(1))
                .map_or(0, |i| i.at);
            eprintln!("{at:06x} {s}");
        }
    }

    /// Starts the scenario at a code offset instead of the title (for testing).
    pub fn start_at(&mut self, at: u32) {
        self.new_game();
        self.pc = self.prog.index.get(&at).copied().unwrap_or(0);
    }

    // ---- the frame ----

    pub fn frame(&mut self) -> Result<(), String> {
        self.frames += 1;
        self.now = (self.frames as f64 * FRAME_MS) as u64;
        if let Some(f) = &self.fade
            && self.now >= f.start + f.ms
        {
            self.fade = None;
        }
        if self.view != Screen::Game {
            return Ok(());
        }
        let done = match &self.wait {
            Wait::None => true,
            Wait::Click | Wait::Choice(_) => false,
            Wait::Until(t) => self.now >= *t,
            Wait::Fade => self.fade.is_none(),
        };
        if done {
            self.wait = Wait::None;
            self.run()?;
        }
        Ok(())
    }

    fn open_title(&mut self) {
        let from = self.stage_draws();
        self.reset_stage();
        self.view = Screen::Title;
        self.fade = Some(Fade {
            from,
            start: self.now,
            ms: 500,
        });
    }

    fn reset_stage(&mut self) {
        self.bg = None;
        self.bg_name.clear();
        self.bg_color = [0, 0, 0, 255];
        self.charas = Default::default();
        self.layer = None;
        self.frozen = None;
        self.fade = None;
        self.window = false;
        self.message = None;
        self.wait = Wait::None;
    }

    fn back_to_title(&mut self) {
        self.trace("back to the title");
        self.stop_voice();
        self.sounds.push(stop_effects());
        self.loops.clear();
        self.stop_bgm(500);
        self.open_title();
    }

    // ---- running the scenario ----

    fn var(&self, kind: u8, n: u16) -> i64 {
        self.vars.get(&(kind, n)).copied().unwrap_or(0)
    }

    fn jump(&mut self, at: i64) {
        match self.prog.index.get(&(at as u32)) {
            Some(&i) => self.pc = i,
            None => {
                self.trace(&format!("no code at {at:#x}"));
                self.pc = self.prog.code.len();
            }
        }
    }

    fn run(&mut self) -> Result<(), String> {
        let prog = self.prog.clone();
        for _ in 0..200_000 {
            if self.view != Screen::Game || !matches!(self.wait, Wait::None) {
                return Ok(());
            }
            let Some(insn) = prog.code.get(self.pc) else {
                self.back_to_title();
                return Ok(());
            };
            self.pc += 1;
            if self.log {
                eprintln!("{:06x} {:02x} {:?}", insn.at, insn.op, insn.args);
            }
            let a = &insn.args;
            let n = |i: usize| a.get(i).map_or(0, |v| v.n());
            let s = |i: usize| prog.string(n(i)).to_string();
            match insn.op {
                0x0b => self.jump(n(0)),
                0x0c => {
                    self.stack.push(self.pc);
                    self.jump(n(0));
                }
                0x0d => match self.stack.pop() {
                    Some(r) => self.pc = r,
                    None => self.back_to_title(),
                },
                0x19..=0x1e | 0x22 => {
                    let (k, v) = (n(0) as u8, n(1) as u16);
                    let x = n(2);
                    let old = self.var(k, v);
                    let new = match insn.op {
                        0x19 => old.wrapping_add(x),
                        0x1a => old.wrapping_sub(x),
                        0x1b => old.wrapping_mul(x),
                        0x1c => old.checked_div(x).unwrap_or(old),
                        0x1d => old.checked_rem(x).unwrap_or(old),
                        0x1e => x,
                        _ => {
                            let hi = n(3).max(x);
                            x + (self.frames as i64 * 7919 % (hi - x + 1).max(1))
                        }
                    };
                    self.set_var(k, v, new);
                }
                0x24..=0x2f => {
                    let v = self.var(n(0) as u8, n(1) as u16);
                    let x = n(2);
                    if compare(insn.op - if insn.op >= 0x2a { 0x2a } else { 0x24 }, v, x) {
                        if insn.op >= 0x2a {
                            self.stack.push(self.pc);
                        }
                        self.jump(n(3));
                    }
                }
                0x30..=0x35 => {
                    let (k, v) = (n(0) as u8, n(1) as u16);
                    let x = self.var(n(2) as u8, n(3) as u16);
                    let old = self.var(k, v);
                    let new = match insn.op {
                        0x30 => old.wrapping_add(x),
                        0x31 => old.wrapping_sub(x),
                        0x32 => old.wrapping_mul(x),
                        0x33 => old.checked_div(x).unwrap_or(old),
                        0x34 => old.checked_rem(x).unwrap_or(old),
                        _ => x,
                    };
                    self.set_var(k, v, new);
                }
                0x37..=0x3c => {
                    let v = self.var(n(0) as u8, n(1) as u16);
                    let x = self.var(n(2) as u8, n(3) as u16);
                    if compare(insn.op - 0x37, v, x) {
                        self.jump(n(4));
                    }
                }
                0x46 => {
                    let name = s(0);
                    self.freeze();
                    self.set_bg(&name);
                }
                0x48 => {
                    let name = s(2);
                    self.freeze();
                    self.show_chara(n(0) as usize, &name, Some(n(1) as i32));
                }
                0x49 => {
                    let name = s(1);
                    self.freeze();
                    self.show_chara(n(0) as usize, &name, None);
                }
                0x4c => {
                    let slot = n(0) as usize;
                    if slot < SLOTS && self.charas[slot].is_some() {
                        self.freeze();
                        self.charas[slot] = None;
                    }
                }
                0x4d | 0x96 => self.transition(n(0)),
                0x4e => {
                    let c = [n(0) as u8, n(1) as u8, n(2) as u8, 255];
                    let ms = n(6).clamp(0, 5000) as u64;
                    let mut d = draw(&solid(1, 1, c), 0.0, 0.0, 1.0);
                    d.dst = [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32];
                    self.fade = Some(Fade {
                        from: vec![d],
                        start: self.now,
                        ms,
                    });
                }
                0x56 => {
                    self.freeze();
                    self.bg = None;
                    self.bg_name = format!("#{:02x}{:02x}{:02x}", n(0), n(1), n(2));
                    self.bg_color = [n(0) as u8, n(1) as u8, n(2) as u8, 255];
                    self.layer = None;
                }
                0x5a => self.play_bgm(&format!("{:02}", n(0))),
                0x5b => self.stop_bgm(300),
                0x5c => self.stop_bgm(n(0).clamp(0, 10000) as u32),
                0x5d | 0x5e => {
                    let name = s(0);
                    self.play_effect(0, &name, false);
                }
                0x5f => self.stop_effect(0, 200),
                0x60 => {
                    let (ch, name) = (n(0) as i32, s(1));
                    self.loops.insert(ch, name.clone());
                    self.play_effect(ch + 1, &name, true);
                }
                0x61 => {
                    let ch = n(0) as i32;
                    self.loops.remove(&ch);
                    self.stop_effect(ch + 1, 300);
                }
                0x64 => self.wait = Wait::Until(self.now + n(0).clamp(0, 60000) as u64),
                0x41 if prog.family == "2009" => {
                    self.wait = Wait::Until(self.now + n(0).clamp(0, 60000) as u64)
                }
                0x7c => self.window = false,
                0x93 => {
                    let name = s(1);
                    self.freeze();
                    self.layer = self.picture(&name, false).map(|i| (name, i));
                }
                0x94 => {
                    self.freeze();
                    self.layer = None;
                }
                0xb4 | 0xc8 => {
                    let answers: Vec<String> = match a.first() {
                        Some(Arg::List(v)) => {
                            v.iter().map(|&i| prog.string(i).to_string()).collect()
                        }
                        _ => Vec::new(),
                    };
                    self.present();
                    self.message_pc = self.pc - 1;
                    self.wait = Wait::Choice(answers);
                }
                0xe6..=0xe9 => {
                    // the 2009 release puts one more number before a voiced or named message
                    let k = (prog.family == "2009") as usize;
                    let (voice, name, text) = match insn.op {
                        0xe6 => (Some(s(k)), Some(s(k + 1)), s(k + 2)),
                        0xe7 => (None, Some(s(k)), s(k + 1)),
                        0xe8 => (Some(s(0)), None, s(1)),
                        _ => (None, None, s(0)),
                    };
                    self.present();
                    self.message_pc = self.pc - 1;
                    self.stop_voice();
                    if let Some(v) = voice {
                        self.play_voice(&v);
                    }
                    self.show_message(name.as_deref(), &text);
                    self.window = true;
                    self.wait = Wait::Click;
                }
                0xf0 => self.back_to_title(),
                _ => {}
            }
        }
        Err("Crowd: the scenario runs on without stopping".into())
    }

    fn set_var(&mut self, k: u8, v: u16, x: i64) {
        self.vars.insert((k, v), x);
        if k == 0x3e {
            self.write_globals();
        }
    }

    /// Keeps the screen as it is until the next transition shows the changes.
    fn freeze(&mut self) {
        if self.frozen.is_none() && self.fade.is_none() {
            let mut d = self.stage_draws();
            if self.window && self.message.is_some() {
                self.window_draws(&mut d);
            }
            self.frozen = Some(d);
        }
    }

    /// Shows what changed since the screen was frozen.
    fn transition(&mut self, effect: i64) {
        let ms = match effect {
            0 => 0,
            e if e >= 80 => 700,
            _ => 300,
        };
        if effect >= 80 {
            self.window = false;
        }
        if let Some(from) = self.frozen.take()
            && ms > 0
        {
            self.fade = Some(Fade {
                from,
                start: self.now,
                ms,
            });
            self.wait = Wait::Fade;
        }
    }

    /// Lets the frozen changes show at once (before a message or a choice).
    fn present(&mut self) {
        if let Some(from) = self.frozen.take()
            && self.fade.is_none()
        {
            self.fade = Some(Fade {
                from,
                start: self.now,
                ms: 200,
            });
        }
    }

    fn set_bg(&mut self, name: &str) {
        self.bg = self.picture(name, false);
        if self.bg.is_none() {
            self.trace(&format!("no picture {name}"));
        }
        self.bg_name = name.to_string();
        self.layer = None;
        // a new background takes the characters away (the scenario shows them again after it)
        self.charas = Default::default();
    }

    fn show_chara(&mut self, slot: usize, name: &str, x: Option<i32>) {
        if slot >= SLOTS {
            return;
        }
        let Some(image) = self.picture(name, true) else {
            self.trace(&format!("no picture {name}"));
            return;
        };
        let x = x.unwrap_or_else(|| {
            SLOT_X.get(slot).copied().unwrap_or(400) * self.screen.0 as i32 / 800
        });
        self.charas[slot] = Some(Chara {
            name: name.to_string(),
            image,
            x,
        });
    }

    // ---- drawing ----

    fn solid(&self, rgba: [u8; 4]) -> Draw {
        let mut d = draw(&solid(1, 1, rgba), 0.0, 0.0, 1.0);
        d.dst = [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32];
        d
    }

    fn stage_draws(&self) -> Vec<Draw> {
        let mut out = vec![self.solid(self.bg_color)];
        let y = self.stage_y;
        if let Some(bg) = &self.bg {
            out.push(draw(bg, 0.0, y, 1.0));
        }
        for c in self.charas.iter().flatten() {
            let left = c.x as f32 - c.image.w as f32 / 2.0;
            let top = self.screen.1 as f32 - y - c.image.h as f32;
            out.push(draw(&c.image, left, top, 1.0));
        }
        if let Some((_, l)) = &self.layer {
            out.push(draw(l, 0.0, y, 1.0));
        }
        out
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = match self.view {
            Screen::Title
            | Screen::Slots {
                from_title: true, ..
            } => self.title_draws(),
            _ => match &self.frozen {
                Some(f) => f.clone(),
                None => self.stage_draws(),
            },
        };
        let fading = self.fade.is_some();
        if let Some(f) = &self.fade {
            let p = (self.now.saturating_sub(f.start) as f32 / f.ms.max(1) as f32).min(1.0);
            for d in &f.from {
                let mut d = d.clone();
                d.place.alpha *= 1.0 - p;
                out.push(d);
            }
        }
        if matches!(
            self.view,
            Screen::Game
                | Screen::Menu
                | Screen::Slots {
                    from_title: false,
                    ..
                }
        ) {
            if self.window && self.frozen.is_none() && (!fading || self.message.is_some()) {
                self.window_draws(&mut out);
            }
            if let Wait::Choice(answers) = &self.wait {
                let answers = answers.clone();
                self.choice_draws(&answers, &mut out);
            }
        }
        match self.view {
            Screen::Menu => self.menu_draws(&mut out),
            Screen::Slots { save, .. } => self.slot_draws(save, &mut out),
            _ => {}
        }
        out
    }

    fn title_picture(&mut self) -> Option<Rc<Image>> {
        if self.title.is_none() {
            let t = ["p_trialmenu", "p_title", "p_menu"]
                .iter()
                .find_map(|n| self.picture(n, false));
            self.title = Some(t);
        }
        self.title.clone().flatten()
    }

    /// The title's buttons: where each is on the screen, where its lit look is in the sheet
    /// (None: drawn by us) and what it does.
    fn title_buttons(&mut self) -> Vec<([i32; 4], Option<[i32; 2]>, TitleAction)> {
        use TitleAction::*;
        let sheet = self.title_picture().map(|t| (t.w, t.h));
        match sheet {
            // 2008: START, LOAD and EXIT stepping down to the right; lit ones below in a column
            Some((800, 760)) => [(510, 248), (560, 318), (610, 388)]
                .iter()
                .zip([Start, Load, Exit])
                .enumerate()
                .map(|(i, (&(x, y), a))| {
                    ([x, y, x + 184, y + 50], Some([184, 600 + i as i32 * 50]), a)
                })
                .collect(),
            // 2010: five vertical labels from 動作終了 on the left to 初めから; the plain ones
            // then the lit ones in a row below
            Some((800, 900)) => [Exit, Nothing, Nothing, Load, Start]
                .into_iter()
                .enumerate()
                .map(|(i, a)| {
                    let x = 43 + 54 * i as i32;
                    ([x, 365, x + 40, 575], Some([200 + 40 * i as i32, 600]), a)
                })
                .collect(),
            // 2009 (1024x768): Start, Load, Config, Volume and Exit down the middle; the plain
            // ones then the lit ones side by side below
            Some((1024, 1300)) => [Start, Load, Nothing, Nothing, Exit]
                .into_iter()
                .enumerate()
                .map(|(i, a)| {
                    let y = 345 + 61 * i as i32;
                    ([312, y, 712, y + 46], Some([400, 768 + 46 * i as i32]), a)
                })
                .collect(),
            _ => [Start, Load, Exit]
                .into_iter()
                .enumerate()
                .map(|(i, a)| {
                    (
                        [520, 260 + i as i32 * 70, 760, 316 + i as i32 * 70],
                        None,
                        a,
                    )
                })
                .collect(),
        }
    }

    fn title_draws(&mut self) -> Vec<Draw> {
        let mut out = vec![self.solid([0, 0, 0, 255])];
        let buttons = self.title_buttons();
        let sheet = self.title_picture();
        if let Some(t) = &sheet {
            let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
            out.push(part(t, [0, 0, w, h], 0.0, 0.0));
        }
        for (r, lit, action) in buttons {
            match (&sheet, lit) {
                (Some(t), Some([sx, sy])) => {
                    if inside(r, self.pointer.0, self.pointer.1) && action != TitleAction::Nothing {
                        let src = [sx, sy, r[2] - r[0], r[3] - r[1]];
                        out.push(part(t, src, r[0] as f32, r[1] as f32));
                    }
                }
                _ => {
                    let label = match action {
                        TitleAction::Start => "はじめから",
                        TitleAction::Load => "つづきから",
                        _ => "ゲーム終了",
                    };
                    self.button(label, r, &mut out);
                }
            }
        }
        out
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let top = h - self.window_height();
        match self.picture("p_message", false) {
            Some(m) if m.h >= 445 => {
                let mut d = part(&m, [0, 292, 800, 153], 0.0, top);
                d.place.alpha = 0.8;
                out.push(d);
            }
            _ => {
                let mut d = draw(&solid(1, 1, [0, 0, 0, 200]), 0.0, 0.0, 1.0);
                d.dst = [0.0, top, w, h];
                out.push(d);
            }
        }
        if let Some(m) = &self.message {
            let mut y = top + 16.0;
            if let Some(n) = &m.name {
                out.push(draw(n, 30.0, y, 1.0));
                y += LINE_STEP;
            }
            for l in &m.lines {
                out.push(draw(l, 40.0, y, 1.0));
                y += LINE_STEP;
            }
        }
    }

    /// The message window's height at the bottom of the screen (in 2009 it covers the band
    /// below the picture and a little of it).
    fn window_height(&self) -> f32 {
        if self.stage_y > 0.0 { 176.0 } else { 153.0 }
    }

    /// The choice bars: 584x56 (2008, 2010) or the whole width, 90 high (2009).
    fn bar_size(&self) -> (i32, i32, i32) {
        if self.screen.0 == 1024 {
            (1024, 90, 0)
        } else {
            (584, 56, 12)
        }
    }

    fn choice_rects(&self, n: usize) -> Vec<[i32; 4]> {
        let (bw, bh, gap) = self.bar_size();
        let mid = (self.screen.1 as i32 - self.window_height() as i32) / 2;
        let top = mid - n as i32 * (bh + gap) / 2;
        let left = (self.screen.0 as i32 - bw) / 2;
        (0..n as i32)
            .map(|i| {
                let y = top + i * (bh + gap);
                [left, y, left + bw, y + bh]
            })
            .collect()
    }

    fn choice_draws(&mut self, answers: &[String], out: &mut Vec<Draw>) {
        let bars = self.cache.get("p_select#bars").cloned().or_else(|| {
            let b = self.picture("p_select", false)?;
            // 2008 and 2010: two bars, then their masks; 2009: four plain bars
            let b = if b.h == 224 {
                Rc::new(data::masked_below(&b))
            } else {
                b
            };
            self.cache.insert("p_select#bars".into(), b.clone());
            Some(b)
        });
        for (r, text) in self.choice_rects(answers.len()).into_iter().zip(answers) {
            let lit = inside(r, self.pointer.0, self.pointer.1);
            let (bw, bh, _) = self.bar_size();
            match &bars {
                // 2009: the dark bar, lit as the light grey one
                Some(b) if b.w == 1024 && b.h >= 360 => {
                    let row = if lit { 2 } else { 3 };
                    out.push(part(b, [0, row * 90, bw, bh], r[0] as f32, r[1] as f32));
                }
                Some(b) if b.h >= 112 => {
                    out.push(part(
                        b,
                        [0, if lit { bh } else { 0 }, bw, bh],
                        r[0] as f32,
                        r[1] as f32,
                    ));
                }
                _ => {
                    let mut d = draw(
                        &solid(
                            1,
                            1,
                            if lit {
                                [40, 160, 40, 230]
                            } else {
                                [0, 0, 0, 200]
                            },
                        ),
                        0.0,
                        0.0,
                        1.0,
                    );
                    d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
                    out.push(d);
                }
            }
            let t = self.text(text, [255, 255, 255], TEXT_SIZE);
            let x = (r[0] + r[2]) as f32 / 2.0 - t.w as f32 / 2.0;
            let y = (r[1] + r[3]) as f32 / 2.0 - t.h as f32 / 2.0;
            out.push(draw(&t, x, y, 1.0));
        }
    }

    fn menu_items(&self) -> Vec<(&'static str, [i32; 4])> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        let top = h / 2 - 2 * 60;
        ["セーブ", "ロード", "タイトルに戻る", "戻る"]
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let y = top + i as i32 * 60;
                (*t, [w / 2 - 160, y, w / 2 + 160, y + 48])
            })
            .collect()
    }

    fn menu_draws(&mut self, out: &mut Vec<Draw>) {
        out.push(self.solid([0, 0, 0, 120]));
        for (t, r) in self.menu_items() {
            self.button(t, r, out);
        }
    }

    fn button(&mut self, text: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let lit = inside(r, self.pointer.0, self.pointer.1);
        let mut d = draw(
            &solid(
                1,
                1,
                if lit {
                    [90, 90, 200, 230]
                } else {
                    [20, 20, 40, 220]
                },
            ),
            0.0,
            0.0,
            1.0,
        );
        d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        out.push(d);
        let t = self.text(text, [255, 255, 255], TEXT_SIZE);
        let x = (r[0] + r[2]) as f32 / 2.0 - t.w as f32 / 2.0;
        let y = (r[1] + r[3]) as f32 / 2.0 - t.h as f32 / 2.0;
        out.push(draw(&t, x, y, 1.0));
    }

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        let w = self.screen.0 as i32;
        let (bw, bh) = (w / 2 - 60, 80);
        (0..SLOTS as i32)
            .map(|i| {
                let x = if i < 5 { 30 } else { w / 2 + 30 };
                let y = 90 + (i % 5) * (bh + 14);
                [x, y, x + bw, y + bh]
            })
            .collect()
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        out.push(self.solid([0, 0, 24, 220]));
        let head = self.text(if save { "セーブ" } else { "ロード" }, [255, 255, 255], 30);
        out.push(draw(&head, 30.0, 24.0, 1.0));
        let hint = self.text("右クリックで戻る", [200, 200, 200], 18);
        out.push(draw(
            &hint,
            self.screen.0 as f32 - hint.w as f32 - 30.0,
            34.0,
            1.0,
        ));
        for (i, r) in self.slot_rects().into_iter().enumerate() {
            let label = match &self.slots[i] {
                Some(s) => format!("No.{:02}  {}", i + 1, s.text),
                None => format!("No.{:02}  ----", i + 1),
            };
            let lit = inside(r, self.pointer.0, self.pointer.1);
            let mut d = draw(
                &solid(
                    1,
                    1,
                    if lit {
                        [90, 90, 200, 230]
                    } else {
                        [20, 20, 40, 220]
                    },
                ),
                0.0,
                0.0,
                1.0,
            );
            d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
            out.push(d);
            let t = self.text(&label, [255, 255, 255], 18);
            let mut d = draw(&t, (r[0] + 12) as f32, (r[1] + 28) as f32, 1.0);
            d.place.clip = [r[0] as f32, r[1] as f32, (r[2] - 6) as f32, r[3] as f32];
            out.push(d);
        }
    }

    // ---- text ----

    fn text(&mut self, s: &str, rgb: [u8; 3], size: i32) -> Rc<Image> {
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

    fn show_message(&mut self, name: Option<&str>, text: &str) {
        let name_img = name
            .filter(|n| !n.is_empty())
            .map(|n| self.text(&format!("【{n}】"), [255, 230, 160], TEXT_SIZE));
        let per_line = ((self.screen.0 as i32 - 80) / TEXT_SIZE).max(10) as usize;
        let mut lines = Vec::new();
        for l in text.split(['\n', '\r']).filter(|l| !l.is_empty()) {
            for piece in wrap(l, per_line) {
                lines.push(self.text(&piece, [255, 255, 255], TEXT_SIZE));
            }
        }
        lines.truncate(if name_img.is_some() { 3 } else { 4 });
        let first = match name {
            Some(n) if !n.is_empty() => format!("{n} {text}"),
            _ => text.to_string(),
        };
        self.message = Some(Message {
            name: name_img,
            lines,
            first: first.chars().take(40).collect(),
        });
    }

    // ---- pictures and sounds ----

    /// cg/<name>.cwp; `chara`: the picture and its mask side by side.
    fn picture(&mut self, name: &str, chara: bool) -> Option<Rc<Image>> {
        let key = name.to_lowercase();
        if let Some(i) = self.cache.get(&key) {
            return Some(i.clone());
        }
        let b = ["cwp", "gax", "CWP"]
            .iter()
            .find_map(|ext| self.files.read(&format!("cg/{name}.{ext}")))?;
        let img = match data::cwp(&b) {
            Ok(i) => i,
            Err(e) => {
                self.trace(&format!("{name}: {e}"));
                return None;
            }
        };
        let img = Rc::new(if chara { data::masked(&img) } else { img });
        if self.cache.len() > 40 {
            self.cache.clear();
        }
        self.cache.insert(key, img.clone());
        Some(img)
    }

    fn wave(&mut self, name: &str) -> Option<Vec<u8>> {
        let (file, pck) = self.wave.as_ref()?;
        let (off, size) = pck.find(name)?;
        let file = file.clone();
        self.files.read_range(&file, off, size).map(data::eog)
    }

    fn play_bgm(&mut self, name: &str) {
        if self.bgm == name {
            return;
        }
        let Some(data) = self.files.read(&format!("bgm/{name}.eog")).map(data::eog) else {
            self.trace(&format!("no music {name}"));
            return;
        };
        self.bgm = name.to_string();
        self.sounds.push(Sound {
            data,
            volume: 0.7,
            bgm: true,
            voice: false,
            effect_channel: None,
            stop: false,
            fade_ms: 0,
            looped: true,
        });
    }

    fn stop_bgm(&mut self, fade_ms: u32) {
        self.bgm.clear();
        self.sounds.push(Sound {
            data: Vec::new(),
            volume: 0.0,
            bgm: true,
            voice: false,
            effect_channel: None,
            stop: true,
            fade_ms,
            looped: false,
        });
    }

    fn play_effect(&mut self, channel: i32, name: &str, looped: bool) {
        let Some(data) = self.wave(name) else {
            self.trace(&format!("no sound {name}"));
            return;
        };
        self.sounds.push(Sound {
            data,
            volume: 0.8,
            bgm: false,
            voice: false,
            effect_channel: Some(channel),
            stop: false,
            fade_ms: 0,
            looped,
        });
    }

    fn stop_effect(&mut self, channel: i32, fade_ms: u32) {
        self.sounds.push(Sound {
            data: Vec::new(),
            volume: 0.0,
            bgm: false,
            voice: false,
            effect_channel: Some(channel),
            stop: true,
            fade_ms,
            looped: false,
        });
    }

    fn play_voice(&mut self, name: &str) {
        let Some(data) = self.wave(name) else {
            self.trace(&format!("no voice {name}"));
            return;
        };
        self.sounds.push(Sound {
            data,
            volume: 1.0,
            bgm: false,
            voice: true,
            effect_channel: None,
            stop: false,
            fade_ms: 0,
            looped: false,
        });
    }

    fn stop_voice(&mut self) {
        self.sounds.push(Sound {
            data: Vec::new(),
            volume: 0.0,
            bgm: false,
            voice: true,
            effect_channel: None,
            stop: true,
            fade_ms: 0,
            looped: false,
        });
    }

    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if x >= 0 {
            self.pointer = (x, y);
        }
        match self.view {
            Screen::Title => {
                let hit = self
                    .title_buttons()
                    .into_iter()
                    .find(|b| inside(b.0, x, y))
                    .map(|b| b.2);
                match hit {
                    Some(TitleAction::Start) => self.new_game(),
                    Some(TitleAction::Load) => {
                        self.view = Screen::Slots {
                            save: false,
                            from_title: true,
                        }
                    }
                    Some(TitleAction::Exit) => self.ended = true,
                    _ => {}
                }
            }
            Screen::Menu => {
                let hit = self
                    .menu_items()
                    .into_iter()
                    .position(|(_, r)| inside(r, x, y));
                match hit {
                    Some(0) => {
                        self.view = Screen::Slots {
                            save: true,
                            from_title: false,
                        }
                    }
                    Some(1) => {
                        self.view = Screen::Slots {
                            save: false,
                            from_title: false,
                        }
                    }
                    Some(2) => {
                        self.view = Screen::Game;
                        self.back_to_title();
                    }
                    Some(_) => self.view = Screen::Game,
                    None => {}
                }
            }
            Screen::Slots { save, .. } => {
                let Some(i) = self.slot_rects().into_iter().position(|r| inside(r, x, y)) else {
                    return;
                };
                if save {
                    self.save(i);
                    self.view = Screen::Game;
                } else if self.slots[i].is_some() {
                    self.load(i);
                }
            }
            Screen::Game => self.game_click(x, y),
        }
    }

    fn new_game(&mut self) {
        self.reset_stage();
        self.vars.retain(|k, _| k.0 == 0x3e);
        self.stack.clear();
        self.loops.clear();
        self.stop_bgm(500);
        self.view = Screen::Game;
        self.pc = 0;
    }

    fn game_click(&mut self, x: i32, y: i32) {
        match &self.wait {
            Wait::Click => self.wait = Wait::None,
            Wait::Until(_) => self.wait = Wait::None,
            Wait::Fade => {
                self.fade = None;
                self.wait = Wait::None;
            }
            Wait::Choice(answers) => {
                let n = answers.len();
                let hit = if x < 0 {
                    None
                } else {
                    self.choice_rects(n).iter().position(|r| inside(*r, x, y))
                };
                if let Some(i) = hit {
                    self.choose(i as i64 + 1);
                }
            }
            Wait::None => {}
        }
    }

    /// Goes on after the `c9` case of the answer (numbered from 1).
    fn choose(&mut self, answer: i64) {
        self.choice = answer;
        self.wait = Wait::None;
        self.message = None;
        self.window = false;
        let prog = self.prog.clone();
        if let Some(i) = prog.code[self.pc..]
            .iter()
            .position(|c| c.op == 0xc9 && c.args.first().map(|a| a.n()) == Some(answer))
        {
            self.pc += i + 1;
        }
    }

    /// A right click: the menu in the game, back out of the menu and the save screen.
    pub fn cancel(&mut self) {
        match self.view {
            Screen::Game => self.view = Screen::Menu,
            Screen::Menu => self.view = Screen::Game,
            Screen::Slots {
                from_title: true, ..
            } => self.view = Screen::Title,
            Screen::Slots { .. } => self.view = Screen::Game,
            Screen::Title => {}
        }
    }

    pub fn wheel(&mut self, _up: bool) {}

    pub fn ended(&self) -> bool {
        self.ended
    }

    // ---- saves ----

    fn slot_name(i: usize) -> String {
        format!("save{:02}.txt", i + 1)
    }

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&Self::slot_name(i))?;
        let t = String::from_utf8_lossy(&b).to_string();
        let text = t
            .lines()
            .find_map(|l| l.strip_prefix("text="))
            .unwrap_or("")
            .to_string();
        Some(Slot { text })
    }

    fn read_globals(&mut self) {
        let Some(b) = self.files.read_save("global.txt") else {
            return;
        };
        for l in String::from_utf8_lossy(&b).lines() {
            if let Some((k, v)) = l.split_once('=')
                && let (Ok(k), Ok(v)) = (k.parse::<u16>(), v.parse::<i64>())
            {
                self.vars.insert((0x3e, k), v);
            }
        }
    }

    fn write_globals(&mut self) {
        let mut g: Vec<_> = self.vars.iter().filter(|(k, _)| k.0 == 0x3e).collect();
        g.sort();
        let s: String = g.iter().map(|((_, k), v)| format!("{k}={v}\n")).collect();
        self.files.write_save("global.txt", s.as_bytes());
    }

    fn save(&mut self, i: usize) {
        let at = |i: usize, p: &Program| p.code.get(i).map_or(u32::MAX, |c| c.at);
        let mut s = format!("pc={}\n", at(self.message_pc, &self.prog));
        for r in &self.stack {
            s += &format!("ret={}\n", at(*r, &self.prog));
        }
        s += &format!("bg={}\n", self.bg_name);
        for (slot, c) in self.charas.iter().enumerate() {
            if let Some(c) = c {
                s += &format!("chara={slot},{},{}\n", c.x, c.name);
            }
        }
        if let Some((n, _)) = &self.layer {
            s += &format!("layer={n}\n");
        }
        s += &format!("bgm={}\n", self.bgm);
        for (ch, n) in &self.loops {
            s += &format!("loop={ch},{n}\n");
        }
        let mut vars: Vec<_> = self.vars.iter().filter(|(k, _)| k.0 != 0x3e).collect();
        vars.sort();
        for ((k, n), v) in vars {
            s += &format!("var={k},{n},{v}\n");
        }
        let text = self
            .message
            .as_ref()
            .map_or(String::new(), |m| m.first.clone());
        s += &format!("text={text}\n");
        self.files.write_save(&Self::slot_name(i), s.as_bytes());
        self.slots[i] = Some(Slot { text });
    }

    fn load(&mut self, i: usize) {
        let Some(b) = self.files.read_save(&Self::slot_name(i)) else {
            return;
        };
        let t = String::from_utf8_lossy(&b).to_string();
        let from = match self.view {
            Screen::Slots {
                from_title: true, ..
            }
            | Screen::Title => self.title_draws(),
            _ => self.stage_draws(),
        };
        self.reset_stage();
        self.vars.retain(|k, _| k.0 == 0x3e);
        self.stack.clear();
        self.stop_voice();
        self.sounds.push(stop_effects());
        self.loops.clear();
        let index =
            |v: &str, p: &Program| v.parse::<u32>().ok().and_then(|a| p.index.get(&a).copied());
        let prog = self.prog.clone();
        let mut bgm = String::new();
        for l in t.lines() {
            let Some((k, v)) = l.split_once('=') else {
                continue;
            };
            match k {
                "pc" => self.pc = index(v, &prog).unwrap_or(0),
                "ret" => {
                    if let Some(r) = index(v, &prog) {
                        self.stack.push(r);
                    }
                }
                "bg" => {
                    if let Some(hex) = v.strip_prefix('#') {
                        let c = u32::from_str_radix(hex, 16).unwrap_or(0);
                        self.bg_color = [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255];
                        self.bg_name = v.to_string();
                    } else if !v.is_empty() {
                        self.set_bg(v);
                    }
                }
                "chara" => {
                    let p: Vec<&str> = v.splitn(3, ',').collect();
                    if let [slot, x, name] = p[..] {
                        self.show_chara(slot.parse().unwrap_or(0), name, x.parse().ok());
                    }
                }
                "layer" => self.layer = self.picture(v, false).map(|i| (v.to_string(), i)),
                "bgm" => bgm = v.to_string(),
                "loop" => {
                    if let Some((ch, n)) = v.split_once(',')
                        && let Ok(ch) = ch.parse::<i32>()
                    {
                        self.loops.insert(ch, n.to_string());
                        self.play_effect(ch + 1, n, true);
                    }
                }
                "var" => {
                    let p: Vec<i64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                    if let [k, n, x] = p[..] {
                        self.vars.insert((k as u8, n as u16), x);
                    }
                }
                _ => {}
            }
        }
        self.bgm.clear();
        if bgm.is_empty() {
            self.stop_bgm(300);
        } else {
            self.play_bgm(&bgm);
        }
        self.wait = Wait::None;
        self.view = Screen::Game;
        self.fade = Some(Fade {
            from,
            start: self.now,
            ms: 400,
        });
    }

    /// The frame as one picture (for the native runner).
    pub fn render(&mut self) -> Image {
        let mut img = Image::new(self.screen.0, self.screen.1);
        img.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }
}

/// The comparisons of the conditional jumps, in opcode order: == != < > <= >=.
fn compare(k: u8, a: i64, b: i64) -> bool {
    match k {
        0 => a == b,
        1 => a != b,
        2 => a < b,
        3 => a > b,
        4 => a <= b,
        _ => a >= b,
    }
}

fn stop_effects() -> Sound {
    Sound {
        data: Vec::new(),
        volume: 0.0,
        bgm: false,
        voice: false,
        effect_channel: Some(-1),
        stop: true,
        fade_ms: 300,
        looped: false,
    }
}

/// Breaks a line into pieces of at most `n` characters.
fn wrap(s: &str, n: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        return vec![s.to_string()];
    }
    chars.chunks(n).map(|c| c.iter().collect()).collect()
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
