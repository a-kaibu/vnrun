//! Runs the SCD scripts a frame at a time, from `#start` of SYSTEM.SCD (which goes on to
//! SYSMENU: SYSINIT, the logos of SYSLOGO, then the title).
//!
//! The commands, as read from the game's interpreter (its table at 0x42c190). Arguments are in
//! the order the script pushes them; `s` is a string, `#l` a label. Each command leaves a value
//! (0 unless said) except the jumps and calls:
//!
//! | op | args | |
//! |----|------|-|
//! | 14 | flag, k | flag set / cleared / flipped (k 0 / 1 / 2); from 5000 the game's own |
//! | 15 | flag x n, n, k | 1 when the n flags are all set / all clear / one set / one clear (k 0..3) |
//! | 17 | n | a random number below n |
//! | 18 | t | wait t hundredths of a second |
//! | 1A | #l | go to the label |
//! | 1B | #l x n, n, i | go to label i % n (the last pushed is 0) |
//! | 1C | #l, s | go to the label of the script s |
//! | 1D / 1E / 1F | v x n, n, #l (, s) | call the label here / of the script s / of SYSTEM.SCD; the values go to the variables 6000.. (last pushed first) |
//! | 22 | v | return v |
//! | 25 / 26 | v, k / k | the game's switches 0..5 (1: saves are made at each text) |
//! | 28 | k, n | music n of BGM.VIC: k 0..3 play (1, 3 for ever), 5 stop, 6 fade out, 4 again |
//! | 2A | k, n, s, ch | sound n of s.VIC on a channel: k 1 for ever, 9 stop; s `*` all again |
//! | 2C | s wav, s vic, ? | a voice: the entry s.WAV of vic.VIC |
//! | 32 | on | no text window (the logos) |
//! | 37 | r, g, b, i | colour i |
//! | 38 | kind, y pad, x pad, h, w, y, x, ?, colour, id | a window (text, choices, or a group of hot spots) |
//! | 3B / 3C | ?, id | open (on top) / close a window |
//! | 3D | id | clear the window's text |
//! | 43 | #l x n, n, &v, ?, id | the choices (lines `!0..` of the window's text) and where each goes |
//! | 44 | y1', x1', y1'', y0, x0, .., id | a hot spot of the window: x0..x1, y0..y1 |
//! | 45 | #sel, #off, #on, &v, ?, id | the window's hot spots call #on / #off as the pointer comes and goes, #sel on a click (v: which) |
//! | 46 | on, i, id | hot spot i of the window may be used |
//! | 47 | ?, ?, ? | wait for the top window's hot spots |
//! | 48 | k | wait for a click (k 2: then clear the text) |
//! | 4D | k | 0: back from #on / #off (else the end); 255: quit |
//! | 50 | ?, t, k | show the work screen with effect k (0 at once, else a fade) over t hundredths |
//! | 53 / 56 | n, s, surface | picture n of s.GR2 / its brightness as the shape, into a surface |
//! | 57 | ?, colour, h, w, y, x, surface | fill |
//! | 58 / 5F | sy, sx, from, h, w, y, x, to | copy pixels / shape |
//! | 5B | my, mx, shape, sy, sx, from, h, w, y, x, to | copy through the shape of another surface |
//! | 98 | s | the speaker (`＠＠＠＠＠` is the hero, `*` keeps it) |
//! | 99 | t, k, n | background n of BG.GR2 (10000 black, 10001 white); the characters go |
//! | 9A | t, k, pose, who | a character (who % 4 == 1: CHR.GR2, else CHR2.GR2; pictures 2 pose + 1, shape 2 pose + 2), up to 3 side by side |
//! | 9B | t, k, n | event picture n of EVENT.GR2 instead of the background |
//! | 9D | k | the game's own screens (2: load) |
//! | A2 | t, k | no characters |
//!
//! Surfaces 10000..10003 are the game's (10002 the work screen, 10003 the window parts of
//! SYS.GR2), 10004 the top window's own, others the script's. An effect k % 100 of 10 draws
//! without showing.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::gr2;
use crate::pack::Pack;
use crate::pola;
use crate::scd::{self, Operand, Script};

const FRAME_MS: f64 = 1000.0 / 60.0;
/// the unit of waits and fades
const UNIT_MS: f64 = 10.0;
const W: u32 = 800;
const H: u32 = 600;
const SLOTS: usize = 10;
const TEXT_SIZE: i32 = 24;
const LINE_STEP: i32 = 32;
const CHOICE_STEP: i32 = 26;
const MAX_FRAMES: usize = 20;
const HERO_NAME: &str = "彰";
const HERO_FAMILY: &str = "市ノ瀬";

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

#[derive(Clone, Debug, PartialEq)]
enum Val {
    Int(u16),
    Str(Vec<u8>),
}

