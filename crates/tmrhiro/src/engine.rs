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
use crate::script::{Command, Script};

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
}

struct Fade {
    from: Vec<Draw>,
    start: u64,
    ms: u64,
}

/// A character in one of the sprite slots, moving from `from` to `to` over `ms`.
struct Chara {
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
    pan: Option<Pan>,
    /// the sprite slots (picture layers 5 and up)
    chars: Vec<Option<Chara>>,
    flash: Option<Flash>,
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
            pan: None,
            chars: (0..10).map(|_| None).collect(),
            flash: None,
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
        if self.fade.take().is_some() {
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
                self.bg = self.pictures(&name, 0.0, 0.0);
                if name == "-" || name.is_empty() {
                    self.bg.clear();
                }
            }
            (0x20, 0x08) => {
                // a picture that moves from (x0, y0) to (x1, y1) over a time
                self.begin_fade(c.u16(0));
                let (x0, y0, x1, y1) = (c.i16(2), c.i16(4), c.i16(6), c.i16(8));
                let name = c.str(12);
                self.bg = self.pictures(&name, 0.0, 0.0);
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
            (0x30, 0x00) => {
                let name = c.str(4);
                if let Some(data) = self.find(&["mus"], &name) {
                    let (data, loop_start) = music(data);
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
            (0x30, 0x01) | (0x30, 0x02) => self.sounds.push(Sound {
                data: Vec::new(),
                volume: 0.0,
                bgm: true,
                voice: false,
                effect_channel: None,
                stop: true,
                fade_ms: if c.op == 2 { c.u16(0) as u32 } else { 0 },
                loop_start: 0,
            }),
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
            (0x50, 0x01) => {
                self.vars.insert((c.u8(1), c.u16(2)), c.u16(4) as i32);
            }
            (0x50, 0x1e) => {
                let v = self.random(c.u16(0) as u32);
                self.vars.insert((2, 0), v);
            }
            (0x50, 0x28) => self.wait = Wait::Until(self.now + c.u32(0) as u64),
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
        let pics = if name.is_empty() || name == "-" {
            match self.chars[slot].take() {
                Some(c) => c.pics,
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
            self.pictures(name, (w - cw) / 2.0, y)
        };
        self.chars[slot] = Some(Chara {
            pics,
            from,
            to,
            start: self.now,
            ms,
            fade,
        });
    }

    fn play(&mut self, kinds: &[&str], name: &str, channel: Option<i32>, voice: bool) {
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
        } else if self.log {
            eprintln!("tmrhiro: no sound {name}");
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
        if let Some(v) = voice {
            let v = v.trim().to_string();
            self.play(&["vic"], &v, None, true);
        }
        self.name = name.map(|n| self.names(n)).unwrap_or_default();
        self.text = self.names(&text);
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
        s.replace("＄α", &first).replace("＄β", &given)
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
        if let Wait::Buttons = self.wait {
            let (normal, over) = self.button_images.clone();
            let base = self.pictures(&normal, 0.0, 0.0);
            for p in &base {
                out.push(draw(&p.image, p.x, p.y, 1.0, [0.0, 0.0, w, h]));
            }
            let (hx, hy) = self.hover;
            let hit = self.buttons.iter().find(|b| {
                hx >= b.rect[0]
                    && hy >= b.rect[1]
                    && hx < b.rect[0] + b.rect[2]
                    && hy < b.rect[1] + b.rect[3]
            });
            if let Some(b) = hit {
                let r = b.rect;
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
        if self.window && self.fade.is_none() && !self.text.is_empty() {
            self.window_draws(&mut out);
        }
        if let Wait::Choice(options) = &self.wait {
            let options = options.clone();
            self.choice_draws(&options, &mut out);
        }
        out
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
        // a dark panel under the text, then the window's own picture if there is one
        let panel = Rc::new(panel(ww.max(1) as u32, wh.max(1) as u32));
        out.push(draw(&panel, wx as f32, wy as f32, 1.0, [0.0, 0.0, w, h]));
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
                images.push((Rc::new(img), (wx + nx - 2) as f32, (wy + ny - 2) as f32));
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
