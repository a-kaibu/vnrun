//! Runs the game's Lua scripts (Lua 5.1 chunks in game.pak) the way the game's program does,
//! and keeps what the screen shows.
//!
//! Each script `s` defines `_s_jump_(label)` (makes the label's coroutine) and `_s_loop_(label)`
//! (resumes it through our `coroutine_resume`). A label's coroutine yields `(kind, script,
//! label)`: "j" jumps to a label of the same script, "e" goes to a script's label, "c" calls
//! one and "r" returns from a call; a yield with no kind is a wait (a page, a fade), and the
//! label runs on in a later frame.
//!
//! The scripts' commands (`layer`, `effect`, `page`, `wait`, ...) are Lua functions that call
//! the program's functions (`_layer`, `_effect`, `_page`, `_wait`, ...): those are ours here
//! (crate::host). Live2D characters, 3D scenes and the dungeon are not drawn.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use luavm::{Lua, Value};
use nexas::files::Files;
use nexas::gfx::{Draw, Image, Place};

use crate::pak::Index;

pub const W: u32 = 1280;
pub const H: u32 = 720;
pub const FRAME: f64 = 1.0 / 60.0;
const STEPS: usize = 2000;
/// characters shown per second while a page is being written
const CPS: f64 = 60.0;

pub struct Sound {
    pub data: Vec<u8>,
    pub volume: f32,
    /// 0 music, 1 voice, 2 voice in the background, 3.. effects
    pub channel: i32,
    pub stop: bool,
    pub fade_ms: u32,
    pub looped: bool,
}

#[derive(Clone)]
pub enum Src {
    File(String),
    Color([u8; 4]),
    Shot(Rc<Image>),
}

/// One part of an effect: a property moving to a value, from `at` seconds after the effect
/// starts over `dur` seconds.
#[derive(Clone, Debug)]
pub struct Track {
    pub prop: i32,
    pub to: [f32; 2],
    pub from: Option<[f32; 2]>,
    pub at: f64,
    pub dur: f64,
}

#[derive(Clone)]
pub struct Layer {
    pub name: String,
    pub src: Src,
    pub prio: i32,
    pub order: u64,
    /// the point of the picture that is placed, as a part of the screen size
    pub base: [f32; 2],
    /// where that point is, as a part of the screen size
    pub off: [f32; 2],
    pub scale: [f32; 2],
    pub rot: f32,
    pub alpha: f32,
    /// 0 copy, 2 add, 3 subtract
    pub blend: i32,
    pub shake: [f32; 2],
    pub tracks: Vec<Track>,
    pub start: f64,
    pub rewind: bool,
    pub release: bool,
}

impl Layer {
    fn new(name: &str, src: Src, prio: i32, order: u64) -> Layer {
        Layer {
            name: name.to_string(),
            src,
            prio,
            order,
            base: [0.0; 2],
            off: [0.0; 2],
            scale: [1.0; 2],
            rot: 0.0,
            alpha: 1.0,
            blend: 0,
            shake: [0.0; 2],
            tracks: Vec::new(),
            start: 0.0,
            rewind: false,
            release: false,
        }
    }

    fn get(&self, prop: i32) -> [f32; 2] {
        match prop {
            EF_BASE => self.base,
            EF_OFFSET | EF_MOVE => self.off,
            EF_OPACITY => [self.alpha; 2],
            EF_SCALE => self.scale,
            EF_ROTATE => [self.rot; 2],
            EF_SHAKE => self.shake,
            _ => [0.0; 2],
        }
    }

    fn set(&mut self, prop: i32, v: [f32; 2]) {
        match prop {
            EF_BASE => self.base = v,
            EF_OFFSET | EF_MOVE => self.off = v,
            EF_OPACITY => self.alpha = v[0].clamp(0.0, 1.0),
            EF_SCALE => self.scale = v,
            EF_ROTATE => self.rot = v[0],
            EF_SHAKE => self.shake = v,
            _ => {}
        }
    }

