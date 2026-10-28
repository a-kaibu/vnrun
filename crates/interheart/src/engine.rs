//! Runs the scenario a frame at a time: it carries out the lines of ACT_<letter>.TXT until one
//! waits (for a click, a time, a transition or a choice), and says what to draw and play.
//!
//! The game's menus live in its program, so the title screen, the message window and the
//! save screen are drawn here: the title from the title picture, titlechip.kg and the hover
//! map TitleTM1.hmp/TitleM1.hmp (button n is drawn from row n of the chip sheet), the message
//! window from MesChip.kg, the choices and the save screen plainly.
//!
//! Commands, as the scripts' own comments name them:
//!
//! ```text
//! EF_B0001_<pic>       background (or event picture), shown with a cross fade of flag 41 ms
//! CG_KG<n>_<pos>       a character (KG<n>.kg) at position pos; DG_KG<n>[_<pos>] takes it away
//! KM_<a>_<b>           moves the character at a to b over flag 44 ms (900+: x, y in flags)
//! FCG_<pic> / FCG_OFF  the face in the message window
//! EF_C0030             all characters off          EF_C0060 / EF_C0061  window off / on
//! EF_BLACK / EF_WHITE  a plain screen              EF_FOUT / EF_WOUT    fade to black / white
//! EF_WAIT_<ms>         a pause (negative too)      EF_NOP[_n]           lets the changes show
//! EF_FLAG_<n>_<v>      flag n = v                  FC_EQU_F<n>_<v>      compare flag n with v
//! EF_JUMP_-1_<a>_<b>   to scene a if equal, else b EF_NEAR_<n>_<a>_<b>  to LB_a if flag n is 0
//! EF_SKIP              the scene ends              BG_BGM<n>[_FIN|_FOUT|_STOP]  music
//! SE_SE<n>[_FOUT|_STOP|_FIN]  a sound              BGCV_<n> / BGCV_OFF  breathing in a loop
//! ```

use std::collections::HashMap;
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::fpk::Archives;
use crate::pic::{self, HoverMap};
use crate::script::{Act, Line, SceneId, text_of};

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
    /// an effect that plays again and again (music always does)
    pub looped: bool,
}

/// The x of each character position (from the game's table), and where the moves of the
/// scripts' "こっくり" start.
const POSITIONS: [i32; 9] = [320, 640, 960, 0, 1280, 480, 800, 160, 1120];

const FRAME_MS: f64 = 1000.0 / 60.0;
const SLOTS: usize = 10;
const TEXT_SIZE: i32 = 28;
const LINE_STEP: i32 = 38;

struct Chara {
    slot: i32,
    name: String,
    image: Rc<Image>,
    from: (f32, f32),
    to: (f32, f32),
    start: u64,
    ms: u64,
}

impl Chara {
    fn at(&self, now: u64) -> (f32, f32) {
        let p = if self.ms == 0 {
            1.0
        } else {
            (now.saturating_sub(self.start) as f32 / self.ms as f32).min(1.0)
        };
        (
            self.from.0 + (self.to.0 - self.from.0) * p,
            self.from.1 + (self.to.1 - self.from.1) * p,
        )
    }

    fn moving(&self, now: u64) -> bool {
        now < self.start + self.ms
    }
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
    Moves,
    Choice(Vec<(String, SceneId)>),
}

/// The screen over the game: the title, the menu or the save slots.
#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Logo(u64),
    Title,
    Game,
    Menu,
    Slots { save: bool, from_title: bool },
}

struct Title {
    bg: Option<Rc<Image>>,
    chip: Option<Rc<Image>>,
    map: Option<HoverMap>,
    /// the buttons of the hover map, or (when the chip sheet is not the usual 2 x 6 cells of
    /// 324 x 52 in 3 rows each) plain ones of our own
    buttons: Vec<(u8, [i32; 4])>,
    classic: bool,
}

/// Our own title buttons: what each does (as the hover map numbers them) and its label.
const PLAIN_TITLE: [(u8, &str); 3] = [(1, "はじめから"), (2, "つづきから"), (6, "ゲーム終了")];

/// The message on show, drawn.
struct Message {
    name: Option<Rc<Image>>,
    lines: Vec<Rc<Image>>,
    /// the first line, for the save slots
    first: String,
}

#[derive(Default, Clone)]
struct Slot {
    text: String,
}

pub struct Engine {
    files: Box<dyn Files>,
    arc: Archives,
    pub screen: (u32, u32),
    pub log: bool,
    acts: HashMap<char, Rc<Act>>,
    act: Rc<Act>,
    letter: char,
    pc: usize,
    /// the scene being played and where it goes when it ends
    scene: SceneId,
    next: SceneId,
    /// a jump just landed on a scene's first line
    entering: bool,
    now: u64,
    frames: u64,
    wait: Wait,
    view: Screen,
    // the stage
    bg: Option<Rc<Image>>,
    bg_name: String,
    bg_color: [u8; 4],
    charas: Vec<Chara>,
    face: Option<(String, Rc<Image>)>,
    frozen: Option<Vec<Draw>>,
    fade: Option<Fade>,
    window: bool,
    message: Option<Message>,
    /// the line of the message on show (where a save starts again)
    message_pc: usize,
    /// a save starts at a scene's (or a choice's) first line rather than a message
    message_enters: bool,
    flags: HashMap<i32, i32>,
    equal: bool,
    bgm: String,
    chara_ids: Vec<String>,
    sounds: Vec<Sound>,
    font: Option<Rc<fontdue::Font>>,
    title: Option<Title>,
    meschip: Option<Option<Rc<Image>>>,
    select_map: Option<Option<HoverMap>>,
    pointer: (i32, i32),
    cache: HashMap<String, Rc<Image>>,
    text_cache: HashMap<(String, [u8; 3], i32), Rc<Image>>,
    slots: Vec<Option<Slot>>,
    ended: bool,
}

