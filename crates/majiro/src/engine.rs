//! The running game: scripts, threads, pictures, sounds and input, stepped once a frame.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nexas::files::Files;
use nexas::gfx::{Draw, Image, Place};

use crate::arc::Arc;
use crate::gfx::{Gfx, to_image};
use crate::mjo::{Script, hash};
use crate::rct;
use crate::value::Value;
use crate::vm::{Cont, Host, Ret, Stop, Thread, Wait};

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    /// a click: 0 left, 1 right
    Click(usize, i32, i32),
    /// a key: 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip
    Key(usize),
    Wheel(bool),
}

pub struct Engine {
    pub files: Box<dyn Files>,
    pub arcs: Vec<Arc>,
    scripts: HashMap<String, Rc<Script>>,
    funcs: HashMap<u32, (Rc<Script>, usize)>,
    pub persist: HashMap<u32, Value>,
    pub save: HashMap<u32, Value>,
    pub threads: Vec<Thread>,
    pub spawned: Vec<Thread>,
    next_thread: usize,
    /// the thread running now
    pub current: usize,
    pub now: f64,
    pub clock: Option<f64>,
    pub gfx: Gfx,
    /// event hooks in the order they were made: name, the thread that made it, function
    pub hooks: Vec<(String, usize, u32)>,
    pub width: u32,
    pub height: u32,
    pub mouse: (i32, i32),
    pub input: Vec<Input>,
    pub sounds: Vec<SoundEvent>,
    pub log: u8,
    warned: HashSet<String>,
    pub ended: bool,
    pub env: HashMap<String, String>,
    pub sys: crate::sys::State,
    /// draw the screen as it was when the scripts turned updates off
    pub frozen: Option<Vec<Draw>>,
    pub font: Option<fontdue::Font>,
}

