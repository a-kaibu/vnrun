//! Runs the AGE scripts a frame at a time. The game is all in its scripts (the title, the
//! menus, the message window and the maps), so this is the machine they need: variables,
//! pictures (textures) and the sprites that show parts of them, text windows, sounds, input
//! callbacks and saved settings.
//!
//! Memory: global ints (`g`), global strings (`gs`), and per running script its locals (`l`,
//! `ls`) and pointer slots (`lp`, `lsp`) that `lookup-array` fills. Sprites are drawn in the
//! order of their numbers.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::gfx::{Draw, Image, Place};

use crate::alf::Index;
use crate::script::{Arg, Script};

mod save;

pub struct Sound {
    pub data: Vec<u8>,
    pub volume: f32,
    /// 0: music, 1: voice, 2..: effects
    pub channel: i32,
    pub stop: bool,
    pub fade_ms: u32,
    pub looped: bool,
}

const FRAME_MS: f64 = 1000.0 / 60.0;
const STEPS: usize = 400_000;
const BGM: i32 = 0;
const VOICE: i32 = 1;
const SE: i32 = 2;
/// the callback the input loops run once a tick (mouse)
/// mouse callback ids with this bit run once per input poll (0x10 and 0x32 are seen)
const TICK: i32 = 0x10;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ptr {
    G(u32),
    /// a local of the frame at this depth
    L(usize, u32),
    Gs(u32),
    Ls(usize, u32),
    Null,
}

impl Ptr {
    fn add(self, n: i32) -> Ptr {
        let a = |v: u32| v.wrapping_add(n as u32);
        match self {
            Ptr::G(v) => Ptr::G(a(v)),
            Ptr::L(d, v) => Ptr::L(d, a(v)),
            Ptr::Gs(v) => Ptr::Gs(a(v)),
            Ptr::Ls(d, v) => Ptr::Ls(d, a(v)),
            Ptr::Null => Ptr::Null,
        }
    }
}

