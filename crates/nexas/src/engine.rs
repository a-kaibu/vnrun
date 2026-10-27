//! The engine of the 2015 builds (恋愛フェイズ): the script thread, the objects on screen and
//! what the script waits for. Builtin numbers are those of that exe (see the notes in
//! [`builtin`](Engine::builtin)); other builds number them differently.
//!
//! Times in the scripts are in 1/100 s: `(t & 0xFFFF)`, with the easing in the high 16 bits
//! (0 linear, 1 ease in, 2 ease out). The engine runs at 60 frames a second.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use crate::builds::{BUILDS, Build};
use crate::dat::{self, Cell};
use crate::files::{Archives, Files};
use crate::font::Font;
use crate::gfx::{self, Draw, Image, Place, Spm};
use crate::script::{self, Script};
use crate::sjis;
use crate::vm::{Flow, Globals, Host, Step, Thread, Value};

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

/// Encoded sound data requested by the script. The host decodes and plays it.
pub struct Sound {
    pub data: Vec<u8>,
    pub volume: f32,
    pub bgm: bool,
    pub voice: bool,
    pub effect_channel: Option<i32>,
    pub stop: bool,
    pub fade_ms: u32,
    pub loop_start: u32,
}

fn screen_size(rows: &[Vec<Cell>]) -> (u32, u32) {
    let Some(row) = rows.first() else {
        return (WIDTH, HEIGHT);
    };
    let Some(marker) = row.iter().position(|cell| matches!(cell, Cell::Str(s) if s.len() == 20 && s.bytes().all(|b| b.is_ascii_digit()))) else {
        // 2009: no marker; the first pair of integers that is a usual screen size
        const SIZES: [(i32, i32); 6] = [
            (640, 480),
            (800, 600),
            (1024, 600),
            (1024, 768),
            (1280, 720),
            (1920, 1080),
        ];
        return row
            .windows(2)
            .find_map(|w| match w {
                [Cell::Int(a), Cell::Int(b)] if SIZES.contains(&(*a, *b)) => {
                    Some((*a as u32, *b as u32))
                }
                _ => None,
            })
            .unwrap_or((WIDTH, HEIGHT));
    };
    match (row.get(marker + 2), row.get(marker + 3)) {
        (Some(Cell::Int(w @ 320..=4096)), Some(Cell::Int(h @ 240..=2160))) => {
            (*w as u32, *h as u32)
        }
        _ => (WIDTH, HEIGHT),
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn finds_complete_voice_ids_in_shift_jis_text() {
        let text = sjis::encode("文字@v10200002「台詞」");
        assert_eq!(message_voice(&text).as_deref(), Some("10200002"));
        assert_eq!(message_voice(b"@v1020abc2"), None);
        assert_eq!(message_voice(b"@v1020"), None);
        assert_eq!(
            message_voice(&[
                0x81, 0x40, b'v', b'1', b'0', b'2', b'0', b'0', b'0', b'0', b'2'
            ]),
            None
        );
    }

    #[test]
    fn reads_size_after_system_serial_and_rejects_invalid_dimensions() {
        let mut row = vec![
            Cell::Str("00000000000000000000".into()),
            Cell::Int(1),
            Cell::Int(1024),
            Cell::Int(600),
        ];
        assert_eq!(screen_size(&[row.clone()]), (1024, 600));
        row[2] = Cell::Int(i32::MAX);
        assert_eq!(screen_size(&[row]), (WIDTH, HEIGHT));
        assert_eq!(screen_size(&[]), (WIDTH, HEIGHT));
    }
}

// The message window. Guessed from the pictures: the engine's own layout is not worked out yet.
const WINDOW: (f32, f32) = (100.0, 473.0);
const TEXT: (i32, i32) = (260, 560);
const LINE: i32 = 36;
/// the plane of the character pictures (drawn first)
const STANDS: i32 = i32::MIN;
const MESSAGE_FONT: &str = "ＭＳゴシック24x24.fnt";

/// A value moving from `from` to `to` over `frames` frames from frame `start`.
#[derive(Debug, Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    start: u64,
    frames: u64,
    ease: u32,
}

impl Tween {
    fn at(v: f32) -> Tween {
        Tween {
            from: v,
            to: v,
            start: 0,
            frames: 0,
            ease: 0,
        }
    }
    fn value(&self, now: u64) -> f32 {
        if self.frames == 0 || now >= self.start + self.frames {
            return self.to;
        }
        let p = now.saturating_sub(self.start) as f32 / self.frames as f32;
        let e = match self.ease {
            1 => p * p,
            2 => 1.0 - (1.0 - p) * (1.0 - p),
            _ => p,
        };
        self.from + (self.to - self.from) * e
    }
    fn done(&self, now: u64) -> bool {
        now >= self.start + self.frames
    }
    fn go(&mut self, to: f32, time: i32, now: u64) {
        *self = Tween {
            from: self.value(now),
            to,
            start: now,
            frames: frames(time),
            ease: (time as u32 >> 16) & 3,
        };
    }
}

/// Script time (1/100 s, easing in the high bits) in frames.
fn frames(time: i32) -> u64 {
    (time as u32 & 0xffff) as u64 * 60 / 100
}

#[derive(Clone)]
enum Look {
    Image(Rc<Image>),
    Spm {
        spm: Rc<Spm>,
        frame: usize,
        /// by the sprite map's image index; None: the file is missing
        images: Vec<Option<Rc<Image>>>,
    },
}

#[derive(Clone)]
struct Obj {
    look: Look,
    x: Tween,
    y: Tween,
    sx: Tween,
    sy: Tween,
    rot: Tween,
    alpha: Tween,
    /// drawn with 描画モード 3/4 and alpha `alpha`; mode 0 (new objects) is drawn opaque
    blend: bool,
    add: bool,
    /// period (frames), from and to (0..1), start frame
    blink: Option<(u64, f32, f32, u64)>,
    button: bool,
    /// the point scaling and turning keep in place, from the position (None: the position)
    pivot: Option<(i32, i32)>,
    flip: (bool, bool),
    /// the part of the screen it shows: left, top, right, bottom
    clip: Option<[Tween; 4]>,
    /// where it was made (2011 ひなたテラス moves are offsets from here)
    origin: (f32, f32),
}