/// The "majiro.env" settings: KEY value lines.
fn parse_env(b: &[u8]) -> HashMap<String, String> {
    let text = encoding_rs::SHIFT_JIS.decode(b).0;
    let mut env = HashMap::new();
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with("//") || l.is_empty() {
            continue;
        }
        let mut it = l.split_whitespace();
        if let (Some(k), Some(v)) = (it.next(), it.next()) {
            env.insert(k.to_string(), v.replace('|', " "));
        }
    }
    env
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let mut arcs = Vec::new();
        let mut names: Vec<String> = files
            .list()
            .into_iter()
            .filter(|n| n.to_lowercase().ends_with(".arc"))
            .collect();
        // scripts first, then the rest in name order
        names.sort_by_key(|n| (!n.to_lowercase().starts_with("scenario"), n.clone()));
        for n in names {
            match Arc::open(files.as_mut(), &n) {
                Ok(a) => arcs.push(a),
                Err(e) => eprintln!("{e}"),
            }
        }
        if arcs.is_empty() {
            return Err("no Majiro archives (*.arc) in the game folder".into());
        }
        let mut e = Engine {
            files,
            arcs,
            scripts: HashMap::new(),
            funcs: HashMap::new(),
            persist: HashMap::new(),
            save: HashMap::new(),
            threads: Vec::new(),
            spawned: Vec::new(),
            next_thread: 1,
            current: 0,
            now: 0.0,
            clock: None,
            gfx: Gfx::new(1, 1),
            hooks: Vec::new(),
            width: 800,
            height: 600,
            mouse: (0, 0),
            input: Vec::new(),
            sounds: Vec::new(),
            log: 1,
            warned: HashSet::new(),
            ended: false,
            env: HashMap::new(),
            sys: Default::default(),
            frozen: None,
            font: None,
        };
        if let Some(b) = e.read_file("majiro.env") {
            e.env = parse_env(&b);
        }
        let num =
            |e: &Engine, k: &str, d: u32| e.env.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
        e.width = num(&e, "CLIENT_WIDTH", 800);
        e.height = num(&e, "CLIENT_HEIGHT", 600);
        e.gfx = Gfx::new(e.width as i32, e.height as i32);
        e.sys = crate::sys::State::new(&e.env);
        e.load_persist();
        let start = e.load_script("start")?;
        let main = start
            .functions
            .iter()
            .find(|(_, o)| **o == start.main)
            .map(|(h, _)| *h)
            .ok_or("start.mjo has no main function")?;
        let mut th = Thread::new(0);
        th.call(&mut e, main, Vec::new(), Cont::Drop)?;
        // start.mjo's "$init@GLOBAL" runs before its main
        let init = hash("$init@GLOBAL");
        if start.functions.contains_key(&init) {
            th.call(&mut e, init, Vec::new(), Cont::Drop)?;
        }
        e.threads.push(th);
        Ok(e)
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        self.font = Some(
            fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }

    pub fn warn(&mut self, msg: String) {
        if self.log > 0 && self.warned.insert(msg.clone()) {
            eprintln!("majiro: {msg}");
        }
    }

    /// A file of the game: in an archive, or loose in the folder.
    pub fn read_file(&mut self, name: &str) -> Option<Vec<u8>> {
        for a in &self.arcs {
            if let Some(e) = a.find(name) {
                let e = e.clone();
                return a.read(self.files.as_mut(), &e);
            }
        }
        self.files.read(name)
    }

    pub fn file_exists(&mut self, name: &str) -> bool {
        self.arcs.iter().any(|a| a.find(name).is_some()) || self.files.size(name).is_some()
    }

    /// Loads (once) a script by name without ".mjo" and registers its functions.
    pub fn load_script(&mut self, name: &str) -> Result<Rc<Script>, String> {
        let key = name.to_lowercase();
        if let Some(s) = self.scripts.get(&key) {
            return Ok(s.clone());
        }
        let file = format!("{name}.mjo");
        let b = self
            .read_file(&file)
            .ok_or_else(|| format!("no script {file}"))?;
        let s = Rc::new(Script::parse(&file, &b)?);
        for (h, o) in &s.functions {
            self.funcs.insert(*h, (s.clone(), *o as usize));
        }
        self.scripts.insert(key, s.clone());
        Ok(s)
    }

    /// The hash of a script's entry function (loading it).
    pub fn script_main(&mut self, name: &str) -> Result<u32, String> {
        let s = self.load_script(name)?;
        s.functions
            .iter()
            .find(|(_, o)| **o == s.main)
            .map(|(h, _)| *h)
            .ok_or_else(|| format!("{name}.mjo has no main function"))
    }

    pub fn spawn(&mut self, func: u32, args: Vec<Value>) -> Result<usize, String> {
        let id = self.next_thread;
        self.next_thread += 1;
        let mut th = Thread::new(id);
        th.call(self, func, args, Cont::Drop)?;
        self.spawned.push(th);
        Ok(id)
    }

    /// Has thread `tid` call `f` (an event handler) when it next runs.
    pub fn post_call(&mut self, tid: usize, f: u32, args: Vec<Value>) -> bool {
        match self
            .threads
            .iter_mut()
            .chain(self.spawned.iter_mut())
            .find(|t| t.id == tid && !t.frames.is_empty())
        {
            Some(t) => {
                t.pending.push((f, args));
                true
            }
            None => false,
        }
    }

    pub fn thread_alive(&self, id: usize) -> bool {
        self.threads
            .iter()
            .any(|t| t.id == id && !t.frames.is_empty())
            || self.spawned.iter().any(|t| t.id == id)
    }

    /// A picture by name (".rct" or ".rc8", with its "_.rc8" mask), cached.
    pub fn picture(&mut self, name: &str) -> Option<Rc<Image>> {
        let lower = name.to_lowercase();
        if let Some(i) = self.sys.pics.get(&lower) {
            return Some(i.clone());
        }
        let img = self.load_picture(name, 0)?;
        let img = Rc::new(img);
        if self.sys.pics.len() > 64 {
            self.sys.pics.clear();
        }
        self.sys.pics.insert(lower, img.clone());
        Some(img)
    }

    fn load_picture(&mut self, name: &str, depth: usize) -> Option<Image> {
        let stem = name
            .strip_suffix(".rct")
            .or_else(|| name.strip_suffix(".rc8"))
            .unwrap_or(name);
        let (bytes, file) = [".rct", ".rc8", ""].iter().find_map(|ext| {
            let f = format!("{stem}{ext}");
            self.read_file(&f).map(|b| (b, f))
        })?;
        let pic = match rct::parse(&bytes) {
            Ok(p) => p,
            Err(err) => {
                self.warn(format!("{file}: {err}"));
                return None;
            }
        };
        let mask = self
            .read_file(&format!("{stem}_.rc8"))
            .and_then(|b| rct::parse(&b).ok());
        let mut img = to_image(&pic, mask.as_ref());
        if let rct::Pic::Rgb {
            base: Some(base), ..
        } = &pic
            && depth < 8
            && let Some(b) = self.load_picture(base, depth + 1)
            && b.w == img.w
            && b.h == img.h
        {
            // pure red shows the base picture through
            for (d, s) in img.px.chunks_exact_mut(4).zip(b.px.chunks_exact(4)) {
                if d[0] == 255 && d[1] == 0 && d[2] == 0 {
                    d[..3].copy_from_slice(&s[..3]);
                }
            }
        }
        Some(img)
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    pub fn point(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
        self.input.push(Input::Click(0, x, y));
    }

    pub fn cancel(&mut self) {
        self.input.push(Input::Click(1, self.mouse.0, self.mouse.1));
    }

    pub fn key(&mut self, k: usize) {
        self.input.push(Input::Key(k));
    }

    pub fn wheel(&mut self, up: bool) {
        self.input.push(Input::Wheel(up));
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// The sprites, bottom first (for debugging).
    pub fn describe_sprites(&self) -> Vec<String> {
        self.gfx
            .order
            .iter()
            .filter_map(|id| {
                let s = self.gfx.sprites.get(id)?;
                Some(format!(
                    "sprite {id}: page {} src {:?} at ({}, {}) alpha {:.2} ext {} visible {}",
                    s.page, s.src, s.x, s.y, s.alpha, s.ext, s.visible
                ))
            })
            .collect()
    }

    /// Where every thread is.
    pub fn where_threads(&self) -> Vec<String> {
        self.threads
            .iter()
            .map(|t| format!("thread {}: {} {:?}", t.id, t.whereabouts(), t.wait))
            .collect()
    }

    /// One frame (1/60 s).
    pub fn frame(&mut self) -> Result<(), String> {
        self.now += 1000.0 / 60.0;
        let now = self.now;
        for s in self.gfx.sprites.values_mut() {
            s.tick(now);
        }
        let input = std::mem::take(&mut self.input);
        self.sys.frame_input(&input, self.mouse);
        for ev in &input {
            self.deliver(*ev);
        }
        let mut i = 0;
        while i < self.threads.len() {
            let mut th = std::mem::replace(&mut self.threads[i], Thread::new(usize::MAX));
            self.current = th.id;
            if self.sys.killed.remove(&th.id) {
                th.unwind();
            }
            // event handlers other threads asked this one to run
            for (f, args) in std::mem::take(&mut th.pending) {
                let saved = th.wait.take();
                if let Err(err) = th.call(self, f, args, Cont::Drop) {
                    self.warn(err);
                    th.wait = saved;
                    continue;
                }
                th.frames.last_mut().unwrap().saved_wait = saved;
            }
            let mut r;
            let mut ticks = 0;
            loop {
                if let Some((w, cont)) = th.wait.clone()
                    && let Some(v) = self.poll(&w, th.id)
                {
                    th.wait = None;
                    th.resume(v, cont);
                }
                r = th.run(self, 500_000);
                // `$wait(-2)` gives the other threads a turn, not a whole frame
                if ticks < 32 && matches!(th.wait, Some((Wait::Tick, _))) {
                    ticks += 1;
                    continue;
                }
                break;
            }
            let ended = th.frames.is_empty();
            let wher = th.whereabouts();
            self.threads[i] = th;
            match r {
                Ok(Stop::Budget) => self.warn(format!("thread busy at {wher}")),
                Ok(_) => {}
                Err(e) => return Err(e),
            }
            if ended {
                self.threads.remove(i);
            } else {
                i += 1;
            }
            let new = std::mem::take(&mut self.spawned);
            self.threads.extend(new);
            if self.ended {
                return Ok(());
            }
        }
        if self.threads.is_empty() {
            self.ended = true;
        }
        Ok(())
    }

    /// Hands a click or key to the threads: a skip block takes it first, then a thread
    /// waiting for one.
    fn deliver(&mut self, ev: Input) {
        let button = match ev {
            Input::Click(b, ..) => b,
            Input::Key(5) => 1,
            Input::Key(4) => 0,
            _ => return,
        };
        let mut used = false;
        for th in self.threads.iter_mut() {
            if th.wait.is_some() && th.has_skip() && th.fire_skip(button) {
                used = true;
            }
        }
        if used {
            self.sys.clicked = None;
        }
    }

    /// Whether a wait is over, and the value it gives.
    fn poll(&mut self, w: &Wait, _tid: usize) -> Option<Value> {
        match w {
            Wait::Until(t) => (self.now >= *t || self.sys.skipping()).then_some(Value::Int(0)),
            Wait::Frame | Wait::Tick => Some(Value::Int(0)),
            Wait::Click | Wait::Page => self.sys.take_click().map(|_| Value::Int(1)),
            Wait::Key => self.sys.take_click().map(|k| Value::Int(k as i32)),
            Wait::Sprite(id) => (!self
                .gfx
                .sprites
                .get(id)
                .is_some_and(|s| s.moving() || s.fade.is_some()))
            .then_some(Value::Int(0)),
            Wait::Sound(ch) => (self.now >= self.sys.sound_end[*ch]).then_some(Value::Int(0)),
            Wait::Movie => Some(Value::Int(0)),
            Wait::Thread(id) => (!self.thread_alive(*id)).then_some(Value::Int(0)),
            Wait::Forever => None,
        }
    }

    /// The pictures to draw, bottom first.
    pub fn draws(&mut self) -> Vec<Draw> {
        if let Some(d) = &self.frozen {
            return d.clone();
        }
        let mut out = Vec::new();
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
        let front = self.gfx.front;
        if let Some(img) = self.gfx.shown(front) {
            let (w, h) = (img.w as i32, img.h as i32);
            out.push(Draw {
                image: img,
                src: [0, 0, w, h],
                dst: [0.0, 0.0, w as f32, h as f32],
                place: place(0.0, 0.0, 1.0),
            });
        }
        let order = self.gfx.order.clone();
        for id in &order {
            let Some(s) = self.gfx.sprites.get(id).cloned() else {
                continue;
            };
            if !s.visible || s.alpha <= 0.0 {
                continue;
            }
            let Some(img) = self.gfx.shown(s.page) else {
                continue;
            };
            let s = &s;
            let mut src = s.src;
            if let Some(&(fx, fy, _)) = crate::sys::anim_frame(s, self.now) {
                src[0] = fx;
                src[1] = fy;
            }
            let (w, h) = (src[2] as f32, src[3] as f32);
            // an "_ext" sprite's position is where its centre goes
            let (left, top) = if s.ext {
                (s.x - (src[2] / 2) as f32, s.y - (src[3] / 2) as f32)
            } else {
                (s.x, s.y)
            };
            let mut pl = place(left, top, s.alpha);
            pl.add = s.add;
            pl.rot = s.rot;
            pl.sx = s.sx;
            pl.sy = s.sy;
            pl.cx = left + w / 2.0;
            pl.cy = top + h / 2.0;
            out.push(Draw {
                image: img,
                src: [src[0], src[1], src[0] + src[2], src[1] + src[3]],
                dst: [left, top, left + w, top + h],
                place: pl,
            });
        }
        out
    }

    /// The screen as one picture (for tests and thumbnails).
    pub fn render(&mut self) -> Image {
        let mut out = Image::new(self.width, self.height);
        out.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut out, &d.image, d.src, d.dst, &d.place);
        }
        out
    }

    fn load_persist(&mut self) {
        if let Some(b) = self.files.read_save("persist.bin") {
            crate::sys::read_vars(&b, &mut self.persist);
        }
    }

    pub fn store_persist(&mut self) {
        let b = crate::sys::write_vars(&self.persist);
        self.files.write_save("persist.bin", &b);
    }
}