struct Frame {
    script: Rc<Script>,
    pc: usize,
    ret: Vec<usize>,
    l: Vec<i32>,
    ls: HashMap<u32, String>,
    lp: HashMap<u32, Ptr>,
    /// mouse callbacks by id (ids with the TICK bit: once per input poll) and pad/key callbacks
    mouse: HashMap<i32, u32>,
    joy: HashMap<i32, u32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Wait {
    None,
    Until(f64),
    Click,
    Anims,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum AnimKind {
    Alpha,
    Scale,
    Move,
    /// the offset a 1ff gives, added to the position
    Offset,
}

/// A change of a sprite over time; it starts from the value the sprite has when it begins.
#[derive(Clone, Copy, Debug)]
struct Anim {
    kind: AnimKind,
    start: f64,
    ms: f64,
    from: Option<[f32; 2]>,
    to: [f32; 2],
}

impl Sprite {
    fn get(&self, k: AnimKind) -> [f32; 2] {
        match k {
            AnimKind::Alpha => [self.alpha, 0.0],
            AnimKind::Scale => self.scale,
            AnimKind::Move => [self.x, self.y],
            AnimKind::Offset => self.off,
        }
    }

    fn put(&mut self, k: AnimKind, v: [f32; 2]) {
        match k {
            AnimKind::Alpha => self.alpha = v[0],
            AnimKind::Scale => self.scale = v,
            AnimKind::Move => (self.x, self.y) = (v[0], v[1]),
            AnimKind::Offset => self.off = v,
        }
    }
}

#[derive(Clone, Debug)]
struct Sprite {
    tex: i32,
    src: [i32; 4],
    x: f32,
    y: f32,
    alpha: f32,
    add: bool,
    /// the colour the picture is multiplied by (0xffffff: as it is)
    tint: i32,
    pivot: (f32, f32),
    scale: [f32; 2],
    /// added to the position (1ff; a 220 then slides it, often back to 0)
    off: [f32; 2],
    anims: Vec<Anim>,
}

#[derive(Clone, Debug, Default)]
struct Window {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    tx: i32,
    ty: i32,
    right: i32,
    lines: Vec<String>,
    /// the letters as they were when the text came: size, colour, shadow
    size: f32,
    rgb: [u8; 3],
    shadow: [u8; 3],
}

pub struct Engine {
    files: Box<dyn Files>,
    index: Index,
    /// the folder's file names by lower-case name
    arcs: HashMap<String, String>,
    scripts: HashMap<usize, Rc<Script>>,
    g: Vec<i32>,
    gs: HashMap<u32, String>,
    gp: HashMap<u32, Ptr>,
    frames: Vec<Frame>,
    textures: HashMap<i32, Rc<Image>>,
    /// how each texture was made, for saves
    recipes: HashMap<i32, Rc<Vec<TexOp>>>,
    /// our save/load menu (Escape), over the game
    menu: Option<save::Menu>,
    /// the music playing (a file number), for saves
    bgm: Option<i32>,
    /// GAMEVERSION from the ini's settings (2eb), "1.00.0000"
    version: String,
    /// number fonts (2da): texture, where 0 is (x y), a digit's size, the space between
    digit_fonts: HashMap<i32, (i32, [i32; 4], i32)>,
    sprites: BTreeMap<i32, Sprite>,
    windows: HashMap<i32, Window>,
    window: i32,
    font: Option<Rc<fontdue::Font>>,
    extra_font: Option<Rc<fontdue::Font>>,
    font_size: f32,
    text_rgb: [u8; 3],
    shadow_rgb: [u8; 3],
    text_cache: HashMap<TextKey, Rc<Image>>,
    tinted: HashMap<(u32, i32), Rc<Image>>,
    now: f64,
    wait: Wait,
    pointer: (i32, i32),
    /// the mouse buttons as the scripts read them, and ticks left until a click lets go
    buttons: i32,
    release: u32,
    clicked: bool,
    keys: Vec<i32>,
    sounds: Vec<Sound>,
    persist: HashMap<u32, i32>,
    switch: Vec<(i32, u32)>,
    ended: bool,
    pub screen: (u32, u32),
    /// a SYS5 game (sys5ini.bin, UTF-16 scripts)
    sys5: bool,
    pub log: bool,
    pub unknown: BTreeMap<u32, u32>,
    pub text_log: Vec<String>,
}

/// A rendered line: text, size, colour and shadow colour.
type TextKey = (String, u32, [u8; 3], [u8; 3]);

fn rgb(c: i32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

/// How a texture was made, so that a save can make it again: from a file or blank, then
/// filled, written on and drawn over; `Pixels` is the picture itself.
#[derive(Clone, Debug)]
enum TexOp {
    File(i32),
    New(u32, u32),
    Pixels(Rc<Image>),
    /// x, y, w, h; alpha; colour
    Fill([i32; 4], i32, i32),
    /// the source as it was made, its part (x, y, w, h) and where it goes
    Blit(Rc<Vec<TexOp>>, [i32; 4], i32, i32),
    Text {
        x: i32,
        y: i32,
        s: String,
        size: f32,
        rgb: [u8; 3],
        shadow: [u8; 3],
    },
}

/// Fills x, y, w, h of a texture with a colour and alpha.
fn fill(t: &mut Image, r: [i32; 4], al: i32, c: i32) {
    t.renew();
    let [x, y, w, h] = r;
    let c = rgb(c);
    for yy in y.max(0)..(y + h).min(t.h as i32) {
        for xx in x.max(0)..(x + w).min(t.w as i32) {
            let i = ((yy as u32 * t.w + xx as u32) * 4) as usize;
            t.px[i..i + 3].copy_from_slice(&c);
            t.px[i + 3] = al.clamp(0, 255) as u8;
        }
    }
}

/// Draws a rendered line of text on a texture, its letters' cell at (x, y).
fn put_text(t: &mut Image, line: &Image, x: i32, y: i32) {
    t.renew();
    let p = Place {
        x: 0.0,
        y: 0.0,
        sx: 1.0,
        sy: 1.0,
        cx: 0.0,
        cy: 0.0,
        rot: 0.0,
        alpha: 1.0,
        add: false,
        flip: (false, false),
        clip: [0.0, 0.0, t.w as f32, t.h as f32],
    };
    let (w, h) = (line.w as f32, line.h as f32);
    let (x, y) = (
        x as f32 - crate::text::PAD as f32,
        y as f32 - crate::text::PAD as f32,
    );
    let src = [0, 0, line.w as i32, line.h as i32];
    nexas::gfx::draw(t, line, src, [x, y, x + w, y + h], &p);
}

/// Draws `src` (x, y, w, h of it) over `dst` at (dx, dy), by the source's alpha.
/// A setting at the end of sys4ini.bin/sys5ini.bin ("GAMEVERSION" -> "1.00.0000"): the key
/// and its value follow each other as zero-ended strings (UTF-16 in SYS5).
fn ini_setting(ini: &[u8], key: &str) -> Option<String> {
    let wide: Vec<u8> = key
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    if let Some(i) = find(ini, &wide) {
        let units: Vec<u16> = ini[i + wide.len()..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    let narrow: Vec<u8> = key.bytes().chain([0]).collect();
    let i = find(ini, &narrow)?;
    let rest = &ini[i + narrow.len()..];
    let end = rest.iter().position(|&b| b == 0)?;
    Some(String::from_utf8_lossy(&rest[..end]).into_owned())
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

fn blit(dst: &mut Image, src: &Image, r: [i32; 4], dx: i32, dy: i32) {
    dst.renew();
    let [sx, sy, w, h] = r;
    for y in 0..h {
        let (ys, yd) = (sy + y, dy + y);
        if ys < 0 || yd < 0 || ys >= src.h as i32 || yd >= dst.h as i32 {
            continue;
        }
        for x in 0..w {
            let (xs, xd) = (sx + x, dx + x);
            if xs < 0 || xd < 0 || xs >= src.w as i32 || xd >= dst.w as i32 {
                continue;
            }
            let i = ((ys as u32 * src.w + xs as u32) * 4) as usize;
            let o = ((yd as u32 * dst.w + xd as u32) * 4) as usize;
            let a = src.px[i + 3] as u32;
            if a == 0 {
                continue;
            }
            let b = dst.px[o + 3] as u32 * (255 - a) / 255;
            let out = a + b;
            for k in 0..3 {
                dst.px[o + k] = ((src.px[i + k] as u32 * a + dst.px[o + k] as u32 * b) / out) as u8;
            }
            dst.px[o + 3] = out.min(255) as u8;
        }
    }
}

impl Engine {
    pub fn new(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let names = files.list();
        let ini = names
            .iter()
            .find(|n| {
                n.eq_ignore_ascii_case("sys4ini.bin") || n.eq_ignore_ascii_case("sys5ini.bin")
            })
            .ok_or("no sys4ini.bin or sys5ini.bin")?
            .clone();
        let mut index = Index::default();
        let ini_bytes = files.read(&ini).ok_or("sys4ini.bin: unreadable")?;
        index.add(&ini_bytes)?;
        let version = ini_setting(&ini_bytes, "GAMEVERSION").unwrap_or_default();
        // append indexes (APPEND01.AAI ...) add or replace files
        let mut appends: Vec<&String> = names
            .iter()
            .filter(|n| n.to_ascii_lowercase().ends_with(".aai"))
            .collect();
        appends.sort();
        for a in appends {
            if let Some(b) = files.read(a) {
                let _ = index.add(&b);
            }
        }
        let arcs = names
            .iter()
            .map(|n| (n.to_ascii_lowercase(), n.clone()))
            .collect();
        let mut e = Engine {
            files,
            index,
            arcs,
            scripts: HashMap::new(),
            g: Vec::new(),
            gs: HashMap::new(),
            gp: HashMap::new(),
            frames: Vec::new(),
            textures: HashMap::new(),
            recipes: HashMap::new(),
            menu: None,
            bgm: None,
            version,
            digit_fonts: HashMap::new(),
            sprites: BTreeMap::new(),
            windows: HashMap::new(),
            window: 1,
            font: None,
            extra_font: None,
            font_size: 24.0,
            text_rgb: [255; 3],
            shadow_rgb: [0x60; 3],
            text_cache: HashMap::new(),
            tinted: HashMap::new(),
            now: 0.0,
            wait: Wait::None,
            pointer: (-1, -1),
            buttons: 0,
            release: 0,
            clicked: false,
            keys: Vec::new(),
            sounds: Vec::new(),
            persist: HashMap::new(),
            switch: Vec::new(),
            ended: false,
            screen: (800, 600),
            sys5: ini.eq_ignore_ascii_case("sys5ini.bin"),
            log: false,
            unknown: BTreeMap::new(),
            text_log: Vec::new(),
        };
        if let Some(b) = e.files.read_save("system.dat") {
            for c in b.chunks_exact(8) {
                let k = u32::from_le_bytes(c[..4].try_into().unwrap());
                let v = i32::from_le_bytes(c[4..].try_into().unwrap());
                e.persist.insert(k, v);
            }
        }
        // SYS5 games carry the letters their own fonts lack (U+E000 a dash, ...)
        if let Some(b) = e.index.find("AGE-EXTEND.TTF").and_then(|n| e.read_file(n)) {
            e.extra_font = fontdue::Font::from_bytes(b, fontdue::FontSettings::default())
                .ok()
                .map(Rc::new);
        }
        let start = e
            .index
            .find("SYSTEM4.BIN")
            .or_else(|| e.index.find("SYSTEM5.BIN"))
            .ok_or("no SYSTEM4.BIN or SYSTEM5.BIN")?;
        e.push_script(start)?;
        // the screen is as large as the largest window SYSTEM4 opens at the top left
        // (800x600 in the older games, 1280x720 in Sankaio)
        let first = e.frames[0].script.clone();
        for i in first
            .insns
            .iter()
            .filter(|i| i.op == 0x70 && i.args.len() == 5)
        {
            let a: Vec<u32> = i
                .args
                .iter()
                .map(|x| if x.t == 0 { x.v } else { u32::MAX })
                .collect();
            if a[3] == 0
                && a[4] == 0
                && (640..=4096).contains(&a[1])
                && (480..=4096).contains(&a[2])
            {
                e.screen.0 = e.screen.0.max(a[1]);
                e.screen.1 = e.screen.1.max(a[2]);
            }
        }
        Ok(e)
    }

    pub fn set_font(&mut self, b: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(b, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        self.font = Some(Rc::new(f));
        Ok(())
    }

    fn read_file(&mut self, n: usize) -> Option<Vec<u8>> {
        let e = self.index.get(n)?.clone();
        // the archive as the folder spells it
        let arc = self.arcs.get(&e.arc.to_ascii_lowercase()).unwrap_or(&e.arc);
        self.files.read_range(arc, e.offset, e.size)
    }

    fn file_name(&self, n: usize) -> String {
        self.index
            .get(n)
            .map_or_else(|| format!("#{n:x}"), |e| e.name.clone())
    }

    fn script(&mut self, n: usize) -> Result<Rc<Script>, String> {
        if let Some(s) = self.scripts.get(&n) {
            return Ok(s.clone());
        }
        let name = self.file_name(n);
        let b = self
            .read_file(n)
            .ok_or_else(|| format!("script {n:#x}: unreadable"))?;
        let s = Rc::new(Script::parse(&name, &b)?);
        self.scripts.insert(n, s.clone());
        Ok(s)
    }

    fn push_script(&mut self, n: usize) -> Result<(), String> {
        let script = self.script(n)?;
        if self.log {
            eprintln!("-> {}", script.name);
        }
        self.frames.push(Frame {
            script,
            pc: 0,
            ret: Vec::new(),
            l: Vec::new(),
            ls: HashMap::new(),
            lp: HashMap::new(),
            mouse: HashMap::new(),
            joy: HashMap::new(),
        });
        Ok(())
    }

    // ---- variables ----

    fn depth(&self) -> usize {
        self.frames.len().saturating_sub(1)
    }

    fn read_ptr(&self, p: Ptr) -> i32 {
        match p {
            Ptr::G(v) => self.g.get(v as usize).copied().unwrap_or(0),
            Ptr::L(d, v) => self
                .frames
                .get(d)
                .and_then(|f| f.l.get(v as usize))
                .copied()
                .unwrap_or(0),
            Ptr::Gs(_) | Ptr::Ls(..) => self.read_str_ptr(p).parse().unwrap_or(0),
            Ptr::Null => 0,
        }
    }

    fn write_ptr(&mut self, p: Ptr, x: i32) {
        match p {
            Ptr::G(v) => {
                let v = v as usize;
                if v >= 1 << 24 {
                    return;
                }
                if v >= self.g.len() {
                    self.g.resize(v + 1, 0);
                }
                self.g[v] = x;
            }
            Ptr::L(d, v) => {
                let v = v as usize;
                if v >= 1 << 20 {
                    return;
                }
                if let Some(f) = self.frames.get_mut(d) {
                    if v >= f.l.len() {
                        f.l.resize(v + 1, 0);
                    }
                    f.l[v] = x;
                }
            }
            Ptr::Gs(_) | Ptr::Ls(..) => self.write_str_ptr(p, x.to_string()),
            Ptr::Null => {}
        }
    }

    fn read_str_ptr(&self, p: Ptr) -> String {
        match p {
            Ptr::Gs(v) => self.gs.get(&v).cloned().unwrap_or_default(),
            Ptr::Ls(d, v) => self
                .frames
                .get(d)
                .and_then(|f| f.ls.get(&v))
                .cloned()
                .unwrap_or_default(),
            _ => self.read_ptr(p).to_string(),
        }
    }

    fn write_str_ptr(&mut self, p: Ptr, s: String) {
        match p {
            Ptr::Gs(v) => {
                self.gs.insert(v, s);
            }
            Ptr::Ls(d, v) => {
                if let Some(f) = self.frames.get_mut(d) {
                    f.ls.insert(v, s);
                }
            }
            _ => {
                let n = s.trim().parse().unwrap_or(0);
                self.write_ptr(p, n);
            }
        }
    }

    /// Where an argument lives (for writes, and as the base of an array).
    fn place_of(&self, a: Arg) -> Ptr {
        let d = self.depth();
        match a.t {
            3 | 4 => Ptr::G(a.v),
            5 => Ptr::Gs(a.v),
            9 | 0xa => Ptr::L(d, a.v),
            0xb => Ptr::Ls(d, a.v),
            6 | 8 => self.gp.get(&a.v).copied().unwrap_or(Ptr::Null),
            0xc..=0xe => self
                .frames
                .get(d)
                .and_then(|f| f.lp.get(&a.v))
                .copied()
                .unwrap_or(Ptr::Null),
            _ => Ptr::Null,
        }
    }

    fn val(&self, a: Arg) -> i32 {
        match a.t {
            0 => a.v as i32,
            1 => f32::from_bits(a.v) as i32,
            2 => self.lit(a).trim().parse().unwrap_or(0),
            _ => self.read_ptr(self.place_of(a)),
        }
    }

    fn lit(&self, a: Arg) -> String {
        self.frames
            .last()
            .and_then(|f| f.script.strings.get(&a.v))
            .cloned()
            .unwrap_or_default()
    }

    fn text(&self, a: Arg) -> String {
        match a.t {
            2 => self.lit(a),
            0 | 1 => self.val(a).to_string(),
            _ => self.read_str_ptr(self.place_of(a)),
        }
    }

    fn set(&mut self, a: Arg, x: i32) {
        let p = self.place_of(a);
        self.write_ptr(p, x);
    }

    fn set_text(&mut self, a: Arg, s: String) {
        let p = self.place_of(a);
        self.write_str_ptr(p, s);
    }

    /// Points a pointer slot (`lp`, `lsp`, ...) at `p`.
    fn set_slot(&mut self, a: Arg, p: Ptr) {
        match a.t {
            6 | 8 => {
                self.gp.insert(a.v, p);
            }
            0xc..=0xe => {
                if let Some(f) = self.frames.last_mut() {
                    f.lp.insert(a.v, p);
                }
            }
            _ => {
                let x = self.read_ptr(p);
                self.set(a, x);
            }
        }
    }

    // ---- running ----

    fn jump(&mut self, target: u32) {
        if let Some(f) = self.frames.last_mut() {
            match f.script.index.get(&target) {
                Some(&i) => f.pc = i,
                None => {
                    if self.log {
                        eprintln!("{}: no instruction at {target:#x}", f.script.name);
                    }
                }
            }
        }
    }

    fn call(&mut self, target: u32) {
        if let Some(f) = self.frames.last_mut() {
            f.ret.push(f.pc);
        }
        self.jump(target);
    }

    fn end_script(&mut self) {
        let f = self.frames.pop();
        if self.log
            && let Some(f) = f
        {
            eprintln!("<- {}", f.script.name);
        }
        if self.frames.is_empty() {
            self.ended = true;
        }
    }

    pub fn frame(&mut self) -> Result<(), String> {
        // the game stands still under our menu
        if self.menu.is_some() {
            return Ok(());
        }
        self.now += FRAME_MS;
        self.settle();
        if self.release > 0 {
            self.release -= 1;
            if self.release == 0 {
                self.buttons &= !1;
            }
        }
        match self.wait {
            Wait::Until(t) if self.now < t => return Ok(()),
            Wait::Click if !std::mem::take(&mut self.clicked) => return Ok(()),
            Wait::Anims if self.animating() => return Ok(()),
            _ => {}
        }
        self.wait = Wait::None;
        self.run()
    }

    fn animating(&self) -> bool {
        self.sprites
            .values()
            .any(|s| s.anims.iter().any(|a| self.now < a.start + a.ms))
    }

    fn run(&mut self) -> Result<(), String> {
        for _ in 0..STEPS {
            if self.ended || self.wait != Wait::None {
                return Ok(());
            }
            let Some(f) = self.frames.last_mut() else {
                self.ended = true;
                return Ok(());
            };
            let Some(insn) = f.script.insns.get(f.pc).cloned() else {
                self.end_script();
                continue;
            };
            f.pc += 1;
            if self.log {
                let name = &f.script.name;
                eprintln!("{name} {:05x} {:x} {:?}", insn.at, insn.op, insn.args);
            }
            self.exec(insn.op, &insn.args)?;
        }
        Ok(())
    }

    fn exec(&mut self, op: u32, a: &[Arg]) -> Result<(), String> {
        let v = |e: &Engine, i: usize| e.val(a[i]);
        match op {
            0x2 | 0x9 => self.end_script(),
            0x3 => {
                let n = v(self, 0) as usize;
                if let Err(err) = self.push_script(n)
                    && self.log
                {
                    eprintln!("call-script: {err}");
                }
            }
            0x5 => {
                let f = self.frames.last_mut().unwrap();
                match f.ret.pop() {
                    Some(pc) => f.pc = pc,
                    None => self.end_script(),
                }
            }
            0x50..=0x54 | 0x56..=0x5f => {
                let (x, y) = (v(self, 1), v(self, 2));
                let r = match op {
                    0x50 => x.wrapping_add(y),
                    0x51 => x.wrapping_sub(y),
                    0x52 => x.wrapping_mul(y),
                    0x53 => x.checked_div(y).unwrap_or(0),
                    0x54 => x.checked_rem(y).unwrap_or(0),
                    0x56 => x & y,
                    0x57 => x | y,
                    0x58 => x.wrapping_shr(y as u32),
                    0x59 => x.wrapping_shl(y as u32),
                    0x5a => (x == y) as i32,
                    0x5b => (x != y) as i32,
                    0x5c => (x < y) as i32,
                    0x5d => (x <= y) as i32,
                    0x5e => (x > y) as i32,
                    _ => (x >= y) as i32,
                };
                self.set(a[0], r);
            }
            0x55 => {
                let x = v(self, 1);
                self.set(a[0], x);
            }
            0x60 => {
                // negate / not? (dst, src)
                let x = v(self, 1);
                self.set(a[0], (x == 0) as i32);
            }
            0x61 => {
                let base = self.place_of(a[1]);
                let p = base.add(v(self, 2));
                self.set_slot(a[0], p);
            }
            0x12c => {
                let base = self.place_of(a[1]);
                let p = base.add(v(self, 2).wrapping_mul(v(self, 3)).wrapping_add(v(self, 4)));
                self.set_slot(a[0], p);
            }
            0x64 => {
                let words = self
                    .frames
                    .last()
                    .and_then(|f| f.script.arrays.get(&a[1].v).cloned())
                    .unwrap_or_default();
                let base = self.place_of(a[0]);
                for (i, w) in words.into_iter().enumerate() {
                    self.write_ptr(base.add(i as i32), w as i32);
                }
            }
            // 0x6c clears (start, count); 0x2d8 fills (start, value, count)
            0x6c | 0x2d8 => {
                let (x, n) = if op == 0x6c {
                    (0, v(self, 1))
                } else {
                    (v(self, 1), v(self, 2))
                };
                let base = self.place_of(a[0]);
                let mut n = n.clamp(0, 1 << 22);
                if let (0, Ptr::G(g)) = (x, base) {
                    // unwritten globals already read as 0
                    n = n.min((self.g.len() as i64 - g as i64).max(0) as i32);
                }
                for i in 0..n {
                    self.write_ptr(base.add(i), x);
                }
            }
            0x135 => {
                let x = v(self, 0) | (1 << (v(self, 1) & 31));
                self.set(a[0], x);
            }
            0x136 => {
                let x = v(self, 0) & !(1 << (v(self, 1) & 31));
                self.set(a[0], x);
            }
            0x13f => {
                let x = (v(self, 1) >> (v(self, 2) & 31)) & 1;
                self.set(a[0], x);
            }
            0x8c => self.jump(a[0].v),
            0x8f => self.call(a[0].v),
            0xa0 => {
                let c = v(self, 0) != 0;
                let t = if c { a[1].v } else { a[2].v };
                if t != u32::MAX {
                    self.jump(t);
                }
            }
            0xa1 => self.switch.clear(),
            0xa2 => self.switch.push((v(self, 0), a[1].v)),
            0xa3 => {
                let x = v(self, 0);
                let t = self
                    .switch
                    .iter()
                    .find(|c| c.0 == x)
                    .map_or(a[1].v, |c| c.1);
                self.switch.clear();
                if t != u32::MAX {
                    self.jump(t);
                }
            }
            0x192 => {
                let s = self.text(a[1]);
                self.set_text(a[0], s);
            }
            0x193 => {
                let s = self.text(a[1]) + &self.text(a[2]);
                self.set_text(a[0], s);
            }
            0x194 | 0x195 => {
                let eq = self.text(a[1]) == self.text(a[2]);
                self.set(a[0], (eq == (op == 0x194)) as i32);
            }
            0x1a6 => {
                let n = nexas::sjis::encode(&self.text(a[1])).len() as i32 / 2;
                self.set(a[0], n);
            }
            0x1c8 => {
                let s = v(self, 1).to_string();
                self.set_text(a[0], s);
            }
            0x1a3 => {
                // a value kept with the settings
                let key = self.persist_key(a[0]);
                if let Some(&x) = self.persist.get(&key) {
                    self.set(a[0], x);
                }
            }
            0x1a2 => {
                let key = self.persist_key(a[0]);
                let x = v(self, 0);
                if self.persist.insert(key, x) != Some(x) {
                    self.save_persist();
                }
            }
            0xc8 => {
                let ms = v(self, 0).max(1) as f64;
                self.wait = Wait::Until(self.now + ms - FRAME_MS / 2.0);
            }
            0xcc | 0xfb => {
                let id = v(self, 0);
                let at = a[1].v;
                let f = self.frames.last_mut().unwrap();
                let map = if op == 0xcc { &mut f.mouse } else { &mut f.joy };
                if at == u32::MAX {
                    map.remove(&id);
                } else {
                    map.insert(id, at);
                }
            }
            0xcd => self.poll_input(),
            0x108 => {
                let b = self.buttons;
                self.set(a[0], b);
            }
            0x109 => {
                let (x, y) = self.pointer;
                self.set(a[0], x);
                self.set(a[1], y);
            }
            0x10a => self.pointer = (v(self, 0), v(self, 1)),
            0x12e => {
                // hit test: dst, -, x, y, rects (x0 x1 y0 y1 each), xs, ys, count
                let (px, py) = (v(self, 2), v(self, 3));
                let n = v(self, 7).clamp(0, 4096);
                let (rects, xs, ys) = (
                    self.place_of(a[4]),
                    self.place_of(a[5]),
                    self.place_of(a[6]),
                );
                let mut hit = -1;
                for i in 0..n {
                    let r: Vec<i32> = (0..4)
                        .map(|k| self.read_ptr(rects.add(i * 4 + k)))
                        .collect();
                    let (bx, by) = (self.read_ptr(xs.add(i)), self.read_ptr(ys.add(i)));
                    if px >= bx + r[0] && px < bx + r[1] && py >= by + r[2] && py < by + r[3] {
                        hit = i;
                        break;
                    }
                }
                self.set(a[0], hit);
            }
            0x101 => {
                if self.animating() {
                    self.wait = Wait::Anims;
                }
            }
            // ---- sound ----
            0xbf => {
                let n = self.bgm_file(v(self, 0));
                self.play(n, BGM, true);
                self.bgm = Some(n);
            }
            0xc0 | 0xc1 => {
                self.stop(BGM, 500);
                self.bgm = None;
            }
            0xc4 => {
                let n = v(self, 0);
                self.play(n, VOICE, false);
            }
            0xb4 => {
                let n = v(self, 0);
                let looped = v(self, 1) == 2;
                self.play(n, SE, looped);
            }
            // ---- text ----
            0x70 => {
                let (ww, wh, wx, wy) = (v(self, 1), v(self, 2), v(self, 3), v(self, 4));
                let w = self.windows.entry(v(self, 0)).or_default();
                w.w = ww;
                w.h = wh;
                w.x = wx;
                w.y = wy;
            }
            0x71 => {
                let n = v(self, 0);
                self.windows.entry(n).or_default().lines.clear();
            }
            0x79 | 0x7a => {
                let (x, y) = (v(self, 1), v(self, 2));
                let w = self.windows.entry(v(self, 0)).or_default();
                w.tx = x;
                w.ty = y;
            }
            0x1c1 => {
                let r = v(self, 1);
                self.windows.entry(v(self, 0)).or_default().right = r;
            }
            0x75 => self.font_size = v(self, 0).clamp(8, 64) as f32,
            0x76 => self.text_rgb = rgb(v(self, 0)),
            0x77 => self.shadow_rgb = rgb(v(self, 0)),
            0x1b5 => self.window = v(self, 0).max(1),
            // SYS5's window for text: hyakusen's NOVEL.BIN picks the full-screen 3 and goes back
            // to 1, and the page it leaves goes away
            0x80 => {
                let n = v(self, 0).max(1);
                if n != self.window {
                    self.windows.entry(self.window).or_default().lines.clear();
                }
                self.window = n;
            }
            0x6e | 0x196 => {
                // display-furigana: window, base text, reading (the reading is not shown); a
                // window of 0 is the current one (SYS5 writes the speaker's name to 2)
                let n = v(self, 0);
                let win = if n > 0 { n } else { self.window };
                let s = self.text(a[1]);
                self.say(win, &s);
            }
            // end-text-line: the next text starts a new line (SYS5 breaks its lines this way)
            0x6f => {
                let w = self.windows.entry(self.window).or_default();
                if w.lines.last().is_some_and(|l| !l.is_empty()) {
                    w.lines.push(String::new());
                }
            }
            0x72 => {
                self.wait = Wait::Click;
                self.clicked = false;
            }
            // ---- pictures ----
            0x1f8 => {
                let (id, w, h) = (v(self, 0), v(self, 1), v(self, 2));
                let (w, h) = (w.clamp(1, 4096) as u32, h.clamp(1, 4096) as u32);
                self.textures.insert(id, Rc::new(Image::new(w, h)));
                self.recipes.insert(id, Rc::new(vec![TexOp::New(w, h)]));
            }
            0x1f9 | 0x20f => {
                let (file, id) = (v(self, 0), v(self, 1));
                self.load_texture(file, id);
            }
            0x1fa => {
                // SYS5 frees a texture a sprite still shows only with the sprite (the old
                // background stays up until the next one is built from its size)
                let id = v(self, 0);
                if !(self.sys5 && self.sprites.values().any(|s| s.tex == id)) {
                    self.textures.remove(&id);
                    self.recipes.remove(&id);
                }
            }
            0x1fb => {
                let id = v(self, 0);
                let s = Sprite {
                    tex: v(self, 1),
                    src: [v(self, 2), v(self, 3), v(self, 4), v(self, 5)],
                    x: v(self, 6) as f32,
                    y: v(self, 7) as f32,
                    alpha: 1.0,
                    add: false,
                    tint: 0xffffff,
                    pivot: (0.0, 0.0),
                    scale: [1.0, 1.0],
                    off: [0.0, 0.0],
                    anims: Vec::new(),
                };
                match self.sprites.get_mut(&id) {
                    Some(old) => {
                        old.tex = s.tex;
                        old.src = s.src;
                        old.x = s.x;
                        old.y = s.y;
                    }
                    None => {
                        self.sprites.insert(id, s);
                    }
                }
            }
            0x1f7 => {
                let (start, n) = (v(self, 0), v(self, 1).max(1));
                let gone: Vec<i32> = self
                    .sprites
                    .range(start..start.saturating_add(n))
                    .map(|(k, _)| *k)
                    .collect();
                for k in gone {
                    self.sprites.remove(&k);
                }
            }
            0x203 => {
                let id = v(self, 0);
                let (mode, alpha, tint) = (v(self, 1), v(self, 2), v(self, 3));
                if let Some(s) = self.sprites.get_mut(&id) {
                    s.add = mode == 1;
                    if alpha >= 0 {
                        s.alpha = alpha.min(255) as f32 / 255.0;
                    }
                    if tint >= 0 {
                        s.tint = tint & 0xffffff;
                    }
                }
            }
            0x202 => {
                let (id, delay, ms) = (v(self, 0), v(self, 1), v(self, 2));
                // -1: back to the alpha the sprite has now; the scripts follow it with a 203
                // that sets the alpha to start from (Sankaio's and Tenmei's message bar: 202 -1,
                // then 203 alpha 0, fades the bar in to the alpha its setup gave it)
                let to = match v(self, 3) {
                    x if x < 0 => self.sprites.get(&id).map_or(1.0, |s| s.alpha),
                    x => x.min(255) as f32 / 255.0,
                };
                let tint = v(self, 4);
                if tint >= 0
                    && let Some(s) = self.sprites.get_mut(&id)
                {
                    s.tint = tint & 0xffffff;
                }
                self.animate(id, AnimKind::Alpha, delay, ms, [to, 0.0]);
            }
            0x21e => {
                let (id, delay, ms) = (v(self, 0), v(self, 1), v(self, 2));
                let to = [v(self, 3) as f32 / 100.0, v(self, 4) as f32 / 100.0];
                self.animate(id, AnimKind::Scale, delay, ms, to);
            }
            0x220 => {
                let (id, delay, ms) = (v(self, 0), v(self, 1), v(self, 2));
                let to = [v(self, 3) as f32, v(self, 4) as f32];
                // after a 1ff it moves the offset (the title's slide-in: 1ff -10 20, then
                // 220 to 0 0); otherwise the position
                let kind = match self.sprites.get(&id) {
                    Some(s)
                        if s.off != [0.0, 0.0]
                            || s.anims.iter().any(|a| a.kind == AnimKind::Offset) =>
                    {
                        AnimKind::Offset
                    }
                    _ => AnimKind::Move,
                };
                self.animate(id, kind, delay, ms, to);
            }
            0x1ff => {
                let id = v(self, 0);
                let off = [v(self, 1) as f32, v(self, 2) as f32];
                if let Some(s) = self.sprites.get_mut(&id) {
                    s.anims.retain(|a| a.kind != AnimKind::Offset);
                    s.off = off;
                }
            }
            0x219 => {
                let id = v(self, 0);
                let (x, y) = (v(self, 1) as f32, v(self, 2) as f32);
                if let Some(s) = self.sprites.get_mut(&id) {
                    s.anims.retain(|a| a.kind != AnimKind::Move);
                    (s.x, s.y) = (x, y);
                }
            }
            0x217 => {
                let id = v(self, 0);
                let (x, y) = (v(self, 1) as f32, v(self, 2) as f32);
                if let Some(s) = self.sprites.get_mut(&id) {
                    s.pivot = (x, y);
                }
            }
            0x218 => {
                let id = v(self, 0);
                let p = self.sprites.get(&id).map_or((0.0, 0.0), |s| s.pivot);
                self.set(a[1], p.0 as i32);
                self.set(a[2], p.1 as i32);
                self.set(a[3], 0);
            }
            0x21a => {
                let id = v(self, 0);
                let p = self.sprites.get(&id).map_or((0.0, 0.0), |s| (s.x, s.y));
                self.set(a[1], p.0 as i32);
                self.set(a[2], p.1 as i32);
                self.set(a[3], 0);
            }
            0x215 => {
                let id = v(self, 1);
                let t = self.sprites.get(&id).map_or(-1, |s| s.tex);
                self.set(a[0], t);
            }
            0x208 => {
                let id = v(self, 0);
                let (w, h) = self
                    .textures
                    .get(&id)
                    .map_or((0, 0), |t| (t.w as i32, t.h as i32));
                self.set(a[1], w);
                self.set(a[2], h);
            }
            0x20b => {
                let id = v(self, 0);
                let (x, y, w, h) = (v(self, 1), v(self, 2), v(self, 3), v(self, 4));
                let (al, c) = (v(self, 5), v(self, 6));
                if let Some(t) = self.textures.get_mut(&id) {
                    fill(Rc::make_mut(t), [x, y, w, h], al, c);
                    self.record(id, TexOp::Fill([x, y, w, h], al, c));
                }
            }
            // draw a texture onto another: src, dst, sx, sy, w, h, dx, dy (SYS5 builds its
            // backgrounds from layers this way)
            0x207 => {
                let (src, dst) = (v(self, 0), v(self, 1));
                let r = [v(self, 2), v(self, 3), v(self, 4), v(self, 5)];
                let (dx, dy) = (v(self, 6), v(self, 7));
                if let Some(s) = self.textures.get(&src).cloned()
                    && let Some(t) = self.textures.get_mut(&dst)
                {
                    blit(Rc::make_mut(t), &s, r, dx, dy);
                    let from = self.recipes.get(&src).cloned();
                    let from = from.unwrap_or_else(|| Rc::new(vec![TexOp::Pixels(s)]));
                    self.record(dst, TexOp::Blit(from, r, dx, dy));
                }
            }
            0x2eb => self.set_text(a[0], self.version.clone()),
            0x2c7 => {
                // a part of a string: chars from start, len of them
                let s = self.text(a[1]);
                let (start, len) = (v(self, 2).max(0) as usize, v(self, 3).max(0) as usize);
                let part = s.chars().skip(start).take(len).collect();
                self.set_text(a[0], part);
            }
            0x2ec => {
                let n = self.text(a[1]).trim().parse().unwrap_or(0);
                self.set(a[0], n);
            }
            0x2da => {
                let r = [v(self, 2), v(self, 3), v(self, 4), v(self, 5)];
                self.digit_fonts
                    .insert(v(self, 0), (v(self, 1), r, v(self, 6)));
            }
            0x23b => {
                let (id, font, n) = (v(self, 0), v(self, 1), v(self, 2));
                let (x, y, digits, pad) = (v(self, 3), v(self, 4), v(self, 5), v(self, 6));
                self.draw_number(id, font, n, (x, y), digits, pad == 1);
            }
            0x204 => {
                let id = v(self, 0);
                let (x, y) = (v(self, 1), v(self, 2));
                let s = self.text(a[3]);
                self.draw_string(id, x, y, &s);
            }
            // display locks and other bookkeeping with nothing to show here
            0x1a7 | 0x1a8 | 0x1f4..=0x1f6 | 0x20c | 0x20e | 0x21c | 0x224 | 0x23c | 0x23d => {}
            _ => {
                *self.unknown.entry(op).or_default() += 1;
            }
        }
        Ok(())
    }

    fn persist_key(&self, a: Arg) -> u32 {
        match self.place_of(a) {
            Ptr::G(v) => v,
            Ptr::Gs(v) => 0x8000_0000 | v,
            _ => u32::MAX,
        }
    }

    fn save_persist(&mut self) {
        let mut b = Vec::new();
        let mut keys: Vec<_> = self.persist.iter().collect();
        keys.sort();
        for (k, v) in keys {
            b.extend(k.to_le_bytes());
            b.extend(v.to_le_bytes());
        }
        self.files.write_save("system.dat", &b);
    }

    fn poll_input(&mut self) {
        let depth = self.depth();
        let Some(f) = self.frames.get(depth) else {
            return;
        };
        let key = self.keys.first().copied();
        let target = match key.and_then(|k| f.joy.get(&k)) {
            Some(&t) => {
                self.keys.remove(0);
                Some(t)
            }
            None => {
                if key.is_some() {
                    self.keys.remove(0);
                }
                f.mouse
                    .iter()
                    .filter(|(id, _)| *id & TICK != 0)
                    .map(|(_, &t)| t)
                    .min()
            }
        };
        if let Some(t) = target {
            self.call(t);
        }
    }

    fn load_texture(&mut self, file: i32, id: i32) {
        if let Some(img) = self.decode_file(file) {
            self.textures.insert(id, Rc::new(img));
            self.recipes.insert(id, Rc::new(vec![TexOp::File(file)]));
        }
    }

    fn decode_file(&mut self, file: i32) -> Option<Image> {
        let name = self.file_name(file as usize);
        match self
            .read_file(file as usize)
            .map(|b| crate::agf::decode(&b))
        {
            Some(Ok(img)) => Some(img),
            Some(Err(e)) => {
                if self.log {
                    eprintln!("{name}: {e}");
                }
                None
            }
            None => {
                if self.log {
                    eprintln!("no file {file:#x}");
                }
                None
            }
        }
    }

    /// Notes a change to a texture in the list of how it was made (saves make it again from
    /// that); a long list becomes the picture itself.
    fn record(&mut self, id: i32, op: TexOp) {
        let Some(t) = self.textures.get(&id).cloned() else {
            return;
        };
        let Some(r) = self.recipes.get_mut(&id) else {
            self.recipes.insert(id, Rc::new(vec![TexOp::Pixels(t)]));
            return;
        };
        let r = Rc::make_mut(r);
        r.push(op);
        if r.len() > 48 {
            *r = vec![TexOp::Pixels(t)];
        }
    }

    /// Makes a texture again from how it was made.
    fn build(&mut self, ops: &[TexOp]) -> Option<Image> {
        let mut img: Option<Image> = None;
        for op in ops {
            match op {
                TexOp::File(n) => img = self.decode_file(*n),
                TexOp::New(w, h) => img = Some(Image::new(*w, *h)),
                TexOp::Pixels(p) => img = Some((**p).clone()),
                TexOp::Fill(r, al, c) => {
                    if let Some(t) = img.as_mut() {
                        fill(t, *r, *al, *c);
                    }
                }
                TexOp::Blit(src, r, dx, dy) => {
                    if let (Some(s), true) = (self.build(src), img.is_some()) {
                        blit(img.as_mut().unwrap(), &s, *r, *dx, *dy);
                    }
                }
                TexOp::Text {
                    x,
                    y,
                    s,
                    size,
                    rgb,
                    shadow,
                } => {
                    if let Some(line) = self.line_image(s, *size, *rgb, *shadow)
                        && let Some(t) = img.as_mut()
                    {
                        put_text(t, &line, *x, *y);
                    }
                }
            }
        }
        img
    }

    /// The file of a play-bgm number: SYS4 gives the file's number, SYS5 the track's
    /// (36: BGM036.WAV, or .OGG/.XWM).
    /// bf's track number names BGMnnn (b2009: bf 23 is BGM023.OGG, file 23 is HISTORY.BIN);
    /// a game without such names gets the file of that number when it is music.
    fn bgm_file(&self, n: i32) -> i32 {
        let named = ["WAV", "OGG", "AOG", "XWM"]
            .iter()
            .find_map(|ext| self.index.find(&format!("BGM{n:03}.{ext}")));
        match named {
            Some(f) => f as i32,
            None if n >= 0 && self.file_name(n as usize).to_uppercase().starts_with("BGM") => n,
            None => -1,
        }
    }

    fn play(&mut self, n: i32, channel: i32, looped: bool) {
        if n < 0 {
            return;
        }
        if self.log {
            eprintln!("play {n} {} on {channel}", self.file_name(n as usize));
        }
        let Some(data) = self.read_file(n as usize) else {
            return;
        };
        let data = if data.starts_with(b"AOGG") && data.len() > 0x14 {
            data[0x14..].to_vec()
        } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"XWMA") {
            // xWMA (WMA in RIFF, SYS5's short tracks): browsers do not decode it
            return;
        } else if data.starts_with(b"OggS") || data.starts_with(b"RIFF") {
            data
        } else {
            // a number the trial has no sound for lands on another file
            return;
        };
        self.sounds.push(Sound {
            data,
            volume: 1.0,
            channel,
            stop: false,
            fade_ms: 0,
            looped,
        });
    }

    fn stop(&mut self, channel: i32, fade_ms: u32) {
        self.sounds.push(Sound {
            data: Vec::new(),
            volume: 0.0,
            channel,
            stop: true,
            fade_ms,
            looped: false,
        });
    }

    fn say(&mut self, win: i32, s: &str) {
        self.text_log.push(s.to_string());
        if self.log {
            eprintln!("TEXT {s}");
        }
        let (size, rgb, shadow) = (self.font_size, self.text_rgb, self.shadow_rgb);
        let w = self.windows.entry(win).or_default();
        if w.lines.iter().all(String::is_empty) {
            (w.size, w.rgb, w.shadow) = (size, rgb, shadow);
        }
        // the right edge, kept on the screen (some games give one past it)
        // (a full-screen window without one keeps the margin of its left one)
        let right = if w.right > w.tx {
            w.right
        } else if w.w >= self.screen.0 as i32 && w.w > 2 * w.tx {
            w.w - w.tx
        } else {
            w.tx + 600
        };
        let width = right.min(self.screen.0 as i32 - 24 - w.x) - w.tx;
        let per_line = (width as f32 / w.size.max(8.0)).floor().max(4.0) as usize;
        let mut line = w.lines.pop().unwrap_or_default();
        for c in s.chars() {
            if c == '\n' {
                w.lines.push(std::mem::take(&mut line));
                continue;
            }
            let units: usize = line
                .chars()
                .map(|c| if (c as u32) < 0x80 { 1 } else { 2 })
                .sum();
            if units + 2 > per_line * 2 && !"、。」』）！？".contains(c) {
                w.lines.push(std::mem::take(&mut line));
            }
            line.push(c);
        }
        w.lines.push(line);
    }

    fn line_image(
        &mut self,
        s: &str,
        size: f32,
        rgb: [u8; 3],
        shadow: [u8; 3],
    ) -> Option<Rc<Image>> {
        let font = self.font.clone()?;
        let key = (s.to_string(), size as u32, rgb, shadow);
        if let Some(i) = self.text_cache.get(&key) {
            return Some(i.clone());
        }
        let extra = self.extra_font.clone();
        let img = Rc::new(crate::text::render(
            &font,
            extra.as_deref(),
            s,
            size,
            rgb,
            shadow,
        ));
        if self.text_cache.len() > 256 {
            self.text_cache.clear();
        }
        self.text_cache.insert(key, img.clone());
        Some(img)
    }

    fn draw_string(&mut self, id: i32, x: i32, y: i32, s: &str) {
        let (size, rgb, shadow) = (self.font_size, self.text_rgb, self.shadow_rgb);
        let Some(line) = self.line_image(s, size, rgb, shadow) else {
            return;
        };
        if let Some(t) = self.textures.get_mut(&id) {
            put_text(Rc::make_mut(t), &line, x, y);
            let s = s.to_string();
            self.record(
                id,
                TexOp::Text {
                    x,
                    y,
                    s,
                    size,
                    rgb,
                    shadow,
                },
            );
        }
    }

    /// 23b: sprite `id` shows `n` in number font `font`, `digits` wide (zero padded when
    /// `pad`, else right aligned), on a texture of its own.
    fn draw_number(&mut self, id: i32, font: i32, n: i32, at: (i32, i32), digits: i32, pad: bool) {
        let Some(&(src, [sx, sy, w, h], gap)) = self.digit_fonts.get(&font) else {
            return;
        };
        let Some(img) = self.textures.get(&src).cloned() else {
            return;
        };
        let digits = digits.clamp(1, 16);
        let mut text = n.max(0).to_string();
        if pad {
            text = format!("{:0>1$}", text, digits as usize);
        }
        let step = w - gap;
        let width = (step * (digits - 1) + w).max(1);
        let tex = -0x10000 - id;
        let (tw, th) = (width as u32, h.max(1) as u32);
        self.textures.insert(tex, Rc::new(Image::new(tw, th)));
        self.recipes.insert(tex, Rc::new(vec![TexOp::New(tw, th)]));
        let from = self.recipes.get(&src).cloned();
        let from = from.unwrap_or_else(|| Rc::new(vec![TexOp::Pixels(img.clone())]));
        let skip = (digits - text.len() as i32).max(0);
        for (i, c) in text
            .bytes()
            .rev()
            .take(digits as usize)
            .collect::<Vec<_>>()
            .iter()
            .rev()
            .enumerate()
        {
            let r = [sx + (c - b'0') as i32 * w, sy, w, h];
            let dx = (skip + i as i32) * step;
            if let Some(t) = self.textures.get_mut(&tex) {
                blit(Rc::make_mut(t), &img, r, dx, 0);
            }
            self.record(tex, TexOp::Blit(from.clone(), r, dx, 0));
        }
        let s = self.sprites.entry(id).or_insert_with(|| Sprite {
            tex,
            src: [0; 4],
            x: 0.0,
            y: 0.0,
            alpha: 1.0,
            add: false,
            tint: 0xffffff,
            pivot: (0.0, 0.0),
            scale: [1.0, 1.0],
            off: [0.0, 0.0],
            anims: Vec::new(),
        });
        s.tex = tex;
        s.src = [0, 0, width, h];
        (s.x, s.y) = (at.0 as f32, at.1 as f32);
    }

    // ---- drawing ----

    fn place(&self, s: &Sprite) -> (Place, f32, f32) {
        let mut cur = s.clone();
        for a in &s.anims {
            let Some(from) = a.from else {
                continue;
            };
            let t = if a.ms <= 0.0 {
                1.0
            } else {
                ((self.now - a.start) / a.ms).clamp(0.0, 1.0) as f32
            };
            let lerp = |i: usize| from[i] + (a.to[i] - from[i]) * t;
            cur.put(a.kind, [lerp(0), lerp(1)]);
        }
        let (alpha, scale) = (cur.alpha, cur.scale);
        let place = Place {
            x: 0.0,
            y: 0.0,
            sx: scale[0],
            sy: scale[1],
            cx: s.pivot.0,
            cy: s.pivot.1,
            rot: 0.0,
            alpha,
            add: s.add,
            flip: (false, false),
            clip: [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32],
        };
        (place, cur.x + cur.off[0], cur.y + cur.off[1])
    }

    fn animate(&mut self, id: i32, kind: AnimKind, delay: i32, ms: i32, to: [f32; 2]) {
        let start = self.now + delay.max(0) as f64;
        if let Some(s) = self.sprites.get_mut(&id) {
            s.anims.retain(|a| a.kind != kind);
            s.anims.push(Anim {
                kind,
                start,
                ms: ms.max(0) as f64,
                from: None,
                to,
            });
        }
    }

    /// Starts the animations whose time has come (from the sprite as it is then) and settles
    /// the finished ones into the sprites.
    fn settle(&mut self) {
        let now = self.now;
        for s in self.sprites.values_mut() {
            let mut i = 0;
            while i < s.anims.len() {
                let a = s.anims[i];
                if now >= a.start && a.from.is_none() {
                    s.anims[i].from = Some(s.get(a.kind));
                }
                if now >= a.start + a.ms {
                    s.put(a.kind, a.to);
                    s.anims.remove(i);
                } else {
                    i += 1;
                }
            }
        }
    }

    fn tint(&mut self, t: &Rc<Image>, c: i32) -> Rc<Image> {
        if c == 0xffffff {
            return t.clone();
        }
        if let Some(i) = self.tinted.get(&(t.id, c)) {
            return i.clone();
        }
        if self.tinted.len() > 64 {
            self.tinted.clear();
        }
        let mut img = (**t).clone();
        let m = rgb(c);
        for p in img.px.chunks_exact_mut(4) {
            for k in 0..3 {
                p[k] = (p[k] as u32 * m[k] as u32 / 255) as u8;
            }
        }
        let img = Rc::new(img);
        self.tinted.insert((t.id, c), img.clone());
        img
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        self.settle();
        let mut out = Vec::new();
        let sprites: Vec<Sprite> = self.sprites.values().cloned().collect();
        for s in &sprites {
            let Some(t) = self.textures.get(&s.tex).cloned() else {
                continue;
            };
            let t = self.tint(&t, s.tint);
            let [sx, sy, w, h] = s.src;
            if w <= 0 || h <= 0 {
                continue;
            }
            let (place, x, y) = self.place(s);
            out.push(Draw {
                image: t.clone(),
                src: [sx, sy, sx + w, sy + h],
                dst: [x, y, x + w as f32, y + h as f32],
                place,
            });
        }
        // the text of the message window, over the pictures
        let wins: Vec<(i32, Window)> = self.windows.iter().map(|(k, w)| (*k, w.clone())).collect();
        for (_, w) in wins {
            let lh = (w.size * 1.25).round();
            for (i, line) in w.lines.iter().enumerate() {
                if line.is_empty() {
                    continue;
                }
                let Some(img) = self.line_image(line, w.size, w.rgb, w.shadow) else {
                    continue;
                };
                let x = (w.x + w.tx) as f32 - crate::text::PAD as f32;
                let y = (w.y + w.ty) as f32 + i as f32 * lh - crate::text::PAD as f32;
                out.push(Draw {
                    image: img.clone(),
                    src: [0, 0, img.w as i32, img.h as i32],
                    dst: [x, y, x + img.w as f32, y + img.h as f32],
                    place: Place {
                        x: 0.0,
                        y: 0.0,
                        sx: 1.0,
                        sy: 1.0,
                        cx: 0.0,
                        cy: 0.0,
                        rot: 0.0,
                        alpha: 1.0,
                        add: false,
                        flip: (false, false),
                        clip: [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32],
                    },
                });
            }
        }
        self.menu_draws(&mut out);
        out
    }

    pub fn render(&mut self) -> Image {
        let mut img = Image::new(self.screen.0, self.screen.1);
        img.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if x >= 0 {
            self.pointer = (x, y);
        }
        if self.menu.is_some() {
            self.menu_click(x, y);
            return;
        }
        self.buttons |= 1;
        self.release = 3;
        self.clicked = true;
    }

    /// A right click (cancel).
    pub fn cancel(&mut self) {
        if self.menu.is_some() {
            self.menu = None;
            return;
        }
        self.keys.push(10);
        self.clicked = true;
    }

    /// A key as the pad numbers: 0..3 up, down, left, right, 4 enter, 10 cancel.
    pub fn key(&mut self, k: i32) {
        self.keys.push(k);
        if k == 4 {
            self.clicked = true;
        }
    }

    pub fn wheel(&mut self, _up: bool) {}

    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// The sprites, one per line (id, texture and its size, source, place, alpha, tint), for
    /// age-run's AGE_SPRITES.
    pub fn sprite_list(&self) -> String {
        let mut out = String::new();
        for (id, s) in &self.sprites {
            let size = self.textures.get(&s.tex).map(|t| (t.w, t.h));
            out += &format!(
                "{id} tex {} {size:?} src {:?} at {} {} off {:?} alpha {} tint {:x}\n",
                s.tex, s.src, s.x, s.y, s.off, s.alpha, s.tint
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_settings_in_both_encodings() {
        let wide: Vec<u8> = concat!("x\0GAMEVERSION\0", "1.00.0000\0RC\0")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(
            ini_setting(&wide, "GAMEVERSION").as_deref(),
            Some("1.00.0000")
        );
        assert_eq!(
            ini_setting(b"GAMEVERSION\x002.10\0", "GAMEVERSION").as_deref(),
            Some("2.10")
        );
        assert_eq!(ini_setting(b"nothing", "GAMEVERSION"), None);
    }
}
