//! Runs the scripts: a frame at a time, it carries out commands until one waits (for a click,
//! a time, or a transition), and says what to draw and play.
//!
//! The commands come in groups (the first number of a command): 0x00 text, 0x10 flow
//! (labels, jumps, calls, choices, buttons), 0x20 pictures, 0x30 sound, 0x40 system menus
//! and 0x50 variables and waits. What each one does was worked out from the scripts of the
//! 2010 and 2023 trials and the game's own command handlers.

use std::collections::HashMap;
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::grd;
use crate::ini::{self, Ini};
use crate::pac::Index;
use crate::save::{SaveData, SavedChara};
use crate::script::{Command, OLD, Script};

mod backlog;
mod old;

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
    /// where a looping sound starts again, in samples
    pub loop_start: u32,
}

/// A picture placed on the screen.
#[derive(Clone)]
struct Pic {
    image: Rc<Image>,
    x: f32,
    y: f32,
}

/// Something the game is waiting for.
enum Wait {
    None,
    Click,
    Until(u64),
    Choice(Vec<(String, String)>),
    Buttons,
    Ended,
}

struct Button {
    rect: [i32; 4],
    label: String,
    /// a map button: its column in mapselect_charactor and its row in mapselect_plate
    icon: Option<[i32; 2]>,
}

struct Fade {
    from: Vec<Draw>,
    start: u64,
    ms: u64,
}

/// A character in one of the sprite slots, moving from `from` to `to` over `ms`.
struct Chara {
    name: String,
    pics: Vec<Pic>,
    from: (f32, f32),
    to: (f32, f32),
    start: u64,
    ms: u64,
    /// fades in (`Some(true)`) or out and goes away (`Some(false)`) while it moves
    fade: Option<bool>,
}

impl Chara {
    fn progress(&self, now: u64) -> f32 {
        if self.ms == 0 {
            1.0
        } else {
            (now.saturating_sub(self.start) as f32 / self.ms as f32).min(1.0)
        }
    }
}

/// A flash of colour: in, held, out (ms).
struct Flash {
    image: Rc<Image>,
    start: u64,
    ms: [u64; 3],
}

/// The save or load screen.
struct Menu {
    save: bool,
    page: usize,
    /// the screen when it opened (saved with the slot), small
    thumb: (u32, u32, Vec<u8>),
    /// the slots of the page: the picture and the lines of text of each saved one
    slots: Vec<Option<SlotView>>,
}

struct MenuLayout {
    at: [i32; 2],
    src: Option<[i32; 2]>,
    size: [i32; 2],
    count: usize,
    rows: usize,
    step: [i32; 2],
    column: [i32; 2],
}

struct SlotView {
    thumb: Rc<Image>,
    lines: Vec<Rc<Image>>,
}

/// A button of the message window (MESSAGEWINDOW TWB_*): what it does, where it is and where its
/// picture is in sys_window_button (normal, then lit one row below).
struct WindowButton {
    key: &'static str,
    rect: [i32; 4],
    src: [i32; 2],
}

const WINDOW_BUTTONS: [&str; 14] = [
    "CLOSE",
    "AUTO",
    "SKIP",
    "BACKLOG",
    "SYSTEM",
    "SYSTEM2",
    "VOICE",
    "SAVE",
    "LOAD",
    "QSAVE",
    "QLOAD",
    "TITLE",
    "EXIT",
    "SCENE_BACK",
];

struct Pan {
    from: (f32, f32),
    to: (f32, f32),
    start: u64,
    ms: u64,
}

pub struct Engine {
    files: Box<dyn Files>,
    packs: Vec<(String, String, Index)>,
    ini: Ini,
    macros: HashMap<String, Vec<String>>,
    pub screen: (u32, u32),
    scripts: HashMap<String, Rc<Script>>,
    script: Rc<Script>,
    pc: usize,
    stack: Vec<(Rc<Script>, usize)>,
    vars: HashMap<(u8, u16), i32>,
    now: u64,
    wait: Wait,
    bg: Vec<Pic>,
    bg_name: String,
    bgm: String,
    /// the scene's title (50.0015), the last voice and the command to run again for a save
    title: String,
    voice: String,
    resume_pc: usize,
    menu: Option<Menu>,
    backlog: Vec<backlog::LogLine>,
    log_view: Option<backlog::LogView>,
    /// the message window is put away (right click or its close button)
    hidden: bool,
    auto: bool,
    skip: bool,
    said_at: u64,
    /// the window's picture and where it goes from the window's place
    window_base: Option<Option<(Rc<Image>, i32, i32)>>,
    /// a picture over the characters (layer 11 of the older games: the calendar)
    overlay: Vec<Pic>,
    pan: Option<Pan>,
    /// the sprite slots (picture layers 5 and up)
    chars: Vec<Option<Chara>>,
    flash: Option<Flash>,
    /// when the screen started shaking
    shake: Option<u64>,
    fade: Option<Fade>,
    window: bool,
    name: String,
    text: String,
    text_images: Vec<(Rc<Image>, f32, f32)>,
    buttons: Vec<Button>,
    button_images: (String, String),
    hover: (i32, i32),
    sounds: Vec<Sound>,
    font: Option<Rc<fontdue::Font>>,
    images: HashMap<String, Option<Rc<grd::Grd>>>,
    rng: u32,
    pub log: bool,
}

const MS_PER_FRAME: u64 = 1000 / 60;
const SHAKE_MS: u64 = 340;

impl Engine {
    pub fn new(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let mut packs = Vec::new();
        for name in files.list() {
            let Some(stem) = name
                .strip_suffix(".pac")
                .or_else(|| name.strip_suffix(".PAC"))
            else {
                continue;
            };
            let Some(head) = files.read_range(&name, 0, 7) else {
                continue;
            };
            let Some(n) = Index::index_size(&head) else {
                continue;
            };
            let Some(b) = files.read_range(&name, 0, n) else {
                continue;
            };
            match Index::parse(&b) {
                Ok(index) => packs.push((stem.to_lowercase(), name.clone(), index)),
                Err(e) => eprintln!("{name}: {e}"),
            }
        }
        if !packs.iter().any(|p| p.0 == "srp") {
            return Err("srp.pac is missing".into());
        }
        let mut e = Engine {
            files,
            packs,
            ini: Ini::default(),
            macros: HashMap::new(),
            screen: (800, 600),
            scripts: HashMap::new(),
            script: Rc::new(Script {
                name: String::new(),
                commands: Vec::new(),
            }),
            pc: 0,
            stack: Vec::new(),
            vars: HashMap::new(),
            now: 0,
            wait: Wait::None,
            bg: Vec::new(),
            bg_name: String::new(),
            bgm: String::new(),
            title: String::new(),
            voice: String::new(),
            resume_pc: 0,
            menu: None,
            backlog: Vec::new(),
            log_view: None,
            hidden: false,
            auto: false,
            skip: false,
            said_at: 0,
            window_base: None,
            overlay: Vec::new(),
            pan: None,
            chars: (0..10).map(|_| None).collect(),
            flash: None,
            shake: None,
            fade: None,
            window: false,
            name: String::new(),
            text: String::new(),
            text_images: Vec::new(),
            buttons: Vec::new(),
            button_images: (String::new(), String::new()),
            hover: (-1, -1),
            sounds: Vec::new(),
            font: None,
            images: HashMap::new(),
            rng: 0x1234_5678,
            log: false,
        };
        for name in ["_system", "_charaview"] {
            if let Some(b) = e.read("srp", name) {
                e.ini.add(&b);
            }
        }
        for name in ["_filemacro", "_charafilemacro"] {
            for line in e
                .read("srp", name)
                .map(|b| ini::lines(&b))
                .unwrap_or_default()
            {
                if let Some((k, v)) = line.split_once('=') {
                    let parts = v.split('/').map(|s| s.trim().to_string()).collect();
                    e.macros.insert(k.trim().to_lowercase(), parts);
                }
            }
        }
        e.screen = (
            e.ini.int("GAMESYSTEM/DEF_WINDOW_WIDTH", 800) as u32,
            e.ini.int("GAMESYSTEM/DEF_WINDOW_HEIGHT", 600) as u32,
        );
        e.script = e.load("start")?;
        if e.script.commands.first().is_some_and(|c| c.group == OLD) {
            e.ini.add(old::SETTINGS.trim_start().as_bytes());
            let exe = e
                .files
                .list()
                .into_iter()
                .find(|n| n.to_lowercase().ends_with(".exe"));
            let names = exe
                .and_then(|n| e.files.read(&n))
                .and_then(|b| old::player_names(&b));
            if let Some([given, kana, short]) = names {
                let text = format!(
                    "[GAMESYSTEM]\nPLAYER_NAME01={given}\nPLAYER_NAME02={kana}\nPLAYER_NAME03={short}\n"
                );
                e.ini.add(&nexas::sjis::encode(&text));
            }
        }
        Ok(e)
    }

