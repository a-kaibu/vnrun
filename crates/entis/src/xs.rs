//! The 2024 generation of Entis GLS games (WitchWizard, 狙われた優等生): no Cotopha program,
//! the system is native code in the exe and the scenario is "xscript" XML (`.srcxml` in
//! script.noa: `<msg>`, `<bg>`, `<char>`, `<select>`, `<call>`, ...). This runs those scripts
//! with a system of its own built on the game's skins (wm_title.noa / wm_window.noa
//! system.inf): logo, title, message window, save and load pages.

use std::collections::HashMap;
use std::rc::Rc;

use nexas::gfx::{Draw, Image, Place};

use crate::engine::{Engine, SoundEvent};
use crate::skin::{Node, Skin};

const W: f32 = 1280.0;
const H: f32 = 720.0;
/// ms a character takes to appear
const CHAR_MS: f64 = 25.0;
const SLOTS_PER_PAGE: usize = 5;
const THUMB: (u32, u32) = (146, 83);
/// the offset of a standing picture at pos="left" / "right"
const SIDE: i32 = 300;

pub struct Script {
    name: String,
    code: Vec<Node>,
    labels: HashMap<String, usize>,
}

#[derive(Clone, Debug, PartialEq)]
enum Val {
    N(f64),
    S(String),
}

impl Val {
    fn num(&self) -> f64 {
        match self {
            Val::N(n) => *n,
            Val::S(s) => s.trim().parse().unwrap_or(0.0),
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Val::N(n) => *n != 0.0,
            Val::S(s) => !s.is_empty(),
        }
    }
}

#[derive(Clone, Default, Debug)]
struct Char {
    id: String,
    src: String,
    x: i32,
}

#[derive(Clone, Default, Debug)]
struct Layer {
    id: String,
    src: String,
    x: i32,
    y: i32,
    z: i32,
}

/// What is on the screen (all of it goes into a save).
#[derive(Clone, Default, Debug)]
struct Scene {
    bg: Option<String>,
    /// an event picture: over the standing pictures
    ev: Option<String>,
    chars: Vec<Char>,
    face: Option<String>,
    layers: Vec<Layer>,
    filter: Option<String>,
    bgm: Option<(String, f32)>,
    /// the message window: 0 the large one, 1 the small one
    mview: i32,
    /// false after `<msgshow show="false">` until the next message
    window: bool,
}

struct Trans {
    /// the screen before (without the message window)
    snap: Rc<Image>,
    start: f64,
    dur: f64,
    /// through white or black
    via: Option<[u8; 4]>,
}

#[derive(Clone, Default)]
struct Msg {
    name: String,
    /// the text shown before this message (`add`)
    prefix: String,
    /// the whole text of the window
    text: String,
    start: f64,
    keep: bool,
    voice_ms: f64,
    done_at: Option<f64>,
    /// clicked past and not kept: the next message starts a new page, and the window
    /// hides until then (but stays for a `<select>` right after)
    read: bool,
}

enum Wait {
    None,
    Time(f64),
    Msg,
    /// options (text, label)
    Select(Vec<(String, Option<String>)>),
    Trans,
    Se(f64),
}

#[derive(PartialEq)]
enum Base {
    Logo(usize, f64),
    Title,
    Game,
}

enum Act {
    Save(usize),
    Load(usize),
    Title,
    Exit,
}

enum Over {
    Slots { load: bool, page: usize },
    Confirm { page: &'static str, act: Act },
}

/// A save's heading, for the slot pages.
struct SlotInfo {
    date: String,
    desc: String,
    thumb: Option<Rc<Image>>,
}

pub struct Xs {
    scripts: HashMap<String, Rc<Script>>,
    title: Skin,
    win: Skin,
    base: Base,
    over: Vec<Over>,
    stack: Vec<(Rc<Script>, usize)>,
    wait: Wait,
    vars: HashMap<String, Val>,
    st: Scene,
    trans: Option<Trans>,
    /// the screen at `<begin_trans>`, with its time
    pending: Option<(Rc<Image>, f64, Option<[u8; 4]>)>,
    msg: Option<Msg>,
    /// where the message being shown is: (stack depth, its index)
    msg_at: Option<(usize, usize)>,
    desc: String,
    flash: Option<(Rc<Image>, f64, u32, f64)>,
    shake: Option<(f64, f64, f64, f64, bool, bool)>,
    mouse: (i32, i32),
    auto: bool,
    skip: bool,
    /// the window hidden by the player (right click or the close button)
    hidden: bool,
    text_cache: HashMap<String, Rc<Image>>,
    filtered: HashMap<(u32, String), Rc<Image>>,
    tct: HashMap<String, Option<Rc<Tct>>>,
    solid: HashMap<u32, Rc<Image>>,
    slots: HashMap<usize, Option<Rc<SlotInfo>>>,
    se_end: HashMap<String, f64>,
}

/// A tone curve filter (.tct): a table per channel, in RGB or (info 6) in YCbCr.
pub struct Tct {
    r: [u8; 256],
    g: [u8; 256],
    b: [u8; 256],
    ycc: bool,
}

impl Tct {
    fn parse(b: &[u8]) -> Option<Tct> {
        if !b.starts_with(b"Entis\x1a") {
            return None;
        }
        let mut t = Tct {
            r: std::array::from_fn(|i| i as u8),
            g: std::array::from_fn(|i| i as u8),
            b: std::array::from_fn(|i| i as u8),
            ycc: false,
        };
        let mut p = 0x40;
        while p + 16 <= b.len() {
            let tag = &b[p..p + 8];
            let n = u64::from_le_bytes(b[p + 8..p + 16].try_into().ok()?) as usize;
            let d = b.get(p + 16..p + 16 + n)?;
            let table = |d: &[u8]| -> [u8; 256] { std::array::from_fn(|i| d[i]) };
            match tag {
                b"red     " if n >= 256 => t.r = table(d),
                b"green   " if n >= 256 => t.g = table(d),
                b"blue    " if n >= 256 => t.b = table(d),
                b"info    " if n >= 4 => t.ycc = d[0] == 6,
                _ => {}
            }
            p += 16 + n;
        }
        Some(t)
    }

    fn apply(&self, img: &Image) -> Image {
        let mut out = Image::new(img.w, img.h);
        out.px = img.px.clone();
        for px in out.px.chunks_exact_mut(4) {
            if px[3] == 0 {
                continue;
            }
            if self.ycc {
                let (r, g, b) = (px[0] as f32, px[1] as f32, px[2] as f32);
                let y = 0.299 * r + 0.587 * g + 0.114 * b;
                let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
                let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
                let idx = |v: f32| v.round().clamp(0.0, 255.0) as usize;
                // blue holds the brightness, green Cb, red Cr
                let y = self.b[idx(y)] as f32;
                let cb = self.g[idx(cb)] as f32 - 128.0;
                let cr = self.r[idx(cr)] as f32 - 128.0;
                let c = |v: f32| v.round().clamp(0.0, 255.0) as u8;
                px[0] = c(y + 1.402 * cr);
                px[1] = c(y - 0.344_136 * cb - 0.714_136 * cr);
                px[2] = c(y + 1.772 * cb);
            } else {
                px[0] = self.r[px[0] as usize];
                px[1] = self.g[px[1] as usize];
                px[2] = self.b[px[2] as usize];
            }
        }
        out
    }
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
        clip: [0.0, 0.0, W, H],
    }
}

/// `src` (x0 y0 x1 y1) of `img` with its top left at x, y.
fn put(out: &mut Vec<Draw>, img: &Rc<Image>, src: [i32; 4], x: f32, y: f32, alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let (w, h) = ((src[2] - src[0]) as f32, (src[3] - src[1]) as f32);
    out.push(Draw {
        image: img.clone(),
        src,
        dst: [x, y, x + w, y + h],
        place: place(alpha.min(1.0)),
    });
}

fn whole(img: &Image) -> [i32; 4] {
    [0, 0, img.w as i32, img.h as i32]
}

fn attr<'a>(n: &'a Node, k: &str) -> &'a str {
    n.attr(k).unwrap_or("")
}

