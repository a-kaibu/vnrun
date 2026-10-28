//! Runs an AZ System game (Triptych's trial) a frame at a time, 800x600.
//!
//! The title buttons start the trial's scripts (`tr_01.asb`..), read record by record:
//!
//! - `2a` a message (a literal `\n` breaks the line, `[base:ruby]` keeps the base), `29` the
//!   speaker's name (or none), `36 n` the message style (row n of window.tbl: colour, place,
//!   characters a line), `68` hides the message, `04` a chapter title (the saves' label)
//! - `4d cg y` a background (cg.tbl names it, drawn `y` pixels up), `3b 3 chara` a character
//!   (chara.tbl: body, eyes and mouth), `3d` no character: all three wait for `38`, which shows
//!   them with a fade (its last number: frames); `3e`/`40` show or hide a character at once,
//!   `4f` pans a tall background from its bottom up
//! - `6e 0 track` music (`track_NN.ogg`), `74` stops it, `75 ch .. loop` a sound on a channel,
//!   `78 ch ms` stops one, `79` a voice; `16 ms` waits; `06` ends the script (to the title)
//!
//! The mask wipes (`.map`), the lip sync (`.tmc`) and the colour effects are left out. The
//! title, the window buttons and the click marker come from the game's own pictures; the
//! message box and the save slots are drawn here.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::arc::Arc;
use crate::script::{self, Rec};

pub const W: u32 = 800;
pub const H: u32 = 600;
const SLOTS: usize = 10;

/// The title's buttons, top first: a script to start, the load screen or the end.
const TITLE: [(&str, Act); 6] = [
    ("title_pro_tr", Act::Start(1)),
    ("title_dig1_tr", Act::Start(2)),
    ("title_dig2_tr", Act::Start(3)),
    ("title_h_tr", Act::Start(4)),
    ("title_load_tr", Act::Load),
    ("title_end_tr", Act::End),
];
const TITLE_AT: (i32, i32, i32) = (640, 362, 38);