    /// The TrueType font for text (the games use a Windows font).
    pub fn set_font(&mut self, b: &[u8]) -> Result<(), String> {
        let font = fontdue::Font::from_bytes(b, fontdue::FontSettings::default())?;
        self.font = Some(Rc::new(font));
        Ok(())
    }

    fn read(&mut self, pack: &str, name: &str) -> Option<Vec<u8>> {
        let (_, file, index) = self.packs.iter().find(|p| p.0 == pack)?;
        let e = index.find(name)?;
        let file = file.clone();
        self.files.read_range(&file, e.offset, e.size as usize)
    }

    /// A file from the first archive (by name prefix, such as "grd") that has it.
    fn find(&mut self, kinds: &[&str], name: &str) -> Option<Vec<u8>> {
        let packs: Vec<String> = kinds
            .iter()
            .flat_map(|k| {
                self.packs
                    .iter()
                    .filter(move |p| p.0.contains(k))
                    .map(|p| p.0.clone())
            })
            .collect();
        packs.iter().find_map(|p| self.read(p, name))
    }

    fn load(&mut self, name: &str) -> Result<Rc<Script>, String> {
        let key = name.to_lowercase();
        if let Some(s) = self.scripts.get(&key) {
            return Ok(s.clone());
        }
        let b = self
            .read("srp", name)
            .ok_or_else(|| format!("script {name} is missing"))?;
        let s = Rc::new(Script::parse(name, &b)?);
        self.scripts.insert(key, s.clone());
        Ok(s)
    }

    fn image(&mut self, name: &str) -> Option<Rc<grd::Grd>> {
        let key = name.to_lowercase();
        if let Some(g) = self.images.get(&key) {
            return g.clone();
        }
        let g = self
            .find(&["grd"], name)
            .and_then(|b| grd::decode(&b).map_err(|e| eprintln!("{name}: {e}")).ok());
        if g.is_none() && self.log {
            eprintln!("tmrhiro: no picture {name}");
        }
        let g = g.map(Rc::new);
        self.images.insert(key, g.clone());
        g
    }

    /// The pictures of a name: a file macro (`fm_...`, layers joined by `/`) or one picture,
    /// each placed on its canvas at (`x`, `y`).
    fn pictures(&mut self, name: &str, x: f32, y: f32) -> Vec<Pic> {
        let parts = self
            .macros
            .get(&name.to_lowercase())
            .cloned()
            .unwrap_or_else(|| vec![name.to_string()]);
        let mut out = Vec::new();
        for part in parts {
            if part.is_empty() || part == "-" {
                continue;
            }
            // black and white screens
            if let Some(rgb) = match part.to_lowercase().as_str() {
                "bl" => Some([0, 0, 0]),
                "wh" => Some([255, 255, 255]),
                _ => None,
            } {
                let mut img = Image::new(self.screen.0, self.screen.1);
                img.fill([rgb[0], rgb[1], rgb[2], 255]);
                out.push(Pic {
                    image: Rc::new(img),
                    x: 0.0,
                    y: 0.0,
                });
                continue;
            }
            if let Some(g) = self.image(&part) {
                out.push(Pic {
                    image: g.image.clone(),
                    x: x + g.x as f32,
                    y: y + g.y as f32,
                });
            }
        }
        out
    }

    fn var(&self, bank: u8, n: u16) -> i32 {
        self.vars.get(&(bank, n)).copied().unwrap_or(0)
    }

