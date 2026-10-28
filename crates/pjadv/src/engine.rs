//! Runs a PJADV game (ななついろ★ドロップス' trial) a frame at a time, 800x600.
//!
//! scenario.dat ("PJADV_SF0001", code from 0x20) is a list of u32 words: an instruction's
//! first word is its op (high 24 bits) and its length in words (low 8 bits), the rest its
//! arguments. An argument's top 4 bits say what it is: 0 a number (28 bits, signed), 1 a system
//! variable, 2 a global, 3 a temporary, 4 the last call's result. Jumps name byte offsets.
//! Pictures, sounds and voices are 1-based indexes into filename.dat; messages and names are
//! offsets into textdata.dat (Shift_JIS).
//!
//! - `0200xx dst src`: mov, add, sub, mul, div, mod, and, or; `0201xx`: compares (into dst)
//! - `010002` jump, `010003` call, `010004` return, `010007 v at` jump when v is 0, `010005 ms`
//!   wait, `010000` the end, `01000d` a chapter's name
//! - `010200 g` a new button group, `010201 g id x0 y0 x1 y1 on picture handler` a button (a
//!   click calls its handler), `010202 g on` lets the script poll it
//! - `800101 picture` the background, `800102 _ picture place` a character (place 2, 3, 4:
//!   left, middle, right); both wait for `800100 effect ms` that shows them (effect 0: at once)
//! - `800003 voice name text _ flags`: a message (flags 0x20000: wait for a click), `800202`
//!   hides the message window
//! - `040000 layer picture` a picture on a layer (0: a new one, its number returned), `040001`
//!   removes it, `040003` places, `040005` alpha (0..255), `040007` zoom (percent);
//!   `040100` .. `040101 ms` animate (each on a track: `040102` moves by, `040103` moves from
//!   to, `040105` alpha, `040106` zooms by) and wait
//! - `050000 bgm loop`, `050001 ms` stops it, `050005 bgm ms loop` fades it in, `050100 se ch`,
//!   `050101 ch`
//!
//! The system menus of the script (save, load, settings) are left out; a right click opens our
//! own save slots. Rotations, shakes and picture animations (.anm/.bnm) are left out too.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::pac::{Pac, ini};

pub const W: u32 = 800;
pub const H: u32 = 600;
const SLOTS: usize = 10;
/// The trial's load screen and settings, which our own slots stand in for.
const LOAD_SCREEN: usize = 0x3f774;
const SETTINGS: usize = 0x38828;
/// Where `040000 0` numbers its layers from.
const FIRST_FREE_LAYER: i32 = 0x1000;
/// The message window's text: where, the characters a line, size and line height.
const TEXT_AT: (i32, i32) = (216, 492);
const NAME_AT: (i32, i32) = (192, 470);
const TEXT_COLS: usize = 25;
const TEXT_SIZE: i32 = 22;
const TEXT_LH: i32 = 24;

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

/// A picture as graphic.def describes it: the part of the file, where it goes, its centre.
#[derive(Clone)]
struct Pic {
    img: Rc<Image>,
    src: [i32; 4],
    /// graphic.def's xpos, ypos
    x: f32,
    y: f32,
    /// the EPA's own offset
    ox: f32,
    oy: f32,
    xcenter: Option<f32>,
    ycenter: Option<f32>,
}

impl Pic {
    fn w(&self) -> f32 {
        (self.src[2] - self.src[0]) as f32
    }
    fn h(&self) -> f32 {
        (self.src[3] - self.src[1]) as f32
    }
}

#[derive(Clone)]
struct Sprite {
    file: i32,
    pic: Option<Pic>,
    x: f32,
    y: f32,
    /// 0..255
    alpha: f32,
    /// percent
    zoom: f32,
}

#[derive(Clone, Copy, Debug)]
enum AnimKind {
    By(f32, f32),
    FromTo(f32, f32, f32, f32),
    Alpha(f32),
    ZoomBy(f32),
}

#[derive(Clone, Debug)]
struct Anim {
    track: i32,
    layer: i32,
    kind: AnimKind,
    dur: u32,
    t: u32,
    /// x, y, alpha, zoom when it started
    from: Option<[f32; 4]>,
}

#[derive(Clone)]
struct Button {
    id: i32,
    rect: [i32; 4],
    on: bool,
    pic: Option<Pic>,
    handler: usize,
}

#[derive(Clone, Default)]
struct Group {
    buttons: Vec<Button>,
    active: bool,
}

/// A character of the ADV layer: picture file, place, picture.
#[derive(Clone)]
struct Chara {
    file: i32,
    place: i32,
    pic: Option<Pic>,
}

#[derive(Clone, Default)]
struct Stage {
    bg: Option<(i32, Option<Pic>)>,
    chars: Vec<Chara>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Wait {
    None,
    Click,
    Frames(u32),
    Anims(u32),
    Fade,
    Ended,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Game,
    Slots { save: bool },
}

struct Fade {
    old: Vec<Draw>,
    t: u32,
    dur: u32,
}

pub struct Engine {
    /// the messages read so far (the wheel turned up)
    backlog: vncommon::backlog::Backlog,
    files: Box<dyn Files>,
    pac: Pac,
    code: Vec<u8>,
    text: Vec<u8>,
    names: Vec<String>,
    graphic: HashMap<String, HashMap<String, String>>,
    bgm_ini: HashMap<String, HashMap<String, String>>,
    epas: HashMap<String, Option<(Rc<Image>, i32, i32)>>,
    // the machine
    pc: usize,
    stack: Vec<usize>,
    vars: BTreeMap<(u8, i32), i32>,
    ret: i32,
    // pictures
    stage: Stage,
    shown: Stage,
    /// a `800100` happened: the next background or character starts a new stage
    staged: bool,
    sprites: BTreeMap<i32, Sprite>,
    next_layer: i32,
    pending: Vec<Anim>,
    tracks: BTreeMap<i32, VecDeque<Anim>>,
    fade: Option<Fade>,
    last_scene: Vec<Draw>,
    groups: BTreeMap<i32, Group>,
    /// the message window's buttons (`800204`): ids 0..4 quick save, quick load, save, load,
    /// settings; 6 skip
    win_group: Option<i32>,
    // the message
    window: bool,
    win_pic: String,
    name: Vec<u8>,
    msg: Vec<u8>,
    msg_offs: (i32, i32),
    chapter: String,
    // sound
    sounds: Vec<SoundEvent>,
    bgm: Option<String>,
    bgm_loop: Option<(u32, String)>,
    // the player
    wait: Wait,
    view: View,
    pointer: (i32, i32),
    skip: bool,
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<(Vec<u8>, i32), Rc<Image>>,
    pub trace: bool,
    pub frames: u64,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at.saturating_add(4))
        .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
}

