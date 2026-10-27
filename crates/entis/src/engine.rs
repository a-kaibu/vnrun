//! Runs the script: the threads of the Cotopha program, the engine objects it makes (sprites,
//! resources, the screen window, the input filter) and drawing the screen.

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use nexas::files::Files;
use nexas::gfx::{Draw, Image};

use crate::game::Fs;
use crate::natives::{Obj, Pic};
use crate::text::Fonts;
use crate::value::*;
use crate::vm::{Program, Start, Thread};

pub const FRAME_MS: f64 = 1000.0 / 60.0;

/// A sound to start (`data`) or stop (`None`) on a channel.
pub struct SoundEvent {
    pub channel: String,
    pub data: Option<Rc<Vec<u8>>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: u32,
}

/// A command from a skin item: (item, full item path, notification, parameter).
pub type Command = (String, String, i32, i32);

pub struct Engine {
    pub prog: Program,
    pub globals: Vec<(Str, Var)>,
    pub data: Vec<(Str, Var)>,
    pub fs: Fs,
    /// by thread number; the running one is taken out while it runs
    pub threads: Vec<Option<Thread>>,
    pub current: usize,
    pub now: f64,
    /// when the waiting engine call of the running thread gives up
    pub deadline: Option<f64>,
    pub width: u32,
    pub height: u32,
    pub warned: HashSet<String>,
    pub log: u8,
    pub pics: HashMap<String, Option<Rc<Pic>>>,
    /// the window CreateDisplay made
    pub screen: Option<Obj>,
    pub commands: VecDeque<Command>,
    pub queue_commands: bool,
    pub mouse: (i32, i32),
    /// the button under the pointer
    pub focus: Option<Obj>,
    /// times each virtual joystick button was pushed since it was flushed
    pub joy_pushed: [i32; 16],
    pub sounds: Vec<SoundEvent>,
    pub fonts: Fonts,
    pub ended: bool,
    pub rand: u64,
    pub next_channel: u32,
    /// sprites with a running BeginActivation
    pub active: Vec<std::rc::Weak<std::cell::RefCell<crate::natives::Native>>>,
}

impl Engine {
    pub fn open(mut host: Box<dyn Files>, paths: Option<Vec<String>>) -> Result<Engine, String> {
        let paths = paths.unwrap_or_else(|| host.list());
        let mut fs = Fs::open(host, &paths)?;
        let csx = fs
            .read("script.csx")
            .ok_or("script.csx: not found (no NOA archive with the script, or no password)")?;
        let img = crate::csx::Image::parse(&csx)?;
        let (globals, data, make) = crate::vm::variables(&img);
        let prologue = img.prologue.clone();
        let prog = Program::load(img)?;
        let mut main = Thread::default();
        for (v, c) in make {
            main.queue.push(Start::Construct(v, c));
        }
        for a in prologue {
            main.queue.push(Start::Call(prog.index(a)?));
        }
        main.queue.push(Start::Call(
            *prog.funcs.get("main").ok_or("script.csx: no main")?,
        ));
        Ok(Engine {
            prog,
            globals,
            data,
            fs,
            threads: vec![Some(main)],
            current: 0,
            now: 0.0,
            deadline: None,
            width: 800,
            height: 600,
            warned: HashSet::new(),
            log: 0,
            pics: HashMap::new(),
            screen: None,
            commands: VecDeque::new(),
            queue_commands: false,
            mouse: (-1, -1),
            focus: None,
            joy_pushed: [0; 16],
            sounds: Vec::new(),
            fonts: Fonts::new(),
            ended: false,
            rand: 0x2545_f491_4f6c_dd1d,
            next_channel: 0,
            active: Vec::new(),
        })
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        self.fonts.set(ttf)
    }

    pub fn set_log(&mut self, level: u8) {
        self.log = level;
    }

