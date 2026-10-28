//! Runs the scripts a frame at a time: it follows the rows of the ADV scripts until one waits
//! (for a click, a time or a fade), and says what to draw and play.
//!
//! The story starts at the flow named by init.ini's `FirstFlow` (flow\flow00_000.txt: rows of
//! "call script"), or else at the first script of cfg\chapter_file.ini (2012), or else (2014)
//! at every script of scr.pak in name order. The title and the menus
//! of the trials are drawn by the program itself (not by scripts); this engine draws a title from
//! the game's own pictures (obj\d000_000 background, obj\d002_000 logo, the obj\d006_000 button
//! sheet) and its own menu and save slots.
//!
//! Script commands (int 3 of a row; numbers after it are n(0), n(1), ...), as read from the
//! editor's summaries in the scripts:
//!
//! | # | | |
//! |---|-|-|
//! | 0 | | start mark |
//! | 1 | | a note (in a flow: call script s1) |
//! | 2 | ms | wait (in a flow: the end) |
//! | 4 | | message window off |
//! | 5 | -, volume | text: name s1, text s2, voice s3 |
//! | 9 | type, ms | screen update: the changes since the last one come in (type 0 at once, 2 a rule wipe by s1, else a fade) |
//! | 10 / 11 / 13 | | background / CG / standing picture s1 (older trials; sprites 100 / 102 / 200) |
//! | 15 | -, x, y, sprite | load picture s1 into a sprite, its centre at the screen's centre + (x, y) |
//! | 17 | sprite | delete a sprite |
//! | 18 | -, x, y, sprite, sx, sy, .. | change a sprite (n(16) moves, n(17) scales, n(15) opacity / 256) |
//! | 21 | | delete the standing pictures (sprites showing sc\\ pictures) |
//! | 22 | | delete all sprites |
//! | 23 | sprite (n(3)) | swap the sprite's picture for s1 |
//! | 25 | like 18, + n(24) delay, n(25) ms | move / scale a sprite over time |
//! | 26 | sprite | stop 25 |
//! | 30 / 31 / 32 | -, volume, -, fade | music s1 / stop / stop all |
//! | 35 / 36 / 37 | channel, volume, -, -, loop | sound s1 / stop channel / stop all |
//! | 40 | | all sounds stop |
//! | 41 / 42 / 43 | channel, volume | voice s1 / stop / stop all |
//! | 16 | -, -, -, sprite | animated picture s1 (2014; shown still and centred) |
//! | 92 | like 15 | load picture s1 into a sprite (2014) |
//! | 130 / 131 / 138 | -, -, face / face | face picture s1 at the left of the window / delete / delete all (2014) |
//! | 60 | | "replaying a scene?": no in a story (int 37 is the yes way) |
//!
//! Sprites are drawn by number (100 background, 102 CG, 200 characters, ...). A sprite with
//! n(0) = 1 is placed by its centre (x, y from the screen's centre), else by its top left.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place, Rule};

use crate::arc::Archives;
use crate::ini::Ini;
use crate::pic;
use crate::script::{Rec, Script};

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

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    alpha: f32,
}

impl Pose {
    fn mix(a: Pose, b: Pose, p: f32) -> Pose {
        let m = |u: f32, v: f32| u + (v - u) * p;
        Pose {
            x: m(a.x, b.x),
            y: m(a.y, b.y),
            sx: m(a.sx, b.sx),
            sy: m(a.sy, b.sy),
            alpha: m(a.alpha, b.alpha),
        }
    }
}

#[derive(Clone)]
struct Sprite {
    path: String,
    image: Rc<Image>,
    pose: Pose,
    /// (from, start, ms): moving from `from` to `pose`
    tween: Option<(Pose, f64, f64)>,
}

