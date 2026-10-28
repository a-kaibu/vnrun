//! Runs the scripts a frame at a time: it carries out statements until one waits (for a
//! click, a time, a fade, the title's buttons or a choice), and says what to draw and play.
//!
//! The commands, as read from the trials' scripts and the program's command table (the
//! numbers are the program's own; the rest of its 171 commands are skipped):
//!
//! | # | arguments | |
//! |---|-----------|-|
//! | 1 | n | go to statement n (a label) |
//! | 2 | n | call script n |
//! | 3 / 4 / 5 | var, a (, b) | var = a / a + b / a - b |
//! | 8 | var, op, value, depth | if (op '=' or '!'); 9 depth: else; 10 depth: end if |
//! | 13 | effect | the message window on (and the changes since 35 shown with the effect) |
//! | 35 | effect | the message window and all layers off; what is shown up to 13 comes in together |
//! | 36 | menu, -, effect | the title's buttons (the 'e' table); the answer goes to var 1 |
//! | 37 | menu, button | that button can be pressed |
//! | 39 | 0xRRGGBB | the colour under everything |
//! | 43 | voice, who | a voice ('j' sounds) |
//! | 46 / 114 / 150 | sound | a sound ('k'), once / looped / looped on another channel; -1 stops |
//! | 50 | a, b, c, d | waits for a click, then goes to statement a |
//! | 54 | | the end of the script (back to the caller) |
//! | 64 | var | the load screen |
//! | 70 | | waits for a click; the message starts again |
//! | 71 | | quit |
//! | 131 / 132 | music, - / ms | music ('u'), looped / fades out |
//! | 134, 135 n + text, 136 var | | a choice: the answer n goes to var |
//! | 167 | ms | waits |
//!
//! Layer commands (`$ layer op ...`): op 1 (kind, picture, place, effect, -) shows one of the
//! layer's pictures (counted from 1) at one of its places (kind 2: in sepia), op 0 (effect, -) takes it away,
//! op 4 (picture, from, to, ms) moves it from one place to another.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::arc::{Archives, DataStore, Entry};
use crate::data::{ButtonSet, LayerTable};
use crate::pic::{self, Raw, Scheme};
use crate::script::{self, Arg, Stmt};
use crate::sound;

const FRAME_MS: f64 = 1000.0 / 60.0;
const SLOTS: usize = 10;
const TEXT_SIZE: i32 = 26;
const LINE_STEP: i32 = 36;
const TEXT_X: i32 = 190;
const TEXT_Y: i32 = 552;
const NAME_X: i32 = 40;

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

#[derive(Clone)]
struct Layer {
    pic: i32,
    place: i32,
    sepia: bool,
    image: Rc<Image>,
    x: f32,
    y: f32,
    depth: i32,
    /// (from x, from y, to x, to y, start, ms)
    moving: Option<(f32, f32, f32, f32, f64, f64)>,
}

impl Layer {
    fn at(&self, now: f64) -> (f32, f32) {
        match self.moving {
            Some((fx, fy, tx, ty, start, ms)) => {
                let p = ((now - start) / ms.max(1.0)).clamp(0.0, 1.0) as f32;
                (fx + (tx - fx) * p, fy + (ty - fy) * p)
            }
            None => (self.x, self.y),
        }
    }
}

#[derive(Clone, PartialEq)]
enum Wait {
    None,
    Click,
    /// a click, then on to this statement
    ClickGoto(usize),
    Until(f64),
    Fade,
    Title(usize),
    Choice(i32),
    Load,
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Game,
    Menu,
    Slots { save: bool },
}

struct Fade {
    from: Vec<Draw>,
    start: f64,
    ms: f64,
}

/// (button index, answer, rect, normal picture, lit picture)
type TitleButton = (usize, i32, [i32; 4], Option<Rc<Image>>, Option<Rc<Image>>);

pub struct Engine {
    files: Box<dyn Files>,
    arcs: Archives,
    /// the newer games' pictures (Resources/Data0NN.dat), by the number the tables give
    store: DataStore,
    /// the 2023 generation (Resources/Data0NN.dat, ChakraCore beside the exe)
    newer: bool,
    /// where the title's answer goes (36: var 1, 28: its argument)
    title_var: i32,
    scheme: Scheme,
    key: Vec<u8>,
    pics: Vec<Entry>,
    voices: Vec<Entry>,
    effects: Vec<Entry>,
    music: Vec<Entry>,
    scripts: Vec<Entry>,
    loaded: HashMap<usize, Rc<Vec<Stmt>>>,
    tables: Vec<LayerTable>,
    menus: Vec<ButtonSet>,
    pub width: u32,
    pub height: u32,
    pub log: bool,
    font: Option<Rc<fontdue::Font>>,

    cur: usize,
    pc: usize,
    stack: Vec<(usize, usize)>,
    vars: HashMap<i32, i32>,
    layers: BTreeMap<usize, Layer>,
    color: [u8; 4],
    window: bool,
    name: Option<String>,
    lines: Vec<String>,
    wait: Wait,
    view: View,
    frozen: Option<Vec<Draw>>,
    /// the effect of the changes being gathered (35 ... 13)
    gathering: Option<i32>,
    fade: Option<Fade>,
    now: f64,
    choices: Vec<(String, i32)>,
    choice_next: Option<i32>,
    enabled: HashSet<(usize, usize)>,
    bgm: Option<i32>,
    loops: HashMap<u8, i32>,
    sounds: Vec<SoundEvent>,
    pic_cache: HashMap<(usize, bool), Rc<Image>>,
    raw_cache: HashMap<usize, Rc<Raw>>,
    text_cache: HashMap<(String, [u8; 3], i32), Rc<Image>>,
    pointer: (i32, i32),
    skip: bool,
    ended: bool,
    /// where the last click wait was, for saves: (script, statement after it)
    saved_at: Option<(usize, usize)>,
}