/// The window's buttons, right to left from the bottom right corner.
const BUTTONS: [(&str, Act); 3] = [
    ("adv_exit", Act::Title),
    ("adv_load", Act::Load),
    ("adv_save", Act::Save),
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Act {
    Start(usize),
    Load,
    Save,
    Title,
    End,
}

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

/// A picture on a layer; `src` names it for saves (`cg:<id>` or `ch:<id>`).
#[derive(Clone)]
struct Spr {
    src: String,
    img: Rc<Image>,
    x: f32,
    y: f32,
}

#[derive(Clone, Default)]
struct Layer {
    cur: Option<Spr>,
    old: Option<Spr>,
    t: u32,
    dur: u32,
}

impl Layer {
    fn set(&mut self, p: Option<Spr>, dur: u32) {
        self.old = if dur > 0 { self.cur.take() } else { None };
        self.cur = p;
        self.t = 0;
        self.dur = dur;
        if dur == 0 {
            self.old = None;
        }
    }

    fn tick(&mut self) {
        if self.t < self.dur {
            self.t += 1;
        }
        if self.t >= self.dur {
            self.old = None;
        }
    }

    fn done(&mut self) {
        self.t = self.dur;
        self.old = None;
    }

    fn fading(&self) -> bool {
        self.t < self.dur
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Wait {
    None,
    Click,
    Frames(u32),
    Fades,
    Title,
    Ended,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Game,
    Slots { save: bool },
}

/// A tall background moving: from y, to y, frame, frames.
#[derive(Clone, Copy)]
struct Pan(f32, f32, u32, u32);

pub struct Engine {
    files: Box<dyn Files>,
    arc: Arc,
    /// cg.tbl: picture names by id
    cg: Vec<String>,
    /// chara.tbl rows (id - 1), blanks filled from the row before
    chara: Vec<Vec<String>>,
    window_tbl: Vec<Vec<String>>,
    pics: HashMap<String, Option<Rc<Image>>>,
    /// the script running (1: tr_01) and its records
    script: usize,
    recs: Vec<Rec>,
    pc: usize,
    /// the message shown (where a load starts again)
    msg_at: usize,
    bg: Layer,
    ch: Layer,
    /// the background and character waiting for `38` (Some(None): none)
    pend_bg: Option<Option<Spr>>,
    pend_ch: Option<Option<Spr>>,
    pan: Option<Pan>,
    text: String,
    name: Option<String>,
    style: usize,
    window: bool,
    chapter: String,
    wait: Wait,
    view: View,
    pointer: (i32, i32),
    sounds: Vec<SoundEvent>,
    bgm: Option<i32>,
    loops: BTreeMap<i32, String>,
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<(String, i32, [u8; 3]), Rc<Image>>,
    skip: bool,
    pub trace: bool,
    pub frames: u64,
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

fn place(alpha: f32) -> Place {
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
        clip: [0.0, 0.0, W as f32, H as f32],
    }
}

/// A message: a literal `\n` breaks the line, `[base:ruby]` keeps the base.
fn clean(s: &str) -> String {
    let s = s.replace("\\n", "\n");
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(i) = rest.find('[') {
        out += &rest[..i];
        let Some(j) = rest[i..].find(']') else {
            out += &rest[i..];
            rest = "";
            break;
        };
        let inner = &rest[i + 1..i + j];
        out += inner.split(':').next().unwrap_or("");
        rest = &rest[i + j + 1..];
    }
    out + rest
}

fn ms_frames(ms: i32) -> u32 {
    (ms.max(0) as u32 * 60).div_ceil(1000)
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arc = Arc::open(files.as_mut())?;
        let cg = arc
            .tbl(files.as_mut(), "cg.tbl")?
            .into_iter()
            .next()
            .unwrap_or_default();
        let mut chara: Vec<Vec<String>> = Vec::new();
        for row in arc.tbl(files.as_mut(), "chara.tbl")? {
            let prev = chara.last().cloned().unwrap_or_default();
            let n = row.len().max(prev.len());
            let filled = (0..n)
                .map(|j| match row.get(j) {
                    Some(c) if !c.is_empty() => c.clone(),
                    _ => prev.get(j).cloned().unwrap_or_default(),
                })
                .collect();
            chara.push(filled);
        }
        let window_tbl = arc.tbl(files.as_mut(), "window.tbl")?;
        let mut e = Engine {
            files,
            arc,
            cg,
            chara,
            window_tbl,
            pics: HashMap::new(),
            script: 0,
            recs: Vec::new(),
            pc: 0,
            msg_at: 0,
            bg: Layer::default(),
            ch: Layer::default(),
            pend_bg: None,
            pend_ch: None,
            pan: None,
            text: String::new(),
            name: None,
            style: 12,
            window: false,
            chapter: String::new(),
            wait: Wait::Title,
            view: View::Game,
            pointer: (0, 0),
            sounds: Vec::new(),
            bgm: None,
            loops: BTreeMap::new(),
            font: None,
            text_cache: HashMap::new(),
            skip: false,
            trace: std::env::var("AZSYS_LOG").is_ok(),
            frames: 0,
        };
        e.title_screen();
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

    pub fn ended(&self) -> bool {
        self.wait == Wait::Ended
    }

    // ---- files ----

    fn pic(&mut self, name: &str) -> Option<Rc<Image>> {
        if let Some(p) = self.pics.get(name) {
            return p.clone();
        }
        let file = if name.contains('.') {
            name.to_string()
        } else {
            format!("{name}.cpb")
        };
        let img = self
            .arc
            .read(self.files.as_mut(), &file)
            .and_then(|d| crate::cpb::decode(&d).ok())
            .map(Rc::new);
        if img.is_none() && !name.is_empty() {
            self.log(&format!("missing picture {name}"));
        }
        if self.pics.len() > 64 {
            self.pics.clear();
        }
        self.pics.insert(name.to_string(), img.clone());
        img
    }

    /// A character: its body with the first frames of its eyes and mouth on it.
    fn chara_img(&mut self, id: i32) -> Option<Rc<Image>> {
        let key = format!("ch:{id}");
        if let Some(p) = self.pics.get(&key) {
            return p.clone();
        }
        let row = self.chara.get((id - 1).max(0) as usize)?.clone();
        let cell = |j: usize| row.get(j).cloned().unwrap_or_default();
        let num = |j: usize| cell(j).parse::<f32>().unwrap_or(0.0);
        let base = self.pic(&cell(1))?;
        let mut img = (*base).clone();
        for (part, x, y) in [(cell(3), num(4), num(5)), (cell(10), num(11), num(12))] {
            if part.is_empty() {
                continue;
            }
            if let Some(p) = self.pic(&part) {
                let h = p.h as i32 / 3;
                let dst = [x, y, x + p.w as f32, y + h as f32];
                let pl = Place {
                    clip: [0.0, 0.0, img.w as f32, img.h as f32],
                    ..place(1.0)
                };
                nexas::gfx::draw(&mut img, &p, [0, 0, p.w as i32, h], dst, &pl);
            }
        }
        let img = Rc::new(img);
        self.pics.insert(key, Some(img.clone()));
        Some(img)
    }

    fn spr(&mut self, src: &str, y: f32) -> Option<Spr> {
        let (kind, id) = src.split_once(':')?;
        let id: i32 = id.parse().ok()?;
        match kind {
            "cg" => {
                let name = self.cg.get(id as usize)?.clone();
                let img = self.pic(&name)?;
                Some(Spr {
                    src: src.to_string(),
                    img,
                    x: 0.0,
                    y,
                })
            }
            "ch" => {
                let img = self.chara_img(id)?;
                Some(Spr {
                    src: src.to_string(),
                    x: (W as f32 - img.w as f32) / 2.0,
                    y: H as f32 - img.h as f32,
                    img,
                })
            }
            _ => None,
        }
    }

    fn play(&mut self, channel: &str, file: &str, repeat: bool) {
        let data = self.arc.read(self.files.as_mut(), file);
        if data.is_none() {
            self.log(&format!("missing sound {file}"));
            return;
        }
        self.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data,
            volume: 1.0,
            repeat,
            fade_ms: 0.0,
        });
    }

    fn stop(&mut self, channel: &str, fade_ms: f32) {
        self.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data: None,
            volume: 0.0,
            repeat: false,
            fade_ms,
        });
    }

    fn play_bgm(&mut self, track: i32) {
        if self.bgm == Some(track) {
            return;
        }
        self.bgm = Some(track);
        self.play("bgm", &format!("track_{track:02}.ogg"), true);
    }

    // ---- flow ----

    fn title_screen(&mut self) {
        self.reset();
        self.wait = Wait::Title;
        self.play_bgm(1);
    }

    /// No script, no pictures, no sound.
    fn reset(&mut self) {
        self.recs.clear();
        self.script = 0;
        self.bg = Layer::default();
        self.ch = Layer::default();
        self.pend_bg = None;
        self.pend_ch = None;
        self.pan = None;
        self.text.clear();
        self.name = None;
        self.window = false;
        self.skip = false;
        self.stop_all();
        self.view = View::Game;
    }

    fn stop_all(&mut self) {
        for c in ["voice", "se0", "se1", "se2", "se3"] {
            self.stop(c, 0.0);
        }
        self.loops.clear();
        self.stop("bgm", 0.0);
        self.bgm = None;
    }

    fn start(&mut self, n: usize) -> Result<(), String> {
        let d = self
            .arc
            .asb(self.files.as_mut(), &format!("tr_{n:02}.asb"))?;
        self.reset();
        self.recs = script::parse(&d);
        self.script = n;
        self.pc = 0;
        self.msg_at = 0;
        self.style = 12;
        self.chapter.clear();
        self.wait = Wait::None;
        self.log(&format!("start tr_{n:02}: {} records", self.recs.len()));
        Ok(())
    }

    fn arg(r: &Rec, i: usize) -> i32 {
        r.a.get(i).copied().unwrap_or(0)
    }

    fn step(&mut self) -> Result<(), String> {
        let Some(r) = self.recs.get(self.pc) else {
            self.title_screen();
            return Ok(());
        };
        let at = self.pc;
        self.pc += 1;
        let (op, s) = (r.op, r.s.clone());
        let a = |i: usize| Self::arg(r, i);
        let (a1, a2, a3) = (a(1), a(2), a(3));
        let last = r.a.last().copied().unwrap_or(0);
        let n = r.a.len();
        if self.trace {
            self.log(&format!("{at} {op:02x} {:?} {s}", r.a));
        }
        match op {
            script::TEXT => {
                self.text = clean(&s);
                self.window = true;
                self.msg_at = at;
                self.wait = Wait::Click;
            }
            script::NAME => {
                self.name = (a1 != 0 && !s.is_empty()).then_some(s);
            }
            script::CHAPTER => self.chapter = s,
            0x36 => self.style = a1.max(1) as usize,
            0x68 => {
                self.window = false;
                self.text.clear();
            }
            0x4d => {
                let p = self.spr(&format!("cg:{a1}"), -(a2 as f32));
                self.pend_bg = Some(p);
                self.pan = None;
            }
            0x3b => {
                let p = self.spr(&format!("ch:{a2}"), 0.0);
                self.pend_ch = Some(p);
            }
            0x3d => self.pend_ch = Some(None),
            0x38 => {
                let dur = if n > 4 { last.clamp(0, 120) as u32 } else { 0 };
                let dur = if self.skip { 0 } else { dur };
                if let Some(p) = self.pend_bg.take() {
                    self.bg.set(p, dur);
                }
                if let Some(p) = self.pend_ch.take() {
                    self.ch.set(p, dur);
                }
                if dur > 0 {
                    self.wait = Wait::Fades;
                }
            }
            0x3e => {
                let p = self.spr(&format!("ch:{a2}"), 0.0);
                self.ch.set(p, if self.skip { 0 } else { 16 });
            }
            0x40 => self
                .ch
                .set(None, if self.skip { 0 } else { a3.max(0) as u32 }),
            0x4f => {
                if let Some(p) = self.spr(&format!("cg:{a2}"), 0.0) {
                    let from = H as f32 - p.img.h as f32;
                    let mut p = p;
                    p.y = from;
                    self.bg.set(Some(p), 0);
                    self.pan = Some(Pan(from, 0.0, 0, 90));
                }
            }
            0x16 => {
                if !self.skip {
                    self.wait = Wait::Frames(ms_frames(a1));
                }
            }
            0x6e => self.play_bgm(a2),
            0x74 => {
                self.bgm = None;
                self.stop("bgm", a2 as f32);
            }
            script::SE => {
                let ch = format!("se{a1}");
                let looped = a3 != 0;
                self.play(&ch, &s, looped);
                if looped {
                    self.loops.insert(a1, s);
                } else {
                    self.loops.remove(&a1);
                }
            }
            0x78 => {
                self.loops.remove(&a1);
                self.stop(&format!("se{a1}"), a2 as f32);
            }
            script::VOICE => {
                if !self.skip {
                    self.play("voice", &s, false);
                }
            }
            0x06 | 0x00 => self.title_screen(),
            _ => {}
        }
        Ok(())
    }

    fn fading(&self) -> bool {
        self.bg.fading() || self.ch.fading() || self.pan.is_some()
    }

    pub fn frame(&mut self) -> Result<(), String> {
        self.frames += 1;
        if self.view != View::Game {
            return Ok(());
        }
        self.bg.tick();
        self.ch.tick();
        if let Some(Pan(from, to, t, dur)) = self.pan {
            let t = t + 1;
            let k = (t as f32 / dur as f32).min(1.0);
            if let Some(c) = &mut self.bg.cur {
                c.y = from + (to - from) * k;
            }
            self.pan = (t < dur).then_some(Pan(from, to, t, dur));
        }
        match self.wait {
            Wait::Frames(n) => {
                self.wait = if n <= 1 {
                    Wait::None
                } else {
                    Wait::Frames(n - 1)
                }
            }
            Wait::Fades if !self.fading() => self.wait = Wait::None,
            Wait::Click if self.skip && self.frames.is_multiple_of(3) => self.advance(),
            _ => {}
        }
        let mut steps = 0;
        while self.wait == Wait::None {
            self.step()?;
            steps += 1;
            if steps > 100_000 {
                return Err("the script runs without waiting".into());
            }
        }
        Ok(())
    }

    fn finish_fades(&mut self) {
        self.bg.done();
        self.ch.done();
        if let Some(Pan(_, to, _, _)) = self.pan.take()
            && let Some(c) = &mut self.bg.cur
        {
            c.y = to;
        }
    }

    fn advance(&mut self) {
        match self.wait {
            Wait::Click => {
                self.stop("voice", 0.0);
                self.wait = Wait::None;
            }
            Wait::Fades => {
                self.finish_fades();
                self.wait = Wait::None;
            }
            Wait::Frames(_) => self.wait = Wait::None,
            _ => {}
        }
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    fn act(&mut self, a: Act) {
        match a {
            Act::Start(n) => {
                if let Err(e) = self.start(n) {
                    self.log(&e);
                }
            }
            Act::Load => self.view = View::Slots { save: false },
            Act::Save => self.view = View::Slots { save: true },
            Act::Title => self.title_screen(),
            Act::End => {
                self.stop_all();
                self.wait = Wait::Ended;
            }
        }
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
        if let View::Slots { save } = self.view {
            if let Some(i) = self.slot_rects().iter().position(|r| inside(*r, x, y)) {
                if save {
                    self.save(i);
                    self.view = View::Game;
                } else if self.read_slot(i).is_some() {
                    self.load(i);
                }
            } else {
                self.view = View::Game;
            }
            return;
        }
        if self.wait == Wait::Title {
            if let Some(i) = self.title_rects().iter().position(|r| inside(*r, x, y)) {
                self.act(TITLE[i].1);
            }
            return;
        }
        if self.window
            && let Some(i) = self.button_rects().iter().position(|r| inside(*r, x, y))
        {
            let a = BUTTONS[i].1;
            if a != Act::Save || self.wait == Wait::Click {
                self.act(a);
            }
            return;
        }
        self.skip = false;
        self.advance();
    }

    /// A right click.
    pub fn cancel(&mut self) {
        if self.view != View::Game {
            self.view = View::Game;
        }
    }

    /// A key: 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip.
    pub fn key(&mut self, k: usize) {
        match k {
            4 if self.view == View::Game && self.wait != Wait::Title => {
                self.skip = false;
                self.advance();
            }
            5 => self.cancel(),
            6 if self.wait != Wait::Title => self.skip = !self.skip,
            _ => {}
        }
    }

    /// The wheel: down goes forward.
    pub fn wheel(&mut self, up: bool) {
        if !up && self.view == View::Game {
            self.advance();
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

    fn solid(rgba: [u8; 4], alpha: f32, r: [i32; 4]) -> Draw {
        let mut img = Image::new(1, 1);
        img.px.copy_from_slice(&rgba);
        Draw {
            image: Rc::new(img),
            src: [0, 0, 1, 1],
            dst: r.map(|v| v as f32),
            place: place(alpha),
        }
    }

    fn draw(img: &Rc<Image>, src: [i32; 4], x: f32, y: f32, alpha: f32) -> Draw {
        Draw {
            image: img.clone(),
            src,
            dst: [
                x,
                y,
                x + (src[2] - src[0]) as f32,
                y + (src[3] - src[1]) as f32,
            ],
            place: place(alpha),
        }
    }

    fn whole(img: &Rc<Image>, x: f32, y: f32, alpha: f32) -> Draw {
        Self::draw(img, [0, 0, img.w as i32, img.h as i32], x, y, alpha)
    }

    fn text_draw(&mut self, s: &str, size: i32, rgb: [u8; 3], x: i32, y: i32, out: &mut Vec<Draw>) {
        let img = self.text_img(s, size, rgb);
        out.push(Self::whole(&img, x as f32, y as f32, 1.0));
    }

    fn title_rects(&mut self) -> Vec<[i32; 4]> {
        let (x, y0, step) = TITLE_AT;
        (0..TITLE.len() as i32)
            .map(|i| [x, y0 + i * step, x + 133, y0 + i * step + 36])
            .collect()
    }

    fn button_rects(&mut self) -> Vec<[i32; 4]> {
        let mut x = W as i32 - 6;
        let mut out = Vec::new();
        for (n, _) in BUTTONS {
            let (w, h) = self.pic(n).map_or((60, 16), |p| (p.w as i32, p.h as i32));
            out.push([x - w, H as i32 - 6 - h, x, H as i32 - 6]);
            x -= w + 6;
        }
        out
    }

    /// A picture, or its `-o` one under the pointer.
    fn hover_draw(&mut self, name: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let over = inside(r, self.pointer.0, self.pointer.1);
        let p = if over {
            self.pic(&format!("{name}-o")).or_else(|| self.pic(name))
        } else {
            self.pic(name)
        };
        if let Some(p) = p {
            out.push(Self::whole(&p, r[0] as f32, r[1] as f32, 1.0));
        }
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        if self.wait == Wait::Title {
            if let Some(p) = self.pic("title_bg_tr") {
                out.push(Self::whole(&p, 0.0, 0.0, 1.0));
            }
            for (i, r) in self.title_rects().into_iter().enumerate() {
                self.hover_draw(TITLE[i].0, r, &mut out);
            }
        } else {
            for l in [&self.bg, &self.ch] {
                let a = if l.dur == 0 {
                    1.0
                } else {
                    l.t as f32 / l.dur as f32
                };
                if let Some(o) = &l.old {
                    out.push(Self::whole(&o.img, o.x, o.y, 1.0 - a));
                }
                if let Some(c) = &l.cur {
                    out.push(Self::whole(&c.img, c.x, c.y, a));
                }
            }
            if self.window && !self.text.is_empty() {
                self.window_draws(&mut out);
            }
        }
        if let View::Slots { save } = self.view {
            self.slot_draws(save, &mut out);
        }
        out
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let row = self
            .window_tbl
            .get(self.style - 1)
            .filter(|r| r.len() > 11)
            .cloned()
            .unwrap_or_default();
        let num = |j: usize, d: i32| row.get(j).and_then(|c| c.parse().ok()).unwrap_or(d);
        let (x, y) = (num(6, 23), num(7, 453));
        let (cols, lines, size, lh) = (num(8, 56), num(9, 4), num(10, 26), num(11, 30));
        let rgb = row
            .get(4)
            .and_then(|c| u32::from_str_radix(c.trim_start_matches("0x"), 16).ok())
            .map_or([255, 255, 255], |v| {
                [(v >> 16) as u8, (v >> 8) as u8, v as u8]
            });
        let top = if self.name.is_some() {
            y - lh - 10
        } else {
            y - 12
        };
        let right = (x + cols * size / 2 + 16).min(W as i32);
        out.push(Self::solid(
            [0, 0, 0, 255],
            0.55,
            [x - 16, top, right, y + lines * lh + 10],
        ));
        if let Some(n) = self.name.clone() {
            self.text_draw(&format!("【{n}】"), size, rgb, x, y - lh - 4, out);
        }
        let mut ly = y;
        let mut end = (x, y);
        for line in wrap(&self.text.clone(), (cols / 2).max(1) as usize) {
            let img = self.text_img(&line, size, rgb);
            end = (x + img.w as i32, ly);
            out.push(Self::whole(&img, x as f32, ly as f32, 1.0));
            ly += lh;
        }
        if self.wait == Wait::Click
            && let Some(p) = self.pic("adv_key_wait")
        {
            let n = (p.h / p.w.max(1)).max(1) as i32;
            let f = (self.frames / 5) as i32 % n;
            let s = p.w as i32;
            out.push(Self::draw(
                &p,
                [0, f * s, s, (f + 1) * s],
                end.0 as f32,
                (end.1 + 2) as f32,
                1.0,
            ));
        }
        for (i, r) in self.button_rects().into_iter().enumerate() {
            self.hover_draw(BUTTONS[i].0, r, out);
        }
    }

    // ---- saves ----

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        (0..SLOTS as i32)
            .map(|i| {
                let (c, r) = (i % 2, i / 2);
                let (x, y) = (30 + c * 375, 90 + r * 96);
                [x, y, x + 360, y + 80]
            })
            .collect()
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        out.push(Self::solid(
            [0, 0, 0, 255],
            0.85,
            [0, 0, W as i32, H as i32],
        ));
        let title = if save { "SAVE" } else { "LOAD" };
        self.text_draw(title, 28, [255, 255, 255], 30, 30, out);
        let (px, py) = self.pointer;
        for (i, r) in self.slot_rects().into_iter().enumerate() {
            let c = if inside(r, px, py) {
                [150, 30, 40, 255]
            } else {
                [70, 20, 30, 255]
            };
            out.push(Self::solid(c, 0.9, r));
            let label = match self.read_slot(i) {
                Some(s) => format!("{:02}  {}", i + 1, s.label),
                None => format!("{:02}  ----", i + 1),
            };
            self.text_draw(&label, 18, [255, 255, 255], r[0] + 12, r[1] + 28, out);
        }
    }

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&format!("slot{i:02}.sav"))?;
        Slot::parse(&String::from_utf8(b).ok()?)
    }

    fn save(&mut self, i: usize) {
        if self.wait != Wait::Click || self.script == 0 {
            return;
        }
        let t: String = self
            .text
            .chars()
            .filter(|c| !c.is_control())
            .take(8)
            .collect();
        let s = Slot {
            label: format!("{} {t}", self.chapter),
            script: self.script,
            at: self.msg_at,
            bg: self.bg.cur.as_ref().map(|p| (p.src.clone(), p.y)),
            ch: self.ch.cur.as_ref().map(|p| p.src.clone()),
            bgm: self.bgm,
            loops: self.loops.clone(),
            style: self.style,
            name: self.name.clone(),
            chapter: self.chapter.clone(),
        };
        self.files
            .write_save(&format!("slot{i:02}.sav"), s.write().as_bytes());
    }

    fn load(&mut self, i: usize) {
        let Some(s) = self.read_slot(i) else {
            return;
        };
        if let Err(e) = self.start(s.script) {
            self.log(&e);
            return;
        }
        self.pc = s.at;
        if let Some((src, y)) = s.bg {
            let p = self.spr(&src, y);
            self.bg.set(p, 0);
        }
        if let Some(src) = s.ch {
            let p = self.spr(&src, 0.0);
            self.ch.set(p, 0);
        }
        if let Some(b) = s.bgm {
            self.play_bgm(b);
        }
        for (c, n) in s.loops {
            self.play(&format!("se{c}"), &n, true);
            self.loops.insert(c, n);
        }
        self.style = s.style;
        self.name = s.name;
        self.chapter = s.chapter;
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
}