    fn end(&self) -> f64 {
        self.tracks.iter().map(|t| t.at + t.dur).fold(0.0, f64::max)
    }

    /// Moves the effect on to `now`; false when the layer is to go (released at its end).
    fn step(&mut self, now: f64) -> bool {
        if self.tracks.is_empty() {
            return true;
        }
        let mut t = now - self.start;
        let end = self.end();
        if t >= end && self.rewind && end > 0.0 {
            self.start += (t / end).floor() * end;
            t = now - self.start;
            for tr in &mut self.tracks {
                tr.from = None;
            }
        }
        let mut tracks = std::mem::take(&mut self.tracks);
        for tr in &mut tracks {
            if t < tr.at {
                continue;
            }
            let from = *tr.from.get_or_insert_with(|| self.get(tr.prop));
            let k = if tr.dur <= 0.0 {
                1.0
            } else {
                ((t - tr.at) / tr.dur).clamp(0.0, 1.0) as f32
            };
            self.set(
                tr.prop,
                [
                    from[0] + (tr.to[0] - from[0]) * k,
                    from[1] + (tr.to[1] - from[1]) * k,
                ],
            );
        }
        self.tracks = tracks;
        if t >= end && !self.rewind {
            self.tracks.clear();
            if self.shake != [0.0; 2] {
                self.shake = [0.0; 2];
            }
            return !self.release;
        }
        true
    }

    pub fn busy(&self) -> bool {
        !self.tracks.is_empty() && !self.rewind
    }
}

pub const EF_ATTACH: i32 = 0;
pub const EF_BASE: i32 = 1;
pub const EF_OFFSET: i32 = 2;
pub const EF_MOVE: i32 = 3;
pub const EF_OPACITY: i32 = 4;
pub const EF_SCALE: i32 = 5;
pub const EF_ROTATE: i32 = 6;
pub const EF_WAIT: i32 = 7;
pub const EF_SHAKE: i32 = 8;
pub const EF_BLEND: i32 = 9;
pub const EF_RELEASE: i32 = 10;
pub const EF_REWIND: i32 = 12;
pub const EF_PRIO: i32 = 13;
pub const EF_SOUND: i32 = 15;
pub const EF_COLOR: i32 = 16;

/// An `effect` call waiting for `ef_attach`.
#[derive(Clone, Debug)]
pub struct Step {
    pub kind: i32,
    pub v: [f32; 2],
    pub rgba: [u8; 4],
    pub name: String,
    pub t: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    None,
    /// a page is shown until a click
    Page,
    /// the title menu (our own, from tiItemTitle.dat)
    Title,
    Choice(Vec<String>),
    Ended,
}

pub struct TitleItem {
    pub code: i32,
    pub normal: String,
    pub over: String,
    pub src_y: [i32; 2],
    pub size: [i32; 2],
    pub dst: [i32; 2],
}

/// What the program's functions change: shared by the natives and the engine.
pub struct St {
    pub files: Box<dyn Files>,
    pub index: Index,
    pub images: HashMap<String, Option<Rc<Image>>>,
    pub font: Option<Rc<fontdue::Font>>,
    pub text_cache: HashMap<(String, u32), Rc<Image>>,
    pub now: f64,
    pub layers: Vec<Layer>,
    pub order: u64,
    pub last: String,
    pub steps: Vec<Step>,
    pub cursor: f64,
    pub msg: String,
    pub name: String,
    pub shown: f64,
    pub block: Block,
    pub vars: HashMap<String, Value>,
    pub globals: HashMap<String, Value>,
    pub flags: HashMap<i64, Value>,
    pub sounds: Vec<Sound>,
    pub waits: HashMap<String, f64>,
    pub title_bg: String,
    pub title_items: Vec<TitleItem>,
    pub result: Option<i32>,
    /// the variable a choice's number goes into
    pub result_key: String,
    pub pointer: (i32, i32),
    pub log: bool,
    pub seed: u64,
    pub face: String,
    pub stand: String,
    pub unknown: std::collections::HashSet<String>,
}