fn num(n: &Node, k: &str, def: f64) -> f64 {
    match n.attr(k).map(str::trim) {
        Some(v) if !v.is_empty() => v.parse().unwrap_or(def),
        _ => def,
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Days since 1970-01-01 as (year, month, day).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

// ---- expressions: `!s.PLAYED_OPENING`, `s.PLAYED_OPENING = 1`

struct Lex<'a> {
    s: &'a [u8],
    p: usize,
}

impl Lex<'_> {
    fn ws(&mut self) {
        while self.p < self.s.len() && (self.s[self.p] as char).is_whitespace() {
            self.p += 1;
        }
    }
    fn eat(&mut self, t: &str) -> bool {
        self.ws();
        if self.s[self.p..].starts_with(t.as_bytes()) {
            // "=" is not the start of "=="
            if t == "=" && self.s.get(self.p + 1) == Some(&b'=') {
                return false;
            }
            self.p += t.len();
            true
        } else {
            false
        }
    }
    fn ident(&mut self) -> Option<String> {
        self.ws();
        let st = self.p;
        while self.p < self.s.len() {
            let c = self.s[self.p];
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' || c >= 0x80 {
                self.p += 1;
            } else {
                break;
            }
        }
        (self.p > st).then(|| String::from_utf8_lossy(&self.s[st..self.p]).into_owned())
    }
}

impl Xs {
    fn eval_str(&mut self, src: &str) -> Val {
        let mut last = Val::N(0.0);
        for stmt in src.split([';', '\n', '\r']) {
            if stmt.trim().is_empty() {
                continue;
            }
            let mut l = Lex {
                s: stmt.as_bytes(),
                p: 0,
            };
            last = self.assign(&mut l);
        }
        last
    }

    fn assign(&mut self, l: &mut Lex) -> Val {
        let save = l.p;
        if let Some(name) = l.ident()
            && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            for op in ["+=", "-=", "*=", "/=", "="] {
                if l.eat(op) {
                    let v = self.assign(l);
                    let old = self.vars.get(&name).cloned().unwrap_or(Val::N(0.0));
                    let nv = match op {
                        "+=" => match (&old, &v) {
                            (Val::S(a), b) => Val::S(format!("{a}{}", show(b))),
                            _ => Val::N(old.num() + v.num()),
                        },
                        "-=" => Val::N(old.num() - v.num()),
                        "*=" => Val::N(old.num() * v.num()),
                        "/=" => Val::N(if v.num() == 0.0 {
                            0.0
                        } else {
                            old.num() / v.num()
                        }),
                        _ => v,
                    };
                    self.vars.insert(name, nv.clone());
                    return nv;
                }
            }
        }
        l.p = save;
        self.binary(l, 0)
    }

    fn binary(&mut self, l: &mut Lex, min: u8) -> Val {
        let mut a = self.unary(l);
        loop {
            let ops: [(&str, u8); 13] = [
                ("||", 1),
                ("&&", 2),
                ("==", 3),
                ("!=", 3),
                ("<=", 4),
                (">=", 4),
                ("<", 4),
                (">", 4),
                ("+", 5),
                ("-", 5),
                ("*", 6),
                ("/", 6),
                ("%", 6),
            ];
            let mut found = None;
            for (op, prec) in ops {
                if prec < min.max(1) {
                    continue;
                }
                let save = l.p;
                if l.eat(op) {
                    found = Some((op, prec));
                    break;
                }
                l.p = save;
            }
            let Some((op, prec)) = found else {
                return a;
            };
            let b = self.binary(l, prec + 1);
            let t = |c: bool| Val::N(c as i32 as f64);
            a = match op {
                "||" => t(a.truthy() || b.truthy()),
                "&&" => t(a.truthy() && b.truthy()),
                "==" => t(match (&a, &b) {
                    (Val::S(x), Val::S(y)) => x == y,
                    _ => a.num() == b.num(),
                }),
                "!=" => t(match (&a, &b) {
                    (Val::S(x), Val::S(y)) => x != y,
                    _ => a.num() != b.num(),
                }),
                "<=" => t(a.num() <= b.num()),
                ">=" => t(a.num() >= b.num()),
                "<" => t(a.num() < b.num()),
                ">" => t(a.num() > b.num()),
                "+" => match (&a, &b) {
                    (Val::S(x), y) => Val::S(format!("{x}{}", show(y))),
                    (x, Val::S(y)) => Val::S(format!("{}{y}", show(x))),
                    _ => Val::N(a.num() + b.num()),
                },
                "-" => Val::N(a.num() - b.num()),
                "*" => Val::N(a.num() * b.num()),
                "/" => Val::N(if b.num() == 0.0 {
                    0.0
                } else {
                    a.num() / b.num()
                }),
                _ => Val::N(if b.num() == 0.0 {
                    0.0
                } else {
                    a.num() % b.num()
                }),
            };
        }
    }

    fn unary(&mut self, l: &mut Lex) -> Val {
        if l.eat("!") {
            return Val::N(!self.unary(l).truthy() as i32 as f64);
        }
        if l.eat("-") {
            return Val::N(-self.unary(l).num());
        }
        if l.eat("(") {
            let v = self.assign(l);
            l.eat(")");
            return v;
        }
        l.ws();
        if l.p < l.s.len() && (l.s[l.p] == b'"' || l.s[l.p] == b'\'') {
            let q = l.s[l.p];
            let st = l.p + 1;
            let mut e = st;
            while e < l.s.len() && l.s[e] != q {
                e += 1;
            }
            l.p = (e + 1).min(l.s.len());
            return Val::S(String::from_utf8_lossy(&l.s[st..e]).into_owned());
        }
        match l.ident() {
            Some(t) if t.chars().next().is_some_and(|c| c.is_ascii_digit()) => {
                Val::N(t.parse().unwrap_or(0.0))
            }
            Some(t) if t == "true" => Val::N(1.0),
            Some(t) if t == "false" => Val::N(0.0),
            Some(t) => self.vars.get(&t).cloned().unwrap_or(Val::N(0.0)),
            None => {
                l.p = l.s.len();
                Val::N(0.0)
            }
        }
    }
}

fn show(v: &Val) -> String {
    match v {
        Val::N(n) if n.fract() == 0.0 => format!("{}", *n as i64),
        Val::N(n) => format!("{n}"),
        Val::S(s) => s.clone(),
    }
}

/// Whether a game folder is of this kind: xscript and no Cotopha program.
pub fn is_xscript(names: &[String]) -> bool {
    names.iter().any(|n| n.to_lowercase().ends_with(".srcxml"))
        && !names.iter().any(|n| n.to_lowercase().ends_with(".csx"))
}

fn load_skin(e: &mut Engine, archive: &str) -> Skin {
    let name = format!("{archive}/system.inf");
    let node =
        e.fs.read(&name)
            .map(|b| crate::skin::decode_text(&b))
            .and_then(|t| crate::skin::parse_xml(&t).ok());
    match node {
        Some(n) => {
            let mut s = Skin::parse(&format!("{archive}/"), &n);
            s.src = name;
            s
        }
        None => {
            e.warn(format!("{name}: no skin"));
            Skin::parse("", &Node::default())
        }
    }
}

impl Xs {
    pub fn open(e: &mut Engine) -> Result<Xs, String> {
        let mut scripts = HashMap::new();
        let names: Vec<String> =
            e.fs.entries()
                .into_iter()
                .map(|(_, en)| en.name)
                .filter(|n| n.to_lowercase().ends_with(".srcxml"))
                .collect();
        for n in names {
            let Some(b) = e.fs.read(&n) else { continue };
            let text = crate::skin::decode_text(&b).replace(" @l=", " l=");
            let root = match crate::skin::parse_xml(&text) {
                Ok(r) => r,
                Err(err) => {
                    e.warn(format!("{n}: {err}"));
                    continue;
                }
            };
            let code = root.kid("code").map(|c| c.kids.clone()).unwrap_or_default();
            let mut labels = HashMap::new();
            for (i, c) in code.iter().enumerate() {
                if c.tag == "label" {
                    labels.insert(attr(c, "id").to_string(), i);
                }
            }
            let base = n.rsplit('/').next().unwrap_or(&n);
            let stem = base[..base.len() - ".srcxml".len()].to_lowercase();
            scripts.insert(
                stem.clone(),
                Rc::new(Script {
                    name: stem,
                    code,
                    labels,
                }),
            );
        }
        if !scripts.contains_key("main") {
            return Err("xscript: no main.srcxml".into());
        }
        let title = load_skin(e, "wm_title.noa");
        let win = load_skin(e, "wm_window.noa");
        let mut x = Xs {
            scripts,
            title,
            win,
            base: Base::Logo(0, e.now),
            over: Vec::new(),
            stack: Vec::new(),
            wait: Wait::None,
            vars: HashMap::new(),
            st: Scene::default(),
            trans: None,
            pending: None,
            msg: None,
            msg_at: None,
            desc: String::new(),
            flash: None,
            shake: None,
            mouse: (-1, -1),
            auto: false,
            skip: false,
            hidden: false,
            text_cache: HashMap::new(),
            filtered: HashMap::new(),
            tct: HashMap::new(),
            solid: HashMap::new(),
            slots: HashMap::new(),
            se_end: HashMap::new(),
        };
        x.load_system(e);
        Ok(x)
    }