impl Sprite {
    fn pose_at(&self, now: f64) -> Pose {
        match self.tween {
            Some((from, start, ms)) => {
                let p = ((now - start) / ms.max(1.0)).clamp(0.0, 1.0) as f32;
                Pose::mix(from, self.pose, p)
            }
            None => self.pose,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
enum Wait {
    None,
    Click,
    Until(f64),
    Fade,
    Title,
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
    /// 9 of type 2: the old picture goes where its rule picture (`effect\*.bmp`) is dark first
    rule: Option<Rc<Image>>,
}

/// How soft a rule wipe's edge is (part of the rule's brightness range).
const RULE_VAGUE: f32 = 0.25;

/// One running script: its file and the id of the next row.
#[derive(Clone, Debug)]
struct Frame {
    script: String,
    next: i32,
}

/// The text window, from init.ini.
struct Window {
    file: String,
    x: i32,
    y: i32,
    name: (i32, i32),
    text: (i32, i32),
    size: i32,
    step: i32,
    chars: usize,
    color: [u8; 3],
    name_color: [u8; 3],
    /// the click-wait mark: file, x, y, frames side by side, ms a frame
    mark: Option<(String, i32, i32, i32, f64)>,
}

pub struct Engine {
    files: Box<dyn Files>,
    arcs: Archives,
    pub width: u32,
    pub height: u32,
    pub log: bool,
    font: Option<Rc<fontdue::Font>>,
    win: Window,
    first: Vec<String>,
    title_bgm: Option<String>,
    scripts: HashMap<String, Rc<Script>>,
    stack: Vec<Frame>,
    sprites: BTreeMap<i32, Sprite>,
    /// face pictures (2014) shown at the left of the message window: (path, picture)
    faces: BTreeMap<i32, (String, Rc<Image>)>,
    window: bool,
    name: Option<String>,
    lines: Vec<String>,
    wait: Wait,
    view: View,
    frozen: Option<Vec<Draw>>,
    fade: Option<Fade>,
    rules: Vec<(usize, Rule)>,
    now: f64,
    bgm: Option<(String, f32)>,
    loops: BTreeMap<i32, (String, f32)>,
    sounds: Vec<SoundEvent>,
    images: HashMap<String, Rc<Image>>,
    text_cache: HashMap<(String, [u8; 3], i32), Rc<Image>>,
    pointer: (i32, i32),
    skip: bool,
    ended: bool,
    /// where a save made now starts again: the stack after the shown text
    saved_at: Option<Vec<Frame>>,
}

fn white(w: u32, h: u32, rgba: [u8; 4]) -> Rc<Image> {
    let mut i = Image::new(w, h);
    i.fill(rgba);
    Rc::new(i)
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arcs = Archives::open(files.as_mut());
        if !arcs.packs.contains_key("scr") {
            return Err("no scr.pak (NEKOPACK) in the game folder".into());
        }
        let ini = Ini::parse(
            &arcs
                .read(files.as_mut(), "cfg\\init.ini")
                .unwrap_or_default(),
        );
        let (width, height) = (
            ini.int("WindowWidth", 1280).clamp(320, 4096) as u32,
            ini.int("WindowHeight", 720).clamp(240, 4096) as u32,
        );
        let rgb = |k: &str| {
            [
                ini.int(&format!("{k}R"), 255) as u8,
                ini.int(&format!("{k}G"), 255) as u8,
                ini.int(&format!("{k}B"), 255) as u8,
            ]
        };
        let size = ini.int("MsgFontSize", 26).clamp(8, 96);
        let win = Window {
            file: ini.str("MsgWinFilename").unwrap_or("").to_string(),
            x: ini.int("MsgWinPosX", 0),
            y: ini.int("MsgWinPosY", height as i32 - 240),
            name: (ini.int("MsgNamePosX", 140), ini.int("MsgNamePosY", 558)),
            text: (ini.int("MsgStringPosX", 140), ini.int("MsgStringPosY", 588)),
            size,
            step: size + ini.int("MsgLineInterval", 5).clamp(0, 64),
            chars: (ini.int("MsgMaxCharNum", 64).clamp(8, 200) / 2) as usize,
            color: rgb("MsgColor"),
            name_color: rgb("MsgNameColor"),
            mark: ini.str("MsgWaitMarkFilename").map(|f| {
                (
                    f.to_string(),
                    ini.int("MsgWaitMarkPosX", 0),
                    ini.int("MsgWaitMarkPosY", 0),
                    ini.int("MsgWaitMarkPattern", 1).clamp(1, 64),
                    ini.str("MsgWaitMarkSpeed")
                        .and_then(|v| v.parse::<f64>().ok())
                        .map_or(500.0, |v| (v * 1000.0).max(16.0)),
                )
            }),
        };
        let mut first = Vec::new();
        if let Some(f) = ini.str("FirstFlow")
            && arcs.has(f)
        {
            first.push(f.to_string());
        } else if let Some(b) = arcs.read(files.as_mut(), "cfg\\chapter_file.ini") {
            first = chapters(&b);
        }
        if first.is_empty()
            && let Some(p) = arcs.packs.get("scr")
        {
            // no flow and no chapter list (2014): every script of scr.pak in name order
            let mut names: Vec<&String> = p
                .names
                .iter()
                .filter(|n| n.to_lowercase().ends_with(".txt"))
                .collect();
            names.sort_by_key(|n| n.to_lowercase());
            for n in names {
                let n = n.rsplit('\\').next().unwrap_or(n);
                first.push(format!("scr\\{n}"));
            }
        }
        let title_bgm = ini.str("TitleBGM").map(str::to_string);
        Ok(Engine {
            files,
            arcs,
            width,
            height,
            log: false,
            font: None,
            win,
            first,
            title_bgm,
            scripts: HashMap::new(),
            stack: Vec::new(),
            sprites: BTreeMap::new(),
            faces: BTreeMap::new(),
            window: false,
            name: None,
            lines: Vec::new(),
            wait: Wait::Title,
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
        })
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        self.font = Some(Rc::new(
            fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
                .map_err(|e| e.to_string())?,
        ));
        self.text_cache.clear();
        Ok(())
    }

    /// Starts the story at once, at a script or flow (`scr\name.txt`), for the native runner.
    pub fn start_at(&mut self, script: &str) {
        self.start(vec![script.to_string()]);
    }

    fn trace(&self, s: &str) {
        if self.log {
            eprintln!("neko: {s}");
        }
    }

    fn script(&mut self, name: &str) -> Option<Rc<Script>> {
        let key = name.to_lowercase();
        if let Some(s) = self.scripts.get(&key) {
            return Some(s.clone());
        }
        let b = self.arcs.read(self.files.as_mut(), name)?;
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

    /// Pushes a script (its start row) to run next.
    fn call(&mut self, name: &str) {
        let Some(s) = self.script(name) else {
            self.trace(&format!("no script {name}"));
            return;
        };
        let next = s.start().map_or(0, |r| r.id());
        self.stack.push(Frame {
            script: name.to_string(),
            next,
        });
    }

    fn start(&mut self, scripts: Vec<String>) {
        self.reset();
        self.stop_music(500.0);
        // the last pushed runs first
        for s in scripts.iter().rev() {
            self.call(s);
        }
        self.wait = Wait::None;
        self.freeze();
        self.frozen = Some(vec![self.solid([0, 0, 0, 255])]);
    }

    pub fn frame(&mut self) -> Result<(), String> {
        self.now += FRAME_MS;
        if let Some(f) = &self.fade
            && self.now - f.start >= f.ms
        {
            self.fade = None;
        }
        let now = self.now;
        for s in self.sprites.values_mut() {
            if let Some((_, start, ms)) = s.tween
                && now - start >= ms
            {
                s.tween = None;
            }
        }
        match self.wait {
            Wait::Fade if self.fade.is_none() => self.wait = Wait::None,
            Wait::Until(t) if self.now >= t => self.wait = Wait::None,
            Wait::Click if self.skip && self.view == View::Game => self.advance(),
            _ => {}
        }
        if self.wait == Wait::Title
            && self.bgm.is_none()
            && self.view == View::Game
            && let Some(b) = self.title_bgm.clone()
        {
            self.play_music(&b, 1.0, 0.0);
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
                // the story is over
                self.trace("the story ended");
                self.back_to_title();
                return;
            };
            let Some(s) = self.script(&top.script) else {
                self.stack.pop();
                continue;
            };
            let Some(rec) = s.get(top.next).cloned() else {
                self.stack.pop();
                continue;
            };
            if let Some(f) = self.stack.last_mut() {
                f.next = rec.next();
            }
            let flow = top.script.to_lowercase().starts_with("flow");
            self.exec(&rec, flow);
        }
    }

    fn exec(&mut self, r: &Rec, flow: bool) {
        let c = r.cmd();
        if self.log && c != 1 {
            let nz: Vec<(usize, i32)> = r.ints[4..36]
                .iter()
                .enumerate()
                .filter(|(_, v)| **v != 0)
                .map(|(i, v)| (i, *v))
                .collect();
            self.trace(&format!("{} #{c} {nz:?} {:?}", r.id(), r.s(1)));
        }
        if flow {
            match c {
                1 => {
                    let s = r.s(1).to_string();
                    self.call(&s);
                }
                2 => {
                    // the end of the story
                    self.stack.clear();
                }
                _ => {}
            }
            return;
        }
        match c {
            0 | 1 => {}
            2 => {
                self.flush();
                if !self.skip {
                    self.wait = Wait::Until(self.now + r.n(0).max(0) as f64);
                }
            }
            4 => {
                self.flush();
                self.window = false;
            }
            5 => self.text(r),
            9 => {
                let ms = if r.n(0) == 0 { 0.0 } else { r.n(1) as f64 };
                let rule = if r.n(0) == 2 && !r.s(1).is_empty() && !self.skip {
                    self.image(r.s(1))
                } else {
                    None
                };
                self.present_ruled(ms, rule);
            }
            10 | 11 | 13 => {
                let sprite = match c {
                    10 => 100,
                    11 => 102,
                    _ => 200,
                };
                self.load(sprite, r.s(1), r.n(1), r.n(2));
            }
            15 | 92 => self.load(r.n(3), r.s(1), r.n(1), r.n(2)),
            // an animated picture (frames and timing not known): its picture, centred
            16 => self.load(r.n(3), r.s(1), 0, 0),
            17 => {
                self.freeze();
                self.sprites.remove(&r.n(0));
            }
            18 | 25 => self.change(r, c == 25),
            21 => {
                // all standing pictures
                self.freeze();
                self.sprites
                    .retain(|_, s| !s.path.to_lowercase().starts_with("sc\\"));
            }
            22 => {
                self.freeze();
                self.sprites.clear();
            }
            23 => {
                let path = r.s(1).to_string();
                let sprite = r.n(3);
                match self.image(&path) {
                    Some(image) => {
                        self.freeze();
                        match self.sprites.get_mut(&sprite) {
                            Some(s) => {
                                s.image = image;
                                s.path = path;
                            }
                            None => self.load(sprite, &path, 0, 0),
                        }
                    }
                    None => self.trace(&format!("no picture {path}")),
                }
            }
            26 => {
                if let Some(s) = self.sprites.get_mut(&r.n(0)) {
                    s.tween = None;
                }
            }
            30 => {
                let f = r.s(1).to_string();
                self.play_music(&f, vol(r.n(1)), r.n(3) as f32);
            }
            31 | 32 => self.stop_music(r.n(1).max(0) as f32),
            35 => {
                let ch = r.n(0);
                let f = r.s(1).to_string();
                let looped = r.n(4) != 0;
                self.play_sound(ch, &f, vol(r.n(1)), looped);
            }
            36 => self.stop_sound(r.n(0), r.n(1).max(0) as f32),
            37 => {
                for ch in self.loops.keys().copied().collect::<Vec<_>>() {
                    self.stop_sound(ch, 0.0);
                }
                for ch in 0..10 {
                    self.push_sound(&format!("se{ch}"), None, 0.0, false, 0.0);
                }
            }
            40 => {
                self.stop_music(0.0);
                for ch in 0..10 {
                    self.stop_sound(ch, 0.0);
                    self.push_sound(&format!("voice{ch}"), None, 0.0, false, 0.0);
                }
            }
            41 => {
                if !self.skip {
                    let d = self.arcs.read(self.files.as_mut(), r.s(1));
                    let ch = format!("voice{}", r.n(0));
                    self.push_sound(&ch, d, vol(r.n(1)), false, 0.0);
                }
            }
            42 => {
                let ch = format!("voice{}", r.n(0));
                self.push_sound(&ch, None, 0.0, false, r.n(1).max(0) as f32);
            }
            43 => {
                for ch in 0..10 {
                    self.push_sound(&format!("voice{ch}"), None, 0.0, false, 0.0);
                }
            }
            60 => {}
            130 => {
                let path = r.s(1).to_string();
                match self.image(&path) {
                    Some(img) => {
                        self.faces.insert(r.n(2), (path, img));
                    }
                    None => self.trace(&format!("no picture {path}")),
                }
            }
            131 => {
                self.faces.remove(&r.n(0));
            }
            138 => self.faces.clear(),
            _ => {}
        }
    }

    fn text(&mut self, r: &Rec) {
        self.flush();
        let name = r.s(1).trim();
        self.name = (!name.is_empty()).then(|| name.to_string());
        self.lines = r.s(2).split('\n').map(str::to_string).collect();
        self.window = true;
        if !r.s(3).is_empty() && !self.skip {
            let d = self.arcs.read(self.files.as_mut(), r.s(3));
            let v = if r.n(1) > 0 { vol(r.n(1)) } else { 1.0 };
            self.push_sound("voice", d, v, false, 0.0);
        }
        self.saved_at = Some(self.stack.clone());
        self.wait = Wait::Click;
    }

    fn image(&mut self, path: &str) -> Option<Rc<Image>> {
        let key = path.to_lowercase();
        if let Some(i) = self.images.get(&key) {
            return Some(i.clone());
        }
        let bmp = self.arcs.read(self.files.as_mut(), path)?;
        let alp_path = match path.rsplit_once('.') {
            Some((stem, _)) => format!("{stem}.alp"),
            None => format!("{path}.alp"),
        };
        let alp = self.arcs.read(self.files.as_mut(), &alp_path);
        let img = match pic::decode(&bmp, alp.as_deref()) {
            Ok(i) => Rc::new(i),
            Err(e) => {
                self.trace(&format!("{path}: {e}"));
                return None;
            }
        };
        if self.images.len() > 48 {
            self.images.clear();
        }
        self.images.insert(key, img.clone());
        Some(img)
    }

    fn load(&mut self, sprite: i32, path: &str, x: i32, y: i32) {
        let Some(image) = self.image(path) else {
            self.trace(&format!("no picture {path}"));
            return;
        };
        self.freeze();
        self.sprites.insert(
            sprite,
            Sprite {
                path: path.to_string(),
                image,
                pose: Pose {
                    x: x as f32,
                    y: y as f32,
                    sx: 1.0,
                    sy: 1.0,
                    alpha: 1.0,
                },
                tween: None,
            },
        );
    }

    /// 18 (at once) and 25 (over time): n(1) n(2) the place (from the centre),
    /// n(4) n(5) the scale in thousandths, n(15) the opacity in 256ths, n(16) / n(17) whether
    /// the place / the scale change; 25: n(24) a delay and n(25) the time (ms).
    fn change(&mut self, r: &Rec, over_time: bool) {
        let now = self.now;
        let skip = self.skip;
        if !over_time {
            self.freeze();
        }
        let Some(s) = self.sprites.get_mut(&r.n(3)) else {
            return;
        };
        let cur = s.pose_at(now);
        let mut to = cur;
        if r.n(16) != 0 || (!over_time && r.n(16) == 0 && r.n(17) == 0) {
            to.x = r.n(1) as f32;
            to.y = r.n(2) as f32;
        }
        if r.n(17) != 0 || (!over_time && r.n(4) > 0) {
            if r.n(4) > 0 {
                to.sx = r.n(4) as f32 / 1000.0;
            }
            if r.n(5) > 0 {
                to.sy = r.n(5) as f32 / 1000.0;
            }
        }
        if r.n(15) > 0 {
            to.alpha = (r.n(15) as f32 / 256.0).min(1.0);
        }
        let ms = r.n(25);
        if over_time && ms > 0 && !skip {
            s.tween = Some((cur, now + r.n(24).max(0) as f64, ms as f64));
        } else {
            s.tween = None;
        }
        s.pose = to;
    }

    // ---- showing ----

    /// Keeps the screen as it is until the next update (9), a text or a wait.
    fn freeze(&mut self) {
        if self.frozen.is_none() {
            self.fade = None;
            self.frozen = Some(self.stage_draws());
        }
    }

    /// Shows the changes still held back.
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
        let img = white(1, 1, rgba);
        let mut d = draw(&img, 0.0, 0.0, 1.0);
        d.dst = [0.0, 0.0, self.width as f32, self.height as f32];
        d
    }

    fn sprite_draw(&self, s: &Sprite) -> Draw {
        let p = s.pose_at(self.now);
        let (w, h) = (s.image.w as f32 * p.sx, s.image.h as f32 * p.sy);
        // every picture is placed by its centre, relative to the screen's centre
        let x = self.width as f32 / 2.0 - w / 2.0 + p.x;
        let y = self.height as f32 / 2.0 - h / 2.0 + p.y;
        let mut d = draw(&s.image, x, y, p.alpha);
        d.dst = [x, y, x + w, y + h];
        d
    }

    fn stage_draws(&self) -> Vec<Draw> {
        let mut out = vec![self.solid([0, 0, 0, 255])];
        for s in self.sprites.values() {
            out.push(self.sprite_draw(s));
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
        if self.wait == Wait::Title {
            self.title_draws(&mut out);
        } else if self.window && self.fade.is_none() && self.frozen.is_none() {
            self.window_draws(&mut out);
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

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let file = self.win.file.clone();
        match self.image(&file) {
            Some(img) => out.push(draw(&img, self.win.x as f32, self.win.y as f32, 1.0)),
            None => {
                let mut d = self.solid([0, 0, 0, 160]);
                d.dst = [
                    0.0,
                    (self.height as i32 - 200) as f32,
                    self.width as f32,
                    self.height as f32,
                ];
                out.push(d);
            }
        }
        for (_, img) in self.faces.values() {
            let y = self.height as f32 - img.h as f32;
            out.push(draw(img, self.win.x as f32, y, 1.0));
        }
        let (size, step, chars) = (self.win.size, self.win.step, self.win.chars);
        if let Some(n) = self.name.clone() {
            let img = self.label(&n, self.win.name_color, size);
            out.push(draw(
                &img,
                self.win.name.0 as f32 - 2.0,
                self.win.name.1 as f32 - 2.0,
                1.0,
            ));
        }
        let (tx, mut ty) = self.win.text;
        let color = self.win.color;
        for l in self.lines.clone() {
            for piece in wrap(&l, chars) {
                let img = self.label(&piece, color, size);
                out.push(draw(&img, tx as f32 - 2.0, ty as f32 - 2.0, 1.0));
                ty += step;
            }
        }
        if self.wait == Wait::Click
            && let Some((f, x, y, frames, ms)) = self.win.mark.clone()
            && let Some(img) = self.mark_image(&f)
        {
            let fw = img.w as i32 / frames;
            let i = (self.now / ms) as i32 % frames;
            let mut d = draw(&img, x as f32, y as f32, 1.0);
            d.src = [i * fw, 0, (i + 1) * fw, img.h as i32];
            d.dst = [x as f32, y as f32, (x + fw) as f32, y as f32 + img.h as f32];
            out.push(d);
        }
    }

    /// The wait mark; one without an .alp (2014) is keyed on pure green.
    fn mark_image(&mut self, path: &str) -> Option<Rc<Image>> {
        let key = format!("{}#mark", path.to_lowercase());
        if let Some(i) = self.images.get(&key) {
            return Some(i.clone());
        }
        let img = self.image(path)?;
        let mut keyed = Image::new(img.w, img.h);
        keyed.px.copy_from_slice(&img.px);
        for p in keyed.px.chunks_exact_mut(4) {
            if p[..3] == [0, 255, 0] {
                p.copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        let keyed = Rc::new(keyed);
        self.images.insert(key, keyed.clone());
        Some(keyed)
    }

    // ---- the title ----

    /// The title's buttons: (label, column of the button sheet, rectangle).
    /// The title's button sheet (5 rows: normal, pointed, ...) and its columns: 2017 has
    /// obj\\d006_000 with 8 (はじめから 0, つづきから 1, ゲーム終了 6), 2016 obj\\d007_000 with 10
    /// (Start 0, Load 1, Quit 9), 2012 obj\\d001_000 with 6 round ones (ゲーム終了 4), 2010
    /// obj\\c001_000 with 5 (ゲーム終了 4).
    fn title_sheet(&mut self) -> Option<(Rc<Image>, i32, i32)> {
        if let Some(s) = self.image("obj\\d007_000.bmp")
            && s.w % 10 == 0
            && s.h % 5 == 0
            && s.w >= 1000
        {
            return Some((s, 10, 9));
        }
        if let Some(s) = self.image("obj\\d006_000.bmp")
            && s.w % 8 == 0
            && s.h % 5 == 0
            && s.h >= 100
        {
            return Some((s, 8, 6));
        }
        if let Some(s) = self.image("obj\\d001_000.bmp")
            && s.w % 6 == 0
            && s.h % 5 == 0
            && s.w / 6 == s.h / 5
        {
            return Some((s, 6, 4));
        }
        match self.image("obj\\c001_000.bmp") {
            Some(s) if s.w % 5 == 0 && s.h % 5 == 0 && s.w > 1000 => Some((s, 5, 4)),
            _ => None,
        }
    }

    /// (label, column of the sheet, rectangle); the columns 0 and 1 start and load, the last
    /// quits.
    fn title_buttons(&mut self) -> Vec<(&'static str, i32, [i32; 4])> {
        let sheet = self.title_sheet();
        let quit = sheet.as_ref().map_or(6, |s| s.2);
        let items = [("はじめから", 0), ("つづきから", 1), ("ゲーム終了", quit)];
        let (bw, bh) = match &sheet {
            Some((s, cols, _)) => (s.w as i32 / cols, s.h as i32 / 5),
            None => (300, 48),
        };
        if bw == bh {
            // round buttons (2012): a row at the bottom right
            let (gap, y) = (12, self.height as i32 - bh - 30);
            let left = self.width as i32 - 3 * bw - 2 * gap - 40;
            return items
                .iter()
                .enumerate()
                .map(|(i, &(t, col))| {
                    let x = left + i as i32 * (bw + gap);
                    (t, col, [x, y, x + bw, y + bh])
                })
                .collect();
        }
        let (x, top, gap) = (40, self.height as i32 - 250, 12);
        items
            .iter()
            .enumerate()
            .map(|(i, &(t, col))| {
                let y = top + i as i32 * (bh + gap);
                (t, col, [x, y, x + bw, y + bh])
            })
            .collect()
    }

    fn title_draws(&mut self, out: &mut Vec<Draw>) {
        let (w, h) = (self.width as f32, self.height as f32);
        let sheet = self.title_sheet();
        if sheet.as_ref().is_some_and(|s| s.1 == 5) {
            // 2010: the desk (a000)
            if let Some(bg) = self.image("obj\\a000_000.bmp") {
                out.push(draw(&bg, 0.0, 0.0, 1.0));
            }
        } else if let Some(bg) = self.image("obj\\d000_000.bmp") {
            out.push(draw(&bg, 0.0, 0.0, 1.0));
        } else if let Some(bg) = self.image("anime\\d000_000.bmp") {
            // 2014: the title's background is the first frame of an animation (half size),
            // its logo obj\d001_000
            let mut d = draw(&bg, 0.0, 0.0, 1.0);
            d.dst = [0.0, 0.0, w, h];
            out.push(d);
            if let Some(logo) = self.image("obj\\d001_000.bmp") {
                out.push(draw(&logo, (w - logo.w as f32) / 2.0, 40.0, 1.0));
            }
        } else {
            out.push(self.solid([20, 20, 40, 255]));
        }
        if sheet.as_ref().is_some_and(|s| s.1 == 10) {
            // 2016: the girls (d003) at the right, the logo (d005), the copyright line (d006)
            if let Some(girls) = self.image("obj\\d003_000.bmp") {
                out.push(draw(&girls, w - girls.w as f32, 0.0, 1.0));
            }
            if let Some(logo) = self.image("obj\\d005_000.bmp") {
                out.push(draw(&logo, 20.0, 40.0, 1.0));
            }
            if let Some(c) = self.image("obj\\d006_000.bmp") {
                out.push(draw(&c, 20.0, h - c.h as f32 - 4.0, 1.0));
            }
        } else if sheet.as_ref().is_some_and(|s| s.1 == 6) {
            // 2012: the trial's logo at the top right
            if let Some(logo) = self.image("obj\\d000_101.bmp") {
                out.push(draw(&logo, w - logo.w as f32 - 20.0, 20.0, 1.0));
            }
        } else if sheet.is_some()
            && let Some(logo) = self.image("obj\\d002_000.bmp")
        {
            out.push(draw(&logo, 0.0, 0.0, 1.0));
        }
        let sheet = sheet.map(|s| s.0);
        let (px, py) = self.pointer;
        for (t, col, r) in self.title_buttons() {
            let over = inside(r, px, py);
            match &sheet {
                Some(s) => {
                    let (bw, bh) = (r[2] - r[0], r[3] - r[1]);
                    let row = if over { 1 } else { 0 };
                    let mut d = draw(s, r[0] as f32, r[1] as f32, 1.0);
                    d.src = [col * bw, row * bh, (col + 1) * bw, (row + 1) * bh];
                    d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
                    out.push(d);
                }
                None => self.button(t, r, out),
            }
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
        // 58 pixels a row, less on a short screen (800x600 in 2010)
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

    fn play_music(&mut self, file: &str, volume: f32, _fade_in: f32) {
        if self
            .bgm
            .as_ref()
            .is_some_and(|(f, _)| f.eq_ignore_ascii_case(file))
        {
            return;
        }
        let d = self.arcs.read(self.files.as_mut(), file);
        if d.is_none() {
            self.trace(&format!("no music {file}"));
        }
        self.bgm = Some((file.to_string(), volume));
        self.push_sound("bgm", d, volume, true, 0.0);
    }

    fn stop_music(&mut self, ms: f32) {
        self.bgm = None;
        self.push_sound("bgm", None, 0.0, false, ms);
    }

    fn play_sound(&mut self, ch: i32, file: &str, volume: f32, looped: bool) {
        if self.skip && !looped {
            return;
        }
        let d = self.arcs.read(self.files.as_mut(), file);
        if looped {
            self.loops.insert(ch, (file.to_string(), volume));
        } else {
            self.loops.remove(&ch);
        }
        self.push_sound(&format!("se{ch}"), d, volume, looped, 0.0);
    }

    fn stop_sound(&mut self, ch: i32, ms: f32) {
        self.loops.remove(&ch);
        self.push_sound(&format!("se{ch}"), None, 0.0, false, ms);
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
        match self.wait {
            Wait::Title => {
                let hit = self
                    .title_buttons()
                    .into_iter()
                    .find(|b| inside(b.2, x, y))
                    .map(|b| b.1);
                match hit {
                    Some(0) => {
                        let first = self.first.clone();
                        self.start(first);
                    }
                    Some(1) => self.view = View::Slots { save: false },
                    Some(_) => self.ended = true,
                    None => {}
                }
            }
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
            Wait::None => {}
        }
    }

    fn close_slots(&mut self) {
        self.view = if self.wait == Wait::Title {
            View::Game
        } else {
            View::Menu
        };
    }

    pub fn cancel(&mut self) {
        match self.view {
            View::Menu => self.view = View::Game,
            View::Slots { .. } => self.close_slots(),
            View::Game => {
                self.skip = false;
                if self.wait == Wait::Click {
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
        self.faces.clear();
        self.window = false;
        self.lines.clear();
        self.name = None;
        self.view = View::Game;
        self.frozen = None;
        self.fade = None;
        self.skip = false;
        self.saved_at = None;
        for ch in self.loops.keys().copied().collect::<Vec<_>>() {
            self.stop_sound(ch, 300.0);
        }
        self.push_sound("voice", None, 0.0, false, 0.0);
    }

    fn back_to_title(&mut self) {
        self.reset();
        self.stop_music(300.0);
        self.wait = Wait::Title;
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
        let title = match &self.name {
            Some(n) => format!("{n} {first}"),
            None => first,
        };
        s += &format!("title {}\n", title.replace('\n', " "));
        for f in &at {
            s += &format!("frame {} {}\n", f.next, f.script);
        }
        for (k, sp) in &self.sprites {
            let p = sp.pose;
            s += &format!(
                "sprite {k} {} {} {} {} {} {}\n",
                p.x, p.y, p.sx, p.sy, p.alpha, sp.path
            );
        }
        if let Some((b, v)) = &self.bgm {
            s += &format!("bgm {v} {b}\n");
        }
        for (c, (f, v)) in &self.loops {
            s += &format!("loop {c} {v} {f}\n");
        }
        s += &format!("window {}\n", self.window as u8);
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
        for (k, pose, path) in slot.sprites {
            if let Some(image) = self.image(&path) {
                self.sprites.insert(
                    k,
                    Sprite {
                        path,
                        image,
                        pose,
                        tween: None,
                    },
                );
            }
        }
        if let Some((b, v)) = slot.bgm {
            self.play_music(&b, v, 0.0);
        }
        for (c, v, f) in slot.loops {
            self.play_sound(c, &f, v, true);
        }
        self.window = slot.window;
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
    frames: Vec<Frame>,
    sprites: Vec<(i32, Pose, String)>,
    bgm: Option<(String, f32)>,
    loops: Vec<(i32, f32, String)>,
    window: bool,
    name: Option<String>,
    lines: Vec<String>,
}

impl Slot {
    fn parse(s: &str) -> Option<Slot> {
        let mut slot = Slot {
            title: String::new(),
            frames: Vec::new(),
            sprites: Vec::new(),
            bgm: None,
            loops: Vec::new(),
            window: true,
            name: None,
            lines: Vec::new(),
        };
        for line in s.lines() {
            let (k, rest) = line.split_once(' ').unwrap_or((line, ""));
            match k {
                "title" => slot.title = rest.to_string(),
                "frame" => {
                    let (n, script) = rest.split_once(' ')?;
                    slot.frames.push(Frame {
                        script: script.to_string(),
                        next: n.parse().ok()?,
                    });
                }
                "sprite" => {
                    let v: Vec<&str> = rest.splitn(7, ' ').collect();
                    if v.len() == 7 {
                        let f = |i: usize| v[i].parse::<f32>().unwrap_or(0.0);
                        slot.sprites.push((
                            v[0].parse().unwrap_or(0),
                            Pose {
                                x: f(1),
                                y: f(2),
                                sx: f(3),
                                sy: f(4),
                                alpha: f(5),
                            },
                            v[6].to_string(),
                        ));
                    }
                }
                "bgm" => {
                    if let Some((v, f)) = rest.split_once(' ') {
                        slot.bgm = Some((f.to_string(), v.parse().unwrap_or(1.0)));
                    }
                }
                "loop" => {
                    let v: Vec<&str> = rest.splitn(3, ' ').collect();
                    if v.len() == 3 {
                        slot.loops.push((
                            v[0].parse().unwrap_or(0),
                            v[1].parse().unwrap_or(1.0),
                            v[2].to_string(),
                        ));
                    }
                }
                "window" => slot.window = rest == "1",
                "name" => slot.name = Some(rest.to_string()),
                "line" => slot.lines.push(rest.to_string()),
                _ => {}
            }
        }
        (!slot.frames.is_empty()).then_some(slot)
    }
}

/// The scripts of cfg\chapter_file.ini ("#chapter n", then "scr\\name.txt,flag,size" lines):
/// the first of each chapter, in order.
fn chapters(b: &[u8]) -> Vec<String> {
    let text = encoding_rs::SHIFT_JIS.decode(b).0;
    let mut out = Vec::new();
    let mut taken = true;
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        if line.starts_with("#chapter") {
            taken = false;
        } else if !taken && line.to_lowercase().contains(".txt") {
            let file = line.split(',').next().unwrap_or("").replace("\\\\", "\\");
            out.push(file);
            taken = true;
        }
    }
    out
}

/// A volume in hundredths (0: full).
fn vol(v: i32) -> f32 {
    if v <= 0 {
        1.0
    } else {
        (v as f32 / 100.0).min(1.0)
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