pub type Shared = Rc<RefCell<St>>;

impl St {
    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        self.index.read(self.files.as_mut(), name)
    }

    pub fn image(&mut self, name: &str) -> Option<Rc<Image>> {
        let key = name.to_lowercase();
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let b = self
            .read(&format!("{key}.png"))
            .or_else(|| self.read(&key))
            .or_else(|| self.read(&format!("{key}.jpg")));
        let img = b.and_then(|b| match Image::decode(&b) {
            Ok(i) => Some(Rc::new(i)),
            Err(e) => {
                if self.log {
                    eprintln!("deb: {name}: {e}");
                }
                None
            }
        });
        if img.is_none() && self.log {
            eprintln!("deb: no picture {name}");
        }
        if self.images.len() > 64 {
            self.images.retain(|_, v| v.is_none());
        }
        self.images.insert(key, img.clone());
        img
    }

    pub fn layer_mut(&mut self, name: &str) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.name == name)
    }

    pub fn add_layer(&mut self, name: &str, src: Src, prio: i32) {
        self.order += 1;
        let order = self.order;
        let prio = if prio < 0 {
            self.layer_mut(name).map_or(10, |l| l.prio)
        } else {
            prio
        };
        self.layers.retain(|l| l.name != name);
        self.layers.push(Layer::new(name, src, prio, order));
        self.last = name.to_string();
    }

    pub fn clear(&mut self, pat: &str) {
        if pat.is_empty() {
            self.layers.clear();
        } else if let Some(keep) = pat.strip_prefix('*') {
            self.layers.retain(|l| l.name == keep);
        } else if let Some(prefix) = pat.strip_suffix('*') {
            self.layers.retain(|l| !l.name.starts_with(prefix));
        } else {
            self.layers.retain(|l| l.name != pat);
        }
    }

    /// The effect built so far goes on the layer `name` (the last made one when empty).
    pub fn attach(&mut self, name: &str) {
        let steps = std::mem::take(&mut self.steps);
        self.cursor = 0.0;
        let name = if name.is_empty() {
            self.last.clone()
        } else {
            name.to_string()
        };
        let now = self.now;
        let mut sounds = Vec::new();
        let Some(l) = self.layer_mut(&name) else {
            if self.log {
                eprintln!("deb: effect on no layer {name}");
            }
            return;
        };
        l.tracks.clear();
        l.start = now;
        l.rewind = false;
        l.release = false;
        let mut at = 0.0;
        for s in steps {
            match s.kind {
                EF_WAIT => at += s.t,
                EF_RELEASE => l.release = true,
                EF_REWIND => l.rewind = true,
                EF_BLEND => l.blend = s.v[0] as i32,
                EF_PRIO => l.prio = s.v[0] as i32,
                EF_SOUND => sounds.push(s.name.clone()),
                EF_COLOR => {}
                EF_SHAKE => {
                    l.shake = s.v;
                    l.tracks.push(Track {
                        prop: EF_SHAKE,
                        to: s.v,
                        from: Some(s.v),
                        at,
                        dur: s.t,
                    });
                }
                k @ (EF_BASE | EF_OFFSET | EF_MOVE | EF_OPACITY | EF_SCALE | EF_ROTATE) => {
                    if s.t <= 0.0 && at <= 0.0 {
                        l.set(k, s.v);
                    } else {
                        l.tracks.push(Track {
                            prop: k,
                            to: s.v,
                            from: None,
                            at,
                            dur: s.t,
                        });
                    }
                }
                _ => {}
            }
        }
        if l.release && l.tracks.is_empty() {
            // released at once
            l.tracks.push(Track {
                prop: -1,
                to: [0.0; 2],
                from: Some([0.0; 2]),
                at,
                dur: 0.0,
            });
        }
        for s in sounds {
            self.play_sound(&s, 3, 1.0, false);
        }
    }

    pub fn play_sound(&mut self, name: &str, channel: i32, volume: f32, looped: bool) {
        let data = self
            .read(&format!("{}.ogg", name.to_lowercase()))
            .or_else(|| self.read(&format!("{}.wav", name.to_lowercase())));
        match data {
            Some(data) => self.sounds.push(Sound {
                data,
                volume,
                channel,
                stop: false,
                fade_ms: 0,
                looped,
            }),
            None => {
                if self.log {
                    eprintln!("deb: no sound {name}");
                }
            }
        }
    }

    pub fn stop_sound(&mut self, channel: i32, fade_ms: u32) {
        self.sounds.push(Sound {
            data: Vec::new(),
            volume: 0.0,
            channel,
            stop: true,
            fade_ms,
            looped: false,
        });
    }

    pub fn music(&mut self, no: i32, volume: f32) {
        if no <= 0 {
            self.stop_sound(0, 500);
            return;
        }
        // bgmNNa is the start, bgmNNb the part that loops; we loop the second
        let path = [
            format!("music/bgm{no:02}b.wav"),
            format!("music/bgm{no:02}a.wav"),
        ]
        .into_iter()
        .find(|p| self.files.size(p).is_some());
        match path.and_then(|p| self.files.read(&p)) {
            Some(data) => self.sounds.push(Sound {
                data,
                volume,
                channel: 0,
                stop: false,
                fade_ms: 0,
                looped: true,
            }),
            None => {
                if self.log {
                    eprintln!("deb: no music {no}");
                }
            }
        }
    }

    pub fn tick(&mut self) {
        let now = self.now;
        self.layers.retain_mut(|l| l.step(now));
    }

    pub fn text_image(&mut self, s: &str, size: f32) -> Option<Rc<Image>> {
        let font = self.font.clone()?;
        let key = (s.to_string(), size as u32);
        if let Some(i) = self.text_cache.get(&key) {
            return Some(i.clone());
        }
        if self.text_cache.len() > 96 {
            self.text_cache.clear();
        }
        let img = Rc::new(crate::text::render(
            &font,
            s,
            size,
            [255; 3],
            [0x10, 0x10, 0x18],
        ));
        self.text_cache.insert(key, img.clone());
        Some(img)
    }

    fn solid(&mut self, rgba: [u8; 4]) -> Rc<Image> {
        let key = format!("#solid:{rgba:?}");
        if let Some(Some(i)) = self.images.get(&key) {
            return i.clone();
        }
        let mut img = Image::new(16, 16);
        img.fill(rgba);
        let img = Rc::new(img);
        self.images.insert(key, Some(img.clone()));
        img
    }

    /// The pictures of the layers, in drawing order.
    pub fn layer_draws(&mut self) -> Vec<Draw> {
        let mut ls: Vec<Layer> = self.layers.clone();
        ls.sort_by_key(|l| (l.prio, l.order));
        let mut out = Vec::new();
        for l in ls {
            if l.alpha <= 0.0 || l.blend == 3 {
                continue;
            }
            let (img, full) = match &l.src {
                Src::File(f) => match self.image(f) {
                    Some(i) => (i, false),
                    None => continue,
                },
                Src::Color(c) => (self.solid(*c), true),
                Src::Shot(i) => (i.clone(), false),
            };
            let (w, h) = (W as f32, H as f32);
            let shake = if l.shake != [0.0; 2] {
                let k = (self.now * 40.0).sin() as f32;
                [l.shake[0] * k * w * 0.02, l.shake[1] * k * h * 0.02]
            } else {
                [0.0; 2]
            };
            let (px, py) = (l.off[0] * w + shake[0], l.off[1] * h + shake[1]);
            let (iw, ih) = if full {
                (w, h)
            } else {
                (img.w as f32, img.h as f32)
            };
            let (ax, ay) = (l.base[0] * w, l.base[1] * h);
            out.push(Draw {
                src: [0, 0, img.w as i32, img.h as i32],
                dst: [px - ax, py - ay, px - ax + iw, py - ay + ih],
                image: img,
                place: Place {
                    x: 0.0,
                    y: 0.0,
                    sx: l.scale[0],
                    sy: l.scale[1],
                    cx: px,
                    cy: py,
                    rot: l.rot.to_radians(),
                    alpha: l.alpha,
                    add: l.blend == 2,
                    flip: (false, false),
                    clip: [0.0, 0.0, w, h],
                },
            });
        }
        out
    }

    /// The screen as the layers show it now.
    pub fn shot(&mut self) -> Rc<Image> {
        let mut img = Image::new(W, H);
        img.fill([0, 0, 0, 255]);
        for d in self.layer_draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        Rc::new(img)
    }

    pub fn rand(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub fn place(w: u32, h: u32, alpha: f32) -> Place {
    Place {
        x: 0.0,
        y: 0.0,
        sx: 1.0,
        sy: 1.0,
        cx: 0.0,
        cy: 0.0,
        rot: 0.0,
        alpha,
        add: false,
        flip: (false, false),
        clip: [0.0, 0.0, w as f32, h as f32],
    }
}

pub struct Engine {
    pub lua: Lua,
    pub st: Shared,
    /// (script, label) of the running label, and the calls under it
    cur: (String, String),
    calls: Vec<(String, String)>,
    included: std::collections::HashSet<String>,
    clicked: bool,
    ended: bool,
    halted: Option<String>,
    pub trace: bool,
}

impl Engine {
    pub fn new(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let mut index = Index::default();
        let mut paks: Vec<String> = files
            .list()
            .into_iter()
            .filter(|n| n.to_lowercase().ends_with(".pak"))
            .collect();
        paks.sort_by_key(|n| (n.to_lowercase() == "patch.pak", n.to_lowercase()));
        for p in &paks {
            if let Err(e) = index.add(files.as_mut(), p) {
                eprintln!("deb: {e}");
            }
        }
        if !index.has("_global.scb") {
            return Err("no game.pak with _global.scb".into());
        }
        let st = Rc::new(RefCell::new(St {
            files,
            index,
            images: HashMap::new(),
            font: None,
            text_cache: HashMap::new(),
            now: 0.0,
            layers: Vec::new(),
            order: 0,
            last: String::new(),
            steps: Vec::new(),
            cursor: 0.0,
            msg: String::new(),
            name: String::new(),
            shown: 0.0,
            block: Block::None,
            vars: HashMap::new(),
            globals: HashMap::new(),
            flags: HashMap::new(),
            sounds: Vec::new(),
            waits: HashMap::new(),
            title_bg: String::new(),
            title_items: Vec::new(),
            result: None,
            result_key: "Result".into(),
            pointer: (0, 0),
            log: std::env::var_os("DEB_LOG").is_some(),
            seed: 0x9e37_79b9_7f4a_7c15,
            face: String::new(),
            stand: String::new(),
            unknown: Default::default(),
        }));
        let mut lua = Lua::new();
        lua.v51 = true;
        crate::host::install(&mut lua, &st);
        let mut e = Engine {
            lua,
            st,
            cur: (String::new(), String::new()),
            calls: Vec::new(),
            included: Default::default(),
            clicked: false,
            ended: false,
            halted: None,
            trace: std::env::var_os("DEB_TRACE").is_some(),
        };
        e.include("_global")?;
        for f in ["reload", "init"] {
            e.lua
                .call_global(f, vec![])
                .map_err(|err| format!("{f}: {err}"))?;
        }
        let start = if e.st.borrow().index.has("trial.scb") {
            "trial"
        } else {
            "start"
        };
        e.go(start, "top")?;
        Ok(e)
    }

    pub fn set_font(&mut self, b: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(b, fontdue::FontSettings::default())?;
        self.st.borrow_mut().font = Some(Rc::new(f));
        Ok(())
    }

    fn include(&mut self, script: &str) -> Result<(), String> {
        crate::host::include(&mut self.lua, &self.st, script).map_err(|e| e.to_string())?;
        self.included.insert(script.to_string());
        Ok(())
    }

    /// Goes to `label` of `script` (its coroutine is made when it has none).
    fn go(&mut self, script: &str, label: &str) -> Result<(), String> {
        if !self.included.contains(script) {
            self.include(script)?;
        }
        if self.trace {
            eprintln!("deb: -> {script}:{label}");
        }
        let ok = self
            .lua
            .call_global(&format!("_{script}_jump_"), vec![Value::str(label)])
            .map_err(|e| format!("{script}:{label}: {e}"))?;
        if !ok.first().is_some_and(Value::truthy) {
            return Err(format!("no label {script}:{label}"));
        }
        self.cur = (script.to_string(), label.to_string());
        Ok(())
    }

    /// Runs the label until it waits.
    fn run(&mut self) -> Result<(), String> {
        for _ in 0..STEPS {
            if self.st.borrow().block != Block::None {
                return Ok(());
            }
            let (s, l) = self.cur.clone();
            let r = self
                .lua
                .call_global(&format!("_{s}_loop_"), vec![Value::str(&l)])
                .map_err(|e| format!("{s}:{l}: {e}"))?;
            let text = |i: usize| -> Option<String> {
                r.get(i)
                    .and_then(|v| v.as_bytes())
                    .map(|b| String::from_utf8_lossy(b).into_owned())
            };
            let (kind, script, label) = (text(0), text(1), text(2));
            match kind.as_deref() {
                None => return Ok(()),
                Some("j") => {
                    let label = label.ok_or("jump without a label")?;
                    self.go(&s, &label)?;
                }
                Some("e") => {
                    let script = script.filter(|x| !x.is_empty()).unwrap_or(s.clone());
                    self.go(&script, &label.unwrap_or("top".into()))?;
                }
                Some("c") => {
                    self.calls.push(self.cur.clone());
                    let script = script.filter(|x| !x.is_empty()).unwrap_or(s.clone());
                    self.go(&script, &label.unwrap_or("top".into()))?;
                }
                Some("r") => match self.calls.pop() {
                    Some(back) => {
                        if self.trace {
                            eprintln!("deb: <- {}:{}", back.0, back.1);
                        }
                        self.cur = back;
                    }
                    None => {
                        self.ended = true;
                        return Ok(());
                    }
                },
                Some(k) => return Err(format!("{s}:{l}: {k}")),
            }
        }
        Ok(())
    }

    pub fn frame(&mut self) -> Result<(), String> {
        if self.ended {
            return Ok(());
        }
        let clicked = std::mem::take(&mut self.clicked);
        {
            let mut st = self.st.borrow_mut();
            st.now += FRAME;
            st.tick();
            let len = st.msg.chars().count() as f64;
            match st.block.clone() {
                Block::Page => {
                    if st.shown < len {
                        st.shown += CPS * FRAME;
                        if clicked {
                            st.shown = len;
                        }
                    } else if clicked {
                        st.msg.clear();
                        st.name.clear();
                        st.shown = 0.0;
                        st.block = Block::None;
                    }
                }
                Block::Title => {
                    if clicked {
                        let p = st.pointer;
                        let hit = st.title_items.iter().find(|i| {
                            p.0 >= i.dst[0]
                                && p.1 >= i.dst[1]
                                && p.0 < i.dst[0] + i.size[0]
                                && p.1 < i.dst[1] + i.size[1]
                        });
                        match hit.map(|i| i.code) {
                            Some(0) => {
                                st.result = Some(0);
                                st.block = Block::None;
                            }
                            Some(1000) => st.block = Block::Ended,
                            _ => {}
                        }
                    }
                }
                Block::Choice(items) => {
                    if clicked && let Some(i) = choice_at(items.len(), st.pointer) {
                        let k = st.result_key.clone();
                        st.vars.insert(k, Value::Int(i as i64));
                        st.result = Some(i as i32);
                        st.block = Block::None;
                    }
                }
                Block::Ended => self.ended = true,
                Block::None => {}
            }
        }
        if self.ended {
            return Ok(());
        }
        if self.halted.is_some() {
            return Ok(());
        }
        // a script failing (where the dungeon or a battle would begin) stops the scripts, and
        // the screen stays as it is
        if let Err(e) = self.run() {
            eprintln!("deb: {e}");
            self.halted = Some(e);
        }
        Ok(())
    }

    /// Why the scripts stopped, if they did.
    pub fn halted(&self) -> Option<&str> {
        self.halted.as_deref()
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut st = self.st.borrow_mut();
        let mut out = st.layer_draws();
        let (w, h) = (W, H);
        if st.block == Block::Title {
            let bg = st.title_bg.clone();
            if let Some(img) = st.image(&bg) {
                out.push(full(&img, w, h));
            }
            let p = st.pointer;
            let items: Vec<(String, [i32; 4], [i32; 2])> = st
                .title_items
                .iter()
                .map(|i| {
                    let over = p.0 >= i.dst[0]
                        && p.1 >= i.dst[1]
                        && p.0 < i.dst[0] + i.size[0]
                        && p.1 < i.dst[1] + i.size[1];
                    let (f, y) = if over {
                        (&i.over, i.src_y[1])
                    } else {
                        (&i.normal, i.src_y[0])
                    };
                    (f.clone(), [0, y, i.size[0], y + i.size[1]], i.dst)
                })
                .collect();
            for (f, src, dst) in items {
                if let Some(img) = st.image(&f) {
                    out.push(Draw {
                        image: img,
                        src,
                        dst: [
                            dst[0] as f32,
                            dst[1] as f32,
                            (dst[0] + src[2] - src[0]) as f32,
                            (dst[1] + src[3] - src[1]) as f32,
                        ],
                        place: place(w, h, 1.0),
                    });
                }
            }
            return out;
        }
        if !st.msg.is_empty() || !st.name.is_empty() {
            // the message window: MsgBack.png across the bottom
            let wy = h as f32 - 200.0;
            if let Some(back) = st.image("msgback") {
                out.push(Draw {
                    src: [0, 0, back.w as i32, back.h as i32],
                    dst: [0.0, wy, w as f32, h as f32],
                    image: back,
                    place: place(w, h, 0.9),
                });
            } else {
                let s = st.solid([0, 0, 0, 160]);
                out.push(Draw {
                    src: [0, 0, 16, 16],
                    dst: [0.0, wy, w as f32, h as f32],
                    image: s,
                    place: place(w, h, 1.0),
                });
            }
            let size = 26.0;
            if !st.name.is_empty() {
                let name = st.name.clone();
                if let Some(img) = st.text_image(&name, size) {
                    out.push(at(&img, 120.0, wy + 18.0, w, h));
                }
            }
            let n = st.shown as usize;
            let shown: String = st.msg.chars().take(n).collect();
            let mut y = wy + 60.0;
            for line in wrap(&shown, 36) {
                if let Some(img) = st.text_image(&line, size) {
                    out.push(at(&img, 140.0, y, w, h));
                }
                y += size * 1.5;
            }
        }
        if let Block::Choice(items) = st.block.clone() {
            let hover = choice_at(items.len(), st.pointer);
            for (i, t) in items.iter().enumerate() {
                let r = choice_rect(i, items.len());
                let c = if hover == Some(i) {
                    [70, 50, 110, 230]
                } else {
                    [0, 0, 0, 190]
                };
                let s = st.solid(c);
                out.push(Draw {
                    src: [0, 0, 16, 16],
                    dst: [
                        r[0] as f32,
                        r[1] as f32,
                        (r[0] + r[2]) as f32,
                        (r[1] + r[3]) as f32,
                    ],
                    image: s,
                    place: place(w, h, 1.0),
                });
                if let Some(img) = st.text_image(t, 26.0) {
                    let x = r[0] as f32 + (r[2] as f32 - img.w as f32) / 2.0;
                    out.push(at(&img, x, r[1] as f32 + 12.0, w, h));
                }
            }
        }
        out
    }

    pub fn render(&mut self) -> Image {
        let mut img = Image::new(W, H);
        img.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }

    pub fn screen(&self) -> (u32, u32) {
        (W, H)
    }

    pub fn point(&mut self, x: i32, y: i32) {
        self.st.borrow_mut().pointer = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if x >= 0 {
            self.st.borrow_mut().pointer = (x, y);
        }
        self.clicked = true;
    }

    pub fn cancel(&mut self) {}

    /// 4 (enter) advances like a click.
    pub fn key(&mut self, k: i32) {
        if k == 4 {
            self.clicked = true;
        }
    }

    pub fn wheel(&mut self, _up: bool) {}

    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.st.borrow_mut().sounds)
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// A line on what is running, for the headless runner.
    pub fn status(&self) -> String {
        let st = self.st.borrow();
        let mut ls: Vec<&Layer> = st.layers.iter().collect();
        ls.sort_by_key(|l| (l.prio, l.order));
        let names: Vec<String> = ls
            .iter()
            .map(|l| format!("{}@{}", l.name, l.prio))
            .collect();
        format!(
            "{}:{} {:?} [{}] {}「{}」",
            self.cur.0,
            self.cur.1,
            st.block,
            names.join(" "),
            st.name,
            st.msg.chars().take(24).collect::<String>()
        )
    }
}