    fn random(&mut self, n: u32) -> i32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng % n.max(1)) as i32
    }

    pub fn ended(&self) -> bool {
        matches!(self.wait, Wait::Ended)
    }

    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    /// The pointer moved (buttons light up under it).
    pub fn point(&mut self, x: i32, y: i32) {
        self.hover = (x, y);
    }

    /// A click at (x, y), or (-1, -1) for the keyboard.
    pub fn click(&mut self, x: i32, y: i32) {
        if self.menu.is_some() {
            self.menu_click(x, y);
            return;
        }
        if self.log_open() {
            self.log_click(x, y);
            return;
        }
        if self.fade.take().is_some() {
            return;
        }
        if self.hidden {
            self.hidden = false;
            return;
        }
        if self.auto || self.skip {
            self.auto = false;
            self.skip = false;
            return;
        }
        if let Some(key) = self.window_button_at(x, y) {
            self.window_button(key);
            return;
        }
        match &self.wait {
            Wait::Click | Wait::Until(_) => self.wait = Wait::None,
            Wait::Choice(options) => {
                let options = options.clone();
                if let Some(i) = self.choice_at(options.len(), x, y) {
                    let label = options[i].1.clone();
                    self.wait = Wait::None;
                    self.window = false;
                    self.jump_label(&label);
                }
            }
            Wait::Buttons => {
                let hit = self.buttons.iter().find(|b| {
                    x >= b.rect[0]
                        && y >= b.rect[1]
                        && x < b.rect[0] + b.rect[2]
                        && y < b.rect[1] + b.rect[3]
                });
                if let Some(b) = hit {
                    let label = b.label.clone();
                    self.buttons.clear();
                    self.wait = Wait::None;
                    self.jump_label(&label);
                }
            }
            _ => {}
        }
    }

    /// A right click: closes the save or load screen, or puts the message window away (games
    /// without window buttons open the save screen instead).
    pub fn cancel(&mut self) {
        if self.menu.take().is_some() || self.log_view.take().is_some() {
            return;
        }
        if !matches!(self.wait, Wait::Click | Wait::Choice(_)) {
            return;
        }
        if self.window_buttons().is_empty() {
            self.open_menu(true);
        } else {
            self.hidden = !self.hidden;
        }
    }

    fn jump_label(&mut self, label: &str) {
        match self.script.label(label) {
            Some(pc) => self.pc = pc,
            None => eprintln!("{}: no label {label}", self.script.name),
        }
    }

    /// Starts the script of a name from its beginning (for trying out a scene).
    pub fn start_at(&mut self, name: &str) {
        self.stack.clear();
        self.jump_file(name);
    }

    fn jump_file(&mut self, name: &str) {
        match self.load(name) {
            Ok(s) => {
                self.script = s;
                self.pc = 0;
            }
            Err(e) => {
                eprintln!("{e}");
                self.wait = Wait::Ended;
            }
        }
    }

    /// Runs one frame: commands until one waits.
    pub fn frame(&mut self) -> Result<(), String> {
        self.now += MS_PER_FRAME;
        if self.menu.is_some() || self.log_open() {
            return Ok(());
        }
        if let Wait::Click = self.wait {
            let chars = self.text.chars().count() as u64;
            if self.skip || (self.auto && self.now >= self.said_at + 1000 + 60 * chars) {
                self.wait = Wait::None;
            }
        }
        if self.skip {
            self.fade = None;
        }
        if self
            .fade
            .as_ref()
            .is_some_and(|f| self.now >= f.start + f.ms)
        {
            self.fade = None;
        }
        for c in self.chars.iter_mut() {
            if c.as_ref()
                .is_some_and(|c| c.fade == Some(false) && c.progress(self.now) >= 1.0)
            {
                *c = None;
            }
        }
        if self
            .flash
            .as_ref()
            .is_some_and(|f| self.now >= f.start + f.ms.iter().sum::<u64>())
        {
            self.flash = None;
        }
        if matches!(self.wait, Wait::Until(t) if self.now >= t) {
            self.wait = Wait::None;
        }
        let mut steps = 0;
        while matches!(self.wait, Wait::None) && self.fade.is_none() {
            steps += 1;
            if steps > 10_000 {
                return Err(format!(
                    "{}: the script does not stop at {}",
                    self.script.name, self.pc
                ));
            }
            let Some(c) = self.script.commands.get(self.pc).cloned() else {
                // the end of a script returns to its caller
                match self.stack.pop() {
                    Some((s, pc)) => {
                        self.script = s;
                        self.pc = pc;
                        continue;
                    }
                    None => {
                        self.wait = Wait::Ended;
                        break;
                    }
                }
            };
            self.pc += 1;
            if self.log {
                eprintln!(
                    "{}:{} {:02x}.{:04x} {}",
                    self.script.name,
                    self.pc - 1,
                    c.group,
                    c.op,
                    c.str(0).chars().take(60).collect::<String>()
                );
            }
            self.run(&c)?;
        }
        Ok(())
    }

    /// Starts a transition to what the following picture commands set up.
    fn begin_fade(&mut self, effect: u16) {
        if effect == 0 || self.fade.is_some() {
            return;
        }
        let ms = match effect {
            1 => 500,
            2 => 2000,
            3 => 5000,
            _ => 1000,
        };
        self.fade = Some(Fade {
            from: self.scene_draws(),
            start: self.now,
            ms,
        });
    }

    fn run(&mut self, c: &Command) -> Result<(), String> {
        match (c.group, c.op) {
            (0x00, 0x50) => self.window = true,
            (0x00, 0x51) => {
                self.window = false;
                self.text.clear();
            }
            (0x00, op) if op < 0x50 => self.say(c),
            // labels
            (0x10, 0x00) => {}
            (0x10, 0x01) | (0x10, 0x02) => {
                let name = c.str(2);
                if c.op == 0x02 {
                    self.stack.push((self.script.clone(), self.pc));
                }
                if c.u8(1) == 2 {
                    self.jump_file(&name);
                } else {
                    self.jump_label(&name);
                }
            }
            (0x10, 0x03) => match self.stack.pop() {
                Some((s, pc)) => {
                    self.script = s;
                    self.pc = pc;
                }
                None => self.wait = Wait::Ended,
            },
            (0x10, 0x04) => {
                let (cmp, bank, n, value) = (c.u8(0), c.u8(1), c.u16(2), c.u16(4) as i32);
                let v = self.var(bank, n);
                let yes = match cmp {
                    0 => v == value,
                    1 => v != value,
                    2 => v < value,
                    3 => v >= value,
                    4 => v > value,
                    5 => v <= value,
                    _ => false,
                };
                if yes {
                    let name = c.str(8);
                    if c.u8(7) == 2 {
                        self.jump_file(&name);
                    } else {
                        self.jump_label(&name);
                    }
                }
            }
            (0x10, 0x14) => {
                // count:u8 at 2, then " d<text>,<label>" for each option
                let s = c.str(3);
                let options: Vec<(String, String)> = s
                    .split(" d")
                    .filter_map(|o| {
                        let o = o.trim_start_matches('d').trim();
                        let (text, label) = o.rsplit_once(',')?;
                        Some((text.to_string(), label.to_string()))
                    })
                    .collect();
                if !options.is_empty() {
                    self.window = false;
                    self.resume_pc = self.pc - 1;
                    self.wait = Wait::Choice(options);
                }
            }
            (0x10, 0x28) => self.buttons.clear(),
            (0x10, 0x29) => {
                let s = c.str(4);
                let mut it = s.split(',');
                self.button_images = (
                    it.next().unwrap_or("").to_string(),
                    it.next().unwrap_or("").to_string(),
                );
            }
            (0x10, 0x32) => {
                self.buttons.push(Button {
                    rect: [c.i16(0), c.i16(2), c.i16(4), c.i16(6)],
                    label: c.str(8),
                    icon: None,
                });
                // the buttons wait once the last of them is set up
                let next = self.script.commands.get(self.pc);
                if !next.is_some_and(|n| n.group == 0x10 && n.op == 0x32) {
                    self.wait = Wait::Buttons;
                }
            }
            (0x20, 0x00) | (0x20, 0x02) => {
                self.begin_fade(c.u16(0));
                self.pan = None;
                let name = c.str(2);
                self.set_bg(&name);
            }
            (0x20, 0x08) => {
                // a picture that moves from (x0, y0) to (x1, y1) over a time
                self.begin_fade(c.u16(0));
                let (x0, y0, x1, y1) = (c.i16(2), c.i16(4), c.i16(6), c.i16(8));
                let name = c.str(12);
                self.set_bg(&name);
                self.pan = Some(Pan {
                    from: (x0 as f32, y0 as f32),
                    to: (x1 as f32, y1 as f32),
                    start: self.now,
                    ms: c.u16(10) as u64,
                });
            }
            (0x20, 0xc8) => {
                // [effect][count]",names": one goes in the middle, two left and right
                self.begin_fade(c.u16(0));
                let n = c.u8(2) as usize;
                let names: Vec<String> = c.str(3).split(',').map(|s| s.to_string()).collect();
                let names: Vec<&String> = names.iter().filter(|s| !s.is_empty()).collect();
                self.chars.iter_mut().for_each(|c| *c = None);
                let first = if n == 2 { 1 } else { 0 };
                for (i, name) in names.iter().take(n).enumerate() {
                    let at = self.slot_pos(first + i);
                    self.set_chara(first + i, name, at, at, 0, None);
                }
            }
            (0x20, 0x64) | (0x20, 0x65) => {
                // [effect][?][slot] name: one character at its slot's place
                self.begin_fade(c.u16(0));
                let slot = c.u8(3) as usize;
                let at = self.slot_pos(slot);
                self.set_chara(slot, &c.str(4), at, at, 0, None);
            }
            (0x20, 0x66..=0x69) => {
                // [effect][slot][x0][y0][x1][y1][ms] name ('-': the picture already there);
                // 0x67 has one more byte before the slot, 0x68 fades in, 0x69 fades out
                self.begin_fade(c.u16(0));
                let at = if c.op == 0x67 { 3 } else { 2 };
                let slot = c.u8(at) as usize;
                let from = (c.i16(at + 1) as f32, c.i16(at + 3) as f32);
                let to = (c.i16(at + 5) as f32, c.i16(at + 7) as f32);
                let ms = c.u16(at + 9) as u64;
                let fade = match c.op {
                    0x68 => Some(true),
                    0x69 => Some(false),
                    _ => None,
                };
                self.set_chara(slot, &c.str(at + 11), from, to, ms, fade);
            }
            (0x20, 0x6c) => {
                // [effect][slot] name: another picture where the character stands
                self.begin_fade(c.u16(0));
                let slot = c.u8(2) as usize;
                let at = self.chars.get(slot).and_then(|c| c.as_ref()).map(|c| c.to);
                let at = at.unwrap_or_else(|| self.slot_pos(slot));
                self.set_chara(slot, &c.str(3), at, at, 0, None);
            }
            (0x20, 0x19a) => self.shake = Some(self.now),
            (0x20, 0x190) | (0x20, 0x191) => {
                // [r g b 0][in][hold][out ms]
                let mut img = Image::new(self.screen.0, self.screen.1);
                img.fill([c.u8(0), c.u8(1), c.u8(2), 255]);
                let ms = [c.u16(4) as u64, c.u16(6) as u64, c.u16(8) as u64];
                self.wait = Wait::Until(self.now + ms.iter().sum::<u64>());
                self.flash = Some(Flash {
                    image: Rc::new(img),
                    start: self.now,
                    ms,
                });
            }
            (0x30, 0x00) => self.play_bgm(&c.str(4)),
            (0x30, 0x01) | (0x30, 0x02) => {
                self.bgm.clear();
                self.sounds.push(Sound {
                    data: Vec::new(),
                    volume: 0.0,
                    bgm: true,
                    voice: false,
                    effect_channel: None,
                    stop: true,
                    fade_ms: if c.op == 2 { c.u16(0) as u32 } else { 0 },
                    loop_start: 0,
                });
            }
            (0x30, 0x64) => {
                let name = c.str(0);
                self.play(&["vic", "se"], &name, Some(9), false);
            }
            (0x30, 0xc8) => {
                let name = c.str(4);
                self.play(&["se"], &name, Some(0), false);
            }
            (0x30, 0x12c) | (0x30, 0x12d) | (0x30, 0x130) => {
                let name = c.str(4);
                self.play(&["se"], &name, Some(1), false);
            }
            (0x30, 0x191) => {
                let name = c.str(4);
                self.play(&["se", "vic"], &name, Some(2), false);
            }
            (0x30, 0xca) | (0x30, 0x12e) | (0x30, 0x192) => {
                let channel = match c.op {
                    0xca => 0,
                    0x12e => 1,
                    _ => 2,
                };
                self.sounds.push(Sound {
                    data: Vec::new(),
                    volume: 0.0,
                    bgm: false,
                    voice: false,
                    effect_channel: Some(channel),
                    stop: true,
                    fade_ms: 0,
                    loop_start: 0,
                });
            }
            (0x40, 0x00) => self.wait = Wait::Ended,
            (0x40, 0x15) => self.open_menu(false),
            (0x50, 0x01) => {
                self.vars.insert((c.u8(1), c.u16(2)), c.u16(4) as i32);
            }
            (0x50, 0x1e) => {
                let v = self.random(c.u16(0) as u32);
                self.vars.insert((2, 0), v);
            }
            (0x50, 0x28) => self.wait = Wait::Until(self.now + c.u32(0) as u64),
            (0x50, 0x15) => self.title = c.str(0),
            (OLD, _) => self.run_old(c),
            _ => {
                if self.log {
                    eprintln!("tmrhiro: skipped {:02x}.{:04x}", c.group, c.op);
                }
            }
        }
        Ok(())
    }

    /// Where a slot puts a character: 0 in the middle, 1 left and 2 right (GAMESYSTEM
    /// CHARA_SPR_LEFT / CHARA_SPR_RIGHT, moved from the middle).
    fn slot_pos(&self, slot: usize) -> (f32, f32) {
        let side = self.screen.0 as i32 / 4;
        let x = match slot {
            1 => self.ini.int("GAMESYSTEM/CHARA_SPR_LEFT", -side),
            2 => self.ini.int("GAMESYSTEM/CHARA_SPR_RIGHT", side),
            _ => 0,
        };
        (x as f32, 0.0)
    }

    /// Puts a character in a slot (a name of '-' or nothing keeps the pictures there), its
    /// canvas centred on the screen and lowered by CHARA_SPR_PLUS_Y, then moved from `from` to
    /// `to`.
    fn set_chara(
        &mut self,
        slot: usize,
        name: &str,
        from: (f32, f32),
        to: (f32, f32),
        ms: u64,
        fade: Option<bool>,
    ) {
        if slot >= self.chars.len() {
            return;
        }
        let (name, pics) = if name.is_empty() || name == "-" {
            match self.chars[slot].take() {
                Some(c) => (c.name, c.pics),
                None => return,
            }
        } else {
            let canvas = self
                .macros
                .get(&name.to_lowercase())
                .and_then(|p| p.first().cloned())
                .unwrap_or_else(|| name.to_string());
            let w = self.screen.0 as f32;
            let cw = self.image(&canvas).map_or(w, |g| g.canvas.0 as f32);
            let y = self.ini.int("GAMESYSTEM/CHARA_SPR_PLUS_Y", 0) as f32;
            (name.to_string(), self.pictures(name, (w - cw) / 2.0, y))
        };
        self.chars[slot] = Some(Chara {
            name,
            pics,
            from,
            to,
            start: self.now,
            ms,
            fade,
        });
    }

    fn set_bg(&mut self, name: &str) {
        self.bg = self.pictures(name, 0.0, 0.0);
        self.bg_name = name.to_string();
        if name == "-" || name.is_empty() {
            self.bg.clear();
            self.bg_name.clear();
        }
    }

    fn play_bgm(&mut self, name: &str) {
        if let Some(data) = self.find(&["mus"], name) {
            let (data, loop_start) = music(data);
            self.bgm = name.to_string();
            self.sounds.push(Sound {
                data,
                volume: 1.0,
                bgm: true,
                voice: false,
                effect_channel: None,
                stop: false,
                fade_ms: 0,
                loop_start,
            });
        }
    }

    /// Stops the music and every sound.
    fn stop_sounds(&mut self) {
        self.bgm.clear();
        for (bgm, voice, channel) in [
            (true, false, None),
            (false, true, None),
            (false, false, Some(-1)),
        ] {
            self.sounds.push(Sound {
                data: Vec::new(),
                volume: 0.0,
                bgm,
                voice,
                effect_channel: channel,
                stop: true,
                fade_ms: 0,
                loop_start: 0,
            });
        }
    }

    /// The message window's buttons that this game has.
    fn window_buttons(&self) -> Vec<WindowButton> {
        let i = &self.ini;
        let (wx, wy) = (
            i.int("MESSAGEWINDOW/MW_DRAW_POS_X", 0),
            i.int("MESSAGEWINDOW/MW_DRAW_POS_Y", 400),
        );
        let mut out = Vec::new();
        for key in WINDOW_BUTTONS {
            let v = i.ints(&format!("MESSAGEWINDOW/TWB_{key}"));
            if v.len() < 8 || v[0] == 0 {
                continue;
            }
            // kind 0 sits on the window, kind 1 on the bar at the top of the screen
            let (x, y) = if v[1] == 0 {
                (wx + v[2], wy + v[3])
            } else {
                (v[2], v[3])
            };
            out.push(WindowButton {
                key,
                rect: [x, y, v[6], v[7]],
                src: [v[4], v[5]],
            });
        }
        out
    }

    /// Whether the window and its buttons are up (a line or a choice waits).
    fn window_up(&self) -> bool {
        !self.hidden && self.window && !self.text.is_empty() && matches!(self.wait, Wait::Click)
    }

    /// Whether the bar at the top is out (the pointer is on it).
    fn top_bar_out(&self) -> bool {
        self.window_up()
            && self.hover.1 >= 0
            && self.hover.1 < self.ini.int("MESSAGEWINDOW/TWM_HEIGHT_01", 48)
    }

    fn window_button_at(&self, x: i32, y: i32) -> Option<&'static str> {
        if !self.window_up() {
            return None;
        }
        let top = self.top_bar_out();
        let wy = self.ini.int("MESSAGEWINDOW/MW_DRAW_POS_Y", 400);
        self.window_buttons()
            .into_iter()
            .filter(|b| if b.rect[1] < wy { top } else { true })
            .find(|b| inside(b.rect, x, y))
            .map(|b| b.key)
    }

    fn window_button(&mut self, key: &str) {
        match key {
            "CLOSE" => self.hidden = true,
            "AUTO" => self.auto = true,
            "SKIP" => self.skip = true,
            "VOICE" => {
                let v = self.voice.clone();
                if !v.is_empty() {
                    self.play(&["vic"], &v, None, true);
                }
            }
            "BACKLOG" => self.open_log(),
            "SAVE" => self.open_menu(true),
            "LOAD" => self.open_menu(false),
            "QSAVE" => {
                let thumb = self.thumbnail();
                self.save_slot("qsave", thumb);
            }
            "QLOAD" => self.load_slot("qsave"),
            "TITLE" => self.restart(),
            "EXIT" => self.wait = Wait::Ended,
            _ => {}
        }
    }

    /// The screen as it is now, at the size of a save slot's picture.
    fn thumbnail(&mut self) -> (u32, u32, Vec<u8>) {
        let size = self.ini.ints("SAVE/SAVE_THUM_SIZE");
        let (tw, th) = match size[..] {
            [w, h, ..] if w > 0 && h > 0 => (w as u32, h as u32),
            _ => (self.screen.0 / 10, self.screen.1 / 10),
        };
        // the scene and the window, without the buttons under the pointer
        let hover = std::mem::replace(&mut self.hover, (-1, -1));
        let full = self.render();
        self.hover = hover;
        let mut px = Vec::with_capacity((tw * th * 4) as usize);
        for y in 0..th {
            for x in 0..tw {
                let sx = (x * full.w / tw) as usize;
                let sy = (y * full.h / th) as usize;
                let at = (sy * full.w as usize + sx) * 4;
                px.extend_from_slice(&full.px[at..at + 4]);
            }
        }
        (tw, th, px)
    }

    fn open_menu(&mut self, save: bool) {
        let thumb = if save {
            self.thumbnail()
        } else {
            (0, 0, Vec::new())
        };
        self.menu = Some(Menu {
            save,
            page: 0,
            thumb,
            slots: Vec::new(),
        });
        self.load_page();
    }

    /// The ini section of the screen (SAVE or LOAD).
    fn menu_section(&self) -> &'static str {
        match &self.menu {
            Some(m) if !m.save => "LOAD",
            _ => "SAVE",
        }
    }

    /// The slots of the screen: first slot's place and picture, size, count, per column, columns,
    /// steps down a column and across columns.
    fn menu_layout(&self) -> MenuLayout {
        let p = self.menu_section();
        let v = self.ini.ints(&format!("{p}/{p}_BASE_POS"));
        if v.len() >= 14 {
            return MenuLayout {
                at: [v[0], v[1]],
                src: Some([v[2], v[3]]),
                size: [v[4], v[5]],
                count: v[6].max(1) as usize,
                rows: v[7].max(1) as usize,
                step: [v[9], v[10]],
                column: [v[13], v.get(14).copied().unwrap_or(0)],
            };
        }
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        MenuLayout {
            at: [w / 16, h / 10],
            src: None,
            size: [w * 13 / 32, h * 7 / 50],
            count: 10,
            rows: 5,
            step: [0, h * 8 / 50],
            column: [w * 15 / 32, 0],
        }
    }

    fn slot_rect(&self, l: &MenuLayout, k: usize) -> [i32; 4] {
        let (col, row) = ((k / l.rows) as i32, (k % l.rows) as i32);
        [
            l.at[0] + col * l.column[0] + row * l.step[0],
            l.at[1] + col * l.column[1] + row * l.step[1],
            l.size[0],
            l.size[1],
        ]
    }

    /// A button of the screen from the ini ([x, y, src x, src y, w, h]), or one at the bottom
    /// right for games without it.
    fn menu_button(&self, key: &str) -> ([i32; 4], Option<[i32; 2]>) {
        let p = self.menu_section();
        let v = self.ini.ints(&format!("{p}/{p}_{key}"));
        if v.len() >= 6 {
            return ([v[0], v[1], v[4], v[5]], Some([v[2], v[3]]));
        }
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        ([w - w / 5 - 16, h - h / 12 - 8, w / 5, h / 12], None)
    }

    /// The plain and lit pictures of the whole screen (the older games; our own ini key).
    fn menu_images(&self) -> Option<(String, String)> {
        let p = self.menu_section();
        let v = self.ini.get(&format!("{p}/{p}_IMAGES"))?;
        let (a, b) = v.split_once(',')?;
        Some((a.trim().to_string(), b.trim().to_string()))
    }

    fn menu_pages(&self) -> Vec<([i32; 4], [i32; 2])> {
        let p = self.menu_section();
        let v = self.ini.ints(&format!("{p}/{p}_PAGE_POS"));
        if v.len() < 13 {
            return Vec::new();
        }
        (0..v[6].max(0))
            .map(|k| {
                (
                    [v[0] + k * v[9], v[1] + k * v[10], v[4], v[5]],
                    [v[2] + k * v[11], v[3] + k * v[12]],
                )
            })
            .collect()
    }

    fn slot_name(&self, k: usize) -> String {
        let page = self.menu.as_ref().map_or(0, |m| m.page);
        format!("save{:03}", page * self.menu_layout().count + k)
    }

    /// Reads the saved slots of the page and makes their pictures and lines.
    fn load_page(&mut self) {
        let l = self.menu_layout();
        let p = self.menu_section();
        let size = self.ini.ints(&format!("{p}/{p}_B_TEXT_POS"));
        let px = size.get(2).copied().unwrap_or(20);
        let font = self.text_font(px);
        // as many letters as fit right of the text's place
        let fit = ((l.size[0] - size.first().copied().unwrap_or(l.size[0] / 3) - 8) / px.max(1))
            .max(1) as usize;
        let mut slots = Vec::new();
        for k in 0..l.count {
            let name = self.slot_name(k);
            let view = self
                .files
                .read_save(&format!("{name}.dat"))
                .and_then(|b| SaveData::decode(&b))
                .map(|d| {
                    let mut thumb = Image::new(d.thumb.0, d.thumb.1);
                    if thumb.px.len() == d.thumb.2.len() {
                        thumb.px = d.thumb.2;
                    }
                    let lines = [d.title.clone(), d.text.clone()]
                        .iter()
                        .filter(|s| !s.is_empty())
                        .filter_map(|s| {
                            let f = font.as_ref()?;
                            let s: String = s.chars().take(fit).collect();
                            Some(Rc::new(f.render(
                                &nexas::sjis::encode(&s),
                                [255, 255, 255],
                                2,
                            )))
                        })
                        .collect();
                    SlotView {
                        thumb: Rc::new(thumb),
                        lines,
                    }
                });
            slots.push(view);
        }
        if let Some(m) = &mut self.menu {
            m.slots = slots;
        }
    }

    fn menu_click(&mut self, x: i32, y: i32) {
        let Some(save) = self.menu.as_ref().map(|m| m.save) else {
            return;
        };
        if self.menu_images().is_none() && inside(self.menu_button("RETURN_POS").0, x, y) {
            self.menu = None;
            return;
        }
        if let Some(k) = self.menu_pages().iter().position(|p| inside(p.0, x, y)) {
            if let Some(m) = &mut self.menu {
                m.page = k;
            }
            self.load_page();
            return;
        }
        let l = self.menu_layout();
        let Some(k) = (0..l.count).find(|&k| inside(self.slot_rect(&l, k), x, y)) else {
            return;
        };
        let name = self.slot_name(k);
        if save {
            let thumb = self
                .menu
                .as_ref()
                .map(|m| m.thumb.clone())
                .unwrap_or_default();
            self.save_slot(&name, thumb);
            self.load_page();
        } else if self
            .menu
            .as_ref()
            .is_some_and(|m| m.slots.get(k).is_some_and(|s| s.is_some()))
        {
            self.menu = None;
            self.load_slot(&name);
        }
    }

    fn save_slot(&mut self, name: &str, thumb: (u32, u32, Vec<u8>)) {
        let text = strip_ruby(&self.text.replace("\\n", ""));
        let d = SaveData {
            script: self.script.name.clone(),
            pc: self.resume_pc as u32,
            stack: self
                .stack
                .iter()
                .map(|(s, pc)| (s.name.clone(), *pc as u32))
                .collect(),
            vars: self.vars.iter().map(|(&(b, n), &v)| (b, n, v)).collect(),
            bg: self.bg_name.clone(),
            chars: self
                .chars
                .iter()
                .enumerate()
                .filter_map(|(k, c)| {
                    let c = c.as_ref().filter(|c| c.fade != Some(false))?;
                    Some(SavedChara {
                        slot: k as u8,
                        name: c.name.clone(),
                        x: c.to.0,
                        y: c.to.1,
                    })
                })
                .collect(),
            bgm: self.bgm.clone(),
            title: self.title.clone(),
            text,
            thumb,
        };
        self.files.write_save(&format!("{name}.dat"), &d.encode());
    }

    fn load_slot(&mut self, name: &str) {
        let Some(d) = self
            .files
            .read_save(&format!("{name}.dat"))
            .and_then(|b| SaveData::decode(&b))
        else {
            return;
        };
        let Ok(script) = self.load(&d.script) else {
            return;
        };
        let mut stack = Vec::new();
        for (s, pc) in &d.stack {
            match self.load(s) {
                Ok(s) => stack.push((s, *pc as usize)),
                Err(_) => return,
            }
        }
        self.reset();
        self.script = script;
        self.pc = d.pc as usize;
        self.stack = stack;
        // the game's settings and all-time flags (banks 4 and 5) stay as they are
        self.vars.retain(|&(bank, _), _| bank >= 4);
        for (bank, n, v) in d.vars {
            if bank < 4 {
                self.vars.insert((bank, n), v);
            }
        }
        self.set_bg(&d.bg);
        for c in d.chars {
            self.set_chara(c.slot as usize, &c.name, (c.x, c.y), (c.x, c.y), 0, None);
        }
        if !d.bgm.is_empty() {
            self.play_bgm(&d.bgm);
        }
        self.title = d.title;
    }

    /// Clears the screen, the sounds and what was waiting.
    fn reset(&mut self) {
        self.stop_sounds();
        self.stack.clear();
        self.bg.clear();
        self.bg_name.clear();
        self.pan = None;
        self.chars.iter_mut().for_each(|c| *c = None);
        self.overlay.clear();
        self.fade = None;
        self.flash = None;
        self.shake = None;
        self.log_view = None;
        self.window = false;
        self.hidden = false;
        self.auto = false;
        self.skip = false;
        self.name.clear();
        self.text.clear();
        self.text_images.clear();
        self.buttons.clear();
        self.wait = Wait::None;
    }

    /// Back to the title: the start script with the game's variables cleared.
    fn restart(&mut self) {
        self.reset();
        self.vars.retain(|&(bank, _), _| bank >= 4);
        self.jump_file("start");
    }

    /// Plays a sound; false if the game does not have it.
    fn play(&mut self, kinds: &[&str], name: &str, channel: Option<i32>, voice: bool) -> bool {
        if let Some(data) = self.find(kinds, name) {
            let (data, _) = music(data);
            self.sounds.push(Sound {
                data,
                volume: 1.0,
                bgm: false,
                voice,
                effect_channel: channel,
                stop: false,
                fade_ms: 0,
                loop_start: 0,
            });
            true
        } else {
            if self.log {
                eprintln!("tmrhiro: no sound {name}");
            }
            false
        }
    }

    /// A line of text: op bit 1 has a voice (last), bit 2 a name (first), bit 4 is a thought.
    fn say(&mut self, c: &Command) {
        let s = c.str(0);
        let mut parts: Vec<&str> = s.split(',').collect();
        let voice = if c.op & 1 != 0 && parts.len() > 1 {
            parts.pop()
        } else {
            None
        };
        let name = if c.op & 2 != 0 && parts.len() > 1 {
            Some(parts.remove(0))
        } else {
            None
        };
        let text = parts.join(",");
        self.say_line(name, &text, voice);
    }

    /// Shows a line in the window (playing its voice) and waits for a click.
    fn say_line(&mut self, name: Option<&str>, text: &str, voice: Option<&str>) {
        self.voice.clear();
        if let Some(v) = voice {
            let v = v.trim().to_string();
            if self.play(&["vic"], &v, None, true) {
                self.voice = v;
            }
        }
        self.resume_pc = self.pc - 1;
        self.said_at = self.now;
        self.name = name.map(|n| self.names(n)).unwrap_or_default();
        self.text = self.names(text);
        let (name, text, voice) = (self.name.clone(), self.text.clone(), self.voice.clone());
        self.log_line(&name, &text, &voice);
        self.window = true;
        self.text_images.clear();
        self.wait = Wait::Click;
    }

    /// The player's names in place of ＄α and ＄β.
    fn names(&self, s: &str) -> String {
        let first = self
            .ini
            .get("GAMESYSTEM/PLAYER_NAME01")
            .unwrap_or("")
            .to_string();
        let given = self
            .ini
            .get("GAMESYSTEM/PLAYER_NAME02")
            .unwrap_or("")
            .to_string();
        let short = self.ini.get("GAMESYSTEM/PLAYER_NAME03").unwrap_or("");
        s.replace("＄α", &first)
            .replace("＄β", &given)
            .replace("＄γ", short)
    }

    /// Draws the pictures of the scene (no window), bottom first.
    fn scene_draws(&self) -> Vec<Draw> {
        let mut out = Vec::new();
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let pan = self.pan.as_ref().map(|p| {
            let t = if p.ms == 0 {
                1.0
            } else {
                ((self.now - p.start) as f32 / p.ms as f32).min(1.0)
            };
            (
                p.from.0 + (p.to.0 - p.from.0) * t,
                p.from.1 + (p.to.1 - p.from.1) * t,
            )
        });
        for pic in &self.bg {
            let (dx, dy) = pan.unwrap_or((0.0, 0.0));
            out.push(draw(
                &pic.image,
                pic.x + dx,
                pic.y + dy,
                1.0,
                [0.0, 0.0, w, h],
            ));
        }
        for c in self.chars.iter().flatten() {
            let t = c.progress(self.now);
            let dx = c.from.0 + (c.to.0 - c.from.0) * t;
            let dy = c.from.1 + (c.to.1 - c.from.1) * t;
            let alpha = match c.fade {
                Some(true) => t,
                Some(false) => 1.0 - t,
                None => 1.0,
            };
            for pic in &c.pics {
                out.push(draw(
                    &pic.image,
                    pic.x + dx,
                    pic.y + dy,
                    alpha,
                    [0.0, 0.0, w, h],
                ));
            }
        }
        for pic in &self.overlay {
            out.push(draw(&pic.image, pic.x, pic.y, 1.0, [0.0, 0.0, w, h]));
        }
        // a shake moves everything to and fro, less and less, for a third of a second
        if let Some(start) = self.shake {
            let t = self.now.saturating_sub(start);
            if t < SHAKE_MS {
                let k = (t / MS_PER_FRAME) as f32;
                let fall = 1.0 - t as f32 / SHAKE_MS as f32;
                let (dx, dy) = ((k * 2.1).sin() * 24.0 * fall, (k * 1.7).cos() * 20.0 * fall);
                for d in &mut out {
                    d.dst[0] += dx;
                    d.dst[2] += dx;
                    d.dst[1] += dy;
                    d.dst[3] += dy;
                    d.place.x += dx;
                    d.place.y += dy;
                }
            }
        }
        if let Some(f) = &self.flash {
            let t = self.now.saturating_sub(f.start) as f32;
            let [a, b, c] = f.ms.map(|v| v as f32);
            let alpha = if t < a {
                t / a.max(1.0)
            } else if t < a + b {
                1.0
            } else {
                1.0 - (t - a - b) / c.max(1.0)
            };
            out.push(draw(
                &f.image,
                0.0,
                0.0,
                alpha.clamp(0.0, 1.0),
                [0.0, 0.0, w, h],
            ));
        }
        out
    }

    /// What to draw this frame, bottom first.
    pub fn draws(&mut self) -> Vec<Draw> {
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let mut out = Vec::new();
        match &self.fade {
            Some(f) => {
                let t = ((self.now - f.start) as f32 / f.ms as f32).clamp(0.0, 1.0);
                out.extend(f.from.iter().cloned());
                for mut d in self.scene_draws() {
                    d.place.alpha *= t;
                    out.push(d);
                }
            }
            None => out.extend(self.scene_draws()),
        }
        if self.log_open() {
            self.log_draws(&mut out);
            return out;
        }
        if let Wait::Buttons = self.wait {
            let (normal, over) = self.button_images.clone();
            let base = self.pictures(&normal, 0.0, 0.0);
            for p in &base {
                out.push(draw(&p.image, p.x, p.y, 1.0, [0.0, 0.0, w, h]));
            }
            let (hx, hy) = self.hover;
            let hit = self
                .buttons
                .iter()
                .find(|b| inside(b.rect, hx, hy))
                .map(|b| (b.rect, b.icon.is_some()));
            let icons: Vec<([i32; 4], [i32; 2])> = self
                .buttons
                .iter()
                .filter_map(|b| Some((b.rect, b.icon?)))
                .collect();
            if !icons.is_empty() {
                self.map_draws(&icons, hit.map(|h| h.0), &mut out);
            }
            if let Some((r, false)) = hit {
                for p in self.pictures(&over, 0.0, 0.0) {
                    let clip = [
                        r[0] as f32,
                        r[1] as f32,
                        (r[0] + r[2]) as f32,
                        (r[1] + r[3]) as f32,
                    ];
                    out.push(draw(&p.image, p.x, p.y, 1.0, clip));
                }
            }
        }
        if self.window && !self.hidden && self.fade.is_none() && !self.text.is_empty() {
            self.window_draws(&mut out);
        }
        if self.window_up() {
            self.window_button_draws(&mut out);
        }
        if let Wait::Choice(options) = &self.wait {
            let options = options.clone();
            self.choice_draws(&options, &mut out);
        }
        if self.menu.is_some() {
            self.menu_draws(&mut out);
        }
        out
    }

    /// The buttons on the window, the bar at the top when the pointer is there, and the
    /// click-wait mark.
    fn window_button_draws(&mut self, out: &mut Vec<Draw>) {
        let wy = self.ini.int("MESSAGEWINDOW/MW_DRAW_POS_Y", 400);
        let wx = self.ini.int("MESSAGEWINDOW/MW_DRAW_POS_X", 0);
        let top = self.top_bar_out();
        if top && let Some(g) = self.image("sys_window_menu") {
            out.push(draw(&g.image, g.x as f32, g.y as f32, 1.0, [0.0; 4]));
        }
        if let Some(g) = self.image("sys_window_button") {
            for b in self.window_buttons() {
                if b.rect[1] < wy && !top {
                    continue;
                }
                let lit = inside(b.rect, self.hover.0, self.hover.1)
                    || (b.key == "AUTO" && self.auto)
                    || (b.key == "SKIP" && self.skip);
                let sy = b.src[1] + if lit { b.rect[3] } else { 0 };
                let src = [b.src[0] - g.x, sy - g.y, b.rect[2], b.rect[3]];
                out.push(part(&g.image, src, b.rect[0] as f32, b.rect[1] as f32));
            }
        }
        // the click-wait mark: CW_DRAW_PAT pictures side by side, CW_DRAW_TIME ms each
        let i = &self.ini;
        let (cx, cy) = (
            i.int("MESSAGEWINDOW/CW_DRAW_POS_X", -1),
            i.int("MESSAGEWINDOW/CW_DRAW_POS_Y", -1),
        );
        let (cw, ch) = (
            i.int("MESSAGEWINDOW/CW_DRAW_WIDTH", 0),
            i.int("MESSAGEWINDOW/CW_DRAW_HEIGHT", 0),
        );
        let pat = i.int("MESSAGEWINDOW/CW_DRAW_PAT", 1).max(1) as u64;
        let ms = i.int("MESSAGEWINDOW/CW_DRAW_TIME", 50).max(1) as u64;
        if cx >= 0
            && cw > 0
            && let Some(g) = self
                .image("sys_click_wait")
                .or_else(|| self.image("sys_clickwait"))
        {
            let k = ((self.now / ms) % pat) as i32;
            let src = [k * cw - g.x, -g.y, cw, ch];
            out.push(part(&g.image, src, (wx + cx) as f32, (wy + cy) as f32));
        }
    }

    /// The save or load screen over the game.
    fn menu_draws(&mut self, out: &mut Vec<Draw>) {
        let Some((save, page)) = self.menu.as_ref().map(|m| (m.save, m.page)) else {
            return;
        };
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let kind = if save { "save" } else { "load" };
        // the older games draw the whole screen twice, plain and lit
        let images = self.menu_images();
        let base_name = images
            .as_ref()
            .map_or(format!("sys_{kind}_base"), |i| i.0.clone());
        let lit = images.and_then(|i| self.image(&i.1));
        match self.image(&base_name) {
            Some(g) => out.push(draw(
                &g.image,
                g.x as f32,
                g.y as f32,
                1.0,
                [0.0, 0.0, w, h],
            )),
            None => {
                let p = Rc::new(panel(self.screen.0, self.screen.1));
                out.push(draw(&p, 0.0, 0.0, 1.0, [0.0, 0.0, w, h]));
            }
        }
        let buttons = self.image(&format!("sys_{kind}_button"));
        let hover = self.hover;
        // a button of the sheet: normal, lit one row below, chosen two rows below
        let sheet = |out: &mut Vec<Draw>, r: [i32; 4], src: Option<[i32; 2]>, chosen: bool| {
            let state = if chosen {
                2
            } else if inside(r, hover.0, hover.1) {
                1
            } else {
                0
            };
            if let Some(g) = &lit {
                if state > 0 {
                    let mut d = draw(&g.image, g.x as f32, g.y as f32, 1.0, [0.0, 0.0, w, h]);
                    d.place.clip = [
                        r[0] as f32,
                        r[1] as f32,
                        (r[0] + r[2]) as f32,
                        (r[1] + r[3]) as f32,
                    ];
                    out.push(d);
                }
                return;
            }
            match (&buttons, src) {
                (Some(g), Some(s)) => {
                    let src = [s[0] - g.x, s[1] + state * r[3] - g.y, r[2], r[3]];
                    out.push(part(&g.image, src, r[0] as f32, r[1] as f32));
                }
                _ => {
                    let p = Rc::new(panel(r[2].max(1) as u32, r[3].max(1) as u32));
                    let a = if state > 0 { 1.0 } else { 0.6 };
                    out.push(draw(&p, r[0] as f32, r[1] as f32, a, [0.0, 0.0, w, h]));
                }
            }
        };
        let l = self.menu_layout();
        let p = self.menu_section();
        let thumb_at = self.ini.ints(&format!("{p}/{p}_B_THUM_POS"));
        let text_at = self.ini.ints(&format!("{p}/{p}_B_TEXT_POS"));
        let (tx, ty) = (
            thumb_at.first().copied().unwrap_or(8),
            thumb_at.get(1).copied().unwrap_or(8),
        );
        let (sx, sy) = (
            text_at.first().copied().unwrap_or(l.size[0] / 3),
            text_at.get(1).copied().unwrap_or(8),
        );
        let size = text_at.get(2).copied().unwrap_or(20);
        for k in 0..l.count {
            let r = self.slot_rect(&l, k);
            sheet(out, r, l.src, false);
            let Some(Some(v)) = self.menu.as_ref().and_then(|m| m.slots.get(k)) else {
                continue;
            };
            out.push(draw(
                &v.thumb,
                (r[0] + tx) as f32,
                (r[1] + ty) as f32,
                1.0,
                [0.0, 0.0, w, h],
            ));
            for (n, line) in v.lines.iter().enumerate() {
                let y = r[1] + sy + n as i32 * (size + 12);
                out.push(draw(
                    line,
                    (r[0] + sx) as f32,
                    y as f32,
                    1.0,
                    [0.0, 0.0, w, h],
                ));
            }
        }
        for (k, (r, src)) in self.menu_pages().into_iter().enumerate() {
            sheet(out, r, Some(src), k == page);
        }
        if self.menu_images().is_some() {
            return;
        }
        let (r, src) = self.menu_button("RETURN_POS");
        sheet(out, r, src, false);
        if src.is_none()
            && let Some(f) = self.text_font(size)
        {
            let img = Rc::new(f.render(&nexas::sjis::encode("戻る"), [255, 255, 255], 2));
            let x = r[0] as f32 + (r[2] as f32 - img.w as f32) / 2.0;
            let y = r[1] as f32 + (r[3] as f32 - img.h as f32) / 2.0;
            out.push(draw(&img, x, y, 1.0, [0.0, 0.0, w, h]));
        }
    }

    fn text_font(&self, size: i32) -> Option<Font> {
        self.font.clone().map(|f| Font::outline(f, size))
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let i = &self.ini;
        let (wx, wy) = (
            i.int("MESSAGEWINDOW/MW_DRAW_POS_X", 0),
            i.int("MESSAGEWINDOW/MW_DRAW_POS_Y", 400),
        );
        let (ww, wh) = (
            i.int("MESSAGEWINDOW/MW_DRAW_WIDTH", self.screen.0 as i32),
            i.int("MESSAGEWINDOW/MW_DRAW_HEIGHT", 200),
        );
        let size = i.int("MESSAGEWINDOW/TEXT_FONT", 24);
        let (tx, ty) = (
            i.int("MESSAGEWINDOW/TEXT_DRAW_POS_X", 40),
            i.int("MESSAGEWINDOW/TEXT_DRAW_POS_Y", 40),
        );
        let next = i.int("MESSAGEWINDOW/TEXT_DRAW_NEXT", size + 8).max(size);
        let per_line = i.int("MESSAGEWINDOW/TEXT_MAX", 40).max(1) as usize;
        let (nx, ny) = (
            i.int("MESSAGEWINDOW/NP_DRAW_POS_X", 40),
            i.int("MESSAGEWINDOW/NP_DRAW_POS_Y", 8),
        );
        let color = i.color("MESSAGEWINDOW/TEXT_COLOR", [255, 255, 255]);
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        // the window's picture (white, coloured by SYS_DEF_WINDOW_STAT, AARRGGBB), or a dark
        // panel for games without one
        if self.window_base.is_none() {
            let stat = self
                .ini
                .get("SYSTEM/SYS_DEF_WINDOW_STAT")
                .and_then(|v| u32::from_str_radix(v.trim(), 16).ok());
            let base = self.image("sys_window_base").map(|g| {
                let c = stat.unwrap_or(0x8000_0000).to_be_bytes();
                let mut img = (*g.image).clone();
                for p in img.px.chunks_exact_mut(4) {
                    for k in 0..3 {
                        p[k] = (p[k] as u32 * c[k + 1] as u32 / 255) as u8;
                    }
                    p[3] = (p[3] as u32 * c[0] as u32 / 255) as u8;
                }
                (Rc::new(img), 0, 0)
            });
            // the older games' window is one picture placed on its canvas
            let base = base.or_else(|| {
                self.image("sys_textwindow")
                    .map(|g| (g.image.clone(), g.x, g.y))
            });
            self.window_base = Some(base);
        }
        let (base, bx, by) = match self.window_base.clone().flatten() {
            Some(b) => b,
            None => (Rc::new(panel(ww.max(1) as u32, wh.max(1) as u32)), 0, 0),
        };
        out.push(draw(
            &base,
            (wx + bx) as f32,
            (wy + by) as f32,
            1.0,
            [0.0, 0.0, w, h],
        ));
        // the name on its plate (the older games; NP_PLATE_POS is our own key)
        let plate = self.ini.ints("MESSAGEWINDOW/NP_PLATE_POS");
        let plate = match (
            plate.len() >= 2 && !self.name.is_empty(),
            self.image("sys_nameplate"),
        ) {
            (true, Some(g)) => {
                let (px, py) = (wx + plate[0] + g.x, wy + plate[1] + g.y);
                out.push(draw(&g.image, px as f32, py as f32, 1.0, [0.0, 0.0, w, h]));
                Some([px, py, g.image.w as i32, g.image.h as i32])
            }
            _ => None,
        };
        if self.text_images.is_empty() {
            let Some(font) = self.text_font(size) else {
                return;
            };
            let mut lines = Vec::new();
            for line in self.text.split("\\n") {
                let line = strip_ruby(line);
                // TEXT_MAX counts half-width cells (Shift_JIS bytes)
                let mut cur = String::new();
                let mut cells = 0;
                for ch in line.chars() {
                    let w = if (ch as u32) < 0x80 || ('｡'..='ﾟ').contains(&ch) {
                        1
                    } else {
                        2
                    };
                    if cells + w > per_line {
                        lines.push(std::mem::take(&mut cur));
                        cells = 0;
                    }
                    cur.push(ch);
                    cells += w;
                }
                lines.push(cur);
            }
            let mut images = Vec::new();
            for (k, line) in lines.iter().enumerate() {
                let img = font.render(&nexas::sjis::encode(line), color, 2);
                images.push((
                    Rc::new(img),
                    (wx + tx - 2) as f32,
                    (wy + ty + k as i32 * next - 2) as f32,
                ));
            }
            if !self.name.is_empty() {
                let img = font.render(&nexas::sjis::encode(&self.name), color, 2);
                let (x, y) = match plate {
                    Some(r) => (
                        r[0] + (r[2] - img.w as i32) / 2,
                        r[1] + (r[3] - img.h as i32) / 2,
                    ),
                    None => (wx + nx - 2, wy + ny - 2),
                };
                images.push((Rc::new(img), x as f32, y as f32));
            }
            self.text_images = images;
        }
        for (img, x, y) in &self.text_images {
            out.push(draw(img, *x, *y, 1.0, [0.0, 0.0, w, h]));
        }
    }

    fn choice_rects(&self, n: usize) -> Vec<[i32; 4]> {
        let i = &self.ini;
        let (cw, ch) = (
            i.int("SELECT/SELECT_WIDTH", 600),
            i.int("SELECT/SELECT_HEIGHT", 60),
        );
        let y0 = i.int("SELECT/SELECT_POSY", 200);
        let gap = i.int("SELECT/SELECT_NEXT_Y", 20);
        let x = (self.screen.0 as i32 - cw) / 2;
        (0..n)
            .map(|k| [x, y0 + k as i32 * (ch + gap), cw, ch])
            .collect()
    }

    fn choice_at(&self, n: usize, x: i32, y: i32) -> Option<usize> {
        self.choice_rects(n)
            .iter()
            .position(|r| x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3])
    }

    fn choice_draws(&mut self, options: &[(String, String)], out: &mut Vec<Draw>) {
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let size = self.ini.int("SELECT/SELECT_TEXT_FONT_SIZE", 24);
        let rects = self.choice_rects(options.len());
        let base = self.image("sys_select");
        let font = self.text_font(size);
        let hover = self.choice_at(options.len(), self.hover.0, self.hover.1);
        for (k, ((text, _), r)) in options.iter().zip(&rects).enumerate() {
            match &base {
                // the picture holds the states one above the other, a box high each
                Some(g) => {
                    let row = if hover == Some(k) { 1 } else { 0 };
                    let mut d = draw(
                        &g.image,
                        r[0] as f32 + g.x as f32,
                        r[1] as f32 + g.y as f32 - (row * r[3]) as f32,
                        1.0,
                        [0.0, 0.0, w, h],
                    );
                    d.place.clip = [
                        r[0] as f32,
                        r[1] as f32,
                        (r[0] + r[2]) as f32,
                        (r[1] + r[3]) as f32,
                    ];
                    out.push(d);
                }
                None => {
                    let p = Rc::new(panel(r[2] as u32, r[3] as u32));
                    out.push(draw(&p, r[0] as f32, r[1] as f32, 1.0, [0.0, 0.0, w, h]));
                }
            }
            if let Some(font) = &font {
                let img = Rc::new(font.render(&nexas::sjis::encode(text), [255, 255, 255], 2));
                let x = r[0] as f32 + (r[2] as f32 - img.w as f32) / 2.0;
                let y = r[1] as f32 + (r[3] as f32 - img.h as f32) / 2.0;
                out.push(draw(&img, x, y, 1.0, [0.0, 0.0, w, h]));
            }
        }
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

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3]
}