fn ms_frames(ms: i32) -> u32 {
    (ms.clamp(0, 600_000) as u32 * 60).div_ceil(1000)
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3]
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

/// How long an Ogg Vorbis file plays, in frames: the last page's granule over the rate.
fn ogg_frames(d: &[u8]) -> Option<u32> {
    let id = d.windows(7).position(|w| w == b"\x01vorbis")?;
    let rate = u32_at(d, id + 12);
    let last = d.windows(4).rposition(|w| w == b"OggS")?;
    let granule = u64::from_le_bytes(d.get(last + 6..last + 14)?.try_into().ok()?);
    (rate > 0).then(|| (granule * 60 / rate as u64) as u32)
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let pac = Pac::open(files.as_mut())?;
        let mut get = |n: &str| {
            pac.read(files.as_mut(), n)
                .ok_or_else(|| format!("{n} is missing"))
        };
        let code = get("scenario.dat")?;
        if !code.starts_with(b"PJADV_SF") {
            return Err("scenario.dat: not a PJADV scenario".into());
        }
        let mut text = get("textdata.dat").or_else(|_| get("textdata.bin"))?;
        // an encrypted textdata.bin: xor with a key that steps by 0x5c
        if text.starts_with(&[0x95, 0x6b, 0x3c, 0x9d, 0x63]) {
            let mut k = 0xc5u8;
            for b in text.iter_mut() {
                *b ^= k;
                k = k.wrapping_add(0x5c);
            }
        }
        let fl = get("filename.dat")?;
        let n = u32_at(&fl, 12) as usize;
        let names = (0..n.min(fl.len() / 32))
            .map(|i| {
                let raw = fl.get(16 + i * 32..16 + i * 32 + 32).unwrap_or(&[]);
                let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
                nexas::sjis::decode(&raw[..end])
            })
            .collect();
        let graphic = ini(&nexas::sjis::decode(
            &get("graphic.def").unwrap_or_default(),
        ));
        let bgm_ini = ini(&nexas::sjis::decode(&get("BGM.ini").unwrap_or_default()));
        Ok(Engine {
            backlog: Default::default(),
            files,
            pac,
            code,
            text,
            names,
            graphic,
            bgm_ini,
            epas: HashMap::new(),
            pc: 0x20,
            stack: Vec::new(),
            vars: BTreeMap::new(),
            ret: 0,
            stage: Stage::default(),
            shown: Stage::default(),
            staged: true,
            sprites: BTreeMap::new(),
            next_layer: FIRST_FREE_LAYER,
            pending: Vec::new(),
            tracks: BTreeMap::new(),
            fade: None,
            last_scene: Vec::new(),
            groups: BTreeMap::new(),
            win_group: None,
            window: false,
            win_pic: "sys_mainwin".into(),
            name: Vec::new(),
            msg: Vec::new(),
            msg_offs: (0, 0),
            chapter: String::new(),
            sounds: Vec::new(),
            bgm: None,
            bgm_loop: None,
            wait: Wait::None,
            view: View::Game,
            pointer: (0, 0),
            skip: false,
            font: None,
            text_cache: HashMap::new(),
            trace: std::env::var("PJADV_LOG").is_ok(),
            frames: 0,
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

    fn log(&self, s: &str) {
        if self.trace {
            eprintln!("[{}] {s}", self.frames);
        }
    }

    pub fn ended(&self) -> bool {
        self.wait == Wait::Ended
    }

    // ---- files ----

    fn file_name(&self, i: i32) -> Option<String> {
        (i > 0).then(|| self.names.get(i as usize - 1).cloned())?
    }

    /// A string of textdata.dat.
    fn text_at(&self, off: i32) -> Vec<u8> {
        if off <= 0 {
            return Vec::new();
        }
        let s = self.text.get(off as usize..).unwrap_or(&[]);
        let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        s[..end].to_vec()
    }

    fn epa(&mut self, file: &str) -> Option<(Rc<Image>, i32, i32)> {
        let key = file.to_lowercase();
        if let Some(e) = self.epas.get(&key) {
            return e.clone();
        }
        let e =
            self.pac
                .read(self.files.as_mut(), &key)
                .and_then(|d| match crate::epa::decode(&d) {
                    Ok(e) => Some((Rc::new(e.img), e.ox, e.oy)),
                    Err(err) => {
                        self.log(&format!("{file}: {err}"));
                        None
                    }
                });
        if self.epas.len() > 48 {
            self.epas.clear();
        }
        self.epas.insert(key, e.clone());
        e
    }

    /// A picture by its index in filename.dat: its graphic.def section, or `<name>.epa`.
    fn pic(&mut self, file: i32) -> Option<Pic> {
        let name = self.file_name(file)?;
        let def = self.graphic.get(&name.to_lowercase()).cloned();
        let num = |k: &str| {
            def.as_ref()
                .and_then(|d| d.get(k))
                .and_then(|v| v.trim().parse::<f32>().ok())
        };
        let src = def
            .as_ref()
            .and_then(|d| d.get("src").cloned())
            .unwrap_or_else(|| format!("{name}.epa"));
        let Some((img, ox, oy)) = self.epa(&src) else {
            self.log(&format!("missing picture {name}"));
            return None;
        };
        let mut rect = [0, 0, img.w as i32, img.h as i32];
        if let Some(fp) = def.as_ref().and_then(|d| d.get("filepos")) {
            let v: Vec<i32> = fp
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            if let [x, y, w, h] = v[..] {
                rect = [x, y, x + w, y + h];
            }
        }
        Some(Pic {
            img,
            src: rect,
            x: num("xpos").unwrap_or(0.0),
            y: num("ypos").unwrap_or(0.0),
            ox: ox as f32,
            oy: oy as f32,
            xcenter: num("xcenter"),
            ycenter: num("ycenter"),
        })
    }

    fn play(&mut self, channel: &str, file: &str, repeat: bool) -> Option<u32> {
        let data = self.pac.read(self.files.as_mut(), file);
        let Some(data) = data else {
            self.log(&format!("missing sound {file}"));
            return None;
        };
        let len = ogg_frames(&data);
        self.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data: Some(data),
            volume: 1.0,
            repeat,
            fade_ms: 0.0,
        });
        len
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

    /// Music by its name: a BGM.ini section (an intro, then a loop) or an Ogg file.
    fn play_bgm(&mut self, name: &str, repeat: bool) {
        if self.bgm.as_deref() == Some(name) {
            return;
        }
        self.bgm = Some(name.to_string());
        self.bgm_loop = None;
        let sec = self.bgm_ini.get(&name.to_lowercase()).cloned();
        let intro = sec.as_ref().and_then(|s| s.get("ogg_intro_src").cloned());
        let looped = sec
            .as_ref()
            .and_then(|s| s.get("ogg_loop_src").cloned())
            .unwrap_or_else(|| format!("{name}.ogg"));
        match intro {
            Some(i) if repeat => {
                if let Some(n) = self.play("bgm", &i, false) {
                    self.bgm_loop = Some((n, looped));
                }
            }
            _ => {
                self.play("bgm", &looped, repeat);
            }
        }
    }

    fn stop_bgm(&mut self, ms: i32) {
        self.bgm = None;
        self.bgm_loop = None;
        self.stop("bgm", ms as f32);
    }

    // ---- the machine ----

    fn val(&self, v: u32) -> i32 {
        let i = ((v << 4) as i32) >> 4;
        match v >> 28 {
            0 => i,
            t @ 1..=3 => self.vars.get(&(t as u8, i)).copied().unwrap_or(0),
            4 => self.ret,
            _ => 0,
        }
    }

    fn set(&mut self, v: u32, x: i32) {
        let i = ((v << 4) as i32) >> 4;
        match v >> 28 {
            t @ 1..=3 => {
                if x == 0 {
                    self.vars.remove(&(t as u8, i));
                } else {
                    self.vars.insert((t as u8, i), x);
                }
            }
            4 => self.ret = x,
            _ => {}
        }
    }

    fn step(&mut self) -> Result<(), String> {
        let at = self.pc;
        if at + 4 > self.code.len() {
            self.log("ran off the end of the scenario");
            self.wait = Wait::Ended;
            return Ok(());
        }
        let w = u32_at(&self.code, at);
        let n = (w & 0xff) as usize;
        if n == 0 {
            self.pc += 4;
            return Ok(());
        }
        let op = w >> 8;
        let raw: Vec<u32> = (1..n).map(|i| u32_at(&self.code, at + i * 4)).collect();
        self.pc = at + n * 4;
        let vals: Vec<i32> = raw.iter().map(|&v| self.val(v)).collect();
        let a = |i: usize| vals.get(i).copied().unwrap_or(0);
        if self.trace {
            let args: Vec<String> = raw.iter().map(|v| format!("{v:x}")).collect();
            self.log(&format!("{at:06x} {op:06x} {}", args.join(" ")));
        }
        match op {
            0x020001..=0x02000a | 0x020100..=0x020107 if raw.len() >= 2 => {
                let (x, y) = (a(0), a(1));
                let r = match op {
                    0x020001 => y,
                    0x020002 => x.wrapping_add(y),
                    0x020003 => x.wrapping_sub(y),
                    0x020004 => x.wrapping_mul(y),
                    0x020005 => x.checked_div(y).unwrap_or(0),
                    0x020006 => x.checked_rem(y).unwrap_or(0),
                    0x020009 => x & y,
                    0x02000a => x | y,
                    0x020100 => (x == y) as i32,
                    0x020101 => (x < y) as i32,
                    0x020102 => (x > y) as i32,
                    0x020103 => (x <= y) as i32,
                    0x020104 => (x >= y) as i32,
                    0x020105 => (x != 0 && y != 0) as i32,
                    0x020106 => (x != 0 || y != 0) as i32,
                    0x020107 => (x != y) as i32,
                    _ => x,
                };
                self.set(raw[0], r);
            }
            0x010000 => {
                self.log("the end");
                self.stop_all();
                self.wait = Wait::Ended;
            }
            0x010002 | 0x010013 => self.pc = a(0) as usize,
            0x010003 => {
                let to = a(0) as usize;
                if to == LOAD_SCREEN {
                    self.view = View::Slots { save: false };
                } else if to != SETTINGS {
                    self.stack.push(self.pc);
                    self.pc = to;
                }
            }
            0x010004 => match self.stack.pop() {
                Some(p) => self.pc = p,
                None => self.restart(),
            },
            0x010005 => {
                if !self.skip {
                    self.wait = Wait::Frames(ms_frames(a(0)));
                }
            }
            0x010007 => {
                if a(0) == 0 {
                    self.pc = a(1) as usize;
                }
            }
            0x01000d => self.chapter = nexas::sjis::decode(&self.text_at(a(0))),
            0x010200 => {
                self.groups.insert(a(0), Group::default());
            }
            0x010201 => {
                let pic = self.pic(a(7));
                let b = Button {
                    id: a(1),
                    rect: [a(2), a(3), a(4), a(5)],
                    on: a(6) != 0,
                    pic,
                    handler: a(8) as usize,
                };
                self.groups.entry(a(0)).or_default().buttons.push(b);
            }
            0x010202 => self.groups.entry(a(0)).or_default().active = a(1) != 0,
            0x010204 => self.groups.entry(a(0)).or_default().active = false,
            // results we do not know: none
            0x003000..=0x003003 | 0x010302 | 0x040009 => self.ret = 0,
            0x040000 => {
                let layer = if a(0) == 0 {
                    self.next_layer += 1;
                    self.next_layer - 1
                } else {
                    a(0)
                };
                let file = a(1);
                let pic = self.pic(file);
                let (x, y) = pic.as_ref().map_or((0.0, 0.0), |p| (p.x, p.y));
                self.sprites.insert(
                    layer,
                    Sprite {
                        file,
                        pic,
                        x,
                        y,
                        alpha: 255.0,
                        zoom: 100.0,
                    },
                );
                self.ret = layer;
            }
            0x040001 => {
                let l = a(0);
                self.sprites.remove(&l);
                for t in self.tracks.values_mut() {
                    t.retain(|an| an.layer != l);
                }
            }
            0x040003 => {
                let (x, y) = (a(1) as f32, a(2) as f32);
                if let Some(s) = self.sprites.get_mut(&a(0)) {
                    (s.x, s.y) = (x, y);
                }
            }
            0x040005 => {
                let v = a(1) as f32;
                if let Some(s) = self.sprites.get_mut(&a(0)) {
                    s.alpha = v.clamp(0.0, 255.0);
                }
            }
            0x040007 => {
                let v = a(1) as f32;
                if let Some(s) = self.sprites.get_mut(&a(0)) {
                    s.zoom = v;
                }
            }
            0x040100 => self.pending.clear(),
            0x040101 => {
                let dur = ms_frames(a(0));
                for mut an in std::mem::take(&mut self.pending) {
                    if an.dur == 0 {
                        an.dur = dur;
                    }
                    self.tracks.entry(an.track).or_default().push_back(an);
                }
                if self.skip {
                    self.finish_anims();
                } else if a(1) == 0 && dur > 0 {
                    self.wait = Wait::Anims(dur);
                }
            }
            0x040102 | 0x040103 | 0x040105 | 0x040106 => {
                let (kind, ms) = match (op, raw.len()) {
                    (0x040102, _) => (AnimKind::By(a(2) as f32, a(3) as f32), a(4)),
                    (0x040103, _) => (
                        AnimKind::FromTo(a(2) as f32, a(3) as f32, a(4) as f32, a(5) as f32),
                        a(6),
                    ),
                    (0x040105, _) => (AnimKind::Alpha(a(2) as f32), a(3)),
                    _ => (AnimKind::ZoomBy(a(2) as f32), a(3)),
                };
                self.pending.push(Anim {
                    track: a(0),
                    layer: a(1),
                    kind,
                    dur: ms_frames(ms),
                    t: 0,
                    from: None,
                });
            }
            0x040201 => self.finish_anims(),
            0x050000 => {
                if let Some(n) = self.file_name(a(0)) {
                    self.play_bgm(&n, a(1) != 0);
                }
            }
            0x050005 => {
                if let Some(n) = self.file_name(a(0)) {
                    self.play_bgm(&n, a(2) != 0);
                }
            }
            0x050001 => self.stop_bgm(a(0)),
            0x050100 => {
                if let Some(n) = self.file_name(a(0)) {
                    self.play(&format!("se{}", a(1)), &format!("{n}.ogg"), false);
                }
            }
            0x050101 => self.stop(&format!("se{}", a(0)), 0.0),
            0x800003 => {
                let voice = a(0);
                self.name = self.text_at(a(1));
                self.msg = self.text_at(a(2));
                self.msg_offs = (a(1), a(2));
                self.window = true;
                if voice > 0
                    && !self.skip
                    && let Some(n) = self.file_name(voice)
                {
                    self.play("voice", &format!("{n}.ogg"), false);
                }
                if a(4) & 0x20000 != 0 {
                    self.wait = Wait::Click;
                }
            }
            0x800101 => {
                self.new_stage();
                let pic = self.pic(a(0));
                self.stage.bg = Some((a(0), pic));
            }
            0x800102 => {
                self.new_stage();
                let pic = self.pic(a(1));
                let c = Chara {
                    file: a(1),
                    place: a(2),
                    pic,
                };
                self.stage.chars.push(c);
            }
            0x800100 => {
                let (effect, ms) = (a(0), a(1));
                self.shown = self.stage.clone();
                self.staged = true;
                let dur = if effect == 0 || self.skip {
                    0
                } else {
                    ms_frames(ms)
                };
                if dur > 0 {
                    self.fade = Some(Fade {
                        old: self.last_scene.clone(),
                        t: 0,
                        dur,
                    });
                    self.wait = Wait::Fade;
                } else {
                    self.fade = None;
                    self.wait = Wait::Frames(1);
                }
            }
            0x800200 => {
                if let Some(n) = self.file_name(a(0))
                    && self.graphic.contains_key(&n.to_lowercase())
                {
                    self.win_pic = n;
                }
            }
            0x800204 => self.win_group = Some(a(0)),
            0x800202 => {
                self.window = false;
                self.msg.clear();
                self.name.clear();
            }
            _ => {}
        }
        Ok(())
    }

    /// The first background or character after a `800100` starts the stage again: no
    /// characters (and the background goes when a new one comes).
    fn new_stage(&mut self) {
        if self.staged {
            self.stage.chars.clear();
            self.staged = false;
        }
    }

    /// Back to the start of the scenario, with nothing on the screen.
    fn restart(&mut self) {
        self.pc = 0x20;
        self.stack.clear();
        self.stage = Stage::default();
        self.shown = Stage::default();
        self.staged = true;
        self.sprites.clear();
        self.pending.clear();
        self.tracks.clear();
        let keep = self.win_group;
        self.groups.retain(|k, _| Some(*k) == keep);
        self.fade = None;
        self.window = false;
        self.msg.clear();
        self.name.clear();
        self.stop_all();
    }

    fn stop_all(&mut self) {
        for c in ["voice", "se0", "se1", "se2", "se3"] {
            self.stop(c, 0.0);
        }
        self.stop_bgm(0);
    }

    fn sprite_state(s: &Sprite) -> [f32; 4] {
        [s.x, s.y, s.alpha, s.zoom]
    }

    fn apply(s: &mut Sprite, kind: AnimKind, from: [f32; 4], k: f32) {
        match kind {
            AnimKind::By(dx, dy) => (s.x, s.y) = (from[0] + dx * k, from[1] + dy * k),
            AnimKind::FromTo(x0, y0, x1, y1) => {
                s.x = from[0] + x0 + (x1 - x0) * k;
                s.y = from[1] + y0 + (y1 - y0) * k;
            }
            AnimKind::Alpha(a) => s.alpha = (from[2] + (a - from[2]) * k).clamp(0.0, 255.0),
            AnimKind::ZoomBy(d) => s.zoom = from[3] + d * k,
        }
    }

    /// One frame of every track's first animation.
    fn tick_anims(&mut self) {
        for track in self.tracks.values_mut() {
            while let Some(an) = track.front_mut() {
                let Some(s) = self.sprites.get_mut(&an.layer) else {
                    track.pop_front();
                    continue;
                };
                let from = *an.from.get_or_insert(Self::sprite_state(s));
                an.t += 1;
                let k = if an.dur == 0 {
                    1.0
                } else {
                    (an.t as f32 / an.dur as f32).min(1.0)
                };
                Self::apply(s, an.kind, from, k);
                if k >= 1.0 {
                    track.pop_front();
                }
                break;
            }
        }
        self.tracks.retain(|_, t| !t.is_empty());
    }

    fn finish_anims(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for track in std::mem::take(&mut self.tracks).into_values() {
            for an in track.into_iter().chain(pending.iter().cloned()) {
                if let Some(s) = self.sprites.get_mut(&an.layer) {
                    let from = an.from.unwrap_or(Self::sprite_state(s));
                    Self::apply(s, an.kind, from, 1.0);
                }
            }
        }
    }

    pub fn frame(&mut self) -> Result<(), String> {
        self.log_page();
        self.frames += 1;
        if let Some((n, name)) = self.bgm_loop.take() {
            if n <= 1 {
                self.play("bgm", &name, true);
            } else {
                self.bgm_loop = Some((n - 1, name));
            }
        }
        if self.view != View::Game {
            return Ok(());
        }
        self.tick_anims();
        if let Some(f) = &mut self.fade {
            f.t += 1;
            if f.t >= f.dur {
                self.fade = None;
            }
        }
        match self.wait {
            Wait::Frames(n) | Wait::Anims(n) => {
                self.wait = match self.wait {
                    _ if n <= 1 => Wait::None,
                    Wait::Frames(_) => Wait::Frames(n - 1),
                    _ => Wait::Anims(n - 1),
                }
            }
            Wait::Fade if self.fade.is_none() => self.wait = Wait::None,
            Wait::Click if self.skip && self.frames.is_multiple_of(3) => self.advance(),
            _ => {}
        }
        let mut steps = 0;
        while self.wait == Wait::None && self.view == View::Game {
            self.step()?;
            steps += 1;
            if steps > 500_000 {
                self.log("the script runs without waiting");
                break;
            }
        }
        self.last_scene = self.scene_draws();
        Ok(())
    }

    fn advance(&mut self) {
        match self.wait {
            Wait::Click => {
                self.stop("voice", 0.0);
                self.wait = Wait::None;
            }
            Wait::Fade => {
                self.fade = None;
                self.wait = Wait::None;
            }
            Wait::Anims(_) => {
                self.finish_anims();
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

    /// The button under (x, y) of an active group.
    fn button_at(&self, x: i32, y: i32) -> Option<usize> {
        self.groups
            .values()
            .filter(|g| g.active)
            .flat_map(|g| g.buttons.iter())
            .find(|b| b.on && inside(b.rect, x, y))
            .map(|b| b.handler)
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if self.backlog.is_open() {
            self.backlog.close();
            return;
        }
        self.pointer = (x, y);
        if let View::Slots { save } = self.view {
            self.slots_click(save, x, y);
            return;
        }
        if self.window
            && let Some(g) = self.win_group.and_then(|g| self.groups.get(&g))
            && let Some(b) = g.buttons.iter().find(|b| inside(b.rect, x, y))
        {
            match b.id {
                0 | 2 if self.wait == Wait::Click => self.view = View::Slots { save: true },
                1 | 3 => self.view = View::Slots { save: false },
                6 => self.skip = !self.skip,
                _ => {}
            }
            return;
        }
        if let Some(h) = self.button_at(x, y) {
            for g in self.groups.values_mut() {
                g.active = false;
            }
            self.stack.push(self.pc);
            self.pc = h;
            self.wait = Wait::None;
            return;
        }
        self.skip = false;
        self.advance();
    }

    /// A right click: our save slots (load only away from a message), or back.
    pub fn cancel(&mut self) {
        if self.backlog.is_open() {
            self.backlog.close();
            return;
        }
        self.view = match self.view {
            View::Game if self.wait == Wait::Click => View::Slots { save: true },
            View::Game => View::Slots { save: false },
            _ => View::Game,
        };
    }

    /// A key: 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip.
    pub fn key(&mut self, k: usize) {
        match k {
            4 if self.view == View::Game => {
                self.skip = false;
                self.advance();
            }
            5 => self.cancel(),
            6 => self.skip = !self.skip,
            _ => {}
        }
    }

    /// The wheel: down goes forward.
    pub fn wheel(&mut self, up: bool) {
        // turned up: the backlog
        if self.backlog.is_open() || up {
            self.log_page();
            if self.backlog.is_open()
                || (self.view == View::Game && matches!(self.wait, Wait::Click) && self.window)
            {
                self.backlog.wheel(up);
                return;
            }
        }
        if !up && self.view == View::Game && self.wait == Wait::Click {
            self.advance();
        }
    }

    // ---- drawing ----

    fn draw_pic(p: &Pic, x: f32, y: f32, alpha: f32, zoom: f32) -> Draw {
        let (x, y) = (x + p.ox, y + p.oy);
        let z = zoom / 100.0;
        Draw {
            image: p.img.clone(),
            src: p.src,
            dst: [x, y, x + p.w(), y + p.h()],
            place: Place {
                sx: z,
                sy: z,
                cx: x + p.xcenter.unwrap_or(p.w() / 2.0),
                cy: y + p.ycenter.unwrap_or(p.h() / 2.0),
                ..place(alpha)
            },
        }
    }

    /// The backgrounds, characters and layers.
    fn scene_draws(&self) -> Vec<Draw> {
        let mut out = Vec::new();
        if let Some((_, Some(p))) = &self.shown.bg {
            out.push(Self::draw_pic(p, p.x, p.y, 1.0, 100.0));
        }
        for c in &self.shown.chars {
            if let Some(p) = &c.pic {
                let mid = match c.place {
                    0 => -p.w(),
                    1 => -p.w() / 2.0,
                    2 => 200.0,
                    4 => 600.0,
                    5 => 800.0,
                    6 => 800.0 + p.w() / 2.0,
                    _ => 400.0,
                };
                let x = mid - p.xcenter.unwrap_or(p.w() / 2.0);
                let y = H as f32 - p.ycenter.unwrap_or(p.h());
                out.push(Self::draw_pic(p, x, y, 1.0, 100.0));
            }
        }
        for s in self.sprites.values() {
            if let Some(p) = &s.pic
                && s.alpha > 0.0
            {
                out.push(Self::draw_pic(p, s.x, s.y, s.alpha / 255.0, s.zoom));
            }
        }
        out
    }

    fn text_img(&mut self, s: &[u8], size: i32) -> Rc<Image> {
        let key = (s.to_vec(), size);
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let img = match &self.font {
            Some(f) => Rc::new(Font::outline(f.clone(), size).render(s, [255, 255, 255], 2)),
            None => Rc::new(Image::new(1, 1)),
        };
        if self.text_cache.len() > 300 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, img.clone());
        img
    }

    fn whole(img: &Rc<Image>, x: f32, y: f32, alpha: f32) -> Draw {
        Draw {
            image: img.clone(),
            src: [0, 0, img.w as i32, img.h as i32],
            dst: [x, y, x + img.w as f32, y + img.h as f32],
            place: place(alpha),
        }
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

    fn text_draw(&mut self, s: &str, size: i32, x: i32, y: i32, out: &mut Vec<Draw>) {
        let img = self.text_img(&nexas::sjis::encode(s), size);
        out.push(Self::whole(&img, x as f32, y as f32, 1.0));
    }

    fn named_pic(&mut self, name: &str) -> Option<Pic> {
        let i = self
            .names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))?;
        self.pic(i as i32 + 1)
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = self.draws_inner();
        if self.backlog.is_open() {
            let log = std::mem::take(&mut self.backlog);
            let (w, h) = (W, H);
            let size = (h as i32 / 27).max(16);
            out.extend(log.draws(w, h, size, &mut |s, rgb| {
                let _ = rgb;
                self.text_img(&nexas::sjis::encode(s), size)
            }));
            self.backlog = log;
        }
        out
    }

    /// Keeps the message waiting for a click in the backlog (vncommon::backlog).
    fn log_page(&mut self) {
        if !(self.view == View::Game && matches!(self.wait, Wait::Click) && self.window) {
            return;
        }
        let (name, text) = (
            nexas::sjis::decode(&self.name),
            nexas::sjis::decode(&self.msg),
        );
        self.backlog.push(name.trim(), &text);
    }

    fn draws_inner(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        let scene = self.scene_draws();
        match &self.fade {
            Some(f) => {
                let k = f.t as f32 / f.dur.max(1) as f32;
                out.extend(f.old.iter().cloned());
                out.extend(scene.into_iter().map(|mut d| {
                    d.place.alpha *= k;
                    d
                }));
            }
            None => out.extend(scene),
        }
        let (px, py) = self.pointer;
        for (k, g) in &self.groups {
            if !self.window && Some(*k) == self.win_group {
                continue;
            }
            let lit = g.active || Some(*k) == self.win_group;
            for b in &g.buttons {
                let Some(p) = &b.pic else { continue };
                let mut p = p.clone();
                // the lit button: the part right of it, when the picture has one
                if lit
                    && b.on
                    && inside(b.rect, px, py)
                    && p.src[2] + p.w() as i32 <= p.img.w as i32
                {
                    let w = p.w() as i32;
                    p.src[0] += w;
                    p.src[2] += w;
                }
                out.push(Self::draw_pic(&p, p.x, p.y, 1.0, 100.0));
            }
        }
        if self.window {
            self.window_draws(&mut out);
        }
        if let View::Slots { save } = self.view {
            self.slot_draws(save, &mut out);
        }
        out
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let name = self.win_pic.clone();
        if let Some(p) = self.named_pic(&name) {
            out.push(Self::draw_pic(&p, p.x, p.y, 1.0, 100.0));
        }
        if !self.name.is_empty() {
            let n = self.name.clone();
            let img = self.text_img(&n, TEXT_SIZE);
            out.push(Self::whole(&img, NAME_AT.0 as f32, NAME_AT.1 as f32, 1.0));
        }
        let (x, mut y) = TEXT_AT;
        for line in wrap(&self.msg.clone(), TEXT_COLS) {
            let img = self.text_img(&line, TEXT_SIZE);
            out.push(Self::whole(&img, x as f32, y as f32, 1.0));
            y += TEXT_LH;
        }
        if self.wait == Wait::Click
            && let Some(p) = self.named_pic("sys_click_anime")
        {
            let mut p = p.clone();
            let h = p.h() as i32;
            let frames = (p.img.h as i32 / h.max(1)).max(1);
            let f = (self.frames / 15) as i32 % frames;
            p.src[1] += f * h;
            p.src[3] += f * h;
            out.push(Self::draw_pic(&p, p.x, p.y, 1.0, 100.0));
        }
    }

    // ---- saves ----

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        (0..SLOTS as i32)
            .map(|i| {
                let (c, r) = (i % 2, i / 2);
                let (x, y) = (30 + c * 375, 100 + r * 94);
                [x, y, x + 360, y + 80]
            })
            .collect()
    }

    /// The SAVE and LOAD tabs.
    fn tab_rects() -> [[i32; 4]; 2] {
        [[30, 30, 150, 70], [170, 30, 290, 70]]
    }

    fn slots_click(&mut self, save: bool, x: i32, y: i32) {
        let tabs = Self::tab_rects();
        if inside(tabs[0], x, y) {
            if self.wait == Wait::Click {
                self.view = View::Slots { save: true };
            }
            return;
        }
        if inside(tabs[1], x, y) {
            self.view = View::Slots { save: false };
            return;
        }
        match self.slot_rects().iter().position(|r| inside(*r, x, y)) {
            Some(i) if save => {
                self.save(i);
                self.view = View::Game;
            }
            Some(i) => {
                if self.load(i) {
                    self.view = View::Game;
                }
            }
            None => self.view = View::Game,
        }
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        out.push(Self::solid(
            [40, 10, 30, 255],
            0.88,
            [0, 0, W as i32, H as i32],
        ));
        let (px, py) = self.pointer;
        for (i, (r, label)) in Self::tab_rects()
            .into_iter()
            .zip(["SAVE", "LOAD"])
            .enumerate()
        {
            let on = save == (i == 0);
            let c = if on {
                [255, 39, 128, 255]
            } else {
                [90, 30, 60, 255]
            };
            out.push(Self::solid(c, 0.95, r));
            self.text_draw(label, 24, r[0] + 30, r[1] + 7, out);
        }
        for (i, r) in self.slot_rects().into_iter().enumerate() {
            let c = if inside(r, px, py) {
                [255, 39, 128, 255]
            } else {
                [120, 40, 80, 255]
            };
            out.push(Self::solid(c, 0.9, r));
            let label = match self.read_slot(i) {
                Some(s) => format!("{:02}  {}", i + 1, s.label),
                None => format!("{:02}  ----", i + 1),
            };
            self.text_draw(&label, 18, r[0] + 12, r[1] + 28, out);
        }
    }

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&format!("slot{i:02}.sav"))?;
        Slot::parse(&String::from_utf8(b).ok()?)
    }

    fn save(&mut self, i: usize) {
        if self.wait != Wait::Click {
            return;
        }
        let t: String = nexas::sjis::decode(&self.msg).chars().take(12).collect();
        let mut s = Slot {
            label: format!("{} {t}", self.chapter).trim().to_string(),
            pc: self.pc,
            stack: self.stack.clone(),
            vars: self.vars.clone(),
            ret: self.ret,
            bg: self.shown.bg.as_ref().map(|b| b.0),
            chars: self.shown.chars.iter().map(|c| (c.file, c.place)).collect(),
            sprites: Vec::new(),
            next_layer: self.next_layer,
            bgm: self.bgm.clone(),
            msg: self.msg_offs,
            win: self.win_pic.clone(),
            chapter: self.chapter.clone(),
        };
        // animations end where they go
        self.finish_anims();
        s.sprites = self
            .sprites
            .iter()
            .map(|(&l, sp)| (l, sp.file, sp.x, sp.y, sp.alpha, sp.zoom))
            .collect();
        self.files
            .write_save(&format!("slot{i:02}.sav"), s.write().as_bytes());
    }

    fn load(&mut self, i: usize) -> bool {
        let Some(s) = self.read_slot(i) else {
            return false;
        };
        self.restart();
        self.pc = s.pc;
        self.stack = s.stack;
        self.vars = s.vars;
        self.ret = s.ret;
        if let Some(b) = s.bg {
            let pic = self.pic(b);
            self.stage.bg = Some((b, pic));
        }
        for (file, place) in s.chars {
            let pic = self.pic(file);
            self.stage.chars.push(Chara { file, place, pic });
        }
        self.shown = self.stage.clone();
        for (l, file, x, y, alpha, zoom) in s.sprites {
            let pic = self.pic(file);
            self.sprites.insert(
                l,
                Sprite {
                    file,
                    pic,
                    x,
                    y,
                    alpha,
                    zoom,
                },
            );
        }
        self.next_layer = s.next_layer;
        if let Some(b) = s.bgm {
            self.play_bgm(&b, true);
        }
        self.msg_offs = s.msg;
        self.name = self.text_at(s.msg.0);
        self.msg = self.text_at(s.msg.1);
        self.win_pic = s.win;
        self.chapter = s.chapter;
        self.window = true;
        self.wait = Wait::Click;
        true
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

/// Splits Shift_JIS text into lines of at most `n` characters (closing marks stay with the
/// line before).
fn wrap(s: &[u8], n: usize) -> Vec<Vec<u8>> {
    let chars = nexas::sjis::chars(s);
    let closing = |c: &[u8]| {
        let d = nexas::sjis::decode(c);
        d.chars()
            .all(|ch| "」』）。、！？…―ー〜っゃゅょ".contains(ch))
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let mut end = (i + n).min(chars.len());
        while end < chars.len() && closing(chars[end]) && end - i < n + 3 {
            end += 1;
        }
        out.push(chars[i..end].concat());
        i = end;
    }
    out
}

/// A save, as lines of `key value`.
struct Slot {
    label: String,
    pc: usize,
    stack: Vec<usize>,
    vars: BTreeMap<(u8, i32), i32>,
    ret: i32,
    bg: Option<i32>,
    chars: Vec<(i32, i32)>,
    sprites: Vec<(i32, i32, f32, f32, f32, f32)>,
    next_layer: i32,
    bgm: Option<String>,
    msg: (i32, i32),
    win: String,
    chapter: String,
}

impl Slot {
    fn write(&self) -> String {
        let mut s = String::from("PJADVSAV1\n");
        s += &format!("label {}\n", self.label.replace('\n', " "));
        s += &format!("pc {}\nret {}\n", self.pc, self.ret);
        let st: Vec<String> = self.stack.iter().map(|p| p.to_string()).collect();
        s += &format!("stack {}\n", st.join(" "));
        for ((t, i), v) in &self.vars {
            s += &format!("var {t} {i} {v}\n");
        }
        if let Some(b) = self.bg {
            s += &format!("bg {b}\n");
        }
        for (f, p) in &self.chars {
            s += &format!("ch {f} {p}\n");
        }
        for (l, f, x, y, a, z) in &self.sprites {
            s += &format!("spr {l} {f} {x} {y} {a} {z}\n");
        }
        s += &format!("next {}\n", self.next_layer);
        if let Some(b) = &self.bgm {
            s += &format!("bgm {b}\n");
        }
        s += &format!("msg {} {}\nwin {}\n", self.msg.0, self.msg.1, self.win);
        s += &format!("chapter {}\n", self.chapter);
        s
    }

    fn parse(t: &str) -> Option<Slot> {
        let mut lines = t.lines();
        if lines.next()? != "PJADVSAV1" {
            return None;
        }
        let mut s = Slot {
            label: String::new(),
            pc: 0,
            stack: Vec::new(),
            vars: BTreeMap::new(),
            ret: 0,
            bg: None,
            chars: Vec::new(),
            sprites: Vec::new(),
            next_layer: FIRST_FREE_LAYER,
            bgm: None,
            msg: (0, 0),
            win: "sys_mainwin".into(),
            chapter: String::new(),
        };
        for l in lines {
            let (k, rest) = l.split_once(' ').unwrap_or((l, ""));
            let nums: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|v| v.parse().ok())
                .collect();
            let n = |i: usize| nums.get(i).copied();
            match k {
                "label" => s.label = rest.to_string(),
                "pc" => s.pc = n(0)? as usize,
                "ret" => s.ret = n(0)? as i32,
                "stack" => s.stack = nums.iter().map(|&v| v as usize).collect(),
                "var" => {
                    s.vars.insert((n(0)? as u8, n(1)? as i32), n(2)? as i32);
                }
                "bg" => s.bg = Some(n(0)? as i32),
                "ch" => s.chars.push((n(0)? as i32, n(1)? as i32)),
                "spr" => s.sprites.push((
                    n(0)? as i32,
                    n(1)? as i32,
                    n(2)? as f32,
                    n(3)? as f32,
                    n(4)? as f32,
                    n(5)? as f32,
                )),
                "next" => s.next_layer = n(0)? as i32,
                "bgm" => s.bgm = Some(rest.to_string()),
                "msg" => s.msg = (n(0)? as i32, n(1)? as i32),
                "win" => s.win = rest.to_string(),
                "chapter" => s.chapter = rest.to_string(),
                _ => {}
            }
        }
        (s.pc > 0).then_some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_with_closing_marks() {
        let s = nexas::sjis::encode("あいう。");
        let lines = wrap(&s, 3);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn slot_round_trip() {
        let mut vars = BTreeMap::new();
        vars.insert((2, 0x96), -3);
        let s = Slot {
            label: "x".into(),
            pc: 0x15e4,
            stack: vec![1, 2],
            vars,
            ret: 0,
            bg: Some(5),
            chars: vec![(7, 3)],
            sprites: vec![(1, 2, 3.0, -4.0, 255.0, 100.0)],
            next_layer: 4097,
            bgm: Some("BGM003".into()),
            msg: (1, 2),
            win: "sys_mainwin".into(),
            chapter: String::new(),
        };
        let p = Slot::parse(&s.write()).unwrap();
        assert_eq!(p.pc, 0x15e4);
        assert_eq!(p.vars.get(&(2, 0x96)), Some(&-3));
        assert_eq!(p.sprites.len(), 1);
        assert_eq!(p.stack, vec![1, 2]);
    }
}