    pub fn warn(&mut self, msg: String) {
        if self.log > 0 || self.warned.insert(msg.clone()) {
            eprintln!("entis: {msg}");
        }
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    /// A picture file (ERI, or PNG/BMP), decoded once.
    pub fn pic(&mut self, name: &str) -> Option<Rc<Pic>> {
        let key = name.to_lowercase();
        if key.is_empty() {
            return None;
        }
        if let Some(p) = self.pics.get(&key) {
            return p.clone();
        }
        let mut found = None;
        let lower = name.to_lowercase();
        let mut tries = vec![name.to_string()];
        if !lower.ends_with(".eri") && !lower.ends_with(".mei") && !lower.ends_with(".png") {
            tries.push(format!("{name}.eri"));
        }
        for n in tries {
            if let Some(b) = self.fs.read(&n) {
                match crate::eri::decode(&b) {
                    Ok(e) => {
                        let mut img = Image::new(e.w, e.h);
                        img.px = e.px;
                        found = Some(Rc::new(Pic {
                            img: Rc::new(img),
                            hot: e.hot,
                        }));
                    }
                    Err(err) => match Image::decode(&b) {
                        Ok(img) => {
                            found = Some(Rc::new(Pic {
                                img: Rc::new(img),
                                hot: (0, 0),
                            }))
                        }
                        Err(_) => self.warn(format!("{n}: {err}")),
                    },
                }
                break;
            }
        }
        if found.is_none() {
            self.warn(format!("{name}: no such picture"));
        }
        self.pics.insert(key, found.clone());
        found
    }

    pub fn random(&mut self) -> u32 {
        self.rand ^= self.rand << 13;
        self.rand ^= self.rand >> 7;
        self.rand ^= self.rand << 17;
        (self.rand >> 16) as u32
    }

    /// Waits for `ms` from the first call: true once the time is up.
    pub fn waited(&mut self, ms: f64) -> bool {
        match self.deadline {
            None => {
                if ms <= 0.0 {
                    return true;
                }
                self.deadline = Some(self.now + ms);
                false
            }
            Some(d) => self.now >= d,
        }
    }

    /// Starts a thread that calls a script function.
    pub fn spawn(&mut self, f: usize, this: Option<Var>, args: Vec<Var>) -> usize {
        let mut t = Thread::default();
        t.queue.push(Start::Run(f, this, args));
        self.threads.push(Some(t));
        self.threads.len() - 1
    }

    pub fn thread_running(&self, id: usize) -> bool {
        if id == self.current {
            return true;
        }
        matches!(self.threads.get(id), Some(Some(t)) if !t.done)
    }

    /// Runs one frame (the scripts count on 60 a second).
    pub fn frame(&mut self) -> Result<(), String> {
        if self.ended {
            return Ok(());
        }
        self.now += FRAME_MS;
        self.animate();
        let n = self.threads.len();
        for i in 0..n {
            let Some(mut t) = self.threads[i].take() else {
                continue;
            };
            if t.done {
                self.threads[i] = Some(t);
                continue;
            }
            self.current = i;
            self.deadline = t.deadline.take();
            let r = self.run_thread(&mut t);
            t.deadline = self.deadline.take();
            self.current = usize::MAX;
            let main_done = i == 0 && t.done;
            self.threads[i] = Some(t);
            if let Err(e) = r {
                if i == 0 {
                    self.ended = true;
                    return Err(e);
                }
                self.warn(format!("thread {i} stopped: {e}"));
                if let Some(t) = self.threads[i].as_mut() {
                    t.done = true;
                }
            }
            if main_done {
                self.ended = true;
                break;
            }
        }
        // drop finished threads at the end of the list
        while self.threads.len() > 1 && matches!(self.threads.last(), Some(Some(t)) if t.done) {
            self.threads.pop();
        }
        Ok(())
    }

    // ---- input

    pub fn point(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
        self.update_focus();
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.point(x, y);
        if !self.press_focus() {
            self.joy_pushed[4] += 1;
        }
    }

    /// A right click.
    pub fn cancel(&mut self) {
        self.joy_pushed[5] += 1;
    }

    /// A key, as the input filter maps it: 0 up, 1 down, 2 left, 3 right, 4+ buttons.
    pub fn joy(&mut self, button: usize) {
        if button < self.joy_pushed.len() {
            self.joy_pushed[button] += 1;
        }
    }

    pub fn wheel(&mut self, up: bool) {
        let _ = up;
    }

    // ---- drawing

    /// The pictures of the screen, bottom first.
    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        if let Some(s) = self.screen.clone() {
            self.draw_sprite(&s, crate::natives::Aff::ID, 1.0, &mut out);
        }
        out
    }

    /// The screen as one picture.
    pub fn render(&mut self) -> Image {
        let mut out = Image::new(self.width, self.height);
        out.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut out, &d.image, d.src, d.dst, &d.place);
        }
        out
    }
}