fn full(img: &Rc<Image>, w: u32, h: u32) -> Draw {
    Draw {
        src: [0, 0, img.w as i32, img.h as i32],
        dst: [0.0, 0.0, w as f32, h as f32],
        image: img.clone(),
        place: place(w, h, 1.0),
    }
}

fn at(img: &Rc<Image>, x: f32, y: f32, w: u32, h: u32) -> Draw {
    let p = crate::text::PAD as f32;
    Draw {
        src: [0, 0, img.w as i32, img.h as i32],
        dst: [x - p, y - p, x - p + img.w as f32, y - p + img.h as f32],
        image: img.clone(),
        place: place(w, h, 1.0),
    }
}

/// Lines of at most `cols` full-width characters (a half-width one counts as half).
pub fn wrap(s: &str, cols: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in s.split('\n') {
        let mut line = String::new();
        let mut width = 0.0;
        for c in para.chars() {
            let cw = if (c as u32) < 0x80 { 0.5 } else { 1.0 };
            let closing = "、。」』）！？…ー".contains(c);
            if width + cw > cols as f64 && !closing {
                out.push(std::mem::take(&mut line));
                width = 0.0;
            }
            line.push(c);
            width += cw;
        }
        out.push(line);
    }
    out
}

fn choice_rect(i: usize, n: usize) -> [i32; 4] {
    let (w, h) = (640, 56);
    let top = (H as i32 - 200 - n as i32 * (h + 16)) / 2;
    [(W as i32 - w) / 2, top + i as i32 * (h + 16), w, h]
}

fn choice_at(n: usize, p: (i32, i32)) -> Option<usize> {
    (0..n).find(|&i| {
        let r = choice_rect(i, n);
        p.0 >= r[0] && p.1 >= r[1] && p.0 < r[0] + r[2] && p.1 < r[1] + r[3]
    })
}