impl Host for Engine {
    fn function(&mut self, h: u32) -> Option<(Rc<Script>, usize)> {
        self.funcs.get(&h).cloned()
    }

    fn global(&mut self, scope: u8, h: u32) -> Option<&mut Value> {
        if scope == 0 {
            self.persist.get_mut(&h)
        } else {
            self.save.get_mut(&h)
        }
    }

    fn set_global(&mut self, scope: u8, h: u32, v: Value) {
        if scope == 0 {
            self.persist.insert(h, v);
        } else {
            self.save.insert(h, v);
        }
    }

    fn syscall(&mut self, th: &mut Thread, h: u32, args: Vec<Value>) -> Result<Ret, String> {
        crate::sys::call(self, th, h, args)
    }

    fn text(&mut self, th: &mut Thread, s: &[u8]) -> Result<(), String> {
        let s = encoding_rs::SHIFT_JIS.decode(s).0.into_owned();
        crate::sys::text(self, th, &s)
    }

    fn proc_text(&mut self, th: &mut Thread) {
        crate::sys::proc_text(self, th);
    }

    fn ctrl(&mut self, th: &mut Thread, s: &[u8]) -> Result<Option<Wait>, String> {
        let s = String::from_utf8_lossy(s).into_owned();
        crate::sys::ctrl(self, th, &s)
    }
}

/// `$name@MAJIRO_INTER`'s hash.
pub fn syscall_hash(name: &str) -> u32 {
    hash(&format!("${name}@MAJIRO_INTER"))
}