/// Splits text into lines of at most `n` characters, at its own line breaks too.
fn wrap(s: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in s.split('\n') {
        let chars: Vec<char> = para.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let mut end = (i + n).min(chars.len());
            // keep closing marks with the line before
            while end < chars.len() && "」』）。、！？…ー〜".contains(chars[end]) && end - i < n + 4
            {
                end += 1;
            }
            out.push(chars[i..end].iter().collect());
            i = end;
        }
    }
    out
}

/// A save, as lines of `key value`.
struct Slot {
    label: String,
    script: usize,
    at: usize,
    bg: Option<(String, f32)>,
    ch: Option<String>,
    bgm: Option<i32>,
    loops: BTreeMap<i32, String>,
    style: usize,
    name: Option<String>,
    chapter: String,
}

impl Slot {
    fn write(&self) -> String {
        let mut s = String::from("AZSYSSAV1\n");
        s += &format!("label {}\n", self.label.replace('\n', " "));
        s += &format!(
            "script {}\nat {}\nstyle {}\n",
            self.script, self.at, self.style
        );
        if let Some((src, y)) = &self.bg {
            s += &format!("bg {y} {src}\n");
        }
        if let Some(c) = &self.ch {
            s += &format!("ch {c}\n");
        }
        if let Some(b) = self.bgm {
            s += &format!("bgm {b}\n");
        }
        for (c, n) in &self.loops {
            s += &format!("loop {c} {n}\n");
        }
        if let Some(n) = &self.name {
            s += &format!("name {n}\n");
        }
        s += &format!("chapter {}\n", self.chapter);
        s
    }