impl Obj {
    fn new(look: Look, x: i32, y: i32) -> Obj {
        Obj {
            look,
            x: Tween::at(x as f32),
            y: Tween::at(y as f32),
            sx: Tween::at(1.0),
            sy: Tween::at(1.0),
            rot: Tween::at(0.0),
            alpha: Tween::at(1.0),
            blend: false,
            add: false,
            blink: None,
            button: false,
            pivot: None,
            flip: (false, false),
            clip: None,
            origin: (x as f32, y as f32),
        }
    }
    fn moving(&self, now: u64) -> bool {
        [&self.x, &self.y, &self.sx, &self.sy, &self.rot, &self.alpha]
            .into_iter()
            .chain(self.clip.iter().flatten())
            .any(|t| !t.done(now))
    }
    /// The object's extent on screen, unscaled.
    fn rect(&self, now: u64) -> [f32; 4] {
        let (x, y) = (self.x.value(now), self.y.value(now));
        match &self.look {
            Look::Image(img) => [x, y, x + img.w as f32, y + img.h as f32],
            Look::Spm { spm, frame, .. } => match spm.frames.get(*frame) {
                Some(f) => {
                    let b = f.bounds;
                    [
                        x + b[0] as f32,
                        y + b[1] as f32,
                        x + b[2] as f32,
                        y + b[3] as f32,
                    ]
                }
                None => [x, y, x, y],
            },
        }
    }
    /// Whether (x, y) is on what the object draws: a frame's bounds can be the whole screen
    /// while its parts are one menu item.
    fn hit(&self, now: u64, x: f32, y: f32) -> bool {
        let inside = |r: [f32; 4]| x >= r[0] && y >= r[1] && x < r[2] && y < r[3];
        match &self.look {
            Look::Spm { spm, frame, .. } => {
                let (ox, oy) = (self.x.value(now), self.y.value(now));
                spm.frames.get(*frame).is_some_and(|f| {
                    f.parts.iter().any(|p| {
                        let d = p.dst;
                        inside([
                            ox + d[0] as f32,
                            oy + d[1] as f32,
                            ox + d[2] as f32,
                            oy + d[3] as f32,
                        ])
                    })
                })
            }
            Look::Image(_) => inside(self.rect(now)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Wait {
    None,
    Until(u64),
    Transition,
    /// an object's moves (-1: all objects')
    Obj(i32),
    /// a click on a button
    Select,
    /// a click to go on
    Click,
}

/// The objects on screen by (plane, number).
type Objs = BTreeMap<(i32, i32), Obj>;

pub struct Engine {
    /// the exe build whose builtin numbering the scripts use
    pub build: &'static Build,
    /// 2007: the label of the file to run when the running label ends (set by 0xff06)
    after_start: Option<u32>,
    /// the button picked at the last selection (0x62)
    selected: i32,
    /// the threads that called a bank (0x48/0x49), each stopped at its call
    calls: Vec<Thread>,
    files: Box<dyn Files>,
    arcs: Archives,
    /// archives in the order files are looked up (Config.pac/packlist.dat)
    order: Vec<String>,
    globals: Globals,
    thread: Thread,
    scripts: HashMap<String, Rc<Script>>,
    visuals: Vec<Vec<Cell>>,
    /// Stand.dat: character pictures (row: ?, name, face picture, ?, ?, sprite map, frame, ...)
    stands: Vec<Vec<Cell>>,
    /// Event.dat: the story's scenes (row: type, title, ?, ?, file, bank, ...)
    events: Vec<Vec<Cell>>,
    bgm: Vec<Vec<Cell>>,
    current_bgm: Option<usize>,
    images: HashMap<String, Rc<Image>>,
    spms: HashMap<String, Rc<Spm>>,
    fonts: HashMap<String, Option<Rc<Font>>>,
    plane: i32,
    objs: BTreeMap<(i32, i32), Obj>,
    /// the background: visual.dat row
    background: Option<usize>,
    transition: Option<(Rc<Image>, u64, u64)>,
    /// the scene as it was drawn last frame, for transitions: objects, background
    before: Option<(Objs, Option<usize>)>,
    wait: Wait,
    now: u64,
    flags: HashMap<i32, i32>,
    sysflags: HashMap<i32, i32>,
    seed: u32,
    click: Option<(i32, i32)>,
    /// the parameters of the bank a jump (0x45) starts at
    pending_params: Vec<i32>,
    /// the message window's text: pictures and where they go (empty: the window is closed)
    message: Vec<(Rc<Image>, i32, i32)>,
    /// the player's family and given names (@u00, @u01)
    pub names: Vec<Vec<u8>>,
    visual_images: HashMap<usize, Option<(Rc<Image>, i32, i32)>>,
    pub screen: Image,
    sounds: Vec<Sound>,
    logged: HashSet<u32>,
    pub log: bool,
    /// print every builtin call
    pub log_calls: bool,
    /// the font for games without .fnt fonts (set_font)
    ttf: Option<Rc<fontdue::Font>>,
}

impl Engine {
    pub fn new(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arcs = Archives::open(files.as_mut());
        if !arcs.has("Script") {
            return Err("no Script.pac".into());
        }
        let mut order: Vec<String> = arcs
            .read(files.as_mut(), "Config", "packlist.dat")
            .and_then(|b| dat::parse(&b).ok())
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| {
                        r.first()
                            .map(|c| c.str().trim_end_matches(".pac").to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();
        for p in ["System", "Visual", "Stand", "Face", "Config", "Thumbnail"] {
            if !order.iter().any(|o| o.eq_ignore_ascii_case(p)) {
                order.push(p.to_string());
            }
        }
        let visuals = arcs
            .read(files.as_mut(), "Config", "visual.dat")
            .map(|b| dat::parse(&b))
            .transpose()?
            .unwrap_or_default();
        let stands = arcs
            .read(files.as_mut(), "Config", "Stand.dat")
            .map(|b| dat::parse(&b))
            .transpose()?
            .unwrap_or_default();
        let events = arcs
            .read(files.as_mut(), "Config", "Event.dat")
            .map(|b| dat::parse(&b))
            .transpose()?
            .unwrap_or_default();
        let bgm = arcs
            .read(files.as_mut(), "Config", "BGM.dat")
            .and_then(|b| dat::parse(&b).ok())
            .unwrap_or_default();
        let (width, height) = arcs
            .read(files.as_mut(), "Config", "System.dat")
            .and_then(|b| dat::parse(&b).ok())
            .map_or((WIDTH, HEIGHT), |rows| screen_size(&rows));
        let mut scripts = HashMap::new();
        let build = arcs
            .read(files.as_mut(), "Script", "System.bin")
            .and_then(|b| script::parse(&b).ok())
            .map_or(&BUILDS[0], |s| Build::detect(&s));
        let system = load_script(&arcs, files.as_mut(), &mut scripts, "System.bin", build)?;
        let mut thread = Thread::new("System.bin", system);
        // System.bin label 0 starts the game (System.dat's entry table)
        thread.goto_label(0);
        thread.label_ends = build.early;
        let mut e = Engine {
            after_start: None,
            selected: -1,
            build,
            calls: Vec::new(),
            files,
            arcs,
            order,
            globals: Globals::default(),
            thread,
            scripts,
            visuals,
            stands,
            events,
            bgm,
            current_bgm: None,
            images: HashMap::new(),
            spms: HashMap::new(),
            fonts: HashMap::new(),
            plane: 0,
            objs: BTreeMap::new(),
            background: None,
            transition: None,
            before: None,
            wait: Wait::None,
            now: 0,
            flags: HashMap::new(),
            sysflags: HashMap::new(),
            seed: 0x2545_f491,
            click: None,
            pending_params: Vec::new(),
            message: Vec::new(),
            names: vec![Vec::new(), sjis::encode("主人公")],
            visual_images: HashMap::new(),
            screen: Image::new(width, height),
            sounds: Vec::new(),
            logged: HashSet::new(),
            log: false,
            log_calls: false,
            ttf: None,
        };
        let mut t = e.thread.clone();
        let mut g = std::mem::take(&mut e.globals);
        t.run_init(&mut g, &mut e)?;
        e.globals = g;
        e.thread = t;
        Ok(e)
    }

    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    /// The objects on screen, one per line, for debugging.
    pub fn describe(&self) -> String {
        let now = self.now;
        let mut out = format!(
            "frame {} wait {:?} at {}:{}\n",
            now, self.wait, self.thread.name, self.thread.frames[0].ip
        );
        for ((plane, id), o) in &self.objs {
            let r = o.rect(now);
            out += &format!(
                "{plane}/{id}: rect {:?} scale {:.2} rot {:.0} alpha {:.2}{}{}\n",
                r.map(|v| v as i32),
                o.sx.value(now),
                o.rot.value(now),
                o.alpha.value(now),
                if o.add { " add" } else { "" },
                if o.button { " button" } else { "" },
            );
        }
        out
    }

    /// The scripts have run to their end.
    pub fn ended(&self) -> bool {
        self.thread.done
    }

    /// A click at (x, y) on the screen.
    pub fn click(&mut self, x: i32, y: i32) {
        self.click = Some((x, y));
    }

    /// Traces every instruction the script runs to stderr.
    /// A TrueType font for the text of builds that draw with Windows fonts.
    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())?;
        self.ttf = Some(Rc::new(f));
        self.fonts.clear();
        Ok(())
    }

    pub fn set_trace(&mut self, on: bool) {
        self.thread.trace = on;
    }

    /// Runs one frame. [`draw`](Engine::draw) draws it.
    pub fn frame(&mut self) -> Result<(), String> {
        self.now += 1;
        self.before = Some((self.objs.clone(), self.background));
        let click = self.click.take();
        self.wait = match self.wait {
            Wait::Until(t) if self.now < t => self.wait,
            Wait::Transition if self.transition.is_some() => self.wait,
            Wait::Obj(id)
                if self
                    .objs
                    .iter()
                    .any(|(k, o)| (id < 0 || k.1 == id) && o.moving(self.now)) =>
            {
                self.wait
            }
            Wait::Select => match click.and_then(|(x, y)| self.button_at(x, y)) {
                Some(id) => {
                    // 0x62's result (2007 reads it again with 0xff05)
                    self.thread.regs[0] = id;
                    self.selected = id;
                    Wait::None
                }
                None => Wait::Select,
            },
            Wait::Click if click.is_none() => Wait::Click,
            _ => Wait::None,
        };
        let mut steps = 0;
        while self.wait == Wait::None && !self.thread.done {
            steps += 1;
            if steps > 2_000_000 {
                let at: Vec<String> = self
                    .thread
                    .frames
                    .iter()
                    .map(|f| format!("{:?}:{}", f.func, f.ip))
                    .collect();
                return Err(format!(
                    "{} runs without stopping at {}",
                    self.thread.name,
                    at.join(" ")
                ));
            }
            let mut thread = std::mem::replace(
                &mut self.thread,
                Thread::new("", Rc::new(Script::default())),
            );
            let mut globals = std::mem::take(&mut self.globals);
            let step = thread.step(&mut globals, self);
            self.globals = globals;
            self.thread = thread;
            match step? {
                Step::Next => {}
                Step::Yield => break,
                Step::End => {
                    if let Some(label) = self.after_start.take()
                        && self.thread.goto_label(label)
                    {
                        continue;
                    }
                    break;
                }
                Step::Jump { file, label } => {
                    let params = std::mem::take(&mut self.pending_params);
                    let s = load_script(
                        &self.arcs,
                        self.files.as_mut(),
                        &mut self.scripts,
                        &file,
                        self.build,
                    )?;
                    let trace = self.thread.trace;
                    let mut t = Thread::new(&file, s);
                    t.trace = trace;
                    t.label_ends = self.build.early;
                    let mut g = std::mem::take(&mut self.globals);
                    let init = t.run_init(&mut g, self);
                    self.globals = g;
                    init?;
                    self.thread = t;
                    if !self.thread.enter_bank(label, &params) {
                        return Err(format!("{file} has no label {label}"));
                    }
                }
            }
        }
        if let Some((_, start, len)) = &self.transition
            && self.now >= start + len
        {
            self.transition = None;
        }
        Ok(())
    }

    /// Draws the frame to `screen`.
    pub fn draw(&mut self) -> &Image {
        self.render();
        &self.screen
    }

    /// The screen as it was at the end of the last frame.
    fn last_screen(&mut self) -> Image {
        let Some((objs, background)) = self.before.clone() else {
            return self.draw().clone();
        };
        let now = self.now;
        self.now -= 1;
        let objs = std::mem::replace(&mut self.objs, objs);
        let background = std::mem::replace(&mut self.background, background);
        self.render();
        (self.now, self.objs, self.background) = (now, objs, background);
        self.screen.clone()
    }

    fn button_at(&self, x: i32, y: i32) -> Option<i32> {
        let (x, y) = (x as f32, y as f32);
        self.objs
            .iter()
            .rev()
            .find(|(_, o)| o.button && o.hit(self.now, x, y))
            .map(|(k, _)| k.1)
    }

    fn find(&mut self, name: &str) -> Option<Vec<u8>> {
        let alt = name.strip_suffix(".bmp").map(|s| format!("{s}.png"));
        for pack in &self.order {
            for n in std::iter::once(name).chain(alt.as_deref()) {
                if let Some(b) = self.arcs.read(self.files.as_mut(), pack, n) {
                    return Some(b);
                }
            }
        }
        None
    }

    fn image(&mut self, name: &str) -> Option<Rc<Image>> {
        let key = name.to_lowercase();
        if let Some(i) = self.images.get(&key) {
            return Some(i.clone());
        }
        let img = match self.find(name).map(|b| Image::decode(&b)) {
            Some(Ok(i)) => Rc::new(i),
            Some(Err(e)) => {
                eprintln!("nexas: {name}: {e}");
                return None;
            }
            None => {
                eprintln!("nexas: no image {name}");
                return None;
            }
        };
        self.images.insert(key, img.clone());
        Some(img)
    }

    fn spm(&mut self, name: &str) -> Option<Rc<Spm>> {
        let key = name.to_lowercase();
        if let Some(s) = self.spms.get(&key) {
            return Some(s.clone());
        }
        let spm = match self.find(name).map(|b| Spm::parse(&b)) {
            Some(Ok(s)) => Rc::new(s),
            Some(Err(e)) => {
                eprintln!("nexas: {name}: {e}");
                return None;
            }
            None => {
                eprintln!("nexas: no sprite map {name}");
                return None;
            }
        };
        self.spms.insert(key, spm.clone());
        Some(spm)
    }

    fn font(&mut self, name: &str) -> Option<Rc<Font>> {
        let key = name.to_lowercase();
        if let Some(f) = self.fonts.get(&key) {
            return f.clone();
        }
        let font = match self.find(name).map(|b| Font::parse(&b)) {
            Some(Ok(f)) => Some(Rc::new(f)),
            Some(Err(e)) => {
                eprintln!("nexas: {name}: {e}");
                None
            }
            // a cell size from the name (ＭＳゴシック24x24.fnt), else 24
            None if self.ttf.is_some() => {
                let size = name
                    .split(|c: char| !c.is_ascii_digit())
                    .find_map(|d| d.parse().ok())
                    .unwrap_or(24);
                self.ttf.clone().map(|t| Rc::new(Font::outline(t, size)))
            }
            None => {
                eprintln!("nexas: no font {name}");
                None
            }
        };
        self.fonts.insert(key, font.clone());
        font
    }

    /// Puts a message in the window (see [`WINDOW`]).
    fn show_message(&mut self, name: &[u8], text: &[u8]) {
        const PAD: i32 = 8;
        self.message.clear();
        let Some(font) = self.font(MESSAGE_FONT) else {
            return;
        };
        let text_y = TEXT.1 + self.screen.h as i32 - HEIGHT as i32;
        let name = message_lines(name, &self.names).concat();
        if !name.is_empty() {
            let img = font.render(&name, [255, 255, 255], PAD);
            self.message
                .push((Rc::new(img), TEXT.0 - PAD, text_y - LINE - PAD));
        }
        for (k, line) in message_lines(text, &self.names).iter().enumerate() {
            let img = font.render(line, [255, 255, 255], PAD);
            self.message
                .push((Rc::new(img), TEXT.0 - PAD, text_y + LINE * k as i32 - PAD));
        }
    }

    /// The picture of visual.dat row `n` (the base with a part over it) and its scroll offset.
    fn visual(&mut self, n: usize) -> Option<(Rc<Image>, i32, i32)> {
        if let Some(v) = self.visual_images.get(&n) {
            return v.clone();
        }
        let v = self.compose_visual(n);
        self.visual_images.insert(n, v.clone());
        v
    }

    fn compose_visual(&mut self, n: usize) -> Option<(Rc<Image>, i32, i32)> {
        let row = self.visuals.get(n)?.clone();
        let cell = |k: usize| row.get(k).map_or(0, |c| c.int());
        let base = self.image(row.get(8)?.str())?;
        let over = match row.get(9).map(|c| c.str()) {
            Some(name) if !name.is_empty() => self.image(name),
            _ => None,
        };
        let img = match over {
            Some(over) => {
                let mut img = (*base).clone();
                let (x, y, w, h) = (cell(10), cell(11), over.w as i32, over.h as i32);
                let p = place(0.0, 0.0, 1.0, 1.0, 0.0, 1.0, false);
                gfx::draw(
                    &mut img,
                    &over,
                    [0, 0, w, h],
                    [x as f32, y as f32, (x + w) as f32, (y + h) as f32],
                    &p,
                );
                Rc::new(img)
            }
            None => base,
        };
        Some((img, cell(3), cell(4)))
    }

    /// What to draw for this frame, bottom first, over black.
    pub fn draws(&mut self) -> Vec<Draw> {
        let now = self.now;
        let mut out = Vec::new();
        if let Some((img, sx, sy)) = self.background.and_then(|n| self.visual(n)) {
            let p = place(0.0, 0.0, 1.0, 1.0, 0.0, 1.0, false);
            let (w, h) = (img.w as i32, img.h as i32);
            out.push(Draw {
                image: img.clone(),
                src: [0, 0, w, h],
                dst: [-sx as f32, -sy as f32, (w - sx) as f32, (h - sy) as f32],
                place: p,
            });
        }
        for o in self.objs.values() {
            let (x, y) = (o.x.value(now), o.y.value(now));
            let mut alpha = o.alpha.value(now);
            if let Some((period, from, to, start)) = o.blink {
                let t = ((now - start) % period.max(1)) as f32 / period.max(1) as f32;
                let tri = 1.0 - (2.0 * t - 1.0).abs();
                alpha *= from + (to - from) * tri;
            }
            let mut p = place(
                x,
                y,
                o.sx.value(now),
                o.sy.value(now),
                o.rot.value(now),
                alpha,
                o.add,
            );
            p.flip = o.flip;
            if let Some(c) = &o.clip {
                p.clip = [
                    c[0].value(now),
                    c[1].value(now),
                    c[2].value(now),
                    c[3].value(now),
                ];
            }
            if let Some((px, py)) = o.pivot {
                let r = o.rect(now);
                let half = |v: i32, lo: f32, hi: f32, at: f32| {
                    if v == -999_999 {
                        (lo + hi) / 2.0
                    } else {
                        at + v as f32
                    }
                };
                (p.cx, p.cy) = (half(px, r[0], r[2], x), half(py, r[1], r[3], y));
            }
            match &o.look {
                Look::Image(img) => {
                    let (w, h) = (img.w as i32, img.h as i32);
                    out.push(Draw {
                        image: img.clone(),
                        src: [0, 0, w, h],
                        dst: [x, y, x + w as f32, y + h as f32],
                        place: p,
                    });
                }
                Look::Spm { spm, frame, images } => {
                    let Some(f) = spm.frames.get(*frame) else {
                        continue;
                    };
                    for part in &f.parts {
                        let Some(Some(img)) = images.get(part.image) else {
                            continue;
                        };
                        let d = part.dst;
                        let dst = [
                            x + d[0] as f32,
                            y + d[1] as f32,
                            x + d[2] as f32,
                            y + d[3] as f32,
                        ];
                        out.push(Draw {
                            image: img.clone(),
                            src: part.src,
                            dst,
                            place: p,
                        });
                    }
                }
            }
        }
        if !self.message.is_empty() {
            if let Some(spm) = self.spm("Interface.spm") {
                let images: Vec<Option<Rc<Image>>> =
                    spm.images.iter().take(1).map(|n| self.image(n)).collect();
                let p = place(0.0, 0.0, 1.0, 1.0, 0.0, 1.0, false);
                for part in spm
                    .frames
                    .first()
                    .map_or(&[][..], |f| &f.parts[..1.min(f.parts.len())])
                {
                    let Some(Some(img)) = images.get(part.image) else {
                        continue;
                    };
                    let (x, y) = (WINDOW.0, WINDOW.1 + self.screen.h as f32 - HEIGHT as f32);
                    let d = part.dst.map(|v| v as f32);
                    out.push(Draw {
                        image: img.clone(),
                        src: part.src,
                        dst: [x + d[0], y + d[1], x + d[2], y + d[3]],
                        place: p,
                    });
                }
            }
            let p = place(0.0, 0.0, 1.0, 1.0, 0.0, 1.0, false);
            for (img, x, y) in &self.message {
                let (w, h) = (img.w as i32, img.h as i32);
                out.push(Draw {
                    image: img.clone(),
                    src: [0, 0, w, h],
                    dst: [*x as f32, *y as f32, (x + w) as f32, (y + h) as f32],
                    place: p,
                });
            }
        }
        // the screen before the transition, fading out over it
        if let Some((from, start, len)) = &self.transition {
            let t = ((now - start) as f32 / (*len).max(1) as f32).clamp(0.0, 1.0);
            let (w, h) = (from.w as i32, from.h as i32);
            out.push(Draw {
                image: from.clone(),
                src: [0, 0, w, h],
                dst: [0.0, 0.0, w as f32, h as f32],
                place: place(0.0, 0.0, 1.0, 1.0, 0.0, 1.0 - t, false),
            });
        }
        out
    }

    fn render(&mut self) {
        let mut out = Image::new(self.screen.w, self.screen.h);
        out.fill([0, 0, 0, 255]);
        for d in self.draws() {
            gfx::draw(&mut out, &d.image, d.src, d.dst, &d.place);
        }
        self.screen = out;
    }

    fn obj(&mut self, id: i32) -> Option<&mut Obj> {
        self.objs.get_mut(&(self.plane, id))
    }

    fn add_obj(&mut self, id: i32, look: Look, x: i32, y: i32) {
        self.objs.insert((self.plane, id), Obj::new(look, x, y));
    }

    fn random(&mut self, n: i32) -> i32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        if n <= 0 {
            0
        } else {
            (self.seed % n as u32) as i32
        }
    }
}

fn place(x: f32, y: f32, sx: f32, sy: f32, deg: f32, alpha: f32, add: bool) -> Place {
    Place {
        x,
        y,
        sx,
        sy,
        cx: x,
        cy: y,
        rot: deg.to_radians(),
        alpha,
        add,
        flip: (false, false),
        clip: [f32::MIN, f32::MIN, f32::MAX, f32::MAX],
    }
}

fn load_script(
    arcs: &Archives,
    files: &mut dyn Files,
    cache: &mut HashMap<String, Rc<Script>>,
    name: &str,
    build: &Build,
) -> Result<Rc<Script>, String> {
    let key = name.to_lowercase();
    if let Some(s) = cache.get(&key) {
        return Ok(s.clone());
    }
    // Older builds name scripts without the extension.
    let b = arcs
        .read(files, "Script", name)
        .or_else(|| arcs.read(files, "Script", &format!("{name}.bin")))
        .ok_or(format!("Script.pac has no {name}"))?;
    let mut s = script::parse(&b).map_err(|e| format!("{name}: {e}"))?;
    build.translate(&mut s);
    let s = Rc::new(s);
    cache.insert(key, s.clone());
    Ok(s)
}

fn describe_args(args: &[Value]) -> String {
    let a: Vec<String> = args
        .iter()
        .map(|v| match v {
            Value::Int(i) => i.to_string(),
            Value::Str(s) => format!("{:?}", sjis::decode(s)),
        })
        .collect();
    format!("({})", a.join(", "))
}

fn arg(args: &[Value], k: usize) -> i32 {
    args.get(k).map_or(0, |v| v.int())
}

fn bytes(args: &[Value], k: usize) -> Vec<u8> {
    args.get(k).map(|v| v.bytes()).unwrap_or_default()
}

/// The byte offset of character `k` of Shift_JIS `s` (its length if `s` is shorter).
fn char_at(s: &[u8], k: usize) -> usize {
    sjis::chars(s).iter().take(k).map(|c| c.len()).sum()
}

fn message_voice(s: &[u8]) -> Option<String> {
    let chars = sjis::chars(s);
    for window in chars.windows(10) {
        if window[0] == b"@"
            && window[1] == b"v"
            && window[2..]
                .iter()
                .all(|c| c.len() == 1 && c[0].is_ascii_digit())
        {
            return Some(window[2..].iter().map(|c| c[0] as char).collect());
        }
    }
    None
}

/// The lines of a message without its commands: `@n` new line, `@r` text `@` ruby `@` (the
/// ruby is left out), `@g` and a character (kept), `@u` and two digits (the player's names),
/// `@v` and eight digits (voice), `@s` and four digits, other `@` and a letter.
fn message_lines(s: &[u8], names: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut lines = vec![Vec::new()];
    let mut chars = sjis::chars(s).into_iter();
    let digits = |chars: &mut std::vec::IntoIter<&[u8]>, n: usize| -> usize {
        let d: String = chars.by_ref().take(n).map(|c| c[0] as char).collect();
        d.parse().unwrap_or(0)
    };
    while let Some(c) = chars.next() {
        let line = lines.last_mut().unwrap();
        if c != b"@" {
            line.extend_from_slice(c);
            continue;
        }
        match chars.next() {
            Some(b"n") => lines.push(Vec::new()),
            Some(b"g") => line.extend(chars.next().unwrap_or_default()),
            Some(b"u") => line.extend(names.get(digits(&mut chars, 2)).map_or(&[][..], |n| n)),
            Some(b"v") => _ = digits(&mut chars, 8),
            Some(b"s") => _ = digits(&mut chars, 4),
            // a speed or size change (2011): two digits
            Some(b"m") => _ = digits(&mut chars, 2),
            // a font size (2008): two digits
            Some(b"f") => _ = digits(&mut chars, 2),
            Some(b"r") => {
                for c in chars.by_ref().take_while(|&c| c != b"@") {
                    line.extend_from_slice(c);
                }
                chars.by_ref().take_while(|&c| c != b"@").for_each(drop);
            }
            _ => {}
        }
    }
    lines
}

fn text(args: &[Value], k: usize) -> String {
    args.get(k).map(|v| v.text()).unwrap_or_default()
}

impl Host for Engine {
    fn builtin(
        &mut self,
        vm: &mut Thread,
        _g: &mut Globals,
        id: u32,
        args: &[Value],
    ) -> Result<(Value, Flow), String> {
        let local = id;
        let id = self.build.canonical(id);
        if self.log_calls {
            eprintln!(
                "nexas: {}:{} b{id:#x} (b{local:#x} here) {}",
                vm.name,
                vm.frames[0].ip,
                describe_args(args)
            );
        }
        let a = |k| arg(args, k);
        let now = self.now;
        let mut flow = Flow::Next;
        let mut out = 0;
        match id {
            // 2010 build: one-shot sound effect (name, volume, ...).
            0xff00 | 0xff03 => {
                let channel = (id == 0xff03).then(|| a(0));
                let name = text(args, if channel.is_some() { 1 } else { 0 });
                if !name.is_empty() {
                    let mut data = None;
                    for pack in ["System", "Se"] {
                        for extension in ["", ".ogg", ".wav"] {
                            let path = format!("{name}{extension}");
                            if let Some(bytes) = self.arcs.read(self.files.as_mut(), pack, &path) {
                                data = Some(bytes);
                                break;
                            }
                        }
                        if data.is_some() {
                            break;
                        }
                    }
                    if let Some(data) = data {
                        self.sounds.push(Sound {
                            data,
                            volume: a(if channel.is_some() { 4 } else { 1 }).clamp(0, 100) as f32
                                / 100.0,
                            bgm: false,
                            voice: false,
                            effect_channel: channel,
                            stop: false,
                            fade_ms: if channel.is_some() {
                                (a(2) as u32 & 0xffff) * 10
                            } else {
                                0
                            },
                            loop_start: 0,
                        });
                    }
                }
            }
            0xff04 => self.sounds.push(Sound {
                data: Vec::new(),
                volume: 0.0,
                bgm: false,
                voice: false,
                effect_channel: Some(a(0)),
                stop: true,
                fade_ms: (a(1) as u32 & 0xffff) * 10,
                loop_start: 0,
            }),
            // 2010 build: BGM table row, fade time, ..., volume, ...
            0xff01 => {
                let track = usize::try_from(a(0)).ok();
                if track.is_some() && self.current_bgm == track {
                    return Ok((Value::Int(out), flow));
                }
                if let Some(row) = track.and_then(|n| self.bgm.get(n)) {
                    let name = row.first().map_or("", Cell::str);
                    let loop_start = row.get(2).map_or(0, Cell::int).max(0) as u32;
                    let data = ["", ".ogg", ".wav"].iter().find_map(|ext| {
                        self.arcs
                            .read(self.files.as_mut(), "Bgm", &format!("{name}{ext}"))
                    });
                    if let Some(data) = data {
                        self.current_bgm = track;
                        self.sounds.push(Sound {
                            data,
                            volume: a(3).clamp(0, 100) as f32 / 100.0,
                            bgm: true,
                            voice: false,
                            effect_channel: None,
                            stop: false,
                            fade_ms: (a(1) as u32 & 0xffff) * 10,
                            loop_start,
                        });
                    }
                }
            }
            0xff02 => {
                self.current_bgm = None;
                self.sounds.push(Sound {
                    data: Vec::new(),
                    volume: 0.0,
                    bgm: true,
                    voice: false,
                    effect_channel: None,
                    stop: true,
                    fade_ms: (a(0) as u32 & 0xffff) * 10,
                    loop_start: 0,
                });
            }
            // random number below n
            0x4 => out = self.random(a(0)),
            // wait (time)
            0x5 => {
                self.wait = Wait::Until(now + frames(a(0)));
                flow = Flow::Yield;
            }
            // go to (file, bank, 8 parameters)
            0x45 => {
                self.pending_params = (2..10).map(a).collect();
                flow = Flow::Jump {
                    file: text(args, 0),
                    label: a(1) as u32,
                };
            }
            // start a scene of Event.dat (event, 8 parameters)
            0x46 => {
                let row = self
                    .events
                    .get(a(0) as usize)
                    .ok_or(format!("no event {}", a(0)))?;
                let (file, label) = (
                    row.get(4).map_or("", |c| c.str()).to_string(),
                    row.get(5).map_or(0, |c| c.int()),
                );
                self.pending_params = (1..9).map(a).collect();
                flow = Flow::Jump {
                    file,
                    label: label as u32,
                };
            }
            // call a bank of another file (file, bank, 8 parameters); 0x4a comes back
            0x48 => {
                self.calls.push(vm.clone());
                self.pending_params = (2..10).map(a).collect();
                flow = Flow::Jump {
                    file: text(args, 0),
                    label: a(1) as u32,
                };
            }
            // go to (0x47) or call (0x49) a bank of this file (bank, 8 parameters), keeping
            // its variables
            0x47 | 0x49 => {
                if id == 0x49 {
                    self.calls.push(vm.clone());
                }
                let params: Vec<i32> = (1..9).map(a).collect();
                if !vm.enter_bank(a(0) as u32, &params) {
                    return Err(format!("{} has no bank {}", vm.name, a(0)));
                }
            }
            // return from 0x48/0x49 to the instruction after the call (none: nothing)
            0x4a => {
                if let Some(t) = self.calls.pop() {
                    *vm = t;
                }
            }
            // flags: set (n, v) / get (n); system flags: set (n, v, ?) / get (n)
            0x54 => {
                self.flags.insert(a(0), a(1));
            }
            0x55 => out = self.flags.get(&a(0)).copied().unwrap_or(0),
            0x56 => {
                self.sysflags.insert(a(0), a(1));
            }
            0x57 => out = self.sysflags.get(&a(0)).copied().unwrap_or(0),
            // wait for a click on a button (name, hover bank, ...); the button's object
            // number is the result, which frame() puts in A
            0x62 => {
                self.wait = Wait::Select;
                flow = Flow::Yield;
            }
            // message window: open (type), close, clear the message
            0x64 => {}
            0x65 | 0x69 => self.message.clear(),
            // message (colour: FontColor.dat row, ?, name, text)
            0x67 => {
                if self.log {
                    eprintln!(
                        "nexas: 0x67({}, {}) 「{}」 {}",
                        a(0),
                        a(1),
                        text(args, 2),
                        text(args, 3)
                    );
                }
                let message = bytes(args, 3);
                let data = message_voice(&message).and_then(|id| {
                    self.arcs
                        .read(self.files.as_mut(), "Voice", &format!("{id}.ogg"))
                        .or_else(|| {
                            self.arcs
                                .read(self.files.as_mut(), "Voice", &format!("{id}.wav"))
                        })
                });
                self.sounds.push(Sound {
                    stop: data.is_none(),
                    data: data.unwrap_or_default(),
                    volume: 1.0,
                    bgm: false,
                    voice: true,
                    effect_channel: None,
                    fade_ms: 0,
                    loop_start: 0,
                });
                self.show_message(&bytes(args, 2), &message);
                self.wait = Wait::Click;
                flow = Flow::Yield;
            }
            // background (visual, ...)
            0x78 => self.background = usize::try_from(a(0)).ok(),
            // a free object number
            0x87 => {
                out = (100..)
                    .find(|&n| !self.objs.contains_key(&(self.plane, n)))
                    .unwrap_or(0)
            }
            // transitions (time, no wait)
            0x8a | 0x90 => {
                let len = frames(a(0));
                if len > 0 {
                    self.transition = Some((Rc::new(self.last_screen()), now, len));
                    if a(1) == 0 {
                        self.wait = Wait::Transition;
                        flow = Flow::Yield;
                    }
                }
            }
            // objects: picture (n, file, x, y, ...), sprite map (n, file, frame, x, y, ...),
            // visual (n, visual, x, y, ...)
            0x137 => {
                if let Some(img) = self.image(&text(args, 1)) {
                    self.add_obj(a(0), Look::Image(img), a(2), a(3));
                }
            }
            0x138 => {
                if let Some(spm) = self.spm(&text(args, 1)) {
                    let images = spm.images.clone().iter().map(|n| self.image(n)).collect();
                    let look = Look::Spm {
                        spm,
                        frame: a(2).max(0) as usize,
                        images,
                    };
                    self.add_obj(a(0), look, a(3), a(4));
                }
            }
            // sprite map with a pattern (n, file, animation, pattern, x, y, ...)
            0x139 => {
                if let Some(spm) = self.spm(&text(args, 1)) {
                    let images = spm.images.clone().iter().map(|n| self.image(n)).collect();
                    let look = Look::Spm {
                        spm,
                        frame: a(2).max(0) as usize,
                        images,
                    };
                    self.add_obj(a(0), look, a(4), a(5));
                }
            }
            // a filled rectangle (n, left, top, right, bottom, r, g, b, ...)
            0x13c => {
                let (w, h) = ((a(3) - a(1)).max(1) as u32, (a(4) - a(2)).max(1) as u32);
                let mut img = Image::new(w, h);
                let rgb = [a(5), a(6), a(7)].map(|c| c.clamp(0, 255) as u8);
                for p in img.px.chunks_exact_mut(4) {
                    p.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
                self.add_obj(a(0), Look::Image(Rc::new(img)), a(1), a(2));
            }
            0x140 => {
                if let Some((img, sx, sy)) = usize::try_from(a(1)).ok().and_then(|n| self.visual(n))
                {
                    self.add_obj(a(0), Look::Image(img), a(2) - sx, a(3) - sy);
                }
            }
            // move (n, time, x, y, ?, wait), scale (n, time, x%, y%, ?, wait),
            // turn (n, time, degrees, ?, wait), draw (n, time, mode, %, ?, wait)
            0x144 | 0x148 | 0x14a | 0x159 => {
                let (n, t) = (a(0), a(1));
                let wait = if id == 0x14a { a(4) } else { a(5) };
                let relative = self.build.relative_moves;
                if let Some(o) = self.obj(n) {
                    match id {
                        0x144 => {
                            let (ox, oy) = if relative { o.origin } else { (0.0, 0.0) };
                            o.x.go(ox + a(2) as f32, t, now);
                            o.y.go(oy + a(3) as f32, t, now);
                        }
                        0x148 => {
                            o.sx.go(a(2) as f32 / 100.0, t, now);
                            o.sy.go(a(3) as f32 / 100.0, t, now);
                        }
                        0x14a => o.rot.go(a(2) as f32, t, now),
                        _ => {
                            // 描画モード 0: opaque; 3: alpha, 4: added. A switch from mode 0
                            // fades from nothing, so a new object's fade-in starts at 0.
                            if a(2) == 0 {
                                o.blend = false;
                                o.alpha = Tween::at(1.0);
                            } else {
                                if !o.blend {
                                    o.alpha = Tween::at(0.0);
                                    o.blend = true;
                                }
                                o.add = a(2) == 4;
                                o.alpha.go(a(3) as f32 / 100.0, t, now);
                            }
                        }
                    }
                }
                if wait != 0 {
                    self.wait = Wait::Obj(n);
                    flow = Flow::Yield;
                }
            }
            // blink (n, period, from%, to%, ?)
            0x15a => {
                let (period, from, to) =
                    (frames(a(1)) * 2, a(2) as f32 / 100.0, a(3) as f32 / 100.0);
                if let Some(o) = self.obj(a(0)) {
                    o.blink = Some((period, from, to, now));
                }
            }
            // delete an object (n, -1: all of the plane; ?)
            0x143 => {
                let plane = self.plane;
                if a(0) < 0 {
                    self.objs.retain(|k, _| k.0 != plane);
                } else {
                    self.objs.remove(&(plane, a(0)));
                }
            }
            // make an object a button (n, ...)
            0x162 => {
                if let Some(o) = self.obj(a(0)) {
                    o.button = true;
                }
            }
            // wait for an object's moves (n, -1: all)
            0x165 => {
                self.wait = Wait::Obj(a(0));
                flow = Flow::Yield;
            }
            // character pictures: show (slot, Stand.dat row, ?, time, no wait, filter...), with
            // a place and zoom (slot, row, x, y, zoom%, ?, time, no wait, ...); take away (slot,
            // -1: all; time, ?, wait). They stand on the bottom of the screen, under the objects.
            0xf6 | 0xf7 => {
                let (slot, row) = (a(0), a(1));
                let (x, y, t, nowait) = if id == 0xf6 {
                    (0, 0, a(3), a(4))
                } else {
                    (a(2), a(3), a(6), a(7))
                };
                let Some(r) = self.stands.get(row as usize).cloned() else {
                    return Err(format!("no stand {row}"));
                };
                if let Some(spm) = self.spm(r.get(5).map_or("", |c| c.str())) {
                    let images = spm.images.clone().iter().map(|n| self.image(n)).collect();
                    let frame = r.get(6).map_or(0, |c| c.int()).max(0) as usize;
                    let key = (STANDS, slot);
                    let alpha = self.objs.get(&key).map(|o| o.alpha.value(now));
                    let mut o = Obj::new(
                        Look::Spm { spm, frame, images },
                        self.screen.w as i32 / 2 + x,
                        self.screen.h as i32 + y,
                    );
                    match alpha {
                        Some(v) => o.alpha = Tween::at(v),
                        None => {
                            o.alpha = Tween::at(0.0);
                            o.alpha.go(1.0, t, now);
                        }
                    }
                    self.objs.insert(key, o);
                    if nowait == 0 {
                        self.wait = Wait::Obj(slot);
                        flow = Flow::Yield;
                    }
                }
            }
            0xf8 => {
                let (slot, t) = (a(0), a(1));
                if frames(t) == 0 {
                    self.objs
                        .retain(|k, _| k.0 != STANDS || (slot >= 0 && k.1 != slot));
                } else {
                    for (k, o) in self.objs.iter_mut() {
                        if k.0 == STANDS && (slot < 0 || k.1 == slot) {
                            o.alpha.go(0.0, t, now);
                        }
                    }
                }
                if a(3) != 0 {
                    self.wait = Wait::Obj(slot);
                    flow = Flow::Yield;
                }
            }
            // object x / y / exists
            0x17e | 0x17f => {
                if let Some(o) = self.obj(a(0)) {
                    out = if id == 0x17e {
                        o.x.value(now)
                    } else {
                        o.y.value(now)
                    } as i32;
                }
            }
            0x186 => out = self.obj(a(0)).is_some() as i32,
            // text (n, font, text, x, y, r, g, b, ...)
            0x13a => {
                if let Some(font) = self.font(&text(args, 1)) {
                    let rgb = [a(5), a(6), a(7)].map(|v| v.clamp(0, 255) as u8);
                    const PAD: i32 = 8;
                    let img = font.render(
                        &message_lines(&bytes(args, 2), &self.names).concat(),
                        rgb,
                        PAD,
                    );
                    self.add_obj(a(0), Look::Image(Rc::new(img)), a(3) - PAD, a(4) - PAD);
                }
            }
            // clip (n, time, left, top, right, bottom, ?, ?, wait)
            0x15c => {
                let t = a(1);
                if let Some(o) = self.obj(a(0)) {
                    let c = o.clip.get_or_insert([
                        Tween::at(-1e9),
                        Tween::at(-1e9),
                        Tween::at(1e9),
                        Tween::at(1e9),
                    ]);
                    for (k, ck) in c.iter_mut().enumerate() {
                        ck.go(a(2 + k) as f32, t, now);
                    }
                }
                if a(8) != 0 {
                    self.wait = Wait::Obj(a(0));
                    flow = Flow::Yield;
                }
            }
            // flip (n, left-right, top-bottom)
            0x160 => {
                if let Some(o) = self.obj(a(0)) {
                    o.flip = (a(1) != 0, a(2) != 0);
                }
            }
            // the centre of scaling and turning (n, x, y; -999999: the middle)
            0x17a => {
                if let Some(o) = self.obj(a(0)) {
                    o.pivot = Some((a(1), a(2)));
                }
            }
            // the first free object number from `from` towards `to` (from, to, step), or -1
            0x187 => {
                let (mut n, to, step) = (a(0), a(1), a(2).max(1));
                out = loop {
                    if self.obj(n).is_none() {
                        break n;
                    }
                    if n == to {
                        break -1;
                    }
                    n += if n < to { step } else { -step };
                };
            }
            // strings, in characters: byte at (s, i), find (s, part), part (s, from, n),
            // without a part (s, from, n), with text put in (s, at, text), length (s)
            0x39 => {
                out = args.first().map_or(0, |v| {
                    v.bytes().get(a(1) as usize).map_or(0, |&b| b as i8 as i32)
                })
            }
            0x1d1 => {
                let (s, part) = (bytes(args, 0), bytes(args, 1));
                let chars = sjis::chars(&s);
                out = (0..chars.len())
                    .find(|&k| s[char_at(&s, k)..].starts_with(&part))
                    .map_or(-1, |k| k as i32);
            }
            0x1d0 | 0x1cf => {
                let s = bytes(args, 0);
                let from = char_at(&s, a(1).max(0) as usize);
                let to = char_at(&s[from..], a(2).max(0) as usize) + from;
                let r = if id == 0x1d0 {
                    s[from..to].to_vec()
                } else {
                    [&s[..from], &s[to..]].concat()
                };
                return Ok((Value::Str(r), flow));
            }
            0x1ce => {
                let s = bytes(args, 0);
                let at = char_at(&s, a(1).max(0) as usize);
                return Ok((
                    Value::Str([&s[..at], &bytes(args, 2)[..], &s[at..]].concat()),
                    flow,
                ));
            }
            0x1d2 => out = sjis::chars(&bytes(args, 0)).len() as i32,
            // object plane: set / get
            // 2007: the result of the last selection; the label to go on at when this one ends
            0xff05 => out = self.selected,
            0xff06 => self.after_start = u32::try_from(a(0)).ok(),
            0x19b => self.plane = a(0),
            0x1a1 => out = self.plane,
            _ => {
                if self.log && self.logged.insert(id) {
                    eprintln!(
                        "nexas: b{id:#x} (b{local:#x} here) {} not done",
                        describe_args(args)
                    );
                }
            }
        }
        Ok((Value::Int(out), flow))
    }
}