impl Val {
    fn int(&self) -> u16 {
        match self {
            Val::Int(v) => *v,
            Val::Str(_) => 0,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        match self {
            Val::Str(s) => s.clone(),
            Val::Int(v) => v.to_string().into_bytes(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Wait {
    None,
    Click,
    Until(f64),
    Fade,
    /// the hot spots of a window
    Menu(u16),
    /// the load screen opened from the title
    Load,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Game,
    Menu,
    Slots { save: bool },
}

#[derive(Clone, Debug, PartialEq)]
struct Frame {
    script: String,
    pc: usize,
}

struct Fade {
    from: Rc<Image>,
    start: f64,
    ms: f64,
}

#[derive(Clone, Default)]
struct Window {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    pad: (i32, i32),
    /// the frame's margins: left, top, right, bottom
    margin: [i32; 4],
}

#[derive(Clone, Default)]
struct Spot {
    rect: [i32; 4],
    off: bool,
}

/// What a window's hot spots do.
#[derive(Clone, Default)]
struct Menu {
    /// 0x45: labels called as the pointer comes and goes
    on: Option<Vec<u8>>,
    off: Option<Vec<u8>>,
    /// where a click goes: one label (0x45) or one per choice (0x43)
    sel: Vec<Vec<u8>>,
    var: u16,
    choice: bool,
}

/// A run of the script for a hot spot's #on / #off, and where the wait goes on after it.
#[derive(Clone)]
struct Callback {
    resume: Frame,
    frames: usize,
    stack: usize,
    window: u16,
}

#[derive(Clone, Default, PartialEq)]
struct Scene {
    bg: u16,
    event: u16,
    /// pose by character (1..=17)
    chars: [u16; 18],
}

/// Where a save starts again: the state at the start of a text.
#[derive(Clone)]
struct Snapshot {
    at: Frame,
    frames: Vec<Frame>,
    vars: Vec<(u16, Val)>,
    flags: Vec<u16>,
    modes: [u16; 6],
    scene: Scene,
    bgm: Option<(u16, bool)>,
    se: BTreeMap<u16, (Vec<u8>, u16)>,
    name: Option<String>,
    windows: Vec<u16>,
}

pub struct Engine {
    /// the messages read so far (the wheel turned up)
    backlog: vncommon::backlog::Backlog,
    files: Box<dyn Files>,
    listing: Option<Vec<String>>,
    pub width: u32,
    pub height: u32,
    pub log: bool,
    font: Option<Rc<fontdue::Font>>,
    packs: HashMap<String, Option<Rc<Pack>>>,
    scripts: HashMap<String, Option<Rc<Script>>>,
    // the machine
    cur: Frame,
    frames: Vec<Frame>,
    stack: Vec<Val>,
    vars: HashMap<u16, Val>,
    flags: Vec<u16>,
    modes: [u16; 6],
    goto: Option<Vec<u8>>,
    callback: Option<Callback>,
    // the screen
    surfaces: HashMap<i32, Image>,
    /// the pictures drawn for surfaces, by the id of the surface they were made from
    views: HashMap<i32, (u32, Rc<Image>)>,
    msg_back: Option<Rc<Image>>,
    screen: Rc<Image>,
    fade: Option<Fade>,
    scene: Scene,
    images: HashMap<(String, usize), Option<Rc<Image>>>,
    palette: [[u8; 3]; 16],
    windows: HashMap<u16, Window>,
    /// open windows, the top last
    open: Vec<u16>,
    texts: HashMap<u16, Vec<String>>,
    spots: HashMap<u16, Vec<Spot>>,
    menus: HashMap<u16, Menu>,
    hover: Option<usize>,
    name: Option<String>,
    no_window: bool,
    clear_after_click: bool,
    text_cache: HashMap<(String, [u8; 3], i32), Rc<Image>>,
    // sounds
    bgm: Option<(u16, bool)>,
    se: BTreeMap<u16, (Vec<u8>, u16)>,
    sounds: Vec<SoundEvent>,
    // the player
    wait: Wait,
    view: View,
    now: f64,
    pointer: (i32, i32),
    skip: bool,
    ended: bool,
    saved_at: Option<Snapshot>,
    last_text: String,
    rng: u32,
}

fn solid_image(rgba: [u8; 4]) -> Rc<Image> {
    let mut i = Image::new(1, 1);
    i.fill(rgba);
    Rc::new(i)
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

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
}

/// Text split into lines of at most `width` pixels.
fn wrap(s: &str, size: i32, width: i32) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = if c.is_ascii() { size / 2 } else { size };
        // a closing mark stays on its line
        let closing = "、。」』）！？…ー".contains(c);
        if w + cw > width && !line.is_empty() && !closing {
            out.push(std::mem::take(&mut line));
            w = 0;
        }
        line.push(c);
        w += cw;
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

fn sj(b: &[u8]) -> String {
    encoding_rs::SHIFT_JIS.decode(b).0.to_string()
}

/// Copies `w` x `h` pixels (`rgb`: the colour, else the alpha) between images.
#[allow(clippy::too_many_arguments)]
fn blit(
    to: &mut Image,
    from: &Image,
    (sx, sy): (i32, i32),
    (dx, dy): (i32, i32),
    (w, h): (i32, i32),
    rgb: bool,
) {
    for y in 0..h {
        let (fy, ty) = (sy + y, dy + y);
        if fy < 0 || ty < 0 || fy >= from.h as i32 || ty >= to.h as i32 {
            continue;
        }
        for x in 0..w {
            let (fx, tx) = (sx + x, dx + x);
            if fx < 0 || tx < 0 || fx >= from.w as i32 || tx >= to.w as i32 {
                continue;
            }
            let f = ((fy as u32 * from.w + fx as u32) * 4) as usize;
            let t = ((ty as u32 * to.w + tx as u32) * 4) as usize;
            if rgb {
                to.px[t..t + 3].copy_from_slice(&from.px[f..f + 3]);
            } else {
                to.px[t + 3] = from.px[f + 3];
            }
        }
    }
}

/// Lays `w` x `h` pixels of `from` over `to`, each as opaque as the alpha of `shape` (at
/// `(mx, my)`) says; `shape` None: `from`'s own alpha.
#[allow(clippy::too_many_arguments)]
fn blend(
    to: &mut Image,
    from: &Image,
    shape: Option<(&Image, i32, i32)>,
    (sx, sy): (i32, i32),
    (dx, dy): (i32, i32),
    (w, h): (i32, i32),
) {
    for y in 0..h {
        let (fy, ty) = (sy + y, dy + y);
        if fy < 0 || ty < 0 || fy >= from.h as i32 || ty >= to.h as i32 {
            continue;
        }
        for x in 0..w {
            let (fx, tx) = (sx + x, dx + x);
            if fx < 0 || tx < 0 || fx >= from.w as i32 || tx >= to.w as i32 {
                continue;
            }
            let f = ((fy as u32 * from.w + fx as u32) * 4) as usize;
            let a = match shape {
                Some((m, mx, my)) => {
                    let (px, py) = (mx + x, my + y);
                    if px < 0 || py < 0 || px >= m.w as i32 || py >= m.h as i32 {
                        0
                    } else {
                        m.px[((py as u32 * m.w + px as u32) * 4 + 3) as usize] as u32
                    }
                }
                None => from.px[f + 3] as u32,
            };
            if a == 0 {
                continue;
            }
            let t = ((ty as u32 * to.w + tx as u32) * 4) as usize;
            for c in 0..3 {
                let (s, d) = (from.px[f + c] as u32, to.px[t + c] as u32);
                to.px[t + c] = ((s * a + d * (255 - a)) / 255) as u8;
            }
            to.px[t + 3] = 255;
        }
    }
}

impl Engine {
    pub fn open(files: Box<dyn Files>) -> Result<Engine, String> {
        let mut e = Engine {
            backlog: Default::default(),
            files,
            listing: None,
            width: W,
            height: H,
            log: false,
            font: None,
            packs: HashMap::new(),
            scripts: HashMap::new(),
            cur: Frame {
                script: "SYSTEM".into(),
                pc: 0,
            },
            frames: Vec::new(),
            stack: Vec::new(),
            vars: HashMap::new(),
            flags: vec![0; 0x1000],
            modes: [0; 6],
            goto: None,
            callback: None,
            surfaces: HashMap::new(),
            views: HashMap::new(),
            msg_back: None,
            screen: Rc::new(Image::new(W, H)),
            fade: None,
            scene: Scene::default(),
            images: HashMap::new(),
            palette: [[0; 3]; 16],
            windows: HashMap::new(),
            open: Vec::new(),
            texts: HashMap::new(),
            spots: HashMap::new(),
            menus: HashMap::new(),
            hover: None,
            name: None,
            no_window: false,
            clear_after_click: false,
            text_cache: HashMap::new(),
            bgm: None,
            se: BTreeMap::new(),
            sounds: Vec::new(),
            wait: Wait::None,
            view: View::Game,
            now: 0.0,
            pointer: (0, 0),
            skip: false,
            ended: false,
            saved_at: None,
            last_text: String::new(),
            rng: 0x2545_f491,
        };
        if e.script("SYSTEM").is_none() {
            return Err("no SYSTEM.SCD in DATA/SCRIPT.PAC".into());
        }
        let mut back = Image::new(W, H);
        back.fill([0, 0, 0, 255]);
        e.surfaces.insert(2, back);
        // the window parts, as the game loads them itself
        if let Some(mut parts) = e.picture("SYS", 2).map(|i| (*i).clone())
            && let Some(shape) = e.picture("SYS", 3)
        {
            gr2::apply_mask(&mut parts, &shape);
            e.surfaces.insert(3, parts);
        }
        e.start();
        Ok(e)
    }

    fn start(&mut self) {
        self.cur = Frame {
            script: "SYSTEM".into(),
            pc: 0,
        };
        self.goto = Some(b"#start".to_vec());
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        self.font = Some(Rc::new(
            fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
                .map_err(|e| e.to_string())?,
        ));
        self.text_cache.clear();
        Ok(())
    }

    fn trace(&self, s: &str) {
        if self.log {
            eprintln!("crossover: {s}");
        }
    }

    // ---- files ----

    /// The path of DATA/<name> in the game folder, whatever its case.
    fn data_file(&mut self, name: &str) -> Option<String> {
        if self.listing.is_none() {
            self.listing = Some(self.files.list());
        }
        let list = self.listing.clone().unwrap_or_default();
        let want = format!("DATA/{}", name.to_ascii_uppercase());
        if let Some(f) = list
            .iter()
            .find(|f| f.replace('\\', "/").to_ascii_uppercase() == want)
        {
            return Some(f.clone());
        }
        // only the top of the folder listed (the native runner)
        let dir = list
            .iter()
            .find(|f| f.eq_ignore_ascii_case("DATA"))?
            .clone();
        [
            name.to_ascii_uppercase(),
            name.to_ascii_lowercase(),
            name.to_string(),
        ]
        .into_iter()
        .map(|n| format!("{dir}/{n}"))
        .find(|p| self.files.size(p).is_some())
    }

    /// DATA/<name>: SCRIPT.PAC or <name>.GR2 / .VIC.
    fn pack(&mut self, file: &str) -> Option<Rc<Pack>> {
        let key = file.to_ascii_uppercase();
        if let Some(p) = self.packs.get(&key) {
            return p.clone();
        }
        let p = self
            .data_file(&key)
            .and_then(|p| Pack::open(self.files.as_mut(), &p).map(Rc::new));
        if p.is_none() {
            self.trace(&format!("no DATA/{key}"));
        }
        self.packs.insert(key, p.clone());
        p
    }

    fn script(&mut self, name: &str) -> Option<Rc<Script>> {
        let key = name.to_ascii_uppercase();
        if let Some(s) = self.scripts.get(&key) {
            return s.clone();
        }
        // a loose DATA/<name>.SCD comes first, as in the game
        let file = format!("{key}.SCD");
        let raw = match self.data_file(&file) {
            Some(p) => self.files.read(&p),
            None => self
                .pack("SCRIPT.PAC")
                .and_then(|p| p.read(self.files.as_mut(), &file)),
        };
        let s = raw
            .map(|b| {
                if pola::is_pola(&b) {
                    pola::unpack(&b).unwrap_or_default()
                } else if b.starts_with(b"LL5\0") {
                    gr2::ll5(&b)
                } else {
                    b
                }
            })
            .and_then(|b| match Script::parse(&key, b) {
                Ok(s) => Some(Rc::new(s)),
                Err(e) => {
                    self.trace(&e);
                    None
                }
            });
        if s.is_none() {
            self.trace(&format!("no script {key}"));
        }
        self.scripts.insert(key, s.clone());
        s
    }

    /// Picture n (from 1) of DATA/<arc>.GR2.
    fn picture(&mut self, arc: &str, n: usize) -> Option<Rc<Image>> {
        let key = (arc.to_ascii_uppercase(), n);
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let img = self
            .pack(&format!("{arc}.GR2"))
            .and_then(|p| p.read_nth(self.files.as_mut(), n))
            .and_then(|b| gr2::decode(&b))
            .map(Rc::new);
        if img.is_none() {
            self.trace(&format!("no picture {n} of {arc}.GR2"));
        }
        if self.images.len() > 48 {
            self.images.clear();
        }
        self.images.insert(key, img.clone());
        img
    }

    fn sound(&mut self, arc: &str, n: Option<usize>, name: Option<&str>) -> Option<Vec<u8>> {
        let p = self.pack(&format!("{arc}.VIC"))?;
        let b = match (n, name) {
            (Some(n), _) => p.read_nth(self.files.as_mut(), n),
            (None, Some(name)) => p.read(self.files.as_mut(), &format!("{name}.WAV")),
            _ => None,
        }?;
        // RIFF WAVE of the format 0x674f: an Ogg Vorbis stream after the header
        match b[..b.len().min(512)].windows(4).position(|w| w == b"OggS") {
            Some(at) => Some(b[at..].to_vec()),
            None => Some(b),
        }
    }

    // ---- values ----

    fn var(&self, i: u16) -> Val {
        self.vars.get(&i).cloned().unwrap_or(Val::Int(0))
    }

    fn set_var(&mut self, i: u16, v: Val) {
        self.vars.insert(i, v);
    }

    fn flag(&self, i: u16) -> bool {
        let i = i as usize;
        self.flags
            .get(i / 16)
            .is_some_and(|w| w >> (i % 16) & 1 != 0)
    }

    fn set_flag(&mut self, i: u16, k: u16) {
        let i = i as usize;
        if let Some(w) = self.flags.get_mut(i / 16) {
            let bit = 1 << (i % 16);
            match k {
                0 => *w |= bit,
                1 => *w &= !bit,
                2 => *w ^= bit,
                _ => {}
            }
        }
    }

    fn pop(&mut self) -> Val {
        self.stack.pop().unwrap_or(Val::Int(0))
    }

    fn pop_int(&mut self) -> u16 {
        self.pop().int()
    }

    fn pop_str(&mut self) -> Vec<u8> {
        self.pop().bytes()
    }

    fn push(&mut self, v: u16) {
        if self.stack.len() < 1024 {
            self.stack.push(Val::Int(v));
        }
    }

    /// `n` numbers, in the order they were pushed.
    fn ints(&mut self, n: usize) -> Vec<i32> {
        let mut v: Vec<i32> = (0..n).map(|_| self.pop_int() as i32).collect();
        v.reverse();
        v
    }

    fn random(&mut self, n: u16) -> u16 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        if n == 0 {
            0
        } else {
            ((self.rng >> 1) % n as u32) as u16
        }
    }

    // ---- running ----

    pub fn frame(&mut self) -> Result<(), String> {
        self.log_page();
        self.now += FRAME_MS;
        if let Some(f) = &self.fade
            && (self.now - f.start >= f.ms || self.skip)
        {
            self.fade = None;
        }
        match self.wait {
            Wait::Fade if self.fade.is_none() => self.wait = Wait::None,
            Wait::Until(t) if self.now >= t || self.skip => self.wait = Wait::None,
            Wait::Click if self.skip && self.view == View::Game => self.advance(),
            Wait::Menu(w) if self.view == View::Game && self.callback.is_none() => {
                self.hover_spots(w)
            }
            _ => {}
        }
        if self.view == View::Game {
            self.run();
        }
        Ok(())
    }

    fn run(&mut self) {
        let mut budget = 200_000;
        while self.wait == Wait::None && !self.ended && budget > 0 {
            budget -= 1;
            if let Some(l) = self.goto.take() {
                self.jump_label(&l);
                continue;
            }
            let Some(s) = self.script(&self.cur.script.clone()) else {
                self.back_to_title();
                return;
            };
            if self.cur.pc >= s.code.len() {
                self.trace(&format!("ran off the end of {}", s.name));
                self.back_to_title();
                return;
            }
            self.step(&s);
        }
    }

    fn jump_label(&mut self, l: &[u8]) {
        let name = String::from_utf8_lossy(l).to_string();
        match self.script(&self.cur.script.clone()) {
            Some(s) => match s.label(&name) {
                Some(pc) => self.cur.pc = pc,
                None => {
                    self.trace(&format!("no label {name} in {}", s.name));
                    // as a call that returns at once
                    if !self.ret_frame() {
                        self.back_to_title();
                    }
                }
            },
            None => self.back_to_title(),
        }
    }

    fn step(&mut self, s: &Script) {
        let b = &s.code;
        let mut p = self.cur.pc;
        let op = b[p];
        p += 1;
        match op {
            0 => {
                let (lines, q) = scd::text(b, p);
                if self.modes[1] == 1 {
                    self.snapshot(self.cur.clone());
                }
                self.cur.pc = q;
                self.show_text(lines);
            }
            1 | 2 => {
                let Some((o, q)) = scd::operand(b, p) else {
                    self.trace(&format!("bad operand at {} {p}", s.name));
                    self.back_to_title();
                    return;
                };
                self.cur.pc = q;
                let v = match o {
                    Operand::Num(n) | Operand::Index(n) => Val::Int(n),
                    Operand::Var(i) => self.var(i),
                    Operand::VarVar(i) => {
                        let j = self.var(i).int();
                        self.var(j)
                    }
                    Operand::IndexVar(i) => Val::Int(self.var(i).int()),
                    Operand::Str(t) => Val::Str(t),
                };
                if op == 1 {
                    let x = self.pop();
                    self.set_var(v.int(), x);
                } else if self.stack.len() < 1024 {
                    self.stack.push(v);
                }
            }
            3 => {
                self.pop();
                self.cur.pc = p;
            }
            4 => {
                let (Some(&k), Some(t)) = (
                    b.get(p),
                    b.get(p + 1..p + 5)
                        .map(|t| u32::from_le_bytes(t.try_into().unwrap()) as usize),
                ) else {
                    self.back_to_title();
                    return;
                };
                self.cur.pc = p + 5;
                let c = if k != 0 { self.pop_int() } else { 0 };
                let jump = match k {
                    0 => true,
                    1 => c != 0,
                    2 => {
                        if c != 0 {
                            self.push(1);
                        }
                        c != 0
                    }
                    3 => c == 0,
                    4 => {
                        if c == 0 {
                            self.push(0);
                        }
                        c == 0
                    }
                    _ => false,
                };
                if jump {
                    self.cur.pc = s.start + t;
                }
            }
            5 => {
                let k = b.get(p).copied().unwrap_or(0);
                self.cur.pc = p + 1;
                let (y, x) = (self.pop_int(), self.pop_int());
                let v = match k {
                    0 => x == y,
                    1 => x != y,
                    2 => x < y,
                    3 => x <= y,
                    4 => x > y,
                    5 => x >= y,
                    _ => false,
                };
                self.push(v as u16);
            }
            6 => {
                let k = b.get(p).copied().unwrap_or(0);
                self.cur.pc = p + 1;
                let y = self.pop_int();
                let x = if k != 7 { self.pop_int() } else { 0 };
                let v = match k {
                    0 => x.wrapping_add(y),
                    1 => x.wrapping_sub(y),
                    2 => x.wrapping_mul(y),
                    3 => x.checked_div(y).unwrap_or(0),
                    4 => x.checked_rem(y).unwrap_or(0),
                    5 => x & y,
                    6 => x | y,
                    _ => y.wrapping_neg(),
                };
                self.push(v);
            }
            _ => {
                self.cur.pc = p;
                if self.log {
                    let top: Vec<String> = self
                        .stack
                        .iter()
                        .rev()
                        .take(6)
                        .map(|v| match v {
                            Val::Int(n) => n.to_string(),
                            Val::Str(t) => format!("{:?}", sj(t)),
                        })
                        .collect();
                    self.trace(&format!("{} {} {op:#04x} {top:?}", s.name, p - 1 - s.start));
                }
                self.command(op);
            }
        }
    }

    fn call(&mut self, script: Option<String>, label: Vec<u8>) {
        if self.frames.len() >= MAX_FRAMES {
            self.trace("calls nested too deep");
            self.push(0);
            return;
        }
        self.frames.push(self.cur.clone());
        if let Some(s) = script {
            self.cur = Frame { script: s, pc: 0 };
        }
        self.goto = Some(label);
    }

    /// Back to the caller; false when there is none.
    fn ret_frame(&mut self) -> bool {
        match self.frames.pop() {
            Some(f) => {
                self.cur = f;
                self.push(0);
                true
            }
            None => false,
        }
    }

    /// The values of a call: n, then n values into 6000.. (the last pushed first).
    fn call_args(&mut self) {
        let n = self.pop_int();
        for k in 0..n.min(64) {
            let v = self.pop();
            self.set_var(6000 + k, v);
        }
    }

    fn command(&mut self, op: u8) {
        match op {
            0x14 => {
                let k = self.pop_int();
                let f = self.pop_int();
                self.set_flag(f, k);
                self.push(0);
            }
            0x15 => {
                let k = self.pop_int();
                let n = self.pop_int();
                let mut r = k <= 1;
                for _ in 0..n {
                    let i = self.pop_int();
                    let f = self.flag(i);
                    match k {
                        0 if !f => r = false,
                        1 if f => r = false,
                        2 if f => r = true,
                        3 if !f => r = true,
                        _ => {}
                    }
                }
                self.push(r as u16);
            }
            0x17 => {
                let n = self.pop_int();
                let v = self.random(n);
                self.push(v);
            }
            0x18 => {
                let t = self.pop_int();
                self.push(0);
                if !self.skip {
                    self.wait = Wait::Until(self.now + t as f64 * UNIT_MS);
                }
            }
            0x1a => {
                let l = self.pop_str();
                self.goto = Some(l);
            }
            0x1b => {
                let i = self.pop_int();
                let n = self.pop_int().max(1);
                let pick = i % n;
                let mut to = None;
                for k in 0..n {
                    let l = self.pop_str();
                    if k == pick {
                        to = Some(l);
                    }
                }
                self.goto = to;
            }
            0x1c => {
                let s = sj(&self.pop_str());
                let l = self.pop_str();
                if self.script(&s).is_none() {
                    self.trace(&format!("the trial ends: no script {s}"));
                    self.back_to_title();
                    return;
                }
                self.cur = Frame { script: s, pc: 0 };
                self.goto = Some(l);
            }
            0x1d => {
                let l = self.pop_str();
                self.call_args();
                self.call(None, l);
            }
            0x1e => {
                let s = sj(&self.pop_str());
                let l = self.pop_str();
                self.call_args();
                if self.script(&s).is_none() {
                    self.push(0);
                    return;
                }
                self.call(Some(s), l);
            }
            0x1f => {
                let l = self.pop_str();
                self.call_args();
                self.call(Some("SYSTEM".into()), l);
            }
            0x22 => {
                let v = self.pop();
                match self.frames.pop() {
                    Some(f) => self.cur = f,
                    None => {
                        self.trace("returned from the first script");
                        self.back_to_title();
                        return;
                    }
                }
                self.stack.push(v);
            }
            0x25 => {
                let k = self.pop_int() as usize;
                let v = self.pop_int();
                if k < 6 {
                    self.modes[k] = v;
                }
                self.push(0);
            }
            0x26 => {
                let k = self.pop_int() as usize;
                let v = self.modes.get(k).copied().unwrap_or(0);
                self.push(v);
            }
            0x28 => {
                let a = self.ints(2);
                self.music(a[0], a[1] as u16);
                self.push(0);
            }
            0x2a => {
                let ch = self.pop_int();
                let arc = self.pop_str();
                let a = self.ints(2);
                self.sound_effect(a[0], a[1] as u16, &arc, ch);
                self.push(0);
            }
            0x2c => {
                let _kind = self.pop_int();
                let vic = sj(&self.pop_str());
                let wav = sj(&self.pop_str());
                if !self.skip {
                    let d = self.sound(&vic, None, Some(&wav));
                    if d.is_none() {
                        self.trace(&format!("no voice {wav} in {vic}.VIC"));
                    }
                    self.push_sound("voice", d, 1.0, false, 0.0);
                }
                self.push(0);
            }
            0x32 => {
                self.no_window = self.pop_int() != 0;
                self.push(0);
            }
            0x37 => {
                let a = self.ints(4);
                if let Some(c) = self.palette.get_mut(a[3] as usize) {
                    *c = [a[0] as u8, a[1] as u8, a[2] as u8];
                }
                self.push(0);
            }
            0x38 => {
                let a = self.ints(10);
                let margin = match a[0] {
                    8 => [8; 4],
                    1 => [56, 26, 0, 0],
                    _ => [0; 4],
                };
                self.windows.insert(
                    a[9] as u16,
                    Window {
                        x: a[6],
                        y: a[5],
                        w: a[4],
                        h: a[3],
                        pad: (a[2], a[1]),
                        margin,
                    },
                );
                self.push(0);
            }
            0x3b => {
                let id = self.pop_int();
                self.pop();
                self.open_window(id);
                self.push(0);
            }
            0x3c => {
                let id = self.pop_int();
                self.pop();
                self.close_window(id);
                self.push(0);
            }
            0x3d => {
                let id = self.pop_int();
                self.clear_text(id);
                self.push(0);
            }
            0x43 => {
                let id = self.pop_int();
                self.pop();
                let var = self.pop_int();
                let n = self.pop_int();
                let sel: Vec<Vec<u8>> = (0..n.min(32)).map(|_| self.pop_str()).collect();
                self.menus.insert(
                    id,
                    Menu {
                        on: None,
                        off: None,
                        sel,
                        var,
                        choice: true,
                    },
                );
                self.push(0);
            }
            0x44 => {
                let a = self.ints(9);
                self.spots.entry(a[8] as u16).or_default().push(Spot {
                    rect: [a[7], a[6], a[5], a[0]],
                    off: false,
                });
                self.push(0);
            }
            0x45 => {
                let id = self.pop_int();
                self.pop();
                let var = self.pop_int();
                let on = self.pop_str();
                let off = self.pop_str();
                let sel = self.pop_str();
                self.menus.insert(
                    id,
                    Menu {
                        on: Some(on),
                        off: Some(off),
                        sel: vec![sel],
                        var,
                        choice: false,
                    },
                );
                self.push(0);
            }
            0x46 => {
                let id = self.pop_int();
                let i = self.pop_int() as usize;
                let on = self.pop_int();
                if let Some(s) = self.spots.get_mut(&id).and_then(|v| v.get_mut(i)) {
                    s.off = on == 0;
                }
                self.push(0);
            }
            0x47 => {
                self.ints(3);
                let w = self.open.last().copied().unwrap_or(0);
                self.hover = None;
                self.wait = Wait::Menu(w);
            }
            0x48 => {
                let k = self.pop_int();
                self.push(0);
                self.wait = Wait::Click;
                // k 2: the text goes when the click comes
                self.clear_after_click = k == 2;
            }
            0x49 => {
                self.ints(2);
                self.push(1);
            }
            0x4d => {
                let k = self.pop_int();
                self.push(0);
                match k {
                    0 => match self.callback.take() {
                        Some(cb) => {
                            self.cur = cb.resume;
                            self.frames.truncate(cb.frames);
                            self.stack.truncate(cb.stack);
                            self.wait = Wait::Menu(cb.window);
                        }
                        None => {
                            self.trace("the end");
                            self.back_to_title();
                        }
                    },
                    255 => self.ended = true,
                    _ => {}
                }
            }
            0x50 => {
                let a = self.ints(3);
                self.present(a[1], a[2]);
                self.push(0);
            }
            0x53 | 0x56 => {
                let surf = self.pop_int();
                let arc = sj(&self.pop_str());
                let n = self.pop_int() as usize;
                self.load_surface(surf, &arc, n, op == 0x56);
                self.push(0);
            }
            0x57 => {
                let a = self.ints(7);
                let c = self.palette.get(a[1] as usize).copied().unwrap_or([0; 3]);
                let key = self.surface_key(a[6] as u16);
                if let Some(s) = self.surfaces.get_mut(&key) {
                    for y in a[4].max(0)..(a[4] + a[2]).min(s.h as i32) {
                        for x in a[5].max(0)..(a[5] + a[3]).min(s.w as i32) {
                            let o = ((y as u32 * s.w + x as u32) * 4) as usize;
                            s.px[o..o + 3].copy_from_slice(&c);
                        }
                    }
                    s.renew();
                }
                self.push(0);
            }
            0x58 | 0x5f => {
                let a = self.ints(8);
                let (from, to) = (self.surface_key(a[2] as u16), self.surface_key(a[7] as u16));
                self.copy_pixels(
                    from,
                    to,
                    (a[1], a[0]),
                    (a[6], a[5]),
                    (a[4], a[3]),
                    op == 0x58,
                );
                self.push(0);
            }
            0x5b => {
                let a = self.ints(11);
                let (shape, from, to) = (
                    self.surface_key(a[2] as u16),
                    self.surface_key(a[5] as u16),
                    self.surface_key(a[10] as u16),
                );
                if let (Some(src), Some(m)) = (
                    self.surfaces.get(&from).cloned(),
                    self.surfaces.get(&shape).cloned(),
                ) && let Some(dst) = self.surfaces.get_mut(&to)
                {
                    blend(
                        dst,
                        &src,
                        Some((&m, a[1], a[0])),
                        (a[4], a[3]),
                        (a[9], a[8]),
                        (a[7], a[6]),
                    );
                    dst.renew();
                }
                self.push(0);
            }
            0x98 => {
                let n = sj(&self.pop_str());
                if n != "*" {
                    self.name = Some(self.names(&n));
                }
                self.push(0);
            }
            0x99 => {
                let a = self.ints(3);
                self.scene.bg = a[2] as u16;
                self.scene.event = 0;
                self.scene.chars = [0; 18];
                self.compose_scene(a[0], a[1]);
                self.push(0);
            }
            0x9a => {
                let a = self.ints(4);
                let who = a[3] as usize;
                if (1..18).contains(&who) {
                    self.scene.chars[who] = a[2] as u16;
                }
                self.compose_scene(a[0], a[1]);
                self.push(0);
            }
            0x9b | 0x9c => {
                // 9C: t, k, thumbnail, picture
                let a = self.ints(if op == 0x9c { 4 } else { 3 });
                self.scene = Scene {
                    event: a[a.len() - 1] as u16,
                    ..Scene::default()
                };
                self.compose_scene(a[0], a[1]);
                self.push(0);
            }
            0x9d => {
                let k = self.pop_int();
                self.push(0);
                if k == 2 {
                    self.wait = Wait::Load;
                    self.view = View::Slots { save: false };
                }
            }
            0xa2 => {
                let a = self.ints(2);
                self.scene.chars = [0; 18];
                self.compose_scene(a[0], a[1]);
                self.push(0);
            }
            _ => {
                // the rest only touch the game's own screens: their arguments stay as they are
                // dropped by the statement's end along with the value pushed here
                self.drop_args(op);
                self.push(0);
            }
        }
    }

    /// Drops the arguments of a command this player leaves alone.
    fn drop_args(&mut self, op: u8) {
        let n = match op {
            0x2b | 0x51 => 3,
            0x3f | 0x40 | 0x9f => 2,
            0x42 => 6,
            0x4a..=0x4c => 1,
            _ => {
                self.trace(&format!("command {op:#04x} not known"));
                0
            }
        };
        for _ in 0..n {
            self.pop();
        }
    }
}

impl Engine {
    // ---- text ----

    /// The hero's names in place of the marks the scripts use.
    fn names(&self, s: &str) -> String {
        s.replace("＠＠＠＠＠", HERO_NAME)
            .replace("＊＊＊＊＊", HERO_NAME)
            .replace("＃＃＃＃＃", HERO_FAMILY)
    }

    fn show_text(&mut self, lines: Vec<Vec<u8>>) {
        if self.open.is_empty() {
            self.open_window(1);
        }
        let id = self.open.last().copied().unwrap_or(1);
        let lines: Vec<String> = lines.iter().map(|l| self.names(&sj(l))).collect();
        if id == 1 {
            self.last_text = lines.concat();
        }
        self.texts.entry(id).or_default().extend(lines);
    }

    fn clear_text(&mut self, id: u16) {
        self.texts.remove(&id);
        if id == 1 {
            self.name = None;
        }
    }

    fn open_window(&mut self, id: u16) {
        self.open.retain(|&w| w != id);
        self.open.push(id);
        let key = 4 + id as i32;
        if let Some(w) = self.windows.get(&id)
            && !self.surfaces.contains_key(&key)
            && id != 1
        {
            let m = w.margin;
            let img = Image::new(
                (w.w + m[0] + m[2]).max(1) as u32,
                (w.h + m[1] + m[3]).max(1) as u32,
            );
            self.surfaces.insert(key, img);
        }
    }

    fn close_window(&mut self, id: u16) {
        self.open.retain(|&w| w != id);
        self.texts.remove(&id);
        if id == 1 {
            self.name = None;
        }
    }

    // ---- surfaces ----

    fn surface_key(&self, n: u16) -> i32 {
        match n {
            10000..=10003 => n as i32 - 10000,
            10004 => 4 + self.open.last().copied().unwrap_or(0) as i32,
            _ => 0x22 + n as i32,
        }
    }

    fn load_surface(&mut self, n: u16, arc: &str, pic: usize, shape: bool) {
        let key = self.surface_key(n);
        let Some(img) = self.picture(arc, pic) else {
            return;
        };
        let s = self
            .surfaces
            .entry(key)
            .or_insert_with(|| Image::new(img.w, img.h));
        if shape {
            for y in 0..s.h.min(img.h) {
                for x in 0..s.w.min(img.w) {
                    let (o, i) = (((y * s.w + x) * 4) as usize, ((y * img.w + x) * 4) as usize);
                    s.px[o + 3] = img.px[i];
                }
            }
        } else if key == 2 || (s.w, s.h) == (img.w, img.h) {
            // the work screen keeps its size; a picture keeps the shape loaded before it
            let (w, h) = (img.w as i32, img.h as i32);
            blit(s, &img, (0, 0), (0, 0), (w, h), true);
            if key == 2 {
                s.px.chunks_exact_mut(4).for_each(|p| p[3] = 255);
            }
        } else {
            let mut i = (*img).clone();
            i.renew();
            *s = i;
        }
        s.renew();
    }

    #[allow(clippy::too_many_arguments)]
    fn copy_pixels(
        &mut self,
        from: i32,
        to: i32,
        s: (i32, i32),
        d: (i32, i32),
        size: (i32, i32),
        rgb: bool,
    ) {
        let Some(src) = self.surfaces.get(&from).cloned() else {
            return;
        };
        if let Some(dst) = self.surfaces.get_mut(&to) {
            blit(dst, &src, s, d, size, rgb);
            dst.renew();
        }
    }

    /// A character's picture with its shape as alpha (pose from 1).
    fn sprite(&mut self, who: usize, pose: u16) -> Option<Rc<Image>> {
        let arc = if who % 4 == 1 { "CHR" } else { "CHR2" };
        let key = (format!("{arc}+"), pose as usize);
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let n = pose as usize * 2;
        let img = match (self.picture(arc, n + 1), self.picture(arc, n + 2)) {
            (Some(i), Some(m)) => {
                let mut i = (*i).clone();
                gr2::apply_mask(&mut i, &m);
                Some(Rc::new(i))
            }
            (i, _) => i,
        };
        self.images.insert(key, img.clone());
        img
    }

    /// Draws the background (or event picture) and the characters on the work screen, then
    /// shows it with effect `k` (k % 100 == 10: not yet).
    fn compose_scene(&mut self, t: i32, k: i32) {
        let mut back = Image::new(W, H);
        back.fill([0, 0, 0, 255]);
        let solid = |n: u16| match n {
            10001 => Some([255, 255, 255]),
            10002 => Some([255, 0, 0]),
            10003 => Some([255, 255, 0]),
            10000 => Some([0, 0, 0]),
            _ => None,
        };
        let (arc, n) = if self.scene.event != 0 {
            ("EVENT", self.scene.event)
        } else {
            ("BG", self.scene.bg)
        };
        if let Some(c) = solid(n) {
            back.fill([c[0], c[1], c[2], 255]);
        } else if n != 0
            && let Some(img) = self.picture(arc, n as usize)
        {
            blit(&mut back, &img, (0, 0), (0, 0), (W as i32, H as i32), true);
        }
        let list: Vec<usize> = (1..18).filter(|&i| self.scene.chars[i] != 0).collect();
        let wide = list.contains(&9);
        let placed: Vec<(usize, i32)> = match list.len() {
            0 => vec![],
            1 => vec![(list[0], 0)],
            2 if wide => vec![(list[1], 0), (list[0], 0)],
            2 => vec![(list[1], -180), (list[0], 180)],
            _ if wide => vec![(list[1], -128), (list[2], 128), (list[0], 0)],
            _ => vec![(list[1], -160), (list[2], 160), (list[0], 0)],
        };
        for (who, dx) in placed {
            let pose = self.scene.chars[who];
            if let Some(img) = self.sprite(who, pose) {
                let (w, h) = (img.w as i32, img.h as i32);
                blend(&mut back, &img, None, (0, 0), (dx, 0), (w, h));
            }
        }
        back.renew();
        self.surfaces.insert(2, back);
        // the variables the game's load (SYSTEM.SCD #load) reads
        self.set_var(10, Val::Int(self.scene.bg));
        self.set_var(17, Val::Int(self.scene.event));
        for (k, &who) in list.iter().take(3).enumerate() {
            self.set_var(11 + k as u16, Val::Int(who as u16));
            self.set_var(14 + k as u16, Val::Int(self.scene.chars[who]));
        }
        for k in list.len().min(3)..3 {
            self.set_var(11 + k as u16, Val::Int(0));
            self.set_var(14 + k as u16, Val::Int(0));
        }
        if k % 100 != 10 {
            self.present(t, k % 100);
        }
    }

    /// Shows the work screen, fading over `t` hundredths of a second unless `k` is 0.
    fn present(&mut self, t: i32, k: i32) {
        let Some(back) = self.surfaces.get(&2) else {
            return;
        };
        let mut next = back.clone();
        next.renew();
        let next = Rc::new(next);
        if k != 0 && t > 0 && !self.skip {
            self.fade = Some(Fade {
                from: self.screen.clone(),
                start: self.now,
                ms: t as f64 * UNIT_MS,
            });
            if self.wait == Wait::None {
                self.wait = Wait::Fade;
            }
        }
        self.screen = next;
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

    fn music(&mut self, k: i32, n: u16) {
        match k {
            5 => {
                self.bgm = None;
                self.push_sound("bgm", None, 0.0, false, 0.0);
            }
            6 => {
                self.bgm = None;
                self.push_sound("bgm", None, 0.0, false, 1000.0);
            }
            0..=4 => {
                let (n, forever) = match (k, self.bgm) {
                    (4, Some(b)) => b,
                    (4, None) => return,
                    _ => (n, k == 1 || k == 3),
                };
                if n == 0 || self.bgm == Some((n, forever)) {
                    return;
                }
                let d = self.sound("BGM", Some(n as usize), None);
                if d.is_none() {
                    self.trace(&format!("no music {n}"));
                }
                self.bgm = Some((n, forever));
                self.push_sound("bgm", d, 1.0, forever, 0.0);
            }
            _ => {}
        }
    }

    fn sound_effect(&mut self, k: i32, n: u16, arc: &[u8], ch: u16) {
        let channel = format!("se{ch}");
        if arc == b"*" {
            // the looping sounds again (after a load)
            for (c, (a, n)) in self.se.clone() {
                let d = self.sound(&sj(&a), Some(n as usize), None);
                self.push_sound(&format!("se{c}"), d, 1.0, true, 0.0);
            }
            return;
        }
        if k == 9 {
            self.se.remove(&ch);
            self.push_sound(&channel, None, 0.0, false, 100.0);
            return;
        }
        let forever = k == 1;
        if forever {
            self.se.insert(ch, (arc.to_vec(), n));
        } else {
            self.se.remove(&ch);
            if self.skip {
                return;
            }
        }
        let d = self.sound(&sj(arc), Some(n as usize), None);
        if d.is_none() {
            self.trace(&format!("no sound {n} of {}", sj(arc)));
        }
        self.push_sound(&channel, d, 1.0, forever, 0.0);
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    // ---- drawing ----

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

    fn solid(&self, rgba: [u8; 4], r: [i32; 4]) -> Draw {
        let img = solid_image(rgba);
        let mut d = draw(&img, 0.0, 0.0, 1.0);
        d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        d
    }

    /// The lines of a choice window that can be picked (`!0` first) and where they are.
    fn choice_rects(&self, id: u16) -> Vec<(usize, [i32; 4])> {
        let Some(w) = self.windows.get(&id) else {
            return vec![];
        };
        let mut out = Vec::new();
        let mut k = 0;
        for (i, l) in self.texts.get(&id).into_iter().flatten().enumerate() {
            let y = w.y + w.pad.1 + i as i32 * CHOICE_STEP;
            if l.starts_with("!0") {
                out.push((k, [w.x, y, w.x + w.w, y + CHOICE_STEP]));
                k += 1;
            }
        }
        out
    }

    /// The hot spots of a window that answer the pointer, each with its number.
    fn spot_rects(&self, id: u16) -> Vec<(usize, [i32; 4])> {
        if self.menus.get(&id).is_some_and(|m| m.choice) {
            return self.choice_rects(id);
        }
        self.spots
            .get(&id)
            .into_iter()
            .flatten()
            .enumerate()
            .filter(|(_, s)| !s.off)
            .map(|(i, s)| (i, s.rect))
            .collect()
    }

    /// A surface as a picture to draw, made again only when the surface changed.
    fn view(&mut self, key: i32) -> Option<Rc<Image>> {
        let s = self.surfaces.get(&key)?;
        if let Some((id, img)) = self.views.get(&key)
            && *id == s.id
        {
            return Some(img.clone());
        }
        let img = Rc::new(s.clone());
        self.views.insert(key, (s.id, img.clone()));
        Some(img)
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        for id in self.open.clone() {
            let Some(w) = self.windows.get(&id).cloned() else {
                continue;
            };
            if id == 1 {
                if self.no_window {
                    continue;
                }
                // the message window's back: the top of the window parts, at the bottom
                if self.msg_back.is_none()
                    && let Some(parts) = self.surfaces.get(&3)
                {
                    let mut img = Image::new(W, 179);
                    blit(&mut img, parts, (0, 0), (0, 0), (W as i32, 179), true);
                    blit(&mut img, parts, (0, 0), (0, 0), (W as i32, 179), false);
                    self.msg_back = Some(Rc::new(img));
                }
                match &self.msg_back {
                    Some(img) => out.push(draw(img, 0.0, (H - 179) as f32, 1.0)),
                    None => out.push(self.solid([0, 0, 0, 140], [0, w.y - 30, W as i32, H as i32])),
                }
                self.message_draws(&w, out);
                continue;
            }
            if let Some(img) = self.view(4 + id as i32) {
                out.push(draw(
                    &img,
                    (w.x - w.margin[0]) as f32,
                    (w.y - w.margin[1]) as f32,
                    1.0,
                ));
            }
            let over = match (&self.wait, self.hover) {
                (Wait::Menu(m), Some(h)) if *m == id => Some(h),
                _ => None,
            };
            let mut k = 0;
            for (i, l) in self
                .texts
                .get(&id)
                .cloned()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let (t, item) = match l.strip_prefix("!0") {
                    Some(t) => (t.to_string(), Some(k)),
                    None => (l.clone(), None),
                };
                if item.is_some() {
                    k += 1;
                }
                let y = w.y + w.pad.1 + i as i32 * CHOICE_STEP;
                let lit = item.is_some() && item == over;
                if lit {
                    out.push(self.solid(
                        [255, 255, 255, 60],
                        [w.x + 4, y - 1, w.x + w.w - 4, y + CHOICE_STEP - 1],
                    ));
                }
                let rgb = if lit {
                    [255, 220, 120]
                } else {
                    [255, 255, 255]
                };
                let img = self.label(&t, rgb, TEXT_SIZE - 2);
                out.push(draw(&img, (w.x + w.pad.0) as f32, (y - 1) as f32, 1.0));
            }
        }
    }

    fn message_draws(&mut self, w: &Window, out: &mut Vec<Draw>) {
        let width = w.w - w.pad.0 * 2;
        let (x, y) = (w.x + w.pad.0, w.y + w.pad.1);
        if let Some(n) = self.name.clone() {
            let img = self.label(&format!("【{n}】"), [255, 230, 160], TEXT_SIZE);
            out.push(draw(&img, (x - 2) as f32, (y - LINE_STEP - 4) as f32, 1.0));
        }
        let mut lines = Vec::new();
        for l in self.texts.get(&1).cloned().unwrap_or_default() {
            lines.extend(wrap(&l, TEXT_SIZE, width));
        }
        let n = lines.len();
        for (k, l) in lines.iter().enumerate() {
            let img = self.label(l, [255, 255, 255], TEXT_SIZE);
            out.push(draw(
                &img,
                (x - 2) as f32,
                (y + k as i32 * LINE_STEP - 2) as f32,
                1.0,
            ));
        }
        if self.wait == Wait::Click && self.fade.is_none() && n > 0 {
            let a = 0.5 + 0.5 * ((self.now / 300.0).sin() as f32).abs();
            let img = self.label("▼", [255, 255, 255], TEXT_SIZE - 6);
            out.push(draw(
                &img,
                (w.x + w.w - 40) as f32,
                (y + (n.min(3) as i32 - 1) * LINE_STEP + 6) as f32,
                a,
            ));
        }
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = self.draws_inner();
        if self.backlog.is_open() {
            let log = std::mem::take(&mut self.backlog);
            let (w, h) = (self.width, self.height);
            let size = (h as i32 / 27).max(16);
            out.extend(log.draws(w, h, size, &mut |s, rgb| {
                let _ = rgb;
                self.label(s, rgb, size)
            }));
            self.backlog = log;
        }
        out
    }

    /// Keeps the message waiting for a click in the backlog (vncommon::backlog).
    fn log_page(&mut self) {
        if !(self.view == View::Game && matches!(self.wait, Wait::Click)) {
            return;
        }
        let (name, text) = (self.name.clone().unwrap_or_default(), {
            let mut k: Vec<_> = self.texts.iter().filter(|t| !t.1.is_empty()).collect();
            k.sort_by_key(|t| *t.0);
            k.iter()
                .map(|t| t.1.join("\n"))
                .collect::<Vec<_>>()
                .join("\n")
        });
        self.backlog.push(name.trim(), &text);
    }

    fn draws_inner(&mut self) -> Vec<Draw> {
        let mut out = vec![draw(&self.screen.clone(), 0.0, 0.0, 1.0)];
        if let Some(f) = &self.fade {
            let p = ((self.now - f.start) / f.ms.max(1.0)).clamp(0.0, 1.0) as f32;
            out.push(draw(&f.from, 0.0, 0.0, 1.0 - p));
        }
        self.window_draws(&mut out);
        match self.view {
            View::Menu => self.menu_draws(&mut out),
            View::Slots { save } => self.slot_draws(save, &mut out),
            View::Game => {}
        }
        out
    }

    /// The frame as one picture (for the native runner).
    pub fn render(&mut self) -> Image {
        let mut img = Image::new(self.width, self.height);
        img.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }

    // ---- menus of this player ----

    fn button(&mut self, t: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let over = inside(r, self.pointer.0, self.pointer.1);
        out.push(self.solid(
            if over {
                [60, 110, 160, 230]
            } else {
                [20, 30, 50, 210]
            },
            r,
        ));
        let img = self.label(t, [255, 255, 255], 22);
        let x = (r[0] + r[2]) as f32 / 2.0 - img.w as f32 / 2.0;
        let y = (r[1] + r[3]) as f32 / 2.0 - img.h as f32 / 2.0;
        out.push(draw(&img, x, y, 1.0));
    }

    fn menu_items(&self) -> Vec<(&'static str, [i32; 4])> {
        let names = ["セーブ", "ロード", "タイトルへ戻る", "ゲームに戻る"];
        let (bw, bh, gap) = (320, 50, 18);
        let top = (H as i32 - (bh * 4 + gap * 3)) / 2;
        names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let y = top + i as i32 * (bh + gap);
                (*n, [(W as i32 - bw) / 2, y, (W as i32 + bw) / 2, y + bh])
            })
            .collect()
    }

    fn menu_draws(&mut self, out: &mut Vec<Draw>) {
        out.push(self.solid([0, 0, 0, 150], [0, 0, W as i32, H as i32]));
        for (t, r) in self.menu_items() {
            self.button(t, r, out);
        }
    }

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        let step = 46;
        let (bw, bh) = (W as i32 - 160, step - 8);
        let top = (H as i32 - step * (SLOTS as i32 + 1)) / 2 + 16;
        (0..=SLOTS as i32)
            .map(|i| {
                let y = top + i * step;
                [80, y, 80 + bw, y + bh]
            })
            .collect()
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        out.push(self.solid([0, 0, 0, 200], [0, 0, W as i32, H as i32]));
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
        let head = self.label(if save { "セーブ" } else { "ロード" }, [255, 255, 255], 26);
        out.push(draw(&head, 80.0, (rects[0][1] - 38) as f32, 1.0));
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    /// Runs #on / #off of a window's hot spots as the pointer comes and goes.
    fn hover_spots(&mut self, id: u16) {
        let (px, py) = self.pointer;
        let now = self
            .spot_rects(id)
            .into_iter()
            .find(|(_, r)| inside(*r, px, py))
            .map(|(i, _)| i);
        if now == self.hover {
            return;
        }
        let Some(m) = self.menus.get(&id).cloned() else {
            self.hover = now;
            return;
        };
        if m.choice {
            self.hover = now;
            return;
        }
        let label = match (self.hover, now) {
            (Some(old), _) => {
                self.set_var(m.var, Val::Int(old as u16));
                self.hover = None;
                m.off
            }
            (None, Some(i)) => {
                self.set_var(m.var, Val::Int(i as u16));
                self.hover = Some(i);
                m.on
            }
            (None, None) => None,
        };
        if let Some(l) = label {
            self.callback = Some(Callback {
                resume: self.cur.clone(),
                frames: self.frames.len(),
                stack: self.stack.len(),
                window: id,
            });
            self.goto = Some(l);
            self.wait = Wait::None;
            self.run();
        }
    }

    fn advance(&mut self) {
        if self.wait == Wait::Click {
            self.wait = Wait::None;
            if self.clear_after_click {
                self.clear_text(1);
            }
        }
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if self.backlog.is_open() {
            self.backlog.close();
            return;
        }
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
        if self.callback.is_some() {
            return;
        }
        match self.wait.clone() {
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
            Wait::Menu(id) => {
                let hit = self
                    .spot_rects(id)
                    .into_iter()
                    .find(|(_, r)| inside(*r, x, y))
                    .map(|(i, _)| i);
                let (Some(i), Some(m)) = (hit, self.menus.get(&id).cloned()) else {
                    return;
                };
                let label = if m.choice {
                    m.sel.get(i).cloned()
                } else {
                    m.sel.first().cloned()
                };
                self.set_var(m.var, Val::Int(i as u16));
                self.hover = None;
                self.wait = Wait::None;
                match label {
                    Some(l) => self.goto = Some(l),
                    None => self.push(i as u16),
                }
            }
            Wait::Load | Wait::None => {}
        }
    }

    fn close_slots(&mut self) {
        if self.wait == Wait::Load {
            self.wait = Wait::None;
            self.view = View::Game;
        } else {
            self.view = View::Menu;
        }
    }

    pub fn cancel(&mut self) {
        if self.backlog.is_open() {
            self.backlog.close();
            return;
        }
        match self.view {
            View::Menu => self.view = View::Game,
            View::Slots { .. } => self.close_slots(),
            View::Game => {
                self.skip = false;
                if self.wait == Wait::Click && self.saved_at.is_some() {
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
        // turned up: the backlog
        if self.backlog.is_open() || up {
            self.log_page();
            if self.backlog.is_open()
                || (self.view == View::Game && matches!(self.wait, Wait::Click))
            {
                self.backlog.wheel(up);
                return;
            }
        }
        if !up && self.view == View::Game && self.wait == Wait::Click {
            self.advance();
        }
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    fn reset(&mut self) {
        self.frames.clear();
        self.stack.clear();
        self.callback = None;
        self.goto = None;
        self.scene = Scene::default();
        self.texts.clear();
        self.open.clear();
        self.name = None;
        self.no_window = false;
        self.clear_after_click = false;
        self.hover = None;
        self.view = View::Game;
        self.fade = None;
        self.skip = false;
        self.saved_at = None;
        self.wait = Wait::None;
        for ch in self.se.keys().copied().collect::<Vec<_>>() {
            self.push_sound(&format!("se{ch}"), None, 0.0, false, 200.0);
        }
        self.se.clear();
        self.push_sound("voice", None, 0.0, false, 0.0);
    }

    /// The title again (after the end of the trial, or from the menu).
    fn back_to_title(&mut self) {
        self.reset();
        self.music(6, 0);
        self.vars.retain(|&i, _| i >= 5000);
        self.flags[..5000 / 16].iter_mut().for_each(|f| *f = 0);
        self.modes = [0; 6];
        self.set_var(46, Val::Int(0));
        if let Some(b) = self.surfaces.get_mut(&2) {
            b.fill([0, 0, 0, 255]);
        }
        self.present(50, 2);
        self.cur = Frame {
            script: "SYSMENU".into(),
            pc: 0,
        };
        self.goto = Some(b"#st_menu".to_vec());
    }

    // ---- saves ----

    fn snapshot(&mut self, at: Frame) {
        let mut vars: Vec<(u16, Val)> = self
            .vars
            .iter()
            .filter(|(i, _)| **i < 5000)
            .map(|(i, v)| (*i, v.clone()))
            .collect();
        vars.sort_by_key(|v| v.0);
        self.saved_at = Some(Snapshot {
            at,
            frames: self.frames.clone(),
            vars,
            flags: self.flags[..5000 / 16].to_vec(),
            modes: self.modes,
            scene: self.scene.clone(),
            bgm: self.bgm,
            se: self.se.clone(),
            name: self.name.clone(),
            windows: self.open.clone(),
        });
    }

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&format!("slot{i:02}"))?;
        Slot::parse(&String::from_utf8_lossy(&b))
    }

    fn save(&mut self, i: usize) {
        let Some(s) = self.saved_at.clone() else {
            return;
        };
        let title = format!(
            "{}{}",
            self.name
                .as_ref()
                .map_or(String::new(), |n| format!("【{n}】")),
            self.last_text
        );
        let title: String = title.chars().take(30).collect();
        let hex = |b: &[u8]| b.iter().map(|c| format!("{c:02x}")).collect::<String>();
        let mut o = format!("title {title}\n");
        o += &format!("at {} {}\n", s.at.script, s.at.pc);
        for f in &s.frames {
            o += &format!("frame {} {}\n", f.script, f.pc);
        }
        let vars: Vec<String> = s
            .vars
            .iter()
            .map(|(i, v)| match v {
                Val::Int(n) => format!("{i}:{n}"),
                Val::Str(t) => format!("{i}:s{}", hex(t)),
            })
            .collect();
        o += &format!("vars {}\n", vars.join(" "));
        let flags: Vec<String> = s
            .flags
            .iter()
            .enumerate()
            .filter(|(_, f)| **f != 0)
            .map(|(i, f)| format!("{i}:{f}"))
            .collect();
        o += &format!("flags {}\n", flags.join(" "));
        let modes: Vec<String> = s.modes.iter().map(|m| m.to_string()).collect();
        o += &format!("modes {}\n", modes.join(" "));
        let chars: Vec<String> = s.scene.chars.iter().map(|c| c.to_string()).collect();
        o += &format!(
            "scene {} {} {}\n",
            s.scene.bg,
            s.scene.event,
            chars.join(" ")
        );
        if let Some((n, f)) = s.bgm {
            o += &format!("bgm {n} {}\n", f as u8);
        }
        for (c, (a, n)) in &s.se {
            o += &format!("se {c} {} {n}\n", hex(a));
        }
        let wins: Vec<String> = s.windows.iter().map(|w| w.to_string()).collect();
        o += &format!("windows {}\n", wins.join(" "));
        if let Some(n) = &s.name {
            o += &format!("name {n}\n");
        }
        self.files.write_save(&format!("slot{i:02}"), o.as_bytes());
        self.view = View::Game;
    }

    fn load_slot(&mut self, i: usize) {
        let Some(slot) = self.read_slot(i) else {
            return;
        };
        let s = slot.snap;
        self.reset();
        self.music(5, 0);
        self.vars.retain(|&i, _| i >= 5000);
        for (i, v) in s.vars {
            self.vars.insert(i, v);
        }
        for (k, f) in s.flags.iter().enumerate().take(5000 / 16) {
            self.flags[k] = *f;
        }
        self.modes = s.modes;
        self.frames = s.frames;
        self.cur = s.at;
        self.scene = s.scene;
        self.name = s.name;
        for w in s.windows {
            self.open_window(w);
        }
        if let Some((n, f)) = s.bgm {
            self.music(if f { 1 } else { 0 }, n);
        }
        for (c, (a, n)) in s.se {
            self.sound_effect(1, n, &a, c);
        }
        self.screen = solid_image([0, 0, 0, 255]);
        self.screen = Rc::new({
            let mut i = Image::new(W, H);
            i.fill([0, 0, 0, 255]);
            i
        });
        self.compose_scene(40, 2);
        self.view = View::Game;
    }
}

struct Slot {
    title: String,
    snap: Snapshot,
}

impl Slot {
    fn parse(s: &str) -> Option<Slot> {
        let unhex = |t: &str| -> Vec<u8> {
            (0..t.len() / 2)
                .filter_map(|i| u8::from_str_radix(t.get(i * 2..i * 2 + 2)?, 16).ok())
                .collect()
        };
        let mut title = String::new();
        let mut snap = Snapshot {
            at: Frame {
                script: String::new(),
                pc: 0,
            },
            frames: Vec::new(),
            vars: Vec::new(),
            flags: vec![0; 5000 / 16],
            modes: [0; 6],
            scene: Scene::default(),
            bgm: None,
            se: BTreeMap::new(),
            name: None,
            windows: Vec::new(),
        };
        for line in s.lines() {
            let (k, rest) = line.split_once(' ').unwrap_or((line, ""));
            let f: Vec<&str> = rest.split(' ').filter(|x| !x.is_empty()).collect();
            let n = |i: usize| f.get(i).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
            match k {
                "title" => title = rest.to_string(),
                "at" | "frame" if f.len() == 2 => {
                    let fr = Frame {
                        script: f[0].to_string(),
                        pc: n(1).max(0) as usize,
                    };
                    if k == "at" {
                        snap.at = fr;
                    } else {
                        snap.frames.push(fr);
                    }
                }
                "vars" => {
                    for p in &f {
                        if let Some((i, v)) = p.split_once(':')
                            && let Ok(i) = i.parse::<u16>()
                        {
                            let v = match v.strip_prefix('s') {
                                Some(h) => Val::Str(unhex(h)),
                                None => Val::Int(v.parse().unwrap_or(0)),
                            };
                            snap.vars.push((i, v));
                        }
                    }
                }
                "flags" => {
                    for p in &f {
                        if let Some((i, v)) = p.split_once(':')
                            && let (Ok(i), Ok(v)) = (i.parse::<usize>(), v.parse::<u16>())
                            && i < snap.flags.len()
                        {
                            snap.flags[i] = v;
                        }
                    }
                }
                "modes" => {
                    for (i, m) in snap.modes.iter_mut().enumerate() {
                        *m = n(i) as u16;
                    }
                }
                "scene" => {
                    snap.scene.bg = n(0) as u16;
                    snap.scene.event = n(1) as u16;
                    for (i, c) in snap.scene.chars.iter_mut().enumerate() {
                        *c = n(2 + i) as u16;
                    }
                }
                "bgm" => snap.bgm = Some((n(0) as u16, n(1) != 0)),
                "se" if f.len() == 3 => {
                    snap.se.insert(n(0) as u16, (unhex(f[1]), n(2) as u16));
                }
                "windows" => snap.windows = f.iter().filter_map(|w| w.parse().ok()).collect(),
                "name" => snap.name = Some(rest.to_string()),
                _ => {}
            }
        }
        (!snap.at.script.is_empty()).then_some(Slot { title, snap })
    }
}

#[cfg(test)]
mod tests {
    use super::wrap;

    #[test]
    fn wrapping() {
        assert_eq!(wrap("あいうえ。", 10, 30), vec!["あいう", "え。"]);
        assert_eq!(wrap("あいう。え", 10, 30), vec!["あいう。", "え"]);
    }
}