    // ---- persistent variables (s.*)

    fn load_system(&mut self, e: &mut Engine) {
        let Some(b) = e.fs.host.read_save("xs-system.txt") else {
            return;
        };
        for line in String::from_utf8_lossy(&b).lines() {
            let p: Vec<&str> = line.split('\t').collect();
            if p.len() == 3 {
                let v = if p[1] == "N" {
                    Val::N(p[2].parse().unwrap_or(0.0))
                } else {
                    Val::S(unesc(p[2]))
                };
                self.vars.insert(unesc(p[0]), v);
            }
        }
    }

    fn save_system(&mut self, e: &mut Engine) {
        let mut out = String::new();
        let mut keys: Vec<&String> = self.vars.keys().filter(|k| k.starts_with("s.")).collect();
        keys.sort();
        for k in keys {
            out += &var_line(k, &self.vars[k]);
        }
        e.fs.host.write_save("xs-system.txt", out.as_bytes());
    }

    // ---- sound

    fn play(
        &mut self,
        e: &mut Engine,
        channel: &str,
        name: &str,
        volume: f32,
        repeat: bool,
    ) -> f64 {
        let file = if name.to_lowercase().ends_with(".mio") {
            name.to_string()
        } else {
            format!("{name}.mio")
        };
        let Some(b) = e.fs.read(&file) else {
            e.warn(format!("{file}: no such sound"));
            return 0.0;
        };
        let ms = crate::mio::duration_ms(&b);
        let Some(w) = crate::mio::to_wav(&b) else {
            e.warn(format!("{file}: not a MIO sound"));
            return 0.0;
        };
        e.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data: Some(Rc::new(w)),
            volume,
            repeat,
            fade_ms: 0,
        });
        ms
    }

    fn stop(&mut self, e: &mut Engine, channel: &str, fade_ms: u32) {
        e.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data: None,
            volume: 0.0,
            repeat: false,
            fade_ms,
        });
    }

    fn stop_all(&mut self, e: &mut Engine) {
        for c in ["bgm", "voice", "se1", "se2", "se3", "se4"] {
            self.stop(e, c, 0);
        }
        self.se_end.clear();
    }

    // ---- pictures

    fn pic(&mut self, e: &mut Engine, name: &str) -> Option<(Rc<Image>, (i32, i32))> {
        let p = e.pic(name)?;
        Some((p.img.clone(), p.hot))
    }

    fn solid(&mut self, rgba: [u8; 4]) -> Rc<Image> {
        let key = u32::from_be_bytes(rgba);
        self.solid
            .entry(key)
            .or_insert_with(|| {
                let mut i = Image::new(4, 4);
                i.fill(rgba);
                Rc::new(i)
            })
            .clone()
    }

    fn filtered(&mut self, e: &mut Engine, img: Rc<Image>) -> Rc<Image> {
        let Some(f) = self.st.filter.clone().filter(|f| !f.is_empty()) else {
            return img;
        };
        if !self.tct.contains_key(&f) {
            let t =
                e.fs.read(&format!("{f}.tct"))
                    .and_then(|b| Tct::parse(&b))
                    .map(Rc::new);
            if t.is_none() {
                e.warn(format!("{f}.tct: no such filter"));
            }
            self.tct.insert(f.clone(), t);
        }
        let Some(Some(t)) = self.tct.get(&f).cloned() else {
            return img;
        };
        if self.filtered.len() > 64 {
            self.filtered.clear();
        }
        self.filtered
            .entry((img.id, f))
            .or_insert_with(|| Rc::new(t.apply(&img)))
            .clone()
    }

    /// The pictures of the scene (background, standing pictures, event picture, layers).
    fn scene_draws(&mut self, e: &mut Engine, out: &mut Vec<Draw>) {
        let now = e.now;
        let (mut bx, mut by, mut ax, mut ay) = (0.0, 0.0, 0.0, 0.0);
        if let Some((start, dur, amp, cycle, bg_only, sine)) = self.shake {
            let t = now - start;
            if t >= dur {
                self.shake = None;
            } else {
                let k = 1.0 - t / dur;
                let (dx, dy) = if sine {
                    let a = (t / dur * cycle * std::f64::consts::TAU).sin();
                    (0.0, a * amp * k)
                } else {
                    let r = e.random();
                    (
                        ((r & 0xff) as f64 / 127.5 - 1.0) * amp * k,
                        (((r >> 8) & 0xff) as f64 / 127.5 - 1.0) * amp * k,
                    )
                };
                if bg_only {
                    (bx, by) = (dx as f32, dy as f32);
                } else {
                    (ax, ay) = (dx as f32, dy as f32);
                }
            }
        }
        let st = self.st.clone();
        let mut draw =
            |x: &mut Xs, name: &str, dx: f32, dy: f32, alpha: f32, out: &mut Vec<Draw>| {
                if let Some((img, hot)) = x.pic(e, name) {
                    let img = x.filtered(e, img);
                    let src = whole(&img);
                    put(
                        out,
                        &img,
                        src,
                        -hot.0 as f32 + dx + ax,
                        -hot.1 as f32 + dy + ay,
                        alpha,
                    );
                }
            };
        if let Some(bg) = &st.bg {
            draw(self, bg, bx, by, 1.0, out);
        }
        for c in &st.chars {
            draw(self, &c.src, c.x as f32 + bx, by, 1.0, out);
        }
        if let Some(ev) = &st.ev {
            draw(self, ev, bx, by, 1.0, out);
        }
        let mut layers = st.layers.clone();
        layers.sort_by_key(|l| -l.z);
        for l in &layers {
            draw(self, &l.src, l.x as f32, l.y as f32, 1.0, out);
        }
    }

    fn render(draws: &[Draw]) -> Image {
        let mut img = Image::new(W as u32, H as u32);
        img.fill([0, 0, 0, 255]);
        for d in draws {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }

    fn snapshot(&mut self, e: &mut Engine) -> Rc<Image> {
        let mut d = Vec::new();
        self.scene_draws(e, &mut d);
        self.trans_draws(e, &mut d);
        Rc::new(Xs::render(&d))
    }

    /// The transition running over the scene.
    fn trans_draws(&mut self, e: &mut Engine, out: &mut Vec<Draw>) {
        let Some(t) = &self.trans else { return };
        let p = ((e.now - t.start) / t.dur.max(1.0)) as f32;
        if p >= 1.0 {
            self.trans = None;
            return;
        }
        let snap = t.snap.clone();
        match t.via {
            Some(c) => {
                let col = self.solid(c);
                if p < 0.5 {
                    put(out, &snap, whole(&snap), 0.0, 0.0, 1.0);
                    out.push(cover(&col, p * 2.0));
                } else {
                    out.push(cover(&col, (1.0 - p) * 2.0));
                }
            }
            None => put(out, &snap, whole(&snap), 0.0, 0.0, 1.0 - p),
        }
    }

    /// Changes the scene: at once inside `<begin_trans>`, else with a fade of `ms`.
    fn change(&mut self, e: &mut Engine, ms: f64, effect: &str, f: impl FnOnce(&mut Scene)) {
        let via = match effect {
            "white" => Some([255, 255, 255, 255]),
            "dark" | "black" => Some([0, 0, 0, 255]),
            _ => None,
        };
        if self.pending.is_some() || ms <= 0.0 || self.skip {
            f(&mut self.st);
            return;
        }
        let snap = self.snapshot(e);
        f(&mut self.st);
        self.trans = Some(Trans {
            snap,
            start: e.now,
            dur: ms,
            via,
        });
    }

    // ---- running the script

    fn start_game(&mut self, e: &mut Engine) {
        self.stop_all(e);
        self.vars.retain(|k, _| k.starts_with("s."));
        self.st = Scene::default();
        self.msg = None;
        self.msg_at = None;
        self.trans = None;
        self.pending = None;
        self.desc.clear();
        self.wait = Wait::None;
        self.auto = false;
        self.skip = false;
        self.hidden = false;
        self.stack = vec![(self.scripts["main"].clone(), 0)];
        self.base = Base::Game;
    }

    fn back_to_title(&mut self, e: &mut Engine) {
        self.stop_all(e);
        self.stack.clear();
        self.over.clear();
        self.msg = None;
        self.trans = None;
        self.base = Base::Title;
    }

    fn run(&mut self, e: &mut Engine) {
        for _ in 0..10_000 {
            match self.wait {
                Wait::None => {}
                Wait::Time(t) => {
                    if e.now < t && !self.skip {
                        return;
                    }
                    self.wait = Wait::None;
                }
                Wait::Se(t) => {
                    if e.now < t && !self.skip {
                        return;
                    }
                    self.wait = Wait::None;
                }
                Wait::Trans => {
                    if self.trans.is_some() && !self.skip {
                        return;
                    }
                    self.wait = Wait::None;
                }
                Wait::Msg => {
                    let Some(m) = &mut self.msg else {
                        self.wait = Wait::None;
                        continue;
                    };
                    let total = m.text.chars().count();
                    let shown = m.prefix.chars().count() + ((e.now - m.start) / CHAR_MS) as usize;
                    if m.done_at.is_none() && shown >= total {
                        m.done_at = Some(e.now);
                    }
                    if self.skip {
                        self.advance();
                        continue;
                    }
                    if self.auto
                        && let Some(d) = m.done_at
                        && e.now >= d + m.voice_ms.max(1200.0)
                    {
                        self.advance();
                        continue;
                    }
                    return;
                }
                Wait::Select(_) => return,
            }
            if self.base != Base::Game {
                return;
            }
            self.step(e);
        }
    }

    /// The click that goes on after a message.
    fn advance(&mut self) {
        if let Some(m) = &mut self.msg {
            m.read = !m.keep;
        }
        self.wait = Wait::None;
    }

    fn step(&mut self, e: &mut Engine) {
        let Some((sc, pc)) = self.stack.last().cloned() else {
            self.back_to_title(e);
            return;
        };
        let Some(n) = sc.code.get(pc).cloned() else {
            self.stack.pop();
            return;
        };
        self.stack.last_mut().unwrap().1 += 1;
        if e.log >= 2 {
            eprintln!("xscript: {}:{pc} <{}> {:?}", sc.name, n.tag, n.attrs);
        }
        let ms = |def: f64| num(&n, "time", def);
        match n.tag.as_str() {
            "description" => self.desc = attr(&n, "text").to_string(),
            "label" | "debug" => {}
            "goto" => {
                let cond = attr(&n, "cond");
                if cond.trim().is_empty() || self.eval_str(cond).truthy() {
                    self.jump(e, attr(&n, "target"), attr(&n, "label"));
                }
            }
            "call" => {
                let cond = attr(&n, "cond");
                if cond.trim().is_empty() || self.eval_str(cond).truthy() {
                    let t = attr(&n, "target").to_lowercase();
                    match self.scripts.get(&t).cloned() {
                        Some(s) => {
                            let at = s.labels.get(attr(&n, "label")).copied().unwrap_or(0);
                            self.stack.push((s, at));
                        }
                        None => e.warn(format!("xscript: no script {t}")),
                    }
                }
            }
            "return" => {
                self.stack.pop();
            }
            "eval" => {
                let x = attr(&n, "expr").to_string();
                self.eval_str(&x);
                if x.contains("s.") {
                    self.save_system(e);
                }
            }
            "wait" => self.wait = Wait::Time(e.now + ms(0.0)),
            "wait_chgmv" => self.wait = Wait::Trans,
            "bg" => {
                let src = attr(&n, "src").to_string();
                self.change(e, ms(500.0), attr(&n, "effect"), |s| {
                    s.bg = Some(src);
                    s.ev = None;
                });
            }
            "ev" => {
                let src = attr(&n, "src").to_string();
                self.change(e, ms(500.0), attr(&n, "effect"), |s| s.ev = Some(src));
            }
            "char" => {
                let id = attr(&n, "id").to_string();
                let src = attr(&n, "src").to_string();
                let pos = attr(&n, "pos").to_string();
                self.change(e, ms(300.0), attr(&n, "effect"), |s| {
                    let x = match pos.as_str() {
                        "left" => Some(-SIDE),
                        "right" => Some(SIDE),
                        "center" => Some(0),
                        _ => None,
                    };
                    match s.chars.iter_mut().find(|c| c.id == id) {
                        Some(c) => {
                            c.src = src;
                            if let Some(x) = x {
                                c.x = x;
                            }
                        }
                        None => s.chars.push(Char {
                            id,
                            src,
                            x: x.unwrap_or(0),
                        }),
                    }
                });
            }
            "face" => {
                let f = attr(&n, "src");
                self.st.face = (!f.is_empty()).then(|| f.to_string());
            }
            "layer" => {
                let l = Layer {
                    id: attr(&n, "id").to_string(),
                    src: attr(&n, "src").to_string(),
                    x: num(&n, "x", 0.0) as i32,
                    y: num(&n, "y", 0.0) as i32,
                    z: num(&n, "z", 0.0) as i32,
                };
                self.change(e, ms(0.0), attr(&n, "effect"), |s| {
                    s.layers.retain(|o| o.id != l.id);
                    s.layers.push(l);
                });
            }
            "clear" => {
                let class = attr(&n, "class").to_string();
                let layer = attr(&n, "layer").to_string();
                self.change(e, ms(300.0), attr(&n, "effect"), |s| {
                    if !layer.is_empty() {
                        s.layers.retain(|o| o.id != layer);
                    }
                    match class.as_str() {
                        "char" => s.chars.clear(),
                        "ev" => s.ev = None,
                        "bg" => s.bg = None,
                        "all" => {
                            s.chars.clear();
                            s.ev = None;
                            s.bg = None;
                            s.layers.clear();
                        }
                        _ => {}
                    }
                });
            }
            "begin_trans" => {
                let snap = self.snapshot(e);
                let via = match attr(&n, "effect") {
                    "white" => Some([255, 255, 255, 255]),
                    "dark" | "black" => Some([0, 0, 0, 255]),
                    _ => None,
                };
                self.pending = Some((snap, ms(500.0), via));
            }
            "end_trans" => {
                if let Some((snap, dur, via)) = self.pending.take()
                    && dur > 0.0
                    && !self.skip
                {
                    self.trans = Some(Trans {
                        snap,
                        start: e.now,
                        dur,
                        via,
                    });
                }
            }
            "scrnfilter" => {
                let f = attr(&n, "src").to_string();
                self.change(e, ms(0.0), "", |s| s.filter = (!f.is_empty()).then_some(f));
            }
            "chgmview" => self.st.mview = num(&n, "mode", 0.0) as i32,
            "msgshow" => self.st.window = attr(&n, "show") != "false",
            "flash" => {
                let img = attr(&n, "image");
                let img = self
                    .pic(e, if img.is_empty() { "white" } else { img })
                    .map(|p| p.0)
                    .unwrap_or_else(|| self.solid([255, 255, 255, 255]));
                let count = num(&n, "count", 1.0).max(1.0) as u32;
                let fade = num(&n, "fade", 200.0).max(1.0);
                self.flash = Some((img, e.now, count, fade));
            }
            "shake" => {
                let dur = num(&n, "duration", 300.0);
                let amp = num(&n, "amp", 20.0) / 5.0;
                let cycle = num(&n, "cycle", 1.0);
                let bg_only = attr(&n, "class") == "bg";
                let sine = attr(&n, "type") == "sine";
                self.shake = Some((e.now, dur, amp, cycle, bg_only, sine));
            }
            "bgm" => {
                let src = attr(&n, "src").to_string();
                let vol = num(&n, "volume", 1.0) as f32;
                self.play(e, "bgm", &src, vol, true);
                self.st.bgm = Some((src, vol));
            }
            "stopbgm" => {
                self.stop(e, "bgm", num(&n, "fadeout", 0.0) as u32);
                self.st.bgm = None;
            }
            "se" => {
                let ch = format!("se{}", attr(&n, "buf"));
                let src = attr(&n, "src").to_string();
                let lp = attr(&n, "loop") == "true";
                let len = self.play(e, &ch, &src, num(&n, "volume", 1.0) as f32, lp);
                self.se_end
                    .insert(ch, if lp { f64::INFINITY } else { e.now + len });
            }
            "waitse" => {
                let ch = format!("se{}", attr(&n, "buf"));
                if let Some(&t) = self.se_end.get(&ch)
                    && t.is_finite()
                {
                    self.wait = Wait::Se(t);
                }
            }
            "stopse" => {
                let ch = format!("se{}", attr(&n, "buf"));
                self.stop(e, &ch, num(&n, "fadeout", 0.0) as u32);
                self.se_end.remove(&ch);
            }
            "msg" => self.message(e, &n, pc),
            "select" => {
                let opts = n
                    .kids
                    .iter()
                    .filter(|k| k.tag == "menu")
                    .map(|k| {
                        let l = attr(k, "label");
                        (
                            attr(k, "text").to_string(),
                            (!l.is_empty()).then(|| l.to_string()),
                        )
                    })
                    .collect();
                self.skip = false;
                self.wait = Wait::Select(opts);
            }
            // effects this does not show; the rest do nothing on screen
            "fade_filter" | "set_filter" | "clear_filter" | "begin_keyskip" | "end_keyskip"
            | "begin_replay" | "end_replay" | "end_scene" | "msgcount" | "msgclear" => {}
            t => e.warn(format!("xscript: <{t}> not supported")),
        }
    }

    fn jump(&mut self, e: &mut Engine, target: &str, label: &str) {
        if !target.is_empty() {
            match self.scripts.get(&target.to_lowercase()).cloned() {
                Some(s) => {
                    let at = s.labels.get(label).copied().unwrap_or(0);
                    if let Some(top) = self.stack.last_mut() {
                        *top = (s, at);
                    }
                }
                None => e.warn(format!("xscript: no script {target}")),
            }
            return;
        }
        if let Some(top) = self.stack.last_mut() {
            match top.0.labels.get(label) {
                Some(&at) => top.1 = at,
                None => e.warn(format!("{}: no label {label}", top.0.name)),
            }
        }
    }

    fn message(&mut self, e: &mut Engine, n: &Node, pc: usize) {
        let mut text = attr(n, "text").replace("\\n", "\n");
        // "\n:" ends a line that waits
        if let Some(t) = text.strip_suffix('\n') {
            text = t.to_string() + "\n";
        }
        if text.ends_with(":") {
            text.pop();
        }
        let prefix = match &self.msg {
            Some(m) if n.flag("add") && !m.read => m.text.clone(),
            _ => String::new(),
        };
        self.msg_at = Some((self.stack.len(), pc));
        self.st.window = true;
        let voice = attr(n, "voices")
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        let voice_ms = if voice.is_empty() || self.skip {
            0.0
        } else {
            self.play(e, "voice", &voice, 1.0, false)
        };
        let whole = prefix.clone() + &text;
        self.msg = Some(Msg {
            name: attr(n, "name").to_string(),
            prefix,
            text: whole,
            start: e.now,
            keep: n.flag("keep"),
            voice_ms,
            done_at: None,
            read: false,
        });
        self.wait = Wait::Msg;
    }

    // ---- input

    fn page_node<'a>(skin: &'a Skin, page: &str) -> Option<&'a Node> {
        skin.pages.get(page)
    }

    /// The button of a page under the pointer.
    fn hit(&mut self, e: &mut Engine, which: Which, page: &str, x: i32, y: i32) -> Option<String> {
        let skin = self.skin(which);
        let p = Xs::page_node(skin, page)?.clone();
        let (px, py) = (p.int("x", 0), p.int("y", 0));
        for k in &p.kids {
            if k.tag != "button" {
                continue;
            }
            let (bx, by) = (px + k.int("x", 0), py + k.int("y", 0));
            let (bw, bh) = (k.int("width", 0), k.int("height", 0));
            if x < bx || y < by || x >= bx + bw || y >= by + bh {
                continue;
            }
            // a button that is not a rectangle: its mask picture
            let style = self.skin(which).styles.get(attr(k, "style")).cloned();
            if let Some(m) = style.as_ref().and_then(|s| s.kid("mask"))
                && attr(m, "rect") == "false"
                && !attr(m, "image").is_empty()
            {
                let (id, rect) = crate::skin::image_ref(attr(m, "image"));
                let file = self.skin(which).image_file(&id);
                if let Some(f) = file
                    && let Some((img, _)) = self.pic(e, &f)
                {
                    let r = rect.unwrap_or([0, 0, img.w as i32, img.h as i32]);
                    let (ix, iy) = (r[0] + x - bx, r[1] + y - by);
                    if ix >= 0 && iy >= 0 && (ix as u32) < img.w && (iy as u32) < img.h {
                        let a = img.px[((iy as u32 * img.w + ix as u32) * 4 + 3) as usize];
                        if a < 16 {
                            continue;
                        }
                    }
                }
            }
            return Some(attr(k, "id").to_string());
        }
        None
    }

    fn skin(&self, which: Which) -> &Skin {
        match which {
            Which::Title => &self.title,
            Which::Win => &self.win,
        }
    }

    fn window_page(&self) -> (&'static str, &'static str) {
        if self.st.mview == 1 {
            ("ID_MESSAGE_WINDOW1", "ID_MESSAGE_NAME_WINDOW1")
        } else {
            ("ID_MESSAGE_WINDOW", "ID_MESSAGE_NAME_WINDOW")
        }
    }

    pub fn point(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
    }

    pub fn click(&mut self, e: &mut Engine, x: i32, y: i32) {
        self.mouse = (x, y);
        if let Some(o) = self.over.pop() {
            self.click_over(e, o, x, y);
            return;
        }
        match self.base {
            Base::Logo(step, _) => self.base = Base::Logo(step + 1, e.now),
            Base::Title => match self.hit(e, Which::Title, "ID_TITLE_PAGE", x, y).as_deref() {
                Some("ID_START") => {
                    self.ok_se(e);
                    self.start_game(e);
                }
                Some("ID_LOAD") => {
                    self.ok_se(e);
                    self.over.push(Over::Slots {
                        load: true,
                        page: 0,
                    });
                }
                Some("ID_EXIT") => self.over.push(Over::Confirm {
                    page: "ID_MSGBOX_EXIT",
                    act: Act::Exit,
                }),
                _ => {}
            },
            Base::Game => self.click_game(e, x, y),
        }
    }

    fn ok_se(&mut self, e: &mut Engine) {
        self.play(e, "sys", "wm_window.noa/SE_BUTTON_OK", 1.0, false);
    }

    fn click_game(&mut self, e: &mut Engine, x: i32, y: i32) {
        if self.hidden {
            self.hidden = false;
            return;
        }
        if self.skip {
            self.skip = false;
            return;
        }
        // a sound being waited for: the click cuts it short (`<stopse>` fades it out after)
        if let Wait::Se(_) = self.wait {
            self.wait = Wait::None;
            return;
        }
        if let Wait::Select(opts) = &self.wait {
            let opts = opts.clone();
            for (i, r) in select_rects(opts.len()).into_iter().enumerate() {
                if x >= r[0] && y >= r[1] && x < r[2] && y < r[3] {
                    self.ok_se(e);
                    self.wait = Wait::None;
                    if let Some(l) = &opts[i].1 {
                        self.jump(e, "", l);
                    }
                    return;
                }
            }
            return;
        }
        if self.st.window && self.msg.is_some() {
            let (page, _) = self.window_page();
            if let Some(b) = self.hit(e, Which::Win, page, x, y) {
                self.window_button(e, &b);
                return;
            }
        }
        if let Wait::Msg = self.wait
            && let Some(m) = &mut self.msg
        {
            if m.done_at.is_none() {
                // the rest of the text at once
                m.start = -1e12;
            } else {
                self.stop(e, "voice", 0);
                self.advance();
                self.auto = false;
            }
        }
    }

    fn window_button(&mut self, e: &mut Engine, b: &str) {
        match b {
            "ID_AUTO" => self.auto = !self.auto,
            "ID_SKIP" => self.skip = !self.skip,
            "ID_SAVE" => {
                self.ok_se(e);
                self.over.push(Over::Slots {
                    load: false,
                    page: 0,
                })
            }
            "ID_LOAD" => {
                self.ok_se(e);
                self.over.push(Over::Slots {
                    load: true,
                    page: 0,
                })
            }
            "ID_TITLE" => self.over.push(Over::Confirm {
                page: "ID_MSGBOX_TITLE",
                act: Act::Title,
            }),
            "ID_MSG_CLOSE" => self.hidden = true,
            _ => {}
        }
    }

    fn click_over(&mut self, e: &mut Engine, o: Over, x: i32, y: i32) {
        match o {
            Over::Slots { load, page } => {
                let pid = if load { "ID_LOAD_PAGE" } else { "ID_SAVE_PAGE" };
                let b = self.hit(e, Which::Win, pid, x, y);
                let b = b.as_deref().unwrap_or("");
                if let Some(p) = b.strip_prefix("ID_PAGE_") {
                    let page = p.parse().unwrap_or(page);
                    self.over.push(Over::Slots { load, page });
                } else if let Some(s) = b.strip_prefix("ID_SAVE_") {
                    let slot = page * SLOTS_PER_PAGE + s.parse::<usize>().unwrap_or(0);
                    let used = self.slot(e, slot).is_some();
                    self.over.push(Over::Slots { load, page });
                    if load {
                        if used {
                            if self.base == Base::Game {
                                self.over.push(Over::Confirm {
                                    page: "ID_MSGBOX_LOAD",
                                    act: Act::Load(slot),
                                });
                            } else {
                                self.do_act(e, Act::Load(slot));
                            }
                        }
                    } else if used {
                        self.over.push(Over::Confirm {
                            page: "ID_MSGBOX_OVERWRITE",
                            act: Act::Save(slot),
                        });
                    } else {
                        self.do_act(e, Act::Save(slot));
                    }
                } else if b == "ID_TITLE" {
                    self.over.push(Over::Slots { load, page });
                    if self.base == Base::Game {
                        self.over.push(Over::Confirm {
                            page: "ID_MSGBOX_TITLE",
                            act: Act::Title,
                        });
                    } else {
                        self.over.clear();
                    }
                } else if b != "ID_RETURN" {
                    self.over.push(Over::Slots { load, page });
                }
            }
            Over::Confirm { page, act } => match self.hit(e, Which::Win, page, x, y).as_deref() {
                Some("ID_YES") => {
                    self.ok_se(e);
                    self.do_act(e, act);
                }
                Some("ID_NO") => {}
                _ => self.over.push(Over::Confirm { page, act }),
            },
        }
    }

    fn do_act(&mut self, e: &mut Engine, act: Act) {
        match act {
            Act::Save(n) => self.save(e, n),
            Act::Load(n) => {
                if self.load(e, n) {
                    self.over.clear();
                }
            }
            Act::Title => self.back_to_title(e),
            Act::Exit => e.ended = true,
        }
    }

    /// A right click: closes a page, or hides the window.
    pub fn cancel(&mut self, e: &mut Engine) {
        if self.over.pop().is_some() {
            return;
        }
        match self.base {
            Base::Logo(step, _) => self.base = Base::Logo(step + 1, e.now),
            Base::Game => {
                self.skip = false;
                self.hidden = !self.hidden;
            }
            Base::Title => {}
        }
    }

    pub fn joy(&mut self, e: &mut Engine, button: usize) {
        match button {
            4 => {
                let (x, y) = if self.base == Base::Game && self.over.is_empty() {
                    (640, 300)
                } else {
                    self.mouse
                };
                if matches!(self.wait, Wait::Select(_)) {
                    return;
                }
                self.click(e, x, y);
            }
            5 => self.cancel(e),
            6 if self.base == Base::Game => self.skip = !self.skip,
            _ => {}
        }
    }

    // ---- frames

    pub fn frame(&mut self, e: &mut Engine) -> Result<(), String> {
        if let Base::Logo(step, t0) = self.base {
            let names = ["bland_logo", "caution"];
            if step >= names.len() {
                self.base = Base::Title;
            } else if e.now - t0 > 3500.0 {
                self.base = Base::Logo(step + 1, e.now);
            }
        }
        if let Some(t) = &self.trans
            && (e.now - t.start >= t.dur || self.skip)
        {
            self.trans = None;
        }
        if self.base == Base::Game && self.over.is_empty() {
            self.run(e);
        }
        Ok(())
    }

    pub fn draws(&mut self, e: &mut Engine) -> Vec<Draw> {
        let mut out = Vec::new();
        match self.base {
            Base::Logo(step, t0) => {
                let names = ["bland_logo", "caution"];
                if let Some(n) = names.get(step)
                    && let Some((img, _)) = self.pic(e, n)
                {
                    let t = e.now - t0;
                    let a = (t / 500.0).min(1.0).min((3500.0 - t) / 500.0).max(0.0);
                    put(&mut out, &img, whole(&img), 0.0, 0.0, a as f32);
                }
            }
            Base::Title => {
                self.page_draws(e, Which::Title, "ID_TITLE_PAGE", &HashMap::new(), &mut out);
            }
            Base::Game => self.game_draws(e, &mut out),
        }
        let overs: Vec<(bool, usize, Option<&'static str>)> = self
            .over
            .iter()
            .map(|o| match o {
                Over::Slots { load, page } => (*load, *page, None),
                Over::Confirm { page, .. } => (false, 0, Some(*page)),
            })
            .collect();
        for (load, page, confirm) in overs {
            match confirm {
                Some(p) => {
                    let dim = self.solid([0, 0, 0, 255]);
                    out.push(cover(&dim, 0.4));
                    self.page_draws(e, Which::Win, p, &HashMap::new(), &mut out);
                }
                None => self.slot_draws(e, load, page, &mut out),
            }
        }
        out
    }

    fn game_draws(&mut self, e: &mut Engine, out: &mut Vec<Draw>) {
        self.scene_draws(e, out);
        self.trans_draws(e, out);
        if let Some((img, start, count, fade)) = self.flash.clone() {
            let t = e.now - start;
            let k = (t / fade) as u32;
            if k >= count * 2 {
                self.flash = None;
            } else {
                let f = (t / fade).fract() as f32;
                let a = if k.is_multiple_of(2) { f } else { 1.0 - f };
                out.push(Draw {
                    image: img.clone(),
                    src: whole(&img),
                    dst: [0.0, 0.0, W, H],
                    place: place(a),
                });
            }
        }
        let read = self.msg.as_ref().is_none_or(|m| m.read);
        let show = self.st.window
            && !self.hidden
            && self.msg.is_some()
            && (!read || matches!(self.wait, Wait::Select(_)));
        if show {
            self.window_draws(e, out);
        }
        if let Wait::Select(opts) = &self.wait {
            let opts = opts.clone();
            let back = self.solid([16, 24, 64, 255]);
            let hi = self.solid([64, 96, 200, 255]);
            for (i, r) in select_rects(opts.len()).into_iter().enumerate() {
                let over = self.mouse.0 >= r[0]
                    && self.mouse.1 >= r[1]
                    && self.mouse.0 < r[2]
                    && self.mouse.1 < r[3];
                out.push(Draw {
                    image: if over { hi.clone() } else { back.clone() },
                    src: [0, 0, 4, 4],
                    dst: [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32],
                    place: place(0.8),
                });
                let img = self.text_image(
                    e,
                    &opts[i].0,
                    (r[2] - r[0]) as u32,
                    (r[3] - r[1]) as u32,
                    30,
                    44,
                    "center",
                );
                put(out, &img, whole(&img), r[0] as f32, r[1] as f32 + 12.0, 1.0);
            }
        }
    }

    fn window_draws(&mut self, e: &mut Engine, out: &mut Vec<Draw>) {
        let (page, name_page) = self.window_page();
        let m = self.msg.clone().unwrap_or_default();
        let total = m.text.chars().count();
        let shown = if m.done_at.is_some() {
            total
        } else {
            (m.prefix.chars().count() + ((e.now - m.start).max(0.0) / CHAR_MS) as usize).min(total)
        };
        let visible: String = m.text.chars().take(shown).collect();
        let mut texts = HashMap::new();
        texts.insert("ID_MESSAGE".to_string(), visible);
        self.page_draws(e, Which::Win, page, &texts, out);
        if !m.name.is_empty() {
            let mut t = HashMap::new();
            t.insert("ID_NAME".to_string(), m.name.clone());
            self.page_draws(e, Which::Win, name_page, &t, out);
        }
        if let Some(f) = self.st.face.clone()
            && let Some((img, hot)) = self.pic(e, &f)
        {
            put(out, &img, whole(&img), -hot.0 as f32, -hot.1 as f32, 1.0);
        }
        if m.done_at.is_some()
            && matches!(self.wait, Wait::Msg)
            && ((e.now / 500.0) as i64) % 2 == 0
        {
            let key = if self.st.mview == 1 {
                "wm_window.noa/ID_MW1_KEY_WAIT"
            } else {
                "wm_window.noa/ID_MW_KEY_WAIT"
            };
            if let Some((img, hot)) = self.pic(e, key) {
                put(out, &img, whole(&img), -hot.0 as f32, -hot.1 as f32, 1.0);
            }
        }
    }

    fn slot_draws(&mut self, e: &mut Engine, load: bool, page: usize, out: &mut Vec<Draw>) {
        let pid = if load { "ID_LOAD_PAGE" } else { "ID_SAVE_PAGE" };
        let mut texts = HashMap::new();
        let mut thumbs = Vec::new();
        for i in 0..SLOTS_PER_PAGE {
            let n = page * SLOTS_PER_PAGE + i;
            texts.insert(format!("ID_SAVE_NUM_{i}"), format!("No. {:02}", n + 1));
            match self.slot(e, n) {
                Some(s) => {
                    texts.insert(format!("ID_DATE_{i}"), format!("DATE {}", s.date));
                    texts.insert(format!("ID_COMMENT_{i}"), s.desc.clone());
                    texts.insert(format!("ID_NODATA_{i}"), String::new());
                    if let Some(t) = &s.thumb {
                        thumbs.push((i, t.clone()));
                    }
                }
                None => {
                    texts.insert(format!("ID_DATE_{i}"), "DATE ----.--.-- --:--".into());
                    texts.insert(format!("ID_COMMENT_{i}"), String::new());
                }
            }
        }
        // the page's own button is shown pushed
        texts.insert(format!("#active ID_PAGE_{page}"), String::new());
        self.page_draws(e, Which::Win, pid, &texts, out);
        let node = Xs::page_node(&self.win, pid).cloned().unwrap_or_default();
        for (i, t) in thumbs {
            if let Some(k) = node
                .kids
                .iter()
                .find(|k| attr(k, "id") == format!("ID_NODATA_{i}"))
            {
                put(
                    out,
                    &t,
                    whole(&t),
                    k.int("x", 0) as f32,
                    k.int("y", 0) as f32,
                    1.0,
                );
            }
        }
    }

    /// The items of a skin page, back first. `texts` gives the texts of static texts by id;
    /// an id mapped there that is not a text is hidden, and "#active <id>" shows a button
    /// pushed.
    fn page_draws(
        &mut self,
        e: &mut Engine,
        which: Which,
        page: &str,
        texts: &HashMap<String, String>,
        out: &mut Vec<Draw>,
    ) {
        let Some(p) = Xs::page_node(self.skin(which), page).cloned() else {
            e.warn(format!("skin: no page {page}"));
            return;
        };
        let (px, py) = (p.int("x", 0), p.int("y", 0));
        let hover = self.hit(e, which, page, self.mouse.0, self.mouse.1);
        for k in p.kids.iter().rev() {
            let id = attr(k, "id");
            let (x, y) = ((px + k.int("x", 0)) as f32, (py + k.int("y", 0)) as f32);
            match k.tag.as_str() {
                "image" => {
                    if texts.contains_key(id) {
                        continue;
                    }
                    let file = self.skin(which).image_file(attr(k, "rsrc"));
                    if let Some(f) = file
                        && let Some((img, _)) = self.pic(e, &f)
                    {
                        put(out, &img, whole(&img), x, y, 1.0);
                    }
                }
                "button" => {
                    let Some(style) = self.skin(which).styles.get(attr(k, "style")).cloned() else {
                        continue;
                    };
                    let active = texts.contains_key(&format!("#active {id}"))
                        || (page.starts_with("ID_MESSAGE_WINDOW")
                            && ((id == "ID_AUTO" && self.auto) || (id == "ID_SKIP" && self.skip)));
                    let focus = hover.as_deref() == Some(id);
                    let state = if active {
                        style.kid("active_pushed").or(style.kid("pushed"))
                    } else if focus {
                        style.kid("focus")
                    } else {
                        None
                    }
                    .or(style.kid("normal"));
                    let Some(s) = state else { continue };
                    let (iid, rect) = crate::skin::image_ref(attr(s, "image"));
                    let file = self.skin(which).image_file(&iid);
                    if let Some(f) = file
                        && let Some((img, _)) = self.pic(e, &f)
                    {
                        let r = rect.unwrap_or([0, 0, img.w as i32, img.h as i32]);
                        put(out, &img, [r[0], r[1], r[0] + r[2], r[1] + r[3]], x, y, 1.0);
                    }
                }
                "static_text" => {
                    let text = texts
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| attr(k, "text").to_string());
                    if text.is_empty() {
                        continue;
                    }
                    let style = self.skin(which).styles.get(attr(k, "style")).cloned();
                    let size = style
                        .as_ref()
                        .and_then(|s| s.kid("font"))
                        .map_or(24, |f| f.int("size", 24));
                    let arr = style.as_ref().and_then(|s| s.kid("arrange"));
                    let lh = arr.map_or(size + 6, |a| a.int("line_height", size + 6));
                    let align = arr.map_or("left", |a| attr(a, "align")).to_string();
                    let (w, h) = (k.int("width", 0).max(1), k.int("height", 0).max(1));
                    let img =
                        self.text_image(e, &text, w as u32, h as u32 + 8, size as u32, lh, &align);
                    put(out, &img, whole(&img), x, y, 1.0);
                }
                _ => {}
            }
        }
    }

    /// Text laid out in a box: wrapped at the width, one `line_height` a line.
    #[allow(clippy::too_many_arguments)]
    fn text_image(
        &mut self,
        e: &mut Engine,
        text: &str,
        w: u32,
        h: u32,
        size: u32,
        line_height: i32,
        align: &str,
    ) -> Rc<Image> {
        let key = format!("{w}x{h}/{size}/{line_height}/{align}/{text}");
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        // lines of (char, x)
        let mut lines: Vec<Vec<(char, i32)>> = vec![Vec::new()];
        let mut pen = 0;
        for c in text.chars() {
            if c == '\n' {
                lines.push(Vec::new());
                pen = 0;
                continue;
            }
            let adv = e.fonts.advance(c, size);
            let closing = "、。，．」』）】！？…ー".contains(c);
            if pen + adv > w as i32 && pen > 0 && !closing {
                lines.push(Vec::new());
                pen = 0;
            }
            lines.last_mut().unwrap().push((c, pen));
            pen += adv;
        }
        let mut img = Image::new(w, h);
        for (i, l) in lines.iter().enumerate() {
            let width = l.last().map_or(0, |(c, x)| x + e.fonts.advance(*c, size));
            let off = match align {
                "center" => (w as i32 - width) / 2,
                "right" => w as i32 - width,
                _ => 0,
            };
            for (c, x) in l {
                e.fonts.draw(
                    &mut img,
                    off + x,
                    i as i32 * line_height,
                    *c,
                    size,
                    [255, 255, 255],
                    Some([0, 0, 0]),
                );
            }
        }
        if self.text_cache.len() > 200 {
            self.text_cache.clear();
        }
        let img = Rc::new(img);
        self.text_cache.insert(key, img.clone());
        img
    }

    // ---- saves

    fn slot_name(n: usize) -> String {
        format!("xs-save-{:02}.txt", n + 1)
    }

    fn slot(&mut self, e: &mut Engine, n: usize) -> Option<Rc<SlotInfo>> {
        if let Some(s) = self.slots.get(&n) {
            return s.clone();
        }
        let info = e.fs.host.read_save(&Xs::slot_name(n)).map(|b| {
            let text = String::from_utf8_lossy(&b).into_owned();
            let mut date = String::new();
            let mut desc = String::new();
            let mut thumb = None;
            for line in text.lines() {
                let p: Vec<&str> = line.splitn(2, '\t').collect();
                match p[..] {
                    ["date", v] => date = v.to_string(),
                    ["desc", v] => desc = unesc(v),
                    ["thumb", v] => thumb = unhex_thumb(v),
                    _ => {}
                }
            }
            Rc::new(SlotInfo { date, desc, thumb })
        });
        self.slots.insert(n, info.clone());
        info
    }

    fn save(&mut self, e: &mut Engine, n: usize) {
        let Some((depth, pc)) = self.msg_at else {
            e.warn("xscript: nothing to save yet".into());
            return;
        };
        let mut out = String::from("xscript-save 1\n");
        let date = match e.clock {
            Some(ms) => {
                let secs = (ms / 1000.0) as i64;
                let (y, m, d) = civil(secs.div_euclid(86_400));
                let t = secs.rem_euclid(86_400);
                format!("{y:04}.{m:02}.{d:02} {:02}:{:02}", t / 3600, t / 60 % 60)
            }
            None => "0000.00.00 00:00".into(),
        };
        out += &format!("date\t{date}\n");
        out += &format!("desc\t{}\n", esc(&self.desc));
        for (i, (s, p)) in self.stack.iter().take(depth).enumerate() {
            let at = if i + 1 == depth { pc } else { *p };
            out += &format!("stack\t{}\t{at}\n", s.name);
        }
        let prefix = self
            .msg
            .as_ref()
            .map_or(String::new(), |m| m.prefix.clone());
        out += &format!("prefix\t{}\n", esc(&prefix));
        let st = &self.st;
        if let Some(b) = &st.bg {
            out += &format!("bg\t{b}\n");
        }
        if let Some(b) = &st.ev {
            out += &format!("ev\t{b}\n");
        }
        for c in &st.chars {
            out += &format!("char\t{}\t{}\t{}\n", c.id, c.src, c.x);
        }
        if let Some(f) = &st.face {
            out += &format!("face\t{f}\n");
        }
        for l in &st.layers {
            out += &format!("layer\t{}\t{}\t{}\t{}\t{}\n", l.id, l.src, l.x, l.y, l.z);
        }
        if let Some(f) = &st.filter {
            out += &format!("filter\t{f}\n");
        }
        if let Some((b, v)) = &st.bgm {
            out += &format!("bgm\t{b}\t{v}\n");
        }
        out += &format!("mview\t{}\n", st.mview);
        let mut keys: Vec<&String> = self.vars.keys().filter(|k| !k.starts_with("s.")).collect();
        keys.sort();
        for k in keys {
            out += "var\t";
            out += &var_line(k, &self.vars[k]);
        }
        // the thumbnail: the scene, made small
        let mut d = Vec::new();
        self.scene_draws(e, &mut d);
        let full = Xs::render(&d);
        let (tw, th) = THUMB;
        let mut hex = format!("{tw} {th} ");
        for ty in 0..th {
            for tx in 0..tw {
                let sx = tx * full.w / tw;
                let sy = ty * full.h / th;
                let i = ((sy * full.w + sx) * 4) as usize;
                for c in &full.px[i..i + 3] {
                    hex += &format!("{c:02x}");
                }
            }
        }
        out += &format!("thumb\t{hex}\n");
        e.fs.host.write_save(&Xs::slot_name(n), out.as_bytes());
        self.slots.remove(&n);
        self.play(e, "sys", "wm_window.noa/SE_SAVED", 1.0, false);
    }

    fn load(&mut self, e: &mut Engine, n: usize) -> bool {
        let Some(b) = e.fs.host.read_save(&Xs::slot_name(n)) else {
            return false;
        };
        let text = String::from_utf8_lossy(&b).into_owned();
        let mut stack = Vec::new();
        let mut st = Scene {
            window: true,
            ..Scene::default()
        };
        let mut prefix = String::new();
        let mut desc = String::new();
        let mut vars: Vec<(String, Val)> = Vec::new();
        for line in text.lines() {
            let p: Vec<&str> = line.split('\t').collect();
            match p[..] {
                ["stack", s, at] => match self.scripts.get(s) {
                    Some(sc) => stack.push((sc.clone(), at.parse().unwrap_or(0))),
                    None => {
                        e.warn(format!("{}: no script {s}", Xs::slot_name(n)));
                        return false;
                    }
                },
                ["desc", v] => desc = unesc(v),
                ["prefix", v] => prefix = unesc(v),
                ["bg", v] => st.bg = Some(v.to_string()),
                ["ev", v] => st.ev = Some(v.to_string()),
                ["char", id, src, x] => st.chars.push(Char {
                    id: id.to_string(),
                    src: src.to_string(),
                    x: x.parse().unwrap_or(0),
                }),
                ["face", v] => st.face = Some(v.to_string()),
                ["layer", id, src, x, y, z] => st.layers.push(Layer {
                    id: id.to_string(),
                    src: src.to_string(),
                    x: x.parse().unwrap_or(0),
                    y: y.parse().unwrap_or(0),
                    z: z.parse().unwrap_or(0),
                }),
                ["filter", v] => st.filter = Some(v.to_string()),
                ["bgm", s, v] => st.bgm = Some((s.to_string(), v.parse().unwrap_or(1.0))),
                ["mview", v] => st.mview = v.parse().unwrap_or(0),
                ["var", k, t, v] => vars.push((
                    unesc(k),
                    if t == "N" {
                        Val::N(v.parse().unwrap_or(0.0))
                    } else {
                        Val::S(unesc(v))
                    },
                )),
                _ => {}
            }
        }
        if stack.is_empty() {
            return false;
        }
        self.start_game(e);
        self.vars.extend(vars);
        self.stack = stack;
        self.desc = desc;
        let bgm = st.bgm.clone();
        self.st = st;
        if let Some((b, v)) = bgm {
            self.play(e, "bgm", &b, v, true);
        }
        // the message saved is shown again, after the text before it
        self.msg = (!prefix.is_empty()).then(|| Msg {
            text: prefix,
            ..Msg::default()
        });
        true
    }
}