fn white(w: u32, h: u32, rgba: [u8; 4]) -> Rc<Image> {
    let mut img = Image::new(w, h);
    img.fill(rgba);
    Rc::new(img)
}

impl Engine {
    pub fn new(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arc = Archives::open(files.as_mut());
        if !arc.has("ACT_A.TXT") {
            return Err("Interheart: no ACT_A.TXT in the .fpk archives".into());
        }
        let screen = arc
            .read(files.as_mut(), "meswindow.hmp")
            .and_then(|b| HoverMap::parse(&b))
            .map_or((1280, 720), |m| (m.w as u32, m.h as u32));
        let chara_ids = arc
            .read(files.as_mut(), "charaid.tbl")
            .map(|b| {
                text_of(&b)
                    .lines()
                    .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
                    .map(|l| l.split_whitespace().next().unwrap_or("").to_string())
                    .collect()
            })
            .unwrap_or_default();
        let mut e = Engine {
            files,
            arc,
            screen,
            log: false,
            acts: HashMap::new(),
            act: Rc::new(Act::default()),
            letter: 'A',
            pc: 0,
            scene: ('A', 0, 0),
            next: ('Z', 9999, 99),
            entering: false,
            now: 0,
            frames: 0,
            wait: Wait::None,
            view: Screen::Logo(0),
            bg: None,
            bg_name: String::new(),
            bg_color: [0, 0, 0, 255],
            charas: Vec::new(),
            face: None,
            frozen: None,
            fade: None,
            window: false,
            message: None,
            message_pc: 0,
            message_enters: false,
            flags: HashMap::new(),
            equal: false,
            bgm: String::new(),
            chara_ids,
            sounds: Vec::new(),
            font: None,
            title: None,
            meschip: None,
            select_map: None,
            pointer: (-1, -1),
            cache: HashMap::new(),
            text_cache: HashMap::new(),
            slots: Vec::new(),
            ended: false,
        };
        e.slots = (0..SLOTS).map(|i| e.read_slot(i)).collect();
        e.load_act('A')?;
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
            eprintln!("{}:{} {s}", self.letter, self.pc);
        }
    }

    fn load_act(&mut self, letter: char) -> Result<(), String> {
        if let Some(a) = self.acts.get(&letter) {
            self.act = a.clone();
            self.letter = letter;
            return Ok(());
        }
        let name = format!("ACT_{letter}.TXT");
        let b = self
            .arc
            .read(self.files.as_mut(), &name)
            .ok_or_else(|| format!("{name} is missing"))?;
        let act = Rc::new(Act::parse(&text_of(&b)));
        self.acts.insert(letter, act.clone());
        self.act = act;
        self.letter = letter;
        Ok(())
    }

    /// Starts the scenario at `name` ("A0100_000") instead of the title (for testing).
    pub fn start_at(&mut self, name: &str) {
        let parts: Vec<&str> = name.split('_').collect();
        let id = (|| {
            let mut c = parts.first()?.chars();
            let l = c.next()?;
            Some((l, c.as_str().parse().ok()?, parts.get(1)?.parse().ok()?))
        })();
        self.reset_stage();
        self.view = Screen::Game;
        match id {
            Some(id) => self.goto(id),
            None => self.pc = 0,
        }
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
        match self.view {
            Screen::Logo(until) => {
                if until == 0 {
                    self.start_logo();
                } else if self.now >= until {
                    self.open_title();
                }
                return Ok(());
            }
            Screen::Game => {}
            _ => return Ok(()),
        }
        let done = match &self.wait {
            Wait::None => true,
            Wait::Click | Wait::Choice(_) => false,
            Wait::Until(t) => self.now >= *t,
            Wait::Fade => self.fade.is_none(),
            Wait::Moves => !self.charas.iter().any(|c| c.moving(self.now)) && self.fade.is_none(),
        };
        if done {
            self.wait = Wait::None;
            self.run()?;
        }
        Ok(())
    }

    fn start_logo(&mut self) {
        match self.find_pic("logo") {
            Some(img) => {
                self.bg = Some(img);
                self.fade = Some(Fade {
                    from: vec![self.solid([0, 0, 0, 255])],
                    start: self.now,
                    ms: 500,
                });
                self.view = Screen::Logo(self.now + 2000);
            }
            None => self.open_title(),
        }
    }

    fn open_title(&mut self) {
        let from = self.stage_draws();
        self.reset_stage();
        if self.title.is_none() {
            let bg = self.title_bg();
            let chip = self.find_pic("titlechip");
            let map = ["TitleTM1.hmp", "TitleM1.hmp", "titleM.hmp"]
                .iter()
                .find_map(|n| self.arc.read(self.files.as_mut(), n))
                .and_then(|b| HoverMap::parse(&b));
            let classic = chip.as_ref().is_some_and(|c| classic_title_chip(c)) && map.is_some();
            let buttons = if classic {
                map.as_ref().map(|m| m.boxes()).unwrap_or_default()
            } else {
                let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
                PLAIN_TITLE
                    .iter()
                    .enumerate()
                    .map(|(i, (v, _))| {
                        let y = h / 2 + i as i32 * 76;
                        (*v, [w - 340, y, w - 60, y + 56])
                    })
                    .collect()
            };
            self.title = Some(Title {
                bg,
                chip,
                map,
                buttons,
                classic,
            });
        }
        self.fade = Some(Fade {
            from,
            start: self.now,
            ms: 500,
        });
        self.view = Screen::Title;
        let first_bgm = self.arc.read(self.files.as_mut(), "bgm.tbl").and_then(|b| {
            text_of(&b)
                .lines()
                .map(|l| l.split("//").next().unwrap_or("").trim().to_string())
                .find(|l| l.to_lowercase().ends_with(".ogg"))
        });
        match first_bgm {
            Some(name) => self.play_bgm(name.trim_end_matches(".OGG").trim_end_matches(".ogg")),
            None => self.stop_bgm(500),
        }
    }

    fn title_bg(&mut self) -> Option<Rc<Image>> {
        if let Some(i) = self.find_pic("title") {
            return Some(i);
        }
        let name = self
            .arc
            .packs
            .iter()
            .flat_map(|(_, i)| i.names.iter())
            .find(|n| {
                let l = n.to_lowercase();
                l.starts_with("title") && l.ends_with(".bmp")
            })?
            .clone();
        self.picture(&name)
    }

    fn reset_stage(&mut self) {
        self.bg = None;
        self.bg_name.clear();
        self.bg_color = [0, 0, 0, 255];
        self.charas.clear();
        self.face = None;
        self.frozen = None;
        self.fade = None;
        self.window = false;
        self.message = None;
        self.wait = Wait::None;
    }

    // ---- running the scenario ----

    fn goto(&mut self, id: SceneId) {
        self.trace(&format!("goto {}{:04}_{}", id.0, id.1, id.2));
        if id.0 == 'Z' || self.load_act(id.0).is_err() {
            self.back_to_title();
            return;
        }
        match self.act.scenes.get(&id) {
            Some(&at) => {
                self.pc = at;
                self.entering = true;
            }
            None => self.back_to_title(),
        }
    }

    fn back_to_title(&mut self) {
        self.trace("the scenario ends");
        self.stop_voice();
        self.sounds.push(stop_effects());
        self.open_title();
    }

    fn run(&mut self) -> Result<(), String> {
        for _ in 0..100_000 {
            if self.view != Screen::Game || !matches!(self.wait, Wait::None) {
                return Ok(());
            }
            let Some(line) = self.act.lines.get(self.pc).cloned() else {
                let next = self.next;
                self.goto(next);
                continue;
            };
            match line {
                Line::Scene { id, next } => {
                    if self.entering {
                        self.entering = false;
                        self.scene = id;
                        self.next = next;
                        self.message_pc = self.pc;
                        self.message_enters = true;
                        self.pc += 1;
                    } else {
                        let n = self.next;
                        self.goto(n);
                    }
                }
                Line::Select {
                    id,
                    question,
                    answers,
                } => {
                    if !self.entering {
                        let n = self.next;
                        self.goto(n);
                        continue;
                    }
                    self.entering = false;
                    self.scene = id;
                    self.present(0);
                    self.message_pc = self.pc;
                    self.message_enters = true;
                    self.show_message(None, &question);
                    self.window = true;
                    self.wait = Wait::Choice(answers);
                }
                Line::Command(c) => {
                    self.trace(&c);
                    self.pc += 1;
                    self.command(&c);
                }
                Line::Message {
                    name,
                    voice,
                    voiced,
                    text,
                } => {
                    self.present(0);
                    self.message_pc = self.pc;
                    self.message_enters = false;
                    self.pc += 1;
                    if voice > 0
                        && let Some(n) = &voiced
                    {
                        self.play_voice(n, voice);
                    }
                    self.show_message(name.as_deref(), &text);
                    self.window = true;
                    self.wait = Wait::Click;
                }
            }
        }
        Err("Interheart: the scenario runs on without stopping".into())
    }

    fn flag(&self, n: i32) -> i32 {
        self.flags.get(&n).copied().unwrap_or(0)
    }

    fn effect_ms(&self) -> u64 {
        match self.flags.get(&41) {
            Some(&v) if v > 0 => v as u64,
            Some(_) => 0,
            None => 500,
        }
    }

    fn command(&mut self, c: &str) {
        let parts: Vec<&str> = c.split('_').collect();
        let num = |i: usize| parts.get(i).and_then(|s| s.parse::<i32>().ok());
        match parts[0] {
            "EF" => self.effect(&parts),
            "CG" | "DG" => {
                let Some(name) = parts.get(1) else { return };
                if name.starts_with("KG") {
                    let pos = num(2);
                    if parts[0] == "DG" {
                        self.freeze();
                        self.charas
                            .retain(|ch| !(pos.map_or(&ch.name == name, |p| ch.slot == p)));
                    } else {
                        self.show_chara(name, pos.unwrap_or(1));
                    }
                } else if name.starts_with("BG") || name.starts_with("HG") {
                    self.freeze();
                    self.set_bg(name);
                }
            }
            "KM" => {
                if let (Some(a), Some(b)) = (num(1), num(2)) {
                    self.move_chara(a, b);
                }
            }
            "FCG" => {
                self.freeze();
                match parts.get(1) {
                    Some(&"OFF") | None => self.face = None,
                    Some(n) => self.face = self.find_pic(n).map(|i| (n.to_string(), i)),
                }
            }
            "BG" => {
                let Some(name) = parts.get(1) else { return };
                match parts.get(2).copied() {
                    Some("FOUT") => self.stop_bgm(1000),
                    Some("STOP") => self.stop_bgm(0),
                    _ => self.play_bgm(name),
                }
            }
            "SE" => {
                let Some(name) = parts.get(1) else { return };
                match parts.get(2).copied() {
                    Some("FOUT") => self.stop_effect(0, 500),
                    Some("STOP") | Some("FIN") => self.stop_effect(0, 0),
                    _ => self.play_effect(0, name, false),
                }
            }
            "BGCV" => match parts.get(1) {
                Some(&"OFF") | None => self.stop_effect(1, 300),
                Some(n) => {
                    let n = n.to_string();
                    self.play_effect(1, &n, true)
                }
            },
            "FC"
                // FC_EQU_F004_1
                if parts.get(1) == Some(&"EQU") => {
                    let f = parts
                        .get(2)
                        .and_then(|s| s.trim_start_matches('F').parse().ok())
                        .unwrap_or(0);
                    self.equal = self.flag(f) == num(3).unwrap_or(0);
                }
            _ => {}
        }
    }

    fn effect(&mut self, parts: &[&str]) {
        let num = |i: usize| parts.get(i).and_then(|s| s.parse::<i32>().ok());
        let Some(&what) = parts.get(1) else { return };
        match what {
            "FLAG" => {
                if let (Some(n), Some(v)) = (num(2), num(3)) {
                    self.flags.insert(n, v);
                }
            }
            "WAIT" => {
                let ms = num(2).unwrap_or(0).unsigned_abs() as u64;
                self.present(0);
                self.wait = Wait::Until(self.now + ms);
            }
            "NOP" => {
                self.present(self.effect_ms());
                self.wait = Wait::Moves;
            }
            "BLACK" | "WHITE" => {
                self.freeze();
                self.clear_to(if what == "BLACK" { 0 } else { 255 });
                self.present(0);
            }
            "FOUT" | "WOUT" => {
                self.freeze();
                self.clear_to(if what == "FOUT" { 0 } else { 255 });
                self.present(self.effect_ms().max(300));
                self.wait = Wait::Fade;
            }
            "SKIP" => {
                let n = self.next;
                self.goto(n);
            }
            "JUMP" => {
                // -1: no jump, the scene goes on to its next one
                let to = if self.equal { num(3) } else { num(4) };
                if let Some(to) = to.filter(|t| *t >= 0) {
                    let id = (self.scene.0, self.scene.1, to as u32);
                    self.goto(id);
                }
            }
            "NEAR" => {
                let to = if self.flag(num(2).unwrap_or(0)) == 0 {
                    num(3)
                } else {
                    num(4)
                };
                if let Some(at) = to.and_then(|t| self.act.local.get(&(t as u32)).copied()) {
                    self.pc = at;
                }
            }
            "C0030" => {
                self.freeze();
                self.charas.clear();
            }
            "C0031" => self.stop_voice(),
            "C0060" => self.window = false,
            "C0061" => self.window = true,
            w if w.starts_with('B') && w.len() == 5 => {
                if let Some(name) = parts.get(2) {
                    self.freeze();
                    self.set_bg(name);
                    self.present(self.effect_ms());
                    self.wait = Wait::Fade;
                }
            }
            _ => {}
        }
    }

    fn clear_to(&mut self, v: u8) {
        self.bg = None;
        self.bg_name = if v == 0 { "#black" } else { "#white" }.into();
        self.bg_color = [v, v, v, 255];
        self.charas.clear();
        self.face = None;
    }

    fn set_bg(&mut self, name: &str) {
        // an E-mote picture (name.EMT): its still (altfile=) if it has one, else the picture
        // before it stays
        if let Some(b) = self.arc.read(self.files.as_mut(), &format!("{name}.EMT")) {
            let alt = text_of(&b).lines().find_map(|l| {
                l.trim()
                    .strip_prefix("altfile=")
                    .map(|f| f.trim().to_string())
            });
            if let Some(img) = alt.and_then(|f| self.picture(&f)) {
                self.bg = Some(img);
                self.bg_name = name.to_string();
            }
            return;
        }
        match self.find_pic(name) {
            Some(img) => {
                self.bg = Some(img);
                self.bg_name = name.to_string();
                self.bg_color = [0, 0, 0, 255];
            }
            None => {
                self.trace(&format!("no picture {name}"));
                // the solid ones: BG999999 black, BG99998 white and the like
                let white = name.ends_with("998") || name.ends_with("WHITE");
                self.bg = None;
                self.bg_name = name.to_string();
                self.bg_color = if white { [255; 4] } else { [0, 0, 0, 255] };
            }
        }
    }

    fn position(&self, slot: i32) -> (f32, f32) {
        if slot >= 900 {
            return (self.flag(slot) as f32, self.flag(slot + 1) as f32);
        }
        let x = POSITIONS.get(slot.max(0) as usize).copied().unwrap_or(640);
        (x as f32, 0.0)
    }

    fn show_chara(&mut self, name: &str, slot: i32) {
        let Some(image) = self.find_pic(name) else {
            self.trace(&format!("no picture {name}"));
            return;
        };
        self.freeze();
        let at = self.position(slot);
        let now = self.now;
        if let Some(c) = self.charas.iter_mut().find(|c| c.slot == slot) {
            c.name = name.to_string();
            c.image = image;
        } else {
            self.charas.push(Chara {
                slot,
                name: name.to_string(),
                image,
                from: at,
                to: at,
                start: now,
                ms: 0,
            });
        }
    }

    fn move_chara(&mut self, a: i32, b: i32) {
        let to = self.position(b);
        let ms = self.flags.get(&44).copied().unwrap_or(300).max(0) as u64;
        let now = self.now;
        self.present(0);
        self.charas.retain(|c| c.slot != b || a == b);
        if let Some(c) = self.charas.iter_mut().find(|c| c.slot == a) {
            c.from = c.at(now);
            c.to = to;
            c.start = now;
            c.ms = ms;
            c.slot = b;
        }
    }

    // ---- what is on show ----

    /// Keeps the screen as it is until [`Engine::present`], for the changes that follow.
    fn freeze(&mut self) {
        if self.frozen.is_none() {
            self.fade = None;
            self.frozen = Some(self.stage_draws());
        }
    }

    /// Shows the changes since [`Engine::freeze`], fading over `ms`.
    fn present(&mut self, ms: u64) {
        if let Some(from) = self.frozen.take()
            && ms > 0
        {
            self.fade = Some(Fade {
                from,
                start: self.now,
                ms,
            });
        }
    }

    fn solid(&self, rgba: [u8; 4]) -> Draw {
        let img = white(1, 1, rgba);
        let mut d = draw(&img, 0.0, 0.0, 1.0);
        d.dst = [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32];
        d
    }

    fn stage_draws(&self) -> Vec<Draw> {
        let mut out = vec![self.solid(self.bg_color)];
        if let Some(bg) = &self.bg {
            out.push(draw(bg, 0.0, 0.0, 1.0));
        }
        let mut charas: Vec<&Chara> = self.charas.iter().collect();
        charas.sort_by_key(|c| c.slot == 1);
        for c in charas {
            let (x, y) = c.at(self.now);
            let left = x - c.image.w as f32 / 2.0;
            let top = self.screen.1 as f32 - c.image.h as f32 + y;
            out.push(draw(&c.image, left, top, 1.0));
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
        if let Some(f) = &self.fade {
            let p = (self.now.saturating_sub(f.start) as f32 / f.ms.max(1) as f32).min(1.0);
            for d in &f.from {
                let mut d = d.clone();
                d.place.alpha *= 1.0 - p;
                out.push(d);
            }
        }
        match self.view {
            Screen::Game
            | Screen::Menu
            | Screen::Slots {
                from_title: false, ..
            } => {
                if self.window && self.fade.is_none() {
                    self.window_draws(&mut out);
                }
                if let Wait::Choice(answers) = &self.wait {
                    let answers = answers.clone();
                    self.choice_draws(&answers, &mut out);
                }
            }
            _ => {}
        }
        match self.view {
            Screen::Menu => self.menu_draws(&mut out),
            Screen::Slots { save, .. } => self.slot_draws(save, &mut out),
            _ => {}
        }
        out
    }

    /// The title button at (x, y), as the hover map numbers them (0: none).
    fn title_hit(&self, x: i32, y: i32) -> u8 {
        let Some(t) = &self.title else { return 0 };
        if t.classic {
            return t.map.as_ref().map_or(0, |m| m.at(x, y));
        }
        t.buttons
            .iter()
            .find(|(_, r)| inside(*r, x, y))
            .map_or(0, |b| b.0)
    }

    fn title_draws(&mut self) -> Vec<Draw> {
        let mut out = vec![self.solid([0, 0, 0, 255])];
        let Some(t) = &self.title else { return out };
        if let Some(bg) = &t.bg {
            out.push(draw(bg, 0.0, 0.0, 1.0));
        }
        if !t.classic {
            let buttons = t.buttons.clone();
            for (v, r) in buttons {
                let label = PLAIN_TITLE.iter().find(|b| b.0 == v).map_or("", |b| b.1);
                self.button(label, r, &mut out);
            }
            return out;
        }
        let hover = self.title_hit(self.pointer.0, self.pointer.1);
        for &(v, r) in &t.buttons {
            let state = if v == hover { 1 } else { 0 };
            match &t.chip {
                Some(chip) if v as usize <= TITLE_CELLS.len() => {
                    let cell = TITLE_CELLS[v as usize - 1];
                    let (cw, ch) = (324, 52);
                    let sx = (cell / 6) * cw;
                    let sy = ((cell % 6) * 3 + state) * ch;
                    out.push(part(chip, [sx, sy, cw, ch], r[0] as f32, r[1] as f32));
                }
                _ => {
                    let img = white(
                        1,
                        1,
                        if state == 1 {
                            [80, 80, 160, 200]
                        } else {
                            [0, 0, 0, 160]
                        },
                    );
                    let mut d = draw(&img, 0.0, 0.0, 1.0);
                    d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
                    out.push(d);
                }
            }
        }
        out
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let top = h - 160.0;
        if self.meschip.is_none() {
            let chip = self.find_pic("MesChip").filter(|c| classic_window_chip(c));
            self.meschip = Some(chip);
        }
        match self.meschip.clone().flatten() {
            Some(chip) if chip.w >= 1024 => {
                out.push(scaled(&chip, [0, 0, 1024, 160], [0.0, top, w, h]));
                if self.message.as_ref().is_some_and(|m| m.name.is_some()) {
                    out.push(part(&chip, [256, 160, 262, 46], 0.0, top - 40.0));
                }
            }
            _ => {
                let img = white(1, 1, [0, 0, 32, 180]);
                let mut d = draw(&img, 0.0, 0.0, 1.0);
                d.dst = [0.0, top, w, h];
                out.push(d.clone());
                if self.message.as_ref().is_some_and(|m| m.name.is_some()) {
                    d.dst = [0.0, top - 44.0, 300.0, top];
                    out.push(d);
                }
            }
        }
        if let Some((_, face)) = &self.face {
            out.push(draw(face, 0.0, h - face.h as f32, 1.0));
        }
        let left = if self.face.is_some() { 220.0 } else { 120.0 };
        if let Some(m) = &self.message {
            if let Some(n) = &m.name {
                out.push(draw(n, 24.0, top - 36.0, 1.0));
            }
            for (i, l) in m.lines.iter().enumerate() {
                out.push(draw(
                    l,
                    left,
                    top + 18.0 + (i as i32 * LINE_STEP) as f32,
                    1.0,
                ));
            }
        }
    }

    fn choice_boxes(&mut self, n: usize) -> Vec<[i32; 4]> {
        if self.select_map.is_none() {
            let m = self
                .arc
                .read(self.files.as_mut(), "selmask.hmp")
                .and_then(|b| HoverMap::parse(&b));
            self.select_map = Some(m);
        }
        let from_map: Vec<[i32; 4]> = self
            .select_map
            .as_ref()
            .and_then(|m| m.as_ref())
            .map(|m| {
                m.boxes()
                    .into_iter()
                    .filter(|(v, r)| *v != 254 && r[2] - r[0] > 400)
                    .map(|(_, r)| r)
                    .collect()
            })
            .unwrap_or_default();
        if from_map.len() >= n {
            return from_map[..n].to_vec();
        }
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        let top = (h - 160) / 2 - (n as i32 * 70) / 2;
        (0..n as i32)
            .map(|i| [w / 2 - 370, top + i * 70, w / 2 + 370, top + i * 70 + 52])
            .collect()
    }

    fn choice_draws(&mut self, answers: &[(String, SceneId)], out: &mut Vec<Draw>) {
        let boxes = self.choice_boxes(answers.len());
        let chip = self.meschip.clone().flatten();
        for (r, (text, _)) in boxes.iter().zip(answers) {
            let lit = inside(*r, self.pointer.0, self.pointer.1);
            let dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
            match &chip {
                // the bars of MesChip.kg: plain, then lit
                Some(c) => {
                    let y = if lit { 485 } else { 437 };
                    out.push(scaled(c, [0, y, 844, 47], dst));
                }
                None => {
                    let img = white(
                        1,
                        1,
                        if lit {
                            [90, 90, 200, 220]
                        } else {
                            [0, 0, 0, 170]
                        },
                    );
                    let mut d = draw(&img, 0.0, 0.0, 1.0);
                    d.dst = dst;
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
        let items = ["セーブ", "ロード", "タイトルに戻る", "戻る"];
        let top = h / 2 - 2 * 70;
        items
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let y = top + i as i32 * 70;
                (*t, [w / 2 - 200, y, w / 2 + 200, y + 56])
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
        let img = white(
            1,
            1,
            if lit {
                [90, 90, 200, 230]
            } else {
                [20, 20, 40, 220]
            },
        );
        let mut d = draw(&img, 0.0, 0.0, 1.0);
        d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        out.push(d);
        let t = self.text(text, [255, 255, 255], TEXT_SIZE);
        let x = (r[0] + r[2]) as f32 / 2.0 - t.w as f32 / 2.0;
        let y = (r[1] + r[3]) as f32 / 2.0 - t.h as f32 / 2.0;
        out.push(draw(&t, x, y, 1.0));
    }

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        let (bw, bh) = (w / 2 - 80, 96);
        let top = (h - 5 * (bh + 16)) / 2 + 30;
        (0..SLOTS as i32)
            .map(|i| {
                let x = if i < 5 { 40 } else { w / 2 + 40 };
                let y = top + (i % 5) * (bh + 16);
                [x, y, x + bw, y + bh]
            })
            .collect()
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        out.push(self.solid([0, 0, 24, 220]));
        let head = self.text(if save { "セーブ" } else { "ロード" }, [255, 255, 255], 36);
        out.push(draw(&head, 40.0, 24.0, 1.0));
        let hint = self.text("右クリックで戻る", [200, 200, 200], 22);
        out.push(draw(
            &hint,
            self.screen.0 as f32 - hint.w as f32 - 40.0,
            34.0,
            1.0,
        ));
        for (i, r) in self.slot_rects().into_iter().enumerate() {
            let label = match &self.slots[i] {
                Some(s) => format!("No.{:02}  {}", i + 1, s.text),
                None => format!("No.{:02}  ----", i + 1),
            };
            let lit = inside(r, self.pointer.0, self.pointer.1);
            let img = white(
                1,
                1,
                if lit {
                    [90, 90, 200, 230]
                } else {
                    [20, 20, 40, 220]
                },
            );
            let mut d = draw(&img, 0.0, 0.0, 1.0);
            d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
            out.push(d);
            let t = self.text(&label, [255, 255, 255], 24);
            let mut d = draw(&t, (r[0] + 16) as f32, (r[1] + 30) as f32, 1.0);
            d.place.clip = [r[0] as f32, r[1] as f32, (r[2] - 8) as f32, r[3] as f32];
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

    fn show_message(&mut self, name: Option<&str>, text: &[String]) {
        let name_img = name.map(|n| self.text(n, [255, 255, 255], TEXT_SIZE));
        let mut lines = Vec::new();
        let width = self.screen.0 as i32 - 300;
        for l in text {
            for piece in wrap(l, (width / TEXT_SIZE).max(10) as usize) {
                lines.push(self.text(&piece, [255, 255, 255], TEXT_SIZE));
            }
        }
        lines.truncate(4);
        let first = match name {
            Some(n) => format!("{n} {}", text.first().map_or("", |s| s.as_str())),
            None => text.first().cloned().unwrap_or_default(),
        };
        self.message = Some(Message {
            name: name_img,
            lines,
            first: first.trim().to_string(),
        });
    }

    // ---- pictures and sounds ----

    fn picture(&mut self, file: &str) -> Option<Rc<Image>> {
        let key = file.to_lowercase();
        if let Some(i) = self.cache.get(&key) {
            return Some(i.clone());
        }
        let b = self.arc.read(self.files.as_mut(), file)?;
        let img = match pic::decode(&b) {
            Ok(i) => Rc::new(i),
            Err(e) => {
                self.trace(&format!("{file}: {e}"));
                return None;
            }
        };
        if self.cache.len() > 48 {
            self.cache.clear();
        }
        self.cache.insert(key, img.clone());
        Some(img)
    }

    /// A picture by its name in the scripts: BG100080 is BG100080.bmp, HG000070 may be
    /// HG000070a.bmp, KG010000 is KG010000.kg.
    fn find_pic(&mut self, name: &str) -> Option<Rc<Image>> {
        for ext in [".bmp", "a.bmp", ".kg", ".png", ".jpg", "a.kg"] {
            let f = format!("{name}{ext}");
            if self.arc.has(&f) {
                return self.picture(&f);
            }
        }
        None
    }

    fn sound_file(&mut self, name: &str) -> Option<Vec<u8>> {
        for ext in [".ogg", ".wav"] {
            if let Some(b) = self.arc.read(self.files.as_mut(), &format!("{name}{ext}")) {
                return Some(b);
            }
        }
        None
    }

    fn play_bgm(&mut self, name: &str) {
        if self.bgm.eq_ignore_ascii_case(name) {
            return;
        }
        let Some(data) = self.sound_file(name) else {
            self.trace(&format!("no music {name}"));
            return;
        };
        self.bgm = name.to_string();
        self.sounds.push(Sound {
            data,
            volume: 0.8,
            bgm: true,
            voice: false,
            effect_channel: None,
            stop: false,
            fade_ms: 0,
            looped: false,
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
        let data = if channel == 1 {
            ["BGCV.fpk", "bgcv.fpk"]
                .iter()
                .find_map(|p| {
                    [
                        format!("{name}.ogg"),
                        format!("BGCV{name}.ogg"),
                        format!("BGCV_{name}.ogg"),
                    ]
                    .iter()
                    .find_map(|f| self.arc.read_in(self.files.as_mut(), p, f))
                })
                .or_else(|| self.sound_file(name))
        } else {
            self.sound_file(name)
        };
        let Some(data) = data else {
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

    fn play_voice(&mut self, name: &str, voice: u32) {
        let Some(k) = self.chara_ids.iter().position(|n| n == name) else {
            return;
        };
        let file = format!("{voice:04}.ogg");
        let prefix = format!("cv{:02}", k + 1);
        let packs: Vec<String> = self
            .arc
            .packs
            .iter()
            .map(|(p, _)| p.clone())
            .filter(|p| {
                let l = p.to_lowercase();
                l == format!("{prefix}.fpk") || l.starts_with(&format!("{prefix}_"))
            })
            .collect();
        let data = packs
            .iter()
            .find_map(|p| self.arc.read_in(self.files.as_mut(), p, &file));
        let Some(data) = data else {
            self.trace(&format!("no voice {prefix}/{file}"));
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
            Screen::Logo(_) => self.open_title(),
            Screen::Title => self.title_click(x, y),
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
                        self.stop_voice();
                        self.sounds.push(stop_effects());
                        self.open_title();
                    }
                    Some(_) => self.view = Screen::Game,
                    None => {}
                }
            }
            Screen::Slots { save, from_title } => {
                let Some(i) = self.slot_rects().into_iter().position(|r| inside(r, x, y)) else {
                    return;
                };
                if save {
                    self.save(i);
                    self.view = Screen::Game;
                } else if self.slots[i].is_some() {
                    self.load(i);
                } else if from_title {
                    // nothing there
                }
            }
            Screen::Game => self.game_click(x, y),
        }
    }

    fn title_click(&mut self, x: i32, y: i32) {
        let v = self.title_hit(x, y);
        match v {
            1 => self.new_game(),
            2 => {
                self.view = Screen::Slots {
                    save: false,
                    from_title: true,
                }
            }
            3 => {
                if let Some(i) = self.latest_slot() {
                    self.load(i);
                }
            }
            6 => self.ended = true,
            _ => {}
        }
    }

    fn new_game(&mut self) {
        self.reset_stage();
        self.flags.clear();
        self.stop_bgm(500);
        self.view = Screen::Game;
        self.load_act('A').ok();
        self.pc = 0;
        self.entering = true;
        self.next = ('Z', 9999, 99);
    }

    fn game_click(&mut self, x: i32, y: i32) {
        match &self.wait {
            Wait::Click => {
                self.wait = Wait::None;
            }
            Wait::Until(_) | Wait::Moves => self.wait = Wait::None,
            Wait::Fade => {
                self.fade = None;
                self.wait = Wait::None;
            }
            Wait::Choice(answers) => {
                let answers = answers.clone();
                let boxes = self.choice_boxes(answers.len());
                let hit = if x < 0 {
                    None
                } else {
                    boxes.iter().position(|r| inside(*r, x, y))
                };
                if let Some(i) = hit {
                    self.wait = Wait::None;
                    self.message = None;
                    self.goto(answers[i].1);
                }
            }
            Wait::None => {}
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
            _ => {}
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

    fn latest_slot(&mut self) -> Option<usize> {
        let b = self.files.read_save("latest.txt")?;
        let i: usize = String::from_utf8_lossy(&b).trim().parse().ok()?;
        self.slots.get(i)?.as_ref().map(|_| i)
    }

    fn save(&mut self, i: usize) {
        let mut s = String::new();
        let id = |v: SceneId| format!("{},{},{}", v.0, v.1, v.2);
        s += &format!(
            "act={}\npc={}\nenter={}\n",
            self.letter, self.message_pc, self.message_enters as u8
        );
        s += &format!("scene={}\nnext={}\n", id(self.scene), id(self.next));
        s += &format!("bg={}\nwindow={}\n", self.bg_name, self.window as u8);
        for c in &self.charas {
            s += &format!("chara={},{}\n", c.slot, c.name);
        }
        if let Some((n, _)) = &self.face {
            s += &format!("face={n}\n");
        }
        s += &format!("bgm={}\n", self.bgm);
        let mut flags: Vec<_> = self.flags.iter().collect();
        flags.sort();
        for (k, v) in flags {
            s += &format!("flag={k},{v}\n");
        }
        let text = self
            .message
            .as_ref()
            .map_or(String::new(), |m| m.first.clone());
        s += &format!("text={text}\n");
        self.files.write_save(&Self::slot_name(i), s.as_bytes());
        self.files
            .write_save("latest.txt", i.to_string().as_bytes());
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
        self.flags.clear();
        self.stop_voice();
        self.sounds.push(stop_effects());
        let parse_id = |s: &str| -> Option<SceneId> {
            let v: Vec<&str> = s.split(',').collect();
            Some((
                v.first()?.chars().next()?,
                v.get(1)?.parse().ok()?,
                v.get(2)?.parse().ok()?,
            ))
        };
        let mut pc = 0;
        let mut enter = false;
        let mut bgm = String::new();
        for l in t.lines() {
            let Some((k, v)) = l.split_once('=') else {
                continue;
            };
            match k {
                "act" => {
                    if let Some(c) = v.chars().next() {
                        self.load_act(c).ok();
                    }
                }
                "pc" => pc = v.parse().unwrap_or(0),
                "enter" => enter = v == "1",
                "scene" => self.scene = parse_id(v).unwrap_or(self.scene),
                "next" => self.next = parse_id(v).unwrap_or(self.next),
                "bg" => match v {
                    "#black" | "" => self.clear_to(0),
                    "#white" => self.clear_to(255),
                    n => self.set_bg(n),
                },
                "window" => self.window = v == "1",
                "chara" => {
                    if let Some((slot, name)) = v.split_once(',') {
                        let slot = slot.parse().unwrap_or(1);
                        self.show_chara(name, slot);
                    }
                }
                "face" => self.face = self.find_pic(v).map(|i| (v.to_string(), i)),
                "bgm" => bgm = v.to_string(),
                "flag" => {
                    if let Some((a, b)) = v.split_once(',')
                        && let (Ok(a), Ok(b)) = (a.parse(), b.parse())
                    {
                        self.flags.insert(a, b);
                    }
                }
                _ => {}
            }
        }
        self.frozen = None;
        self.bgm.clear();
        if bgm.is_empty() {
            self.stop_bgm(300);
        } else {
            self.play_bgm(&bgm);
        }
        self.pc = pc;
        self.entering = enter;
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

/// Which cell of titlechip.kg (by column, then by its row of three) each title button of the
/// hover map shows: 1 from the start, 2 load, 3 continue from the last save, 4 settings,
/// 5 reset, 6 quit, 7 CG gallery, 8 scene gallery.
const TITLE_CELLS: [i32; 8] = [1, 2, 0, 3, 4, 5, 6, 7];

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

/// Whether titlechip.kg is the usual sheet: two columns of 324 x 52 cells, a button in three
/// looks (plain, lit, pressed) in each of 6 rows of three, with no gaps between them.
fn classic_title_chip(c: &Image) -> bool {
    if c.w < 648 || c.h < 936 {
        return false;
    }
    (0..936)
        .step_by(2)
        .all(|y| (0..c.w).any(|x| c.px[((y * c.w + x) * 4 + 3) as usize] != 0))
}

/// Whether MesChip.kg is the usual sheet: the window (1024 x 160, opaque to its left edge) at
/// the top and the bars of the choices at 437 and 485.
fn classic_window_chip(c: &Image) -> bool {
    let a = |x: u32, y: u32| c.px[((y * c.w + x) * 4 + 3) as usize];
    c.w >= 1024 && c.h >= 580 && a(4, 80) > 0 && a(4, 150) > 0 && a(1000, 150) > 0 && a(10, 460) > 0
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

/// Part of a picture (`src`: x, y, w, h) stretched over `dst` (x0, y0, x1, y1).
fn scaled(image: &Rc<Image>, src: [i32; 4], dst: [f32; 4]) -> Draw {
    let mut d = draw(image, 0.0, 0.0, 1.0);
    d.src = [src[0], src[1], src[0] + src[2], src[1] + src[3]];
    d.dst = dst;
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