/// Part of a picture (`src`: x, y, w, h) at (x, y).
fn part(image: &Rc<Image>, src: [i32; 4], x: f32, y: f32) -> Draw {
    let mut d = draw(image, x - src[0] as f32, y - src[1] as f32, 1.0, [0.0; 4]);
    d.place.clip = [x, y, x + src[2] as f32, y + src[3] as f32];
    d
}

fn draw(image: &Rc<Image>, x: f32, y: f32, alpha: f32, clip: [f32; 4]) -> Draw {
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
            clip,
        },
    }
}

/// A half dark panel.
fn panel(w: u32, h: u32) -> Image {
    let mut img = Image::new(w, h);
    img.fill([0, 0, 0, 128]);
    img
}

/// Text without its readings: "鈴木<3/すずき>" is "鈴木".
fn strip_ruby(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        match rest[i..].find('>') {
            Some(j) => rest = &rest[i + j + 1..],
            None => {
                rest = &rest[i..];
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Music and sounds: Ogg Vorbis as it is, or the engine's raw PCM ("D", 44.1 kHz 16-bit
/// stereo: 'D' loop:u24 0:u8 length:u32 samples) as a WAV file.
fn music(b: Vec<u8>) -> (Vec<u8>, u32) {
    if b.len() < 9 || b[0] != 0x44 || b[4] != 0 {
        return (b, 0);
    }
    let loop_start = u32::from_le_bytes([b[1], b[2], b[3], 0]);
    let len = u32::from_le_bytes(b[5..9].try_into().unwrap()).min(b.len() as u32 - 9);
    let mut wav = Vec::with_capacity(44 + len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&44100u32.to_le_bytes());
    wav.extend_from_slice(&(44100u32 * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&len.to_le_bytes());
    wav.extend_from_slice(&b[9..9 + len as usize]);
    (wav, loop_start / 4)
}