    fn parse(t: &str) -> Option<Slot> {
        let mut lines = t.lines();
        if lines.next()? != "AZSYSSAV1" {
            return None;
        }
        let mut s = Slot {
            label: String::new(),
            script: 0,
            at: 0,
            bg: None,
            ch: None,
            bgm: None,
            loops: BTreeMap::new(),
            style: 12,
            name: None,
            chapter: String::new(),
        };
        for l in lines {
            let (k, rest) = l.split_once(' ').unwrap_or((l, ""));
            match k {
                "label" => s.label = rest.to_string(),
                "script" => s.script = rest.parse().ok()?,
                "at" => s.at = rest.parse().ok()?,
                "style" => s.style = rest.parse().ok()?,
                "bg" => {
                    let (y, src) = rest.split_once(' ')?;
                    s.bg = Some((src.to_string(), y.parse().ok()?));
                }
                "ch" => s.ch = Some(rest.to_string()),
                "bgm" => s.bgm = rest.parse().ok(),
                "loop" => {
                    let (c, n) = rest.split_once(' ')?;
                    s.loops.insert(c.parse().ok()?, n.to_string());
                }
                "name" => s.name = Some(rest.to_string()),
                "chapter" => s.chapter = rest.to_string(),
                _ => {}
            }
        }
        (s.script > 0).then_some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_text() {
        assert_eq!(clean("a\\nb[咀嚼:そしゃく]c"), "a\nb咀嚼c");
    }
}