fn white(w: u32, h: u32, rgba: [u8; 4]) -> Rc<Image> {
    let mut i = Image::new(w, h);
    i.fill(rgba);
    Rc::new(i)
}

fn sepia(img: &Image) -> Image {
    let mut out = img.clone();
    for p in out.px.chunks_exact_mut(4) {
        let l = (p[0] as f32 * 0.3 + p[1] as f32 * 0.59 + p[2] as f32 * 0.11).min(255.0);
        p[0] = (l * 1.07).min(255.0) as u8;
        p[1] = (l * 0.95) as u8;
        p[2] = (l * 0.78) as u8;
    }
    out
}

/// How long an effect number lasts.
fn effect_ms(e: i32) -> f64 {
    match e {
        0 | 9999 | -1 => 0.0,
        1..=9 => 400.0,
        100..=199 => 700.0,
        _ => 500.0,
    }
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arcs = Archives::open(files.as_mut())?;
        let list = |k: u8| arcs.of(k).cloned().collect::<Vec<Entry>>();
        let (pics, voices, effects, music, scripts) =
            (list(b'b'), list(b'j'), list(b'k'), list(b'u'), list(b'a'));
        if scripts.is_empty() {
            return Err("no scripts ('a' entries)".into());
        }
        let store = DataStore::open(files.as_mut());
        let newer = !store.is_empty();
        let mut tables = Vec::new();
        for e in list(b'f') {
            let b = arcs.read(files.as_mut(), &e).unwrap_or_default();
            tables.push(LayerTable::parse(&b, newer));
        }
        let mut menus = Vec::new();
        for e in list(b'e') {
            let b = arcs.read(files.as_mut(), &e).unwrap_or_default();
            menus.push(ButtonSet::parse(&b, newer));
        }
        let mut key = Vec::new();
        for name in files.list() {
            if name.to_lowercase().ends_with(".exe")
                && let Some(b) = files.read(&name)
                && let Some(k) = sound::key_from_exe(&b)
            {
                key = k;
                break;
            }
        }
        if key.is_empty() {
            key = sound::DEFAULT_KEY.to_vec();
        }
        let scheme = Scheme::tinkerbell_2018();
        // the screen: the size of the first full-screen mask
        let (mut width, mut height) = (1280, 720);
        if let Some(e) = arcs.of(b'n').next()
            && let Some(b) = arcs.read(files.as_mut(), e)
            && let Ok((h, _)) = pic::header(&b, &scheme)
            && h[4] >= 320
            && h[3] >= 240
        {
            (width, height) = (h[4] as u32, h[3] as u32);
        }
        // the newer games: the size of the first picture (a full-screen background)
        if newer
            && let Some(b) = store.read(files.as_mut(), 10_000_000)
            && let Some((w, h)) = match b.first() {
                Some(b'c') | Some(b'b') => pic::png_of(&b)
                    .and_then(|p| Image::decode(p).ok())
                    .map(|i| (i.w as usize, i.h as usize)),
                _ => pic::decode_a2023(&b, None).ok().map(|r| (r.w, r.h)),
            }
            && w >= 320
            && h >= 240
        {
            (width, height) = (w as u32, h as u32);
        }
        let mut e = Engine {
            files,
            arcs,
            store,
            newer,
            title_var: 1,
            scheme,
            key,
            pics,
            voices,
            effects,
            music,
            scripts,
            loaded: HashMap::new(),
            tables,
            menus,
            width,
            height,
            log: false,
            font: None,
            cur: 0,
            pc: 0,
            stack: Vec::new(),
            vars: HashMap::new(),
            layers: BTreeMap::new(),
            color: [0, 0, 0, 255],
            window: false,
            name: None,
            lines: Vec::new(),
            wait: Wait::None,
            view: View::Game,
            frozen: None,
            gathering: None,
            fade: None,
            now: 0.0,
            choices: Vec::new(),
            choice_next: None,
            enabled: HashSet::new(),
            bgm: None,
            loops: HashMap::new(),
            sounds: Vec::new(),
            pic_cache: HashMap::new(),
            raw_cache: HashMap::new(),
            text_cache: HashMap::new(),
            pointer: (0, 0),
            skip: false,
            ended: false,
            saved_at: None,
        };
        e.script(0).ok_or("script 0 unreadable")?;
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

    /// Starts at a script (for the native runner).
    pub fn start_at(&mut self, n: usize) {
        self.cur = n;
        self.pc = 0;
    }

    fn trace(&self, s: &str) {
        if self.log {
            eprintln!("cyberworks: {s}");
        }
    }

    fn script(&mut self, n: usize) -> Option<Rc<Vec<Stmt>>> {
        if let Some(s) = self.loaded.get(&n) {
            return Some(s.clone());
        }
        let e = self.scripts.iter().find(|e| e.id as usize == n)?.clone();
        let b = self.arcs.read(self.files.as_mut(), &e)?;
        let s = Rc::new(script::parse(&b));
        self.loaded.insert(n, s.clone());
        Some(s)
    }

    // ---- running ----

    pub fn frame(&mut self) -> Result<(), String> {
        self.now += FRAME_MS;
        if let Some(f) = &self.fade
            && self.now - f.start >= f.ms
        {
            self.fade = None;
        }
        for l in self.layers.values_mut() {
            if let Some((_, _, tx, ty, start, ms)) = l.moving
                && self.now - start >= ms
            {
                l.moving = None;
                (l.x, l.y) = (tx, ty);
            }
        }
        match self.wait {
            Wait::Fade if self.fade.is_none() => self.wait = Wait::None,
            Wait::Until(t) if self.now >= t => self.wait = Wait::None,
            Wait::Click | Wait::ClickGoto(_) if self.skip && self.view == View::Game => {
                self.advance()
            }
            _ => {}
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
            let Some(s) = self.script(self.cur) else {
                self.trace(&format!("no script {}", self.cur));
                self.ended = true;
                return;
            };
            let Some(st) = s.get(self.pc).cloned() else {
                self.ret();
                continue;
            };
            self.pc += 1;
            self.exec(st);
        }
    }

    fn ret(&mut self) {
        match self.stack.pop() {
            Some((s, pc)) => {
                self.cur = s;
                self.pc = pc;
            }
            None => {
                self.trace("the first script ended");
                self.ended = true;
            }
        }
    }

    fn val(&self, a: Option<&Arg>) -> i32 {
        match a {
            Some(Arg::Num(n)) => *n,
            Some(Arg::Var(v)) => self.vars.get(v).copied().unwrap_or(0),
            _ => 0,
        }
    }

    fn var_of(a: Option<&Arg>) -> Option<i32> {
        match a {
            Some(Arg::Var(v)) => Some(*v),
            _ => None,
        }
    }

    /// Skips forward past the 9 or 10 of `depth` (`to_else`: an else stops it too).
    fn skip_block(&mut self, depth: i32, to_else: bool) {
        let Some(s) = self.script(self.cur) else {
            return;
        };
        while let Some(st) = s.get(self.pc) {
            self.pc += 1;
            if let Stmt::Cmd(c @ (9 | 10), a) = st
                && a.first() == Some(&Arg::Num(depth))
                && (*c == 10 || to_else)
            {
                return;
            }
        }
    }

    fn goto_stmt(&mut self, n: i32) {
        self.pc = n.max(0) as usize;
    }

    fn exec(&mut self, st: Stmt) {
        match st {
            Stmt::Text(t) => self.text_line(t),
            Stmt::Layer(a) => self.layer_cmd(&a),
            Stmt::Cmd(c, a) => self.command(c, &a),
            Stmt::Label | Stmt::Other => {}
        }
    }

    fn text_line(&mut self, t: String) {
        if let Some(k) = self.choice_next.take() {
            self.choices.push((t, k));
            return;
        }
        let t = t.trim_end().to_string();
        if let Some(n) = t.strip_prefix('【').and_then(|s| s.strip_suffix('】')) {
            self.name = Some(n.to_string());
            return;
        }
        if self.lines.len() >= 4 {
            self.lines.remove(0);
        }
        self.lines.push(t);
    }

    fn command(&mut self, c: i32, a: &[Arg]) {
        if self.log {
            self.trace(&format!("{}:{} #{c} {a:?}", self.cur, self.pc - 1));
        }
        let vals: Vec<i32> = a.iter().map(|x| self.val(Some(x))).collect();
        let n = |i: usize| vals.get(i).copied().unwrap_or(0);
        match c {
            1 => self.goto_stmt(n(0)),
            2 => {
                self.stack.push((self.cur, self.pc));
                self.cur = n(0).max(0) as usize;
                self.pc = 0;
            }
            3..=5 => {
                if let Some(v) = Self::var_of(a.first()) {
                    let x = match c {
                        3 => n(1),
                        4 => n(1).wrapping_add(n(2)),
                        _ => n(1).wrapping_sub(n(2)),
                    };
                    self.vars.insert(v, x);
                }
            }
            8 => {
                let (x, op, y, depth) = (n(0), n(1), n(2), n(3));
                let ok = match op {
                    61 => x == y,
                    33 => x != y,
                    60 => x < y,
                    62 => x > y,
                    _ => x == y,
                };
                if !ok {
                    self.skip_block(depth, true);
                }
            }
            9 => self.skip_block(n(0), false),
            10 => {}
            13 => {
                if self.gathering.take().is_some() {
                    self.present(effect_ms(n(0)));
                }
                self.window = true;
            }
            35 => {
                // the window and every layer go; what is shown up to 13 comes in together
                self.flush();
                self.window = false;
                self.freeze();
                self.layers.clear();
                self.gathering = Some(n(0));
            }
            // the newer games: menu, the variable for the answer, effect; every button
            28 if self.newer => {
                let menu = n(0).max(0) as usize;
                if menu < self.menus.len() {
                    self.title_var = Self::var_of(a.get(1)).unwrap_or(1);
                    for k in 1..=self.menus[menu].buttons.len() {
                        self.enabled.insert((menu, k));
                    }
                    self.flush();
                    self.window = false;
                    self.wait = Wait::Title(menu);
                    let ms = effect_ms(n(2));
                    if ms > 0.0 {
                        self.freeze();
                        self.present(ms);
                    }
                }
            }
            36 => {
                self.title_var = 1;
                let menu = n(0).max(0) as usize;
                if menu < self.menus.len() {
                    self.flush();
                    self.window = false;
                    self.wait = Wait::Title(menu);
                    let ms = effect_ms(n(2));
                    if ms > 0.0 {
                        self.freeze();
                        self.present(ms);
                    }
                }
            }
            37 => {
                self.enabled
                    .insert((n(0).max(0) as usize, n(1).max(0) as usize));
            }
            39 => {
                let v = n(0);
                self.color = [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255];
            }
            43 => self.play_voice(n(0)),
            46 => self.play_effect(0, n(0), false),
            114 => self.play_effect(3, n(0), true),
            150 => self.play_effect(4, n(0), true),
            50 => {
                self.flush();
                self.wait = Wait::ClickGoto(n(0).max(0) as usize);
            }
            54 => self.ret(),
            64 => {
                self.view = View::Slots { save: false };
                self.wait = Wait::Load;
            }
            70 => {
                self.flush();
                self.saved_at = Some((self.cur, self.pc));
                self.wait = Wait::Click;
            }
            71 => {
                self.trace("quit");
                self.ended = true;
            }
            97 | 98 => {
                // everything off (before the trial's closing pictures)
                if c == 97 {
                    self.layers.clear();
                }
            }
            131 => self.play_music(n(0)),
            132 => self.stop_music(n(0)),
            134 => {
                self.choices.clear();
                self.choice_next = None;
            }
            135 => self.choice_next = Some(n(0)),
            136 => {
                if let Some(v) = Self::var_of(a.first()) {
                    if self.choices.is_empty() {
                        self.vars.insert(v, 0);
                    } else {
                        self.window = true;
                        self.wait = Wait::Choice(v);
                    }
                }
            }
            167 => {
                self.flush();
                self.wait = Wait::Until(self.now + n(0).max(0) as f64);
            }
            _ => {}
        }
    }

    // ---- layers ----

    fn layer_cmd(&mut self, a: &[Arg]) {
        let vals: Vec<i32> = a.iter().map(|x| self.val(Some(x))).collect();
        let n = |i: usize| vals.get(i).copied().unwrap_or(0);
        let layer = n(0).max(0) as usize;
        if self.log {
            self.trace(&format!("{}:{} $ {a:?}", self.cur, self.pc - 1));
        }
        let Some(table) = self.tables.get(layer).cloned() else {
            return;
        };
        match n(1) {
            1 => {
                let (kind, pic, place, effect) = (n(2), n(3), n(4), n(5));
                let Some(&index) = table.pics.get((pic - 1).max(0) as usize) else {
                    self.trace(&format!("layer {layer}: no picture {pic}"));
                    return;
                };
                let Some(image) = self.picture(index.max(0) as usize, kind == 2) else {
                    return;
                };
                let (x, y, depth) = table.place(place);
                // while gathering, a picture with an effect shows the gathered changes with it
                let fade = effect_ms(effect) > 0.0;
                if fade {
                    self.gathering = None;
                    self.freeze();
                }
                self.layers.insert(
                    layer,
                    Layer {
                        pic,
                        place,
                        sepia: kind == 2,
                        image,
                        x: x as f32,
                        y: y as f32,
                        depth,
                        moving: None,
                    },
                );
                if fade {
                    self.present(effect_ms(effect));
                }
            }
            0 => {
                let effect = n(2);
                let fade = effect_ms(effect) > 0.0;
                if fade && self.layers.contains_key(&layer) {
                    self.gathering = None;
                    self.freeze();
                    self.layers.remove(&layer);
                    self.present(effect_ms(effect));
                } else {
                    self.layers.remove(&layer);
                }
            }
            4 => {
                let (from, to, ms) = (n(3), n(4), n(5));
                let (fx, fy, _) = table.place(from);
                let (tx, ty, depth) = table.place(to);
                if let Some(l) = self.layers.get_mut(&layer) {
                    l.moving = Some((
                        fx as f32, fy as f32, tx as f32, ty as f32, self.now, ms as f64,
                    ));
                    (l.x, l.y) = (tx as f32, ty as f32);
                    l.place = to;
                    l.depth = depth;
                }
            }
            _ => {}
        }
    }

    /// A 'b' picture by its index, drawn over the picture before it when it is a part.
    fn raw(&mut self, i: usize) -> Option<Rc<Raw>> {
        if let Some(r) = self.raw_cache.get(&i) {
            return Some(r.clone());
        }
        if i >= 10_000_000 {
            let d = self.store.read(self.files.as_mut(), i as u32)?;
            let r = match d.first() {
                Some(b'c') | Some(b'b') => Raw::from_image(&Image::decode(pic::png_of(&d)?).ok()?),
                _ => {
                    let base = match pic::base_id_2023(&d) {
                        Some(b) if b as usize != i => self.raw(b as usize),
                        _ => None,
                    };
                    match pic::decode_a2023(&d, base.as_deref()) {
                        Ok(r) => r,
                        Err(err) => {
                            self.trace(&format!("picture {i}: {err}"));
                            return None;
                        }
                    }
                }
            };
            let r = Rc::new(r);
            if self.raw_cache.len() > 24 {
                self.raw_cache.clear();
            }
            self.raw_cache.insert(i, r.clone());
            return Some(r);
        }
        let e = self.pics.get(i)?.clone();
        let d = self.arcs.read(self.files.as_mut(), &e)?;
        let r = match d.first() {
            Some(b'b') | Some(b'c') => {
                let img = Image::decode(pic::png_of(&d)?).ok()?;
                Raw::from_image(&img)
            }
            Some(b'a') | Some(b'd') => {
                let base = if pic::wants_base(&d, &self.scheme) {
                    self.base_of(i)
                } else {
                    None
                };
                match pic::decode_a(&d, &self.scheme, base.as_deref()) {
                    Ok(r) => r,
                    Err(err) => {
                        self.trace(&format!("picture {i}: {err}"));
                        return None;
                    }
                }
            }
            _ => return None,
        };
        let r = Rc::new(r);
        if self.raw_cache.len() > 24 {
            self.raw_cache.clear();
        }
        self.raw_cache.insert(i, r.clone());
        Some(r)
    }

    /// The picture a part is drawn over: the nearest whole one before it.
    fn base_of(&mut self, i: usize) -> Option<Rc<Raw>> {
        for k in (i.saturating_sub(64)..i).rev() {
            let e = self.pics[k].clone();
            let head = self.files_head(&e)?;
            match head.first() {
                Some(b'd') => continue,
                Some(b'a') if pic::wants_base(&head, &self.scheme) => continue,
                Some(b'a') | Some(b'b') | Some(b'c') => return self.raw(k),
                _ => return None,
            }
        }
        None
    }

    fn files_head(&mut self, e: &Entry) -> Option<Vec<u8>> {
        if e.packed != e.size {
            return self.arcs.read(self.files.as_mut(), e);
        }
        let mut short = e.clone();
        short.packed = e.packed.min(256);
        short.size = short.packed;
        self.arcs.read(self.files.as_mut(), &short)
    }

    /// A picture by the number the layer tables use (for cyberworks-dump).
    pub fn picture_by_number(&mut self, i: usize) -> Option<Rc<Image>> {
        self.picture(i, false)
    }

    fn picture(&mut self, i: usize, sep: bool) -> Option<Rc<Image>> {
        if let Some(p) = self.pic_cache.get(&(i, sep)) {
            return Some(p.clone());
        }
        let r = self.raw(i)?;
        let mut img = r.to_image(self.scheme.flipped);
        if sep {
            img = sepia(&img);
        }
        let img = Rc::new(img);
        if self.pic_cache.len() > 48 {
            self.pic_cache.clear();
        }
        self.pic_cache.insert((i, sep), img.clone());
        Some(img)
    }

    // ---- drawing ----

    fn freeze(&mut self) {
        if self.frozen.is_none() {
            self.fade = None;
            self.frozen = Some(self.stage_draws());
        }
    }

    /// Shows changes still being gathered (before a wait).
    fn flush(&mut self) {
        if let Some(e) = self.gathering.take() {
            self.present(effect_ms(e));
        }
    }

    fn present(&mut self, ms: f64) {
        if let Some(from) = self.frozen.take()
            && ms > 0.0
            && !self.skip
        {
            self.fade = Some(Fade {
                from,
                start: self.now,
                ms,
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

    fn stage_draws(&self) -> Vec<Draw> {
        let mut out = vec![self.solid(self.color)];
        let mut ls: Vec<(&usize, &Layer)> = self.layers.iter().filter(|(k, _)| **k != 0).collect();
        ls.sort_by_key(|(k, l)| (l.depth, **k));
        for (_, l) in ls {
            let (x, y) = l.at(self.now);
            out.push(draw(&l.image, x, y, 1.0));
        }
        out
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = match &self.frozen {
            Some(f) => f.clone(),
            None => self.stage_draws(),
        };
        if let Some(f) = &self.fade {
            let p = ((self.now - f.start) / f.ms.max(1.0)).clamp(0.0, 1.0) as f32;
            for d in &f.from {
                let mut d = d.clone();
                d.place.alpha *= 1.0 - p;
                out.push(d);
            }
        }
        if let Wait::Title(m) = self.wait {
            self.title_draws(m, &mut out);
        }
        if self.window && self.fade.is_none() && self.frozen.is_none() {
            self.window_draws(&mut out);
        }
        if let Wait::Choice(_) = self.wait {
            self.choice_draws(&mut out);
        }
        match self.view {
            View::Menu => self.menu_draws(&mut out),
            View::Slots { save } => self.slot_draws(save, &mut out),
            View::Game => {}
        }
        out
    }

    /// A layer's picture at its first place.
    fn layer_pic(&mut self, layer: usize, pic: i32) -> Option<(Rc<Image>, i32, i32)> {
        let t = self.tables.get(layer)?.clone();
        let index = *t.pics.get(pic.max(0) as usize)?;
        let img = self.picture(index.max(0) as usize, false)?;
        let (x, y, _) = t.place(0);
        Some((img, x, y))
    }

    fn title_buttons(&mut self, m: usize) -> Vec<TitleButton> {
        let Some(set) = self.menus.get(m).cloned() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (k, b) in set.buttons.iter().enumerate() {
            if !self.enabled.contains(&(m, k + 1)) {
                continue;
            }
            let shown: Vec<(Rc<Image>, i32, i32)> = b
                .pics
                .iter()
                .filter_map(|&p| self.layer_pic(b.layer, p))
                .collect();
            let Some((first, x, y)) = shown.first().cloned() else {
                continue;
            };
            let rect = [x, y, x + first.w as i32, y + first.h as i32];
            let (normal, hover) = if shown.len() >= 2 {
                (Some(first), Some(shown[1].0.clone()))
            } else {
                (None, Some(first))
            };
            out.push((k, b.answer, rect, normal, hover));
        }
        out
    }

    fn title_draws(&mut self, m: usize, out: &mut Vec<Draw>) {
        let Some(base) = self.menus.get(m).map(|s| s.base) else {
            return;
        };
        if let Some((img, x, y)) = self.layer_pic(base, 0) {
            out.push(draw(&img, x as f32, y as f32, 1.0));
        }
        let (px, py) = self.pointer;
        for (_, _, r, normal, hover) in self.title_buttons(m) {
            let over = inside(r, px, py);
            let img = if over { hover } else { normal };
            if let Some(img) = img {
                out.push(draw(&img, r[0] as f32, r[1] as f32, 1.0));
            }
        }
    }

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

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let (ww, wh) = (self.width as i32, self.height as i32);
        let mut top = wh - 220;
        if let Some(l) = self.layers.get(&0) {
            out.push(draw(&l.image, l.x, l.y, 1.0));
            top = l.y as i32;
        } else if let Some((img, x, y)) = self.layer_pic(0, 0) {
            out.push(draw(&img, x as f32, y as f32, 1.0));
            top = y;
        } else {
            let mut d = self.solid([0, 0, 0, 160]);
            d.dst = [0.0, (wh - 200) as f32, ww as f32, wh as f32];
            out.push(d);
        }
        // the window's buttons light under the pointer
        let (px, py) = self.pointer;
        for layer in 1..=7 {
            if let Some((img, x, y)) = self.layer_pic(layer, 0)
                && inside([x, y, x + img.w as i32, y + img.h as i32], px, py)
            {
                out.push(draw(&img, x as f32, y as f32, 1.0));
            }
        }
        // the sizes are for a 1280-wide screen
        let sc = |v: i32| v * ww / 1280;
        let (size, step, tx) = (sc(TEXT_SIZE), sc(LINE_STEP), sc(TEXT_X));
        let ty = top + sc(TEXT_Y - 500);
        if let Some(n) = self.name.clone() {
            let img = self.text(&n, [255, 255, 255], size);
            out.push(draw(&img, sc(NAME_X) as f32, ty as f32, 1.0));
        }
        let per_line = ((ww - tx - sc(120)) / size.max(1)).max(10) as usize;
        let mut y = ty;
        let lines = self.lines.clone();
        for l in &lines {
            for piece in wrap(l, per_line) {
                if y > wh - sc(40) {
                    break;
                }
                let img = self.text(&piece, [255, 255, 255], size);
                out.push(draw(&img, tx as f32, y as f32, 1.0));
                y += step;
            }
        }
    }

    fn choice_boxes(&self, n: usize) -> Vec<[i32; 4]> {
        let (w, h) = (self.width as i32, self.height as i32);
        let bw = w * 3 / 5;
        let bh = 56;
        let gap = 18;
        let total = n as i32 * bh + (n as i32 - 1).max(0) * gap;
        let top = (h - 220 - total) / 2;
        (0..n as i32)
            .map(|i| {
                let y = top + i * (bh + gap);
                [(w - bw) / 2, y, (w + bw) / 2, y + bh]
            })
            .collect()
    }

    fn choice_draws(&mut self, out: &mut Vec<Draw>) {
        let boxes = self.choice_boxes(self.choices.len());
        let choices = self.choices.clone();
        for (r, (t, _)) in boxes.iter().zip(&choices) {
            self.button(t, *r, out);
        }
    }

    fn button(&mut self, t: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let over = inside(r, self.pointer.0, self.pointer.1);
        let mut d = self.solid(if over {
            [120, 60, 150, 230]
        } else {
            [30, 20, 40, 210]
        });
        d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        out.push(d);
        let img = self.text(t, [255, 255, 255], TEXT_SIZE);
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
        let (bw, bh, gap) = (w - 240, 48, 10);
        let top = (h - (bh + gap) * (SLOTS as i32 + 1)) / 2;
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
        let head = self.text(if save { "セーブ" } else { "ロード" }, [255, 255, 255], 30);
        out.push(draw(&head, 120.0, (rects[0][1] - 44) as f32, 1.0));
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

    // ---- sounds ----

    fn sound(&mut self, list: u8, i: i32) -> Option<Vec<u8>> {
        let e = match list {
            b'j' => self.voices.get(i.max(0) as usize),
            b'k' => self.effects.get(i.max(0) as usize),
            _ => self.music.get(i.max(0) as usize),
        }?
        .clone();
        let d = self.arcs.read(self.files.as_mut(), &e)?;
        Some(sound::decode(d, &self.key))
    }

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

    fn play_voice(&mut self, i: i32) {
        if self.skip || i < 0 {
            return;
        }
        let d = self.sound(b'j', i);
        if d.is_none() {
            self.trace(&format!("no voice {i}"));
        }
        self.push_sound("voice", d, 1.0, false, 0.0);
    }

    fn play_effect(&mut self, channel: u8, i: i32, looped: bool) {
        let ch = format!("se{channel}");
        if i < 0 {
            self.loops.remove(&channel);
            self.push_sound(&ch, None, 0.0, false, 300.0);
            return;
        }
        if looped {
            self.loops.insert(channel, i);
        } else if self.skip {
            return;
        }
        let d = self.sound(b'k', i);
        if d.is_some() {
            self.push_sound(&ch, d, 0.8, looped, 0.0);
        }
    }

    fn play_music(&mut self, i: i32) {
        if i < 0 {
            self.stop_music(500);
            return;
        }
        if self.bgm == Some(i) {
            return;
        }
        self.bgm = Some(i);
        let d = self.sound(b'u', i);
        if d.is_some() {
            self.push_sound("bgm", d, 0.7, true, 0.0);
        }
    }

    fn stop_music(&mut self, ms: i32) {
        self.bgm = None;
        self.push_sound("bgm", None, 0.0, false, ms.max(0) as f32);
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    fn advance(&mut self) {
        match self.wait {
            Wait::Click => {
                self.wait = Wait::None;
                self.lines.clear();
                self.name = None;
            }
            Wait::ClickGoto(n) => {
                self.wait = Wait::None;
                self.pc = n;
            }
            _ => {}
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
                            self.load(i);
                            return;
                        }
                    }
                    Some(_) => self.close_slots(),
                    None => {}
                }
                return;
            }
            View::Game => {}
        }
        if self.fade.is_some() && matches!(self.wait, Wait::Click | Wait::ClickGoto(_)) {
            self.fade = None;
            return;
        }
        match self.wait.clone() {
            Wait::Title(m) => {
                let hit = self
                    .title_buttons(m)
                    .into_iter()
                    .find(|b| inside(b.2, x, y));
                if let Some((_, answer, ..)) = hit {
                    self.vars.insert(self.title_var, answer);
                    self.enabled.retain(|(mm, _)| *mm != m);
                    self.freeze();
                    self.wait = Wait::None;
                    self.present(300.0);
                }
            }
            Wait::Choice(v) => {
                let boxes = self.choice_boxes(self.choices.len());
                if let Some(i) = boxes.iter().position(|r| inside(*r, x, y)) {
                    let answer = self.choices[i].1;
                    self.vars.insert(v, answer);
                    self.choices.clear();
                    self.wait = Wait::None;
                    self.lines.clear();
                    self.name = None;
                }
            }
            Wait::Click | Wait::ClickGoto(_) => {
                if self.window && self.wait == Wait::Click && self.newer {
                    // the newer bar: save, load, then buttons we do not have (the menu is a
                    // right click); a click on any of them does not go on. Save and load are
                    // the first two buttons of the same size (a close button may come first).
                    let size = |e: &mut Self, l: usize| e.layer_pic(l, 0).map(|p| (p.0.w, p.0.h));
                    let save = (1usize..=3)
                        .find(|&l| size(self, l).is_some() && size(self, l) == size(self, l + 1))
                        .unwrap_or(1);
                    for layer in 1usize..=13 {
                        if let Some((img, bx, by)) = self.layer_pic(layer, 0)
                            && (img.w as i32) < self.width as i32 / 2
                            && inside([bx, by, bx + img.w as i32, by + img.h as i32], x, y)
                        {
                            if layer == save {
                                self.view = View::Slots { save: true };
                            } else if layer == save + 1 {
                                self.view = View::Slots { save: false };
                            }
                            return;
                        }
                    }
                } else if self.window && self.wait == Wait::Click {
                    // the window's own buttons: skip, save, load and the system menu
                    for layer in [1usize, 2, 3, 7] {
                        if let Some((img, bx, by)) = self.layer_pic(layer, 0)
                            && inside([bx, by, bx + img.w as i32, by + img.h as i32], x, y)
                        {
                            match layer {
                                1 => self.skip = !self.skip,
                                2 => self.view = View::Slots { save: true },
                                3 => self.view = View::Slots { save: false },
                                _ => self.view = View::Menu,
                            }
                            return;
                        }
                    }
                }
                self.sounds_stop_voice();
                self.advance();
            }
            _ => {}
        }
    }

    fn sounds_stop_voice(&mut self) {
        self.push_sound("voice", None, 0.0, false, 0.0);
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
        match self.view {
            View::Menu => self.view = View::Game,
            View::Slots { .. } => self.close_slots(),
            View::Game => {
                self.skip = false;
                if matches!(self.wait, Wait::Click | Wait::Choice(_)) {
                    self.view = View::Menu;
                }
            }
        }
    }

    pub fn key(&mut self, k: usize) {
        match k {
            4 => {
                let (x, y) = self.pointer;
                if matches!(self.wait, Wait::Click | Wait::ClickGoto(_)) || self.view != View::Game
                {
                    self.click(x, y);
                }
            }
            5 => self.cancel(),
            6 => self.skip = !self.skip,
            _ => {}
        }
    }

    pub fn wheel(&mut self, up: bool) {
        if !up && matches!(self.wait, Wait::Click) && self.view == View::Game {
            self.sounds_stop_voice();
            self.advance();
        }
    }

    pub fn choosing(&self) -> bool {
        matches!(self.wait, Wait::Choice(_) | Wait::Title(_))
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    fn back_to_title(&mut self) {
        self.cur = 0;
        self.pc = 0;
        self.stack.clear();
        self.layers.clear();
        self.window = false;
        self.lines.clear();
        self.name = None;
        self.wait = Wait::None;
        self.view = View::Game;
        self.frozen = None;
        self.gathering = None;
        self.fade = None;
        self.skip = false;
        self.choices.clear();
        self.stop_music(300);
        for ch in [0u8, 3, 4] {
            self.push_sound(&format!("se{ch}"), None, 0.0, false, 300.0);
        }
        self.loops.clear();
    }

    // ---- saves ----

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&format!("slot{i:02}"))?;
        Slot::parse(&String::from_utf8_lossy(&b))
    }

    fn save(&mut self, i: usize) {
        let Some((cur, pc)) = self.saved_at else {
            return;
        };
        let mut s = String::new();
        let first = self.lines.first().cloned().unwrap_or_default();
        let title = match &self.name {
            Some(n) => format!("{n} {first}"),
            None => first,
        };
        s += &format!("title {}\n", title.replace('\n', " "));
        s += &format!("at {cur} {pc}\n");
        for (a, b) in &self.stack {
            s += &format!("call {a} {b}\n");
        }
        for (k, v) in &self.vars {
            s += &format!("var {k} {v}\n");
        }
        for (k, l) in &self.layers {
            s += &format!("layer {k} {} {} {}\n", l.pic, l.place, l.sepia as u8);
        }
        if let Some(b) = self.bgm {
            s += &format!("bgm {b}\n");
        }
        for (c, v) in &self.loops {
            s += &format!("loop {c} {v}\n");
        }
        s += &format!(
            "color {} {} {}\n",
            self.color[0], self.color[1], self.color[2]
        );
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

    fn load(&mut self, i: usize) {
        let Some(slot) = self.read_slot(i) else {
            return;
        };
        self.back_to_title();
        self.cur = slot.at.0;
        self.pc = slot.at.1;
        self.stack = slot.calls;
        self.vars = slot.vars;
        for (k, pic, place, sep) in slot.layers {
            let Some(t) = self.tables.get(k).cloned() else {
                continue;
            };
            let Some(&index) = t.pics.get((pic - 1).max(0) as usize) else {
                continue;
            };
            let Some(image) = self.picture(index.max(0) as usize, sep) else {
                continue;
            };
            let (x, y, depth) = t.place(place);
            self.layers.insert(
                k,
                Layer {
                    pic,
                    place,
                    sepia: sep,
                    image,
                    x: x as f32,
                    y: y as f32,
                    depth,
                    moving: None,
                },
            );
        }
        if let Some(b) = slot.bgm {
            self.play_music(b);
        }
        for (c, v) in slot.loops {
            self.play_effect(c, v, true);
        }
        self.color = slot.color;
        self.window = slot.window;
        self.name = slot.name;
        self.lines = slot.lines;
        self.saved_at = Some(slot.at);
        self.wait = Wait::Click;
        self.view = View::Game;
        self.freeze();
        self.frozen = Some(vec![self.solid([0, 0, 0, 255])]);
        self.present(400.0);
    }
}

struct Slot {
    title: String,
    at: (usize, usize),
    calls: Vec<(usize, usize)>,
    vars: HashMap<i32, i32>,
    layers: Vec<(usize, i32, i32, bool)>,
    bgm: Option<i32>,
    loops: Vec<(u8, i32)>,
    color: [u8; 4],
    window: bool,
    name: Option<String>,
    lines: Vec<String>,
}

impl Slot {
    fn parse(s: &str) -> Option<Slot> {
        let mut slot = Slot {
            title: String::new(),
            at: (0, 0),
            calls: Vec::new(),
            vars: HashMap::new(),
            layers: Vec::new(),
            bgm: None,
            loops: Vec::new(),
            color: [0, 0, 0, 255],
            window: true,
            name: None,
            lines: Vec::new(),
        };
        let mut seen = false;
        for line in s.lines() {
            let (k, rest) = line.split_once(' ').unwrap_or((line, ""));
            let nums: Vec<i64> = rest.split(' ').filter_map(|x| x.parse().ok()).collect();
            let n = |i: usize| nums.get(i).copied().unwrap_or(0);
            match k {
                "title" => slot.title = rest.to_string(),
                "at" => {
                    slot.at = (n(0) as usize, n(1) as usize);
                    seen = true;
                }
                "call" => slot.calls.push((n(0) as usize, n(1) as usize)),
                "var" => {
                    slot.vars.insert(n(0) as i32, n(1) as i32);
                }
                "layer" => slot
                    .layers
                    .push((n(0) as usize, n(1) as i32, n(2) as i32, n(3) != 0)),
                "bgm" => slot.bgm = Some(n(0) as i32),
                "loop" => slot.loops.push((n(0) as u8, n(1) as i32)),
                "color" => slot.color = [n(0) as u8, n(1) as u8, n(2) as u8, 255],
                "window" => slot.window = n(0) != 0,
                "name" => slot.name = Some(rest.to_string()),
                "line" => slot.lines.push(rest.to_string()),
                _ => {}
            }
        }
        seen.then_some(slot)
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