#[derive(Clone, Copy)]
enum Which {
    Title,
    Win,
}

fn var_line(k: &str, v: &Val) -> String {
    match v {
        Val::N(n) => format!("{}\tN\t{n}\n", esc(k)),
        Val::S(s) => format!("{}\tS\t{}\n", esc(k), esc(s)),
    }
}

fn unhex_thumb(v: &str) -> Option<Rc<Image>> {
    let mut it = v.splitn(3, ' ');
    let w: u32 = it.next()?.parse().ok()?;
    let h: u32 = it.next()?.parse().ok()?;
    let hex = it.next()?.as_bytes();
    if hex.len() < (w * h * 6) as usize {
        return None;
    }
    let mut img = Image::new(w, h);
    let d = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
    for i in 0..(w * h) as usize {
        for c in 0..3 {
            let j = (i * 3 + c) * 2;
            img.px[i * 4 + c] = d(hex[j]) << 4 | d(hex[j + 1]);
        }
        img.px[i * 4 + 3] = 255;
    }
    Some(Rc::new(img))
}

/// A picture stretched over the screen.
fn cover(img: &Rc<Image>, alpha: f32) -> Draw {
    Draw {
        image: img.clone(),
        src: whole(img),
        dst: [0.0, 0.0, W, H],
        place: place(alpha.clamp(0.0, 1.0)),
    }
}

/// Where the options of a `<select>` are (x0 y0 x1 y1).
fn select_rects(n: usize) -> Vec<[i32; 4]> {
    let (w, h, gap) = (720, 64, 24);
    let top = (440 - (n as i32 * (h + gap) - gap)) / 2;
    (0..n as i32)
        .map(|i| {
            let y = top.max(20) + i * (h + gap);
            [(1280 - w) / 2, y, (1280 + w) / 2, y + h]
        })
        .collect()
}
