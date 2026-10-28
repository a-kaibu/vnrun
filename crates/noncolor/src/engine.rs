//! Runs a NonColor game's scenes a frame at a time. The games' own system scripts draw
//! everything through sprite commands and a Squirrel UI; here only the scene commands are
//! read, at the level the scenario writers use them (the `defsub` macros of `DAY`):
//!
//! - text lines and `【name,voice】「line」` (`[n]` breaks a line, the voice is
//!   `vo/<speaker>/<voice>.ogg`)
//! - `BG`/`EV name, time@keep, effect, x@y` a new background (without `@1` the characters and
//!   CGs go too), `EV.C`/`BG.C` only the picture; `BG.XY`/`BG.WH` move and zoom it
//! - `ST code, time, x@y` a character (`S001_1AA2AA_000_M`: `st/001M/`, either the whole
//!   picture or a face whose TLG tags name its body and its place there), `ST0 slot, ...`,
//!   `ST2` (positions from the `x0..` table), `ST.DEL`/`ST2.DEL`, `ST.X`/`ST.XY`/`ST2.MOV`
//! - `CG n, path, time, x@y` a picture over them, `CG.DEL`, `CG.XY`/`CG.WH`/`CG.A`
//! - `BGM`, `SE0`..`SE2`, `VO`; `WA ms` waits; `SELECT "text,*label", ...`; `goto`,
//!   `gosub`, `if`/`elseif`/`else`, `rand`, `cal`; `TL` returns to the title
//!
//! The effects (shakes, flashes, emotes, wipes) are left out. The title, the window, the
//! choices and the save slots are drawn from the `sys/title` and `sys/window` pictures.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::arc::Archives;
use crate::script::{self, Scripts, Vars};

pub const W: u32 = 960;
pub const H: u32 = 540;
const SLOTS: usize = 10;
const BG: i32 = 998;
const ST: i32 = 920;
const CG: i32 = 900;

#[derive(Clone, Copy, PartialEq)]
enum Act {
    None,
    New,
    Continue,
    Save,
    Load,
    QSave,
    QLoad,
    Skip,
    Quit,
}

/// How a game lays out its title and window (from its `pos_setting` script).
struct Skin {
    /// pictures under the title buttons: name, left, top
    title: &'static [(&'static str, i32, i32)],
    /// the buttons (three states stacked), centred from `buttons_at` down `step`
    title_buttons: &'static [(&'static str, Act)],
    buttons_at: (i32, i32),
    step: i32,
    window: (i32, i32),
    plate: (i32, i32),
    /// the name: x (its centre when `name_centred`), y (its middle when centred, else top)
    name: (i32, i32),
    name_centred: bool,
    text: (i32, i32),
    size: i32,
    line: i32,
    wrap: usize,
    bar: (i32, i32),
    /// the window's buttons: picture (three states stacked), left, top, what it does
    buttons: &'static [(&'static str, i32, Act)],
    buttons_y: i32,
}

/// 宿星のガールフレンド２ (2018)
const SKIN_2018: Skin = Skin {
    title: &[
        ("sys/title/base", 0, 0),
        ("sys/title/chara", 0, 0),
        ("sys/title/title_logo2", 532, 20),
        ("sys/title/title_logo_1", 546, 122),
    ],
    title_buttons: &[
        ("sys/title/continue", Act::Continue),
        ("sys/title/start", Act::New),
        ("sys/title/load", Act::Load),
        ("sys/title/config", Act::None),
        ("sys/title/exit", Act::Quit),
    ],
    buttons_at: (622, 189),
    step: 51,
    window: (0, 388),
    plate: (184, 379),
    name: (220, 404),
    name_centred: false,
    text: (200, 434),
    size: 20,
    line: 28,
    wrap: 28,
    bar: (468, 513),
    buttons: &[
        ("sys/window/voice", 503, Act::None),
        ("sys/window/back", 524, Act::None),
        ("sys/window/log", 545, Act::None),
        ("sys/window/auto", 566, Act::None),
        ("sys/window/skip", 587, Act::Skip),
        ("sys/window/next", 608, Act::None),
        ("sys/window/qsave", 636, Act::QSave),
        ("sys/window/qload", 697, Act::QLoad),
        ("sys/window/save", 759, Act::Save),
        ("sys/window/load", 808, Act::Load),
        ("sys/window/config", 858, Act::None),
        ("sys/window/close", 920, Act::None),
    ],
    buttons_y: 523,
};

/// 姉小路直子と銀色の死神 (2015)
const SKIN_2015: Skin = Skin {
    title: &[
        ("sys/title/base", 0, 0),
        ("sys/title/chara", 0, 0),
        ("sys/title/title_logo1", 335, 0),
    ],
    title_buttons: &[
        ("sys/title/continue", Act::Continue),
        ("sys/title/start", Act::New),
        ("sys/title/load", Act::Load),
        ("sys/title/config", Act::None),
        ("sys/title/tutorial", Act::None),
        ("sys/title/exit", Act::Quit),
    ],
    buttons_at: (380, 180),
    step: 56,
    window: (0, 366),
    plate: (214, 368),
    name: (324, 388),
    name_centred: true,
    text: (200, 410),
    size: 24,
    line: 36,
    wrap: 26,
    bar: (0, 510),
    buttons: &[
        ("sys/window/voice", 489, Act::None),
        ("sys/window/back", 514, Act::None),
        ("sys/window/log", 539, Act::None),
        ("sys/window/auto", 564, Act::None),
        ("sys/window/skip", 589, Act::Skip),
        ("sys/window/next", 614, Act::None),
        ("sys/window/qsave", 639, Act::QSave),
        ("sys/window/qload", 704, Act::QLoad),
        ("sys/window/save", 769, Act::Save),
        ("sys/window/load", 819, Act::Load),
        ("sys/window/config", 869, Act::None),
        ("sys/window/close", 929, Act::None),
    ],
    buttons_y: 514,
};

pub struct SoundEvent {
    pub channel: String,
    /// absent: stop the channel
    pub data: Option<Vec<u8>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: f32,
}

/// A picture on a layer: `src` names it for saves (`bg/...`, or `st:` and a character code).
#[derive(Clone)]
struct Spr {
    src: String,
    img: Rc<Image>,
    /// where the script puts it
    x: f32,
    y: f32,
    /// which point of the picture goes there (0..1 of its size), moved by `oy` pixels
    ax: f32,
    ay: f32,
    oy: f32,
    sx: f32,
    sy: f32,
    alpha: f32,
}

#[derive(Clone, Default)]
struct Layer {
    cur: Option<Spr>,
    /// the picture going: faded out, or kept under the new one (`keep`)
    old: Option<Spr>,
    keep: bool,
    t: u32,
    dur: u32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Wait {
    None,
    Click,
    Frames(u32),
    Fades,
    Title,
    Choice,
    Ended,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Game,
    Slots { save: bool },
}

pub struct Engine {
    files: Box<dyn Files>,
    arcs: Archives,
    scripts: Scripts,
    pics: HashMap<String, Option<Rc<Image>>>,
    pc: usize,
    stack: Vec<usize>,
    /// whether the last `if`/`elseif` of a chain ran
    taken: bool,
    /// the line of the message or choice shown (where a load starts again)
    msg_at: usize,
    consts: Vars,
    vars: Vars,
    layers: BTreeMap<i32, Layer>,
    text: String,
    name: Option<String>,
    chapter: String,
    window: bool,
    wait: Wait,
    view: View,
    choices: Vec<(String, String)>,
    pointer: (i32, i32),
    sounds: Vec<SoundEvent>,
    /// the voice channels speaking
    voices: usize,
    bgm: Option<String>,
    loops: BTreeMap<String, String>,
    title_bgm: Option<String>,
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<(String, i32), Rc<Image>>,
    skip: bool,
    rng: u32,
    skin: &'static Skin,
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

/// The `j`th `@` field of an argument.
fn field(a: &[String], i: usize, j: usize) -> &str {
    a.get(i)
        .and_then(|s| s.split('@').nth(j))
        .map(str::trim)
        .unwrap_or("")
}

/// A message: `[n]` breaks the line, `[i n]` (indents) go.
fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        out += &rest[..i];
        let Some(j) = rest[i..].find(']') else {
            out += &rest[i..];
            rest = "";
            break;
        };
        if &rest[i..i + j + 1] == "[n]" {
            out.push('\n');
        }
        rest = &rest[i + j + 1..];
    }
    out + rest
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arcs = Archives::open(files.as_mut());
        let mut texts = Vec::new();
        let mut es: Vec<_> = arcs.entries_of("script.dat").cloned().collect();
        es.sort_by_key(|e| e.hash);
        for e in es {
            if let Some(d) = arcs.read_entry(files.as_mut(), &e, &[]) {
                texts.push(d);
            }
        }
        let scripts = Scripts::parse(&texts);
        let Some(&start) = scripts.labels.get("start") else {
            return Err("script.dat: no *start".into());
        };
        let skin = if arcs.exists("sys/title/tutorial.tlg") {
            &SKIN_2015
        } else {
            &SKIN_2018
        };
        let mut consts = Vars::new();
        for l in &scripts.lines {
            let Some(rest) = l.strip_prefix("int ") else {
                continue;
            };
            let first = script::split_top(rest, ':').swap_remove(0);
            for def in script::split_top(&first, ',') {
                let Some((k, v)) = def.split_once('=') else {
                    continue;
                };
                let k = k.trim();
                if k.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && let Some(v) = script::eval(v, &consts)
                {
                    consts.insert(k.to_string(), v);
                }
            }
        }
        let title_bgm = scripts.lines.iter().find_map(|l| {
            let r = l.strip_prefix("string title_bgm=\"")?;
            Some(r[..r.find('"')?].to_string())
        });
        let mut e = Engine {
            files,
            arcs,
            pics: HashMap::new(),
            pc: start,
            stack: Vec::new(),
            taken: false,
            msg_at: start,
            vars: consts.clone(),
            consts,
            layers: BTreeMap::new(),
            text: String::new(),
            name: None,
            chapter: String::new(),
            window: false,
            wait: Wait::Title,
            view: View::Game,
            choices: Vec::new(),
            pointer: (0, 0),
            sounds: Vec::new(),
            voices: 0,
            bgm: None,
            loops: BTreeMap::new(),
            title_bgm,
            font: None,
            text_cache: HashMap::new(),
            skip: false,
            rng: 0x2545_f491,
            skin,
            trace: std::env::var("NONCOLOR_LOG").is_ok(),
            frames: 0,
            scripts,
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

    fn konst(&self, k: &str, default: i64) -> i64 {
        self.consts.get(k).copied().unwrap_or(default)
    }

    fn num(&self, s: &str, default: i64) -> i64 {
        if s.is_empty() {
            return default;
        }
        script::eval(s, &self.vars).unwrap_or(default)
    }

    fn title_screen(&mut self) {
        self.layers.clear();
        self.window = false;
        self.text.clear();
        self.name = None;
        self.choices.clear();
        self.skip = false;
        for c in ["se0", "se1", "se2"] {
            self.stop(c, 300.0);
        }
        self.stop_voices();
        self.loops.clear();
        self.bgm = None;
        if let Some(b) = self.title_bgm.clone() {
            self.play_bgm(&b);
        }
        self.wait = Wait::Title;
    }

    fn new_game(&mut self) {
        self.layers.clear();
        self.stop("bgm", 300.0);
        self.bgm = None;
        self.vars = self.consts.clone();
        self.stack.clear();
        self.chapter.clear();
        self.pc = self.scripts.labels["start"];
        self.wait = Wait::None;
    }

    // ---- files ----

    fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        let d = self.arcs.read(self.files.as_mut(), name);
        if d.is_none() {
            self.log(&format!("{name}: not found"));
        }
        d
    }

    /// A TLG picture by its path without `.tlg`.
    fn tlg(&mut self, path: &str) -> Option<crate::tlg::Tlg> {
        let d = self.read(&format!("{path}.tlg"))?;
        match crate::tlg::decode(&d) {
            Ok(t) => Some(t),
            Err(e) => {
                self.log(&format!("{path}: {e}"));
                None
            }
        }
    }

    fn cached(
        &mut self,
        key: &str,
        load: impl FnOnce(&mut Self) -> Option<Image>,
    ) -> Option<Rc<Image>> {
        if let Some(p) = self.pics.get(key) {
            return p.clone();
        }
        let p = load(self).map(Rc::new);
        if self.pics.len() > 40 {
            self.pics.clear();
        }
        self.pics.insert(key.to_string(), p.clone());
        p
    }

    fn pic(&mut self, path: &str) -> Option<Rc<Image>> {
        self.cached(path, |e| e.tlg(path).map(|t| t.img))
    }

    /// A character: `S001_1AA2AA_010_M` is the face `st/001M/2_010` (the 4th letter of the
    /// pose, the expression) on the body `st/001M/S001_1AA2AA_000_M` where the face's tags
    /// say, or else the whole picture `st/001M/S001_1AA2AA_010_M`.
    fn stand(&mut self, code: &str) -> Option<Rc<Image>> {
        let key = format!("st:{code}");
        let code = code.to_string();
        self.cached(&key, move |e| {
            let p: Vec<&str> = code.split('_').collect();
            if p.len() < 4 || p[0].len() < 2 {
                return None;
            }
            let dir = format!("st/{}{}", &p[0][1..], p[3]);
            let face = format!("{dir}/{}_{}", p[1].get(3..4)?, p[2]);
            if !e.arcs.exists(&format!("{face}.tlg")) {
                return e.tlg(&format!("{dir}/{code}")).map(|t| t.img);
            }
            let face = e.tlg(&face)?;
            let body = format!("{dir}/{}_{}_000_{}", p[0], p[1], p[3]);
            let mut img = match e.tlg(&body) {
                Some(b) => b.img,
                None => {
                    let b = String::from_utf8_lossy(face.tags.get(&b"\x01"[..])?).into_owned();
                    e.tlg(&format!("{dir}/{}", b.trim_end_matches(".tlg")))?.img
                }
            };
            let (x, y) = face.offset().unwrap_or((0, 0));
            let (w, h) = (face.img.w as f32, face.img.h as f32);
            let (x, y) = (x as f32, y as f32);
            nexas::gfx::draw(
                &mut img,
                &face.img,
                [0, 0, w as i32, h as i32],
                [x, y, x + w, y + h],
                &Place {
                    clip: [0.0, 0.0, f32::MAX, f32::MAX],
                    ..place(1.0)
                },
            );
            Some(img)
        })
    }

    fn spr(&mut self, src: &str) -> Option<Spr> {
        let img = match src.strip_prefix("st:") {
            Some(code) => self.stand(code)?,
            None => self.pic(src)?,
        };
        Some(Spr {
            src: src.to_string(),
            img,
            x: (W / 2) as f32,
            y: (H / 2) as f32,
            ax: 0.5,
            ay: 0.5,
            oy: 0.0,
            sx: 1.0,
            sy: 1.0,
            alpha: 1.0,
        })
    }

    // ---- sound ----

    fn play(&mut self, channel: &str, path: &str, repeat: bool, volume: f32) {
        if let Some(d) = self.read(path) {
            self.sounds.push(SoundEvent {
                channel: channel.into(),
                data: Some(d),
                volume,
                repeat,
                fade_ms: 0.0,
            });
        }
    }

    fn play_bgm(&mut self, name: &str) {
        self.play("bgm", &format!("bgm/{name}.ogg"), true, 0.6);
        self.bgm = Some(name.to_string());
    }

    fn stop(&mut self, channel: &str, fade_ms: f32) {
        self.sounds.push(SoundEvent {
            channel: channel.into(),
            data: None,
            volume: 0.0,
            repeat: false,
            fade_ms,
        });
    }

    /// Voices (several speak together in `v1,v2`): `S001_...` is `vo/001/S001_....ogg`.
    fn voice(&mut self, v: &str) {
        self.stop_voices();
        for (i, v) in v.split(',').map(str::trim).enumerate().take(4) {
            let who = v.split('_').next().unwrap_or("");
            if who.len() > 1 && !self.skip {
                let ch = if i == 0 {
                    "voice".into()
                } else {
                    format!("voice{i}")
                };
                self.play(&ch, &format!("vo/{}/{v}.ogg", &who[1..]), false, 1.0);
                self.voices = i + 1;
            }
        }
    }

    fn stop_voices(&mut self) {
        self.stop("voice", 0.0);
        for i in 1..std::mem::take(&mut self.voices) {
            self.stop(&format!("voice{i}"), 0.0);
        }
    }

    // ---- layers ----

    fn frames_of(&self, ms: i64) -> u32 {
        if self.skip {
            0
        } else {
            (ms.max(0) * 60 / 1000) as u32
        }
    }

    /// Puts a picture on a layer (or takes it off), fading over `ms`.
    fn set_layer(&mut self, k: i32, s: Option<Spr>, ms: i64, keep: bool) {
        let dur = self.frames_of(ms);
        let l = self.layers.entry(k).or_default();
        l.old = if dur > 0 { l.cur.take() } else { None };
        l.keep = keep && s.is_some();
        l.cur = s;
        l.t = 0;
        l.dur = dur;
    }

    fn clear_range(&mut self, from: i32, to: i32, ms: i64) {
        let ks: Vec<i32> = self
            .layers
            .range(from..=to)
            .filter(|(_, l)| l.cur.is_some())
            .map(|(k, _)| *k)
            .collect();
        for k in ks {
            self.set_layer(k, None, ms, false);
        }
    }

    fn cur(&mut self, k: i32) -> Option<&mut Spr> {
        self.layers.get_mut(&k)?.cur.as_mut()
    }

    fn tick_layers(&mut self) {
        for l in self.layers.values_mut() {
            if l.t < l.dur {
                l.t += 1;
            }
            if l.t >= l.dur || self.skip {
                l.t = l.dur;
                l.old = None;
            }
        }
    }

    fn fading(&self) -> bool {
        self.layers.values().any(|l| l.t < l.dur)
    }

    // ---- running ----

    pub fn frame(&mut self) -> Result<(), String> {
        self.frames += 1;
        self.tick_layers();
        if self.view != View::Game {
            return Ok(());
        }
        self.wait = match self.wait {
            Wait::Frames(n) if n <= 1 || self.skip => Wait::None,
            Wait::Frames(n) => Wait::Frames(n - 1),
            Wait::Fades if !self.fading() => Wait::None,
            Wait::Click if self.skip && self.frames.is_multiple_of(3) => {
                self.advance();
                Wait::None
            }
            w => w,
        };
        let mut n = 0;
        while self.wait == Wait::None && self.view == View::Game {
            n += 1;
            if n > 20000 {
                return Err(format!("no stop at line {}", self.pc));
            }
            self.step();
        }
        Ok(())
    }

    fn step(&mut self) {
        let Some(line) = self.scripts.lines.get(self.pc).cloned() else {
            self.wait = Wait::Ended;
            return;
        };
        let at = self.pc;
        self.pc += 1;
        let Some(c) = line.chars().next() else {
            return;
        };
        if c == '*' || c == ';' {
            return;
        }
        if c == '【' {
            self.log(&format!("{at}: {line}"));
            let Some(end) = line.find('】') else {
                return;
            };
            let head = &line['【'.len_utf8()..end];
            let (name, voice) = head.split_once(',').unwrap_or((head, ""));
            let name = name.rsplit('@').next().unwrap_or(name).trim();
            if !voice.trim().is_empty() {
                self.voice(voice.trim());
            } else {
                self.stop_voices();
            }
            self.message(
                at,
                (!name.is_empty()).then(|| name.to_string()),
                &line[end + '】'.len_utf8()..],
            );
            return;
        }
        if !(c.is_ascii_alphabetic() || c == '_') {
            self.log(&format!("{at}: {line}"));
            self.stop_voices();
            self.message(at, None, &line);
            return;
        }
        for s in script::split_top(&line, ':') {
            let pc = self.pc;
            self.statement(at, s.trim());
            if self.pc != pc || self.wait != Wait::None {
                break;
            }
        }
    }

    fn message(&mut self, at: usize, name: Option<String>, text: &str) {
        self.text = clean(text);
        self.name = name;
        self.window = true;
        self.msg_at = at;
        self.wait = Wait::Click;
    }

    fn jump(&mut self, label: &str) {
        let l = label.trim().trim_start_matches('*');
        match self.scripts.labels.get(l) {
            Some(&i) => self.pc = i,
            None => {
                self.log(&format!("*{l}: no such label"));
                self.wait = Wait::Ended;
            }
        }
    }

    /// `if(...)` etc.: the condition and the rest of the line.
    fn condition<'a>(&self, s: &'a str) -> Option<(bool, &'a str)> {
        let open = s.find('(')?;
        let mut depth = 0;
        for (i, c) in s[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        let e = &s[open..open + i + 1];
                        let v = script::eval(e, &self.vars).unwrap_or(0);
                        return Some((v != 0, s[open + i + 1..].trim()));
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn statement(&mut self, at: usize, s: &str) {
        if s.starts_with("if(") || s.starts_with("if ") {
            if let Some((v, rest)) = self.condition(s) {
                self.taken = v;
                if v {
                    self.statement(at, rest);
                }
            }
            return;
        }
        if s.starts_with("elseif") {
            if !self.taken
                && let Some((v, rest)) = self.condition(s)
                && v
            {
                self.taken = true;
                self.statement(at, rest);
            }
            return;
        }
        if let Some(rest) = s.strip_prefix("else") {
            if !self.taken {
                self.taken = true;
                self.statement(at, rest.trim());
            }
            return;
        }
        let (name, a) = script::command(s);
        if self.trace && !matches!(name.as_str(), "int" | "string" | "defsub" | "funcSqu") {
            self.log(&format!("{at}: {s}"));
        }
        self.command(at, &name, &a);
    }

    fn rand(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    fn command(&mut self, at: usize, name: &str, a: &[String]) {
        let s = |i: usize| a.get(i).map(|s| s.trim()).unwrap_or("");
        let f = |i: usize, j: usize| field(a, i, j);
        match name {
            "goto" => self.jump(s(0)),
            "gosub" => {
                self.stack.push(self.pc);
                self.jump(s(0));
            }
            "return" => match self.stack.pop() {
                Some(pc) => self.pc = pc,
                None => self.wait = Wait::Ended,
            },
            "rand" => {
                let (lo, hi) = (self.num(s(1), 0), self.num(s(2), 0));
                let v = lo + (self.rand() as i64).rem_euclid((hi - lo + 1).max(1));
                if let Some(k) = s(0).strip_prefix('%') {
                    self.vars.insert(k.to_string(), v);
                }
            }
            "cal" => {
                for e in a {
                    let Some((k, v)) = e.split_once('=') else {
                        continue;
                    };
                    let Some(k) = k.trim().strip_prefix('%') else {
                        continue;
                    };
                    let (k, op) = match k.chars().last() {
                        Some(c @ ('+' | '-' | '*' | '/')) => (&k[..k.len() - 1], Some(c)),
                        _ => (k, None),
                    };
                    let Some(v) = script::eval(v, &self.vars) else {
                        continue;
                    };
                    let old = self.vars.get(k).copied().unwrap_or(0);
                    let v = match op {
                        Some('+') => old + v,
                        Some('-') => old - v,
                        Some('*') => old * v,
                        Some('/') => old.checked_div(v).unwrap_or(0),
                        _ => v,
                    };
                    self.vars.insert(k.to_string(), v);
                }
            }
            "CS" => self.chapter = script::unquote(s(0)).trim().to_string(),
            "TL" => self.title_screen(),
            "ED" => self.wait = Wait::Ended,
            "SELECT" => {
                self.choices = a
                    .iter()
                    .filter_map(|c| {
                        let (t, l) = script::unquote(c).rsplit_once(",*")?;
                        Some((t.to_string(), l.to_string()))
                    })
                    .collect();
                if !self.choices.is_empty() {
                    self.msg_at = at;
                    self.wait = Wait::Choice;
                }
            }
            "WA" => {
                if self.num(s(1), 1) != 0 {
                    self.window = false;
                }
                self.wait = Wait::Frames(self.frames_of(self.num(s(0), 0)));
            }
            "MW.VW" if self.num(s(0), 1) == 0 => self.window = false,
            "CL" => {
                self.window = false;
                let ms = self.num(s(0), 1000);
                self.clear_range(0, 999, ms);
                self.stop("bgm", ms as f32);
                self.bgm = None;
                for c in ["se0", "se1", "se2"] {
                    self.stop(c, ms as f32);
                }
                self.loops.clear();
                self.wait = Wait::Fades;
            }
            "BGM" => {
                if s(0).is_empty() {
                    self.stop("bgm", self.num(f(1, 0), 0) as f32);
                    self.bgm = None;
                } else if self.bgm.as_deref() != Some(s(0)) {
                    self.play_bgm(s(0));
                }
            }
            "SE0" | "SE1" | "SE2" => {
                let ch = name.to_lowercase();
                if s(0).is_empty() {
                    self.stop(&ch, self.num(f(1, 0), 0) as f32);
                    self.loops.remove(&ch);
                } else if !self.skip || self.num(s(3), 1) == 0 {
                    let repeat = self.num(s(3), 1) == 0;
                    let vol = self.num(f(2, 0), 100) as f32 / 100.0;
                    self.play(&ch, &format!("se/{}.ogg", s(0)), repeat, vol * 0.8);
                    if repeat {
                        self.loops.insert(ch, s(0).to_string());
                    } else {
                        self.loops.remove(&ch);
                    }
                }
            }
            "VO" => self.voice(s(0)),
            "BG" | "EV" | "BG.C" | "EV.C" => self.background(name, a),
            "BG.XY" | "EV.XY" => {
                let (x, y) = (self.num(s(1), 480), self.num(s(2), 270));
                if let Some(b) = self.cur(BG) {
                    (b.x, b.y) = (x as f32, y as f32);
                }
            }
            "BG.WH" | "EV.WH" => {
                let (w, h) = (self.num(s(1), 100), self.num(s(2), 100));
                self.vars.insert("_bgw".into(), w);
                self.vars.insert("_bgh".into(), h);
                if let Some(b) = self.cur(BG) {
                    (b.sx, b.sy) = (w as f32 / 100.0, h as f32 / 100.0);
                }
            }
            "ST" | "ST0" | "ST2" => self.character(name, a),
            "ST.DEL" | "ST2.DEL" | "ST.DL" => {
                let n = self.num(s(0), 99);
                let ms = self.num(f(1, 0), 250);
                if n == 99 {
                    self.clear_range(ST, ST + 99, ms);
                } else {
                    self.set_layer(ST + n as i32, None, ms, false);
                }
            }
            "ST.X" | "ST.Y" | "ST.XY" | "ST.A" | "ST2.MOV" => {
                let k = ST + self.num(s(0), 0) as i32;
                let sty = self.konst("StY", 240) as f32;
                let (x, y) = match name {
                    "ST.X" => (Some(self.num(s(2), 480)), None),
                    "ST.Y" => (None, Some(self.num(s(2), 0))),
                    "ST.XY" => (Some(self.num(s(2), 480)), Some(self.num(s(3), 0))),
                    "ST2.MOV" => (
                        (!f(2, 0).is_empty()).then(|| self.xpos(self.num(f(2, 0), 0))),
                        (!f(2, 1).is_empty()).then(|| self.num(f(2, 1), 0)),
                    ),
                    _ => (None, None),
                };
                let alpha = match name {
                    "ST.A" => Some(self.num(s(2), 255)),
                    "ST2.MOV" if !f(2, 2).is_empty() => Some(self.num(f(2, 2), 255)),
                    _ => None,
                };
                if let Some(p) = self.cur(k) {
                    if let Some(x) = x {
                        p.x = x as f32;
                    }
                    if let Some(y) = y {
                        p.y = sty + y as f32;
                    }
                    if let Some(a) = alpha {
                        p.alpha = a as f32 / 255.0;
                    }
                }
            }
            "CG" | "CGS" | "CGC" => {
                let n = self.num(s(0), 0) as i32;
                let path = f(1, 0).to_string();
                let ms = self.num(f(2, 0), 500);
                let (x, y) = (self.num(f(3, 0), 480), self.num(f(3, 1), 270));
                let a = self.num(f(3, 2), 255);
                let (rx, ry) = (self.num(f(5, 0), 1), self.num(f(5, 1), 1));
                if let Some(mut p) = self.spr(&path) {
                    (p.x, p.y, p.alpha) = (x as f32, y as f32, a as f32 / 255.0);
                    (p.ax, p.ay) = (rx as f32 / 2.0, ry as f32 / 2.0);
                    self.set_layer(CG + n, Some(p), ms, false);
                }
            }
            "CG.DEL" => {
                let n = self.num(s(0), 99);
                let ms = self.num(f(1, 0), 250);
                if n == 99 {
                    self.clear_range(CG, CG + 9, ms);
                } else {
                    self.set_layer(CG + n as i32, None, ms, false);
                }
            }
            "CG.XY" | "CG.WH" | "CG.A" => {
                let k = CG + self.num(s(0), 0) as i32;
                let (u, v) = (self.num(s(2), 0), self.num(s(3), 100));
                if let Some(p) = self.cur(k) {
                    match name {
                        "CG.XY" => (p.x, p.y) = (u as f32, v as f32),
                        "CG.WH" => (p.sx, p.sy) = (u as f32 / 100.0, v as f32 / 100.0),
                        _ => p.alpha = u as f32 / 255.0,
                    }
                }
            }
            "loadButton" => {
                let k = self.num(s(0), 0) as i32;
                let (x, y) = (self.num(s(2), 0), self.num(s(3), 0));
                if let Some(mut p) = self.spr(script::unquote(s(1))) {
                    (p.x, p.y) = (x as f32, y as f32);
                    self.set_layer(k, Some(p), 0, false);
                }
            }
            "sp_d" => {
                let k = self.num(s(0), 0) as i32;
                self.set_layer(k, None, 0, false);
            }
            _ => {}
        }
    }

    /// The `x<n>` positions of `ST2`.
    fn xpos(&self, i: i64) -> i64 {
        const X: [i64; 15] = [
            480, 300, 660, -480, 1440, 360, 600, 120, 840, 0, 960, -720, 1680, -120, 1080,
        ];
        self.konst(&format!("x{i}"), X.get(i as usize).copied().unwrap_or(480))
    }

    /// `BG`/`EV name, time@keep, effect, x@y@alpha`, `BG.C`/`EV.C name, time@keep, x@y`.
    fn background(&mut self, name: &str, a: &[String]) {
        let f = |i: usize, j: usize| field(a, i, j);
        let dir = if name.starts_with("BG") { "bg" } else { "ev" };
        let path = format!("{dir}/{}", f(0, 0));
        let change = name.ends_with(".C");
        let ms = self.num(f(1, 0), if change { 250 } else { 1000 });
        let keep = self.num(f(1, 1), 0) != 0;
        let Some(mut p) = self.spr(&path) else {
            return;
        };
        let w = self.vars.get("_bgw").copied().unwrap_or(100) as f32 / 100.0;
        let h = self.vars.get("_bgh").copied().unwrap_or(100) as f32 / 100.0;
        (p.sx, p.sy) = (w, h);
        if change {
            if let Some(old) = self.cur(BG).cloned() {
                (p.x, p.y) = (old.x, old.y);
            }
            if !f(2, 0).is_empty() {
                p.x = self.num(f(2, 0), 480) as f32;
            }
            if !f(2, 1).is_empty() {
                p.y = self.num(f(2, 1), 270) as f32;
            }
        } else {
            p.x = self.num(f(3, 0), 480) as f32;
            p.y = self.num(f(3, 1), 270) as f32;
            p.alpha = self.num(f(3, 2), 255) as f32 / 255.0;
            self.window = false;
        }
        if !keep {
            self.clear_range(CG, ST + 99, ms);
        }
        self.set_layer(BG, Some(p), ms, true);
        if ms > 0 {
            self.wait = Wait::Fades;
        }
    }

    /// `ST code, time, x@y@alpha, _, w@h`, `ST0 slot, code, ...`, `ST2 code, time, move, x@y`.
    fn character(&mut self, name: &str, a: &[String]) {
        let (slot, a) = if name == "ST0" {
            (
                Some(self.num(a.first().map_or("", |s| s.trim()), 0)),
                &a[1.min(a.len())..],
            )
        } else {
            (None, a)
        };
        let f = |i: usize, j: usize| field(a, i, j);
        let code = f(0, 0).to_string();
        let Some(n) = slot.or_else(|| code.get(1..4)?.parse().ok()) else {
            return;
        };
        let k = ST + n as i32;
        let ms = self.num(f(1, 0), 250);
        let h = self.num(f(4, 1), self.konst("StH", 800));
        let sty = self.konst("StY", 240) as f32;
        let Some(mut p) = self.spr(&format!("st:{code}")) else {
            return;
        };
        (p.ay, p.oy) = (1.0, h as f32 / 2.0);
        let old = self.cur(k).cloned();
        let (x, y, alpha) = if name == "ST2" {
            let moves = self.num(f(2, 0), 0) != 0;
            let x = (!f(3, 0).is_empty()).then(|| self.xpos(self.num(f(3, 0), 0)));
            let y = (!f(3, 1).is_empty()).then(|| self.num(f(3, 1), 0));
            match old {
                Some(_) if !moves => (None, None, None),
                Some(_) => (x, y, None),
                None => (x.or(Some(self.xpos(0))), y.or(Some(0)), None),
            }
        } else {
            let x = (!f(2, 0).is_empty()).then(|| self.num(f(2, 0), 480));
            let y = (!f(2, 1).is_empty()).then(|| self.num(f(2, 1), 0));
            let alpha = (!f(2, 2).is_empty()).then(|| self.num(f(2, 2), 255));
            match old {
                Some(_) => (x, y, alpha),
                None => (x.or(Some(480)), y.or(Some(0)), alpha),
            }
        };
        if let Some(o) = &old {
            (p.x, p.y, p.alpha) = (o.x, o.y, o.alpha);
        }
        if let Some(x) = x {
            p.x = x as f32;
        }
        if let Some(y) = y {
            p.y = sty + y as f32;
        }
        if let Some(a) = alpha {
            p.alpha = a as f32 / 255.0;
        }
        // a character shown where another stands takes its place
        let (px, py) = (p.x, p.y);
        let others: Vec<i32> = self
            .layers
            .range(ST..ST + 100)
            .filter(|(o, l)| **o != k && l.cur.as_ref().is_some_and(|c| c.x == px && c.y == py))
            .map(|(o, _)| *o)
            .collect();
        for o in others {
            self.set_layer(o, None, ms, false);
        }
        self.set_layer(k, Some(p), ms, false);
    }

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    fn advance(&mut self) {
        if self.wait == Wait::Click {
            self.wait = Wait::None;
            self.text.clear();
            self.name = None;
        } else if let Wait::Frames(_) = self.wait {
            self.wait = Wait::None;
        }
    }

    fn pick(&mut self, i: usize) {
        let Some((_, l)) = self.choices.get(i).cloned() else {
            return;
        };
        self.choices.clear();
        self.wait = Wait::None;
        self.window = false;
        self.jump(&l);
    }

    fn act(&mut self, a: Act) {
        match a {
            Act::New => self.new_game(),
            Act::Continue => self.load(0),
            Act::Save => self.view = View::Slots { save: true },
            Act::Load => self.view = View::Slots { save: false },
            Act::QSave => self.save(0),
            Act::QLoad => self.load(0),
            Act::Skip => self.skip = !self.skip,
            Act::Quit => self.wait = Wait::Ended,
            Act::None => {}
        }
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
        if let View::Slots { save } = self.view {
            match self.slot_rects().into_iter().position(|r| inside(r, x, y)) {
                Some(i) if save => {
                    self.save(i);
                    self.view = View::Game;
                }
                Some(i) => self.load(i),
                None => self.view = View::Game,
            }
            return;
        }
        match self.wait {
            Wait::Title => {
                let hit = self
                    .title_rects()
                    .into_iter()
                    .position(|(_, r)| inside(r, x, y));
                if let Some(i) = hit {
                    self.act(self.skin.title_buttons[i].1);
                }
            }
            Wait::Choice => {
                if let Some(i) = self
                    .choice_rects()
                    .into_iter()
                    .position(|r| inside(r, x, y))
                {
                    self.pick(i);
                }
            }
            Wait::Ended => {}
            _ => {
                let hit = if self.window {
                    self.window_rects()
                        .into_iter()
                        .position(|(_, r)| inside(r, x, y))
                } else {
                    None
                };
                match hit {
                    Some(i) => self.act(self.skin.buttons[i].2),
                    None => {
                        self.skip = false;
                        self.advance();
                    }
                }
            }
        }
    }

    pub fn cancel(&mut self) {
        if self.view != View::Game {
            self.view = View::Game;
        } else if self.wait == Wait::Click || self.wait == Wait::Choice {
            self.view = View::Slots { save: true };
        }
    }

    /// 0 up, 1 down, 2 left, 3 right, 4 OK, 5 cancel, 6 skip (toggles)
    pub fn key(&mut self, k: usize) {
        match k {
            4 => match self.wait {
                Wait::Title => self.act(Act::New),
                Wait::Choice => self.pick(0),
                _ => self.advance(),
            },
            5 => self.cancel(),
            6 => self.skip = !self.skip,
            _ => {}
        }
    }

    pub fn wheel(&mut self, up: bool) {
        if !up {
            self.advance();
        }
    }

    // ---- drawing ----

    fn text_img(&mut self, s: &str, size: i32) -> Rc<Image> {
        let key = (s.to_string(), size);
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let img = match &self.font {
            Some(f) => Rc::new(Font::outline(f.clone(), size).render(
                &nexas::sjis::encode(s),
                [255, 255, 255],
                2,
            )),
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

    fn spr_draw(p: &Spr, alpha: f32) -> Draw {
        let (w, h) = (p.img.w as f32, p.img.h as f32);
        let (cx, cy) = (p.x, p.y + p.oy);
        let (l, t) = (cx - w * p.ax, cy - h * p.ay);
        Draw {
            image: p.img.clone(),
            src: [0, 0, w as i32, h as i32],
            dst: [l, t, l + w, t + h],
            place: Place {
                sx: p.sx,
                sy: p.sy,
                cx,
                cy,
                ..place(alpha * p.alpha)
            },
        }
    }

    fn text_draw(&mut self, s: &str, size: i32, x: i32, y: i32, out: &mut Vec<Draw>) {
        let img = self.text_img(s, size);
        let src = [0, 0, img.w as i32, img.h as i32];
        out.push(Self::draw(&img, src, x as f32, y as f32, 1.0));
    }

    /// A button picture of three states stacked: (the picture, its screen rect).
    fn button(
        &mut self,
        name: &str,
        x: i32,
        y: i32,
        centred: bool,
    ) -> Option<(Rc<Image>, [i32; 4])> {
        let img = self.pic(name)?;
        let (w, h) = (img.w as i32, img.h as i32 / 3);
        let (l, t) = if centred {
            (x - w / 2, y - h / 2)
        } else {
            (x, y)
        };
        Some((img, [l, t, l + w, t + h]))
    }

    fn button_draw(&self, img: &Rc<Image>, r: [i32; 4], out: &mut Vec<Draw>) {
        let h = r[3] - r[1];
        let state = inside(r, self.pointer.0, self.pointer.1) as i32;
        let src = [0, state * h, r[2] - r[0], (state + 1) * h];
        out.push(Self::draw(img, src, r[0] as f32, r[1] as f32, 1.0));
    }

    fn title_rects(&mut self) -> Vec<(Rc<Image>, [i32; 4])> {
        let k = self.skin;
        let mut out = Vec::new();
        for (i, (n, _)) in k.title_buttons.iter().enumerate() {
            let y = k.buttons_at.1 + i as i32 * k.step;
            match self.button(n, k.buttons_at.0, y, true) {
                Some(b) => out.push(b),
                None => out.push((Rc::new(Image::new(1, 1)), [0, 0, 0, 0])),
            }
        }
        out
    }

    fn window_rects(&mut self) -> Vec<(Rc<Image>, [i32; 4])> {
        let k = self.skin;
        k.buttons
            .iter()
            .map(|(n, x, _)| {
                self.button(n, *x, k.buttons_y, false)
                    .unwrap_or_else(|| (Rc::new(Image::new(1, 1)), [0, 0, 0, 0]))
            })
            .collect()
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        if self.wait == Wait::Title {
            for (n, x, y) in self.skin.title {
                if let Some(p) = self.pic(n) {
                    let src = [0, 0, p.w as i32, p.h as i32];
                    out.push(Self::draw(&p, src, *x as f32, *y as f32, 1.0));
                }
            }
            for (img, r) in self.title_rects() {
                self.button_draw(&img, r, &mut out);
            }
        } else {
            for l in self.layers.values().rev() {
                let a = if l.dur == 0 {
                    1.0
                } else {
                    l.t as f32 / l.dur as f32
                };
                if let Some(o) = &l.old {
                    out.push(Self::spr_draw(o, if l.keep { 1.0 } else { 1.0 - a }));
                }
                if let Some(c) = &l.cur {
                    out.push(Self::spr_draw(c, a));
                }
            }
            if self.window {
                self.window_draws(&mut out);
            }
        }
        if self.wait == Wait::Choice {
            self.choice_draws(&mut out);
        }
        if let View::Slots { save } = self.view {
            self.slot_draws(save, &mut out);
        }
        out
    }

    fn window_draws(&mut self, out: &mut Vec<Draw>) {
        let k = self.skin;
        if let Some(p) = self.pic("sys/window/window2") {
            let src = [0, 0, p.w as i32, p.h as i32];
            out.push(Self::draw(
                &p,
                src,
                k.window.0 as f32,
                k.window.1 as f32,
                1.0,
            ));
        }
        if let Some(n) = self.name.clone() {
            if let Some(p) = self.pic("sys/window/name2")
                && p.w > 1
            {
                let src = [0, 0, p.w as i32, p.h as i32];
                out.push(Self::draw(&p, src, k.plate.0 as f32, k.plate.1 as f32, 1.0));
            }
            let img = self.text_img(&n, k.size);
            let (mut x, mut y) = k.name;
            if k.name_centred {
                x -= img.w as i32 / 2;
                y -= img.h as i32 / 2;
            }
            let src = [0, 0, img.w as i32, img.h as i32];
            out.push(Self::draw(&img, src, x as f32, y as f32, 1.0));
        }
        let mut y = k.text.1;
        for line in wrap(&self.text.clone(), k.wrap) {
            self.text_draw(&line, k.size, k.text.0, y, out);
            y += k.line;
        }
        if let Some(p) = self.pic("sys/window/bar") {
            let src = [0, 0, p.w as i32, p.h as i32];
            out.push(Self::draw(&p, src, k.bar.0 as f32, k.bar.1 as f32, 1.0));
        }
        for (img, r) in self.window_rects() {
            self.button_draw(&img, r, out);
        }
    }

    fn choice_rects(&mut self) -> Vec<[i32; 4]> {
        let (w, h) = match self.pic("sys/window/selectbar") {
            Some(p) => (p.w as i32, p.h as i32 / 3),
            None => (600, 44),
        };
        let n = self.choices.len() as i32;
        let top = 220 - n * (h + 12) / 2;
        let x = (W as i32 - w) / 2;
        (0..n)
            .map(|i| [x, top + i * (h + 12), x + w, top + i * (h + 12) + h])
            .collect()
    }

    fn choice_draws(&mut self, out: &mut Vec<Draw>) {
        let bar = self.pic("sys/window/selectbar");
        let labels: Vec<String> = self.choices.iter().map(|c| c.0.clone()).collect();
        for (r, label) in self.choice_rects().into_iter().zip(labels) {
            match &bar {
                Some(p) => self.button_draw(p, r, out),
                None => out.push(Self::solid([20, 40, 120, 255], 0.85, r)),
            }
            let img = self.text_img(&label, self.skin.size + 2);
            let x = (r[0] + r[2]) / 2 - img.w as i32 / 2;
            let y = (r[1] + r[3]) / 2 - img.h as i32 / 2;
            let src = [0, 0, img.w as i32, img.h as i32];
            out.push(Self::draw(&img, src, x as f32, y as f32, 1.0));
        }
    }

    // ---- saves ----

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        (0..SLOTS as i32)
            .map(|i| {
                let (c, r) = (i % 2, i / 2);
                let (x, y) = (50 + c * 440, 80 + r * 84);
                [x, y, x + 420, y + 70]
            })
            .collect()
    }

    fn slot_draws(&mut self, save: bool, out: &mut Vec<Draw>) {
        out.push(Self::solid(
            [0, 10, 40, 255],
            0.85,
            [0, 0, W as i32, H as i32],
        ));
        let title = if save { "SAVE" } else { "LOAD" };
        self.text_draw(title, 28, 50, 24, out);
        let (px, py) = self.pointer;
        for (i, r) in self.slot_rects().into_iter().enumerate() {
            let c = if inside(r, px, py) {
                [80, 140, 240, 255]
            } else {
                [30, 50, 110, 255]
            };
            out.push(Self::solid(c, 0.85, r));
            let label = match self.read_slot(i) {
                Some(s) => format!("{:02}  {}", i + 1, s.label),
                None => format!("{:02}  ----", i + 1),
            };
            self.text_draw(&label, 20, r[0] + 14, r[1] + 22, out);
        }
    }

    fn read_slot(&mut self, i: usize) -> Option<Slot> {
        let b = self.files.read_save(&format!("slot{i:02}.sav"))?;
        Slot::parse(&String::from_utf8(b).ok()?)
    }

    fn save(&mut self, i: usize) {
        if !matches!(self.wait, Wait::Click | Wait::Choice) {
            return;
        }
        let t: String = self
            .text
            .chars()
            .filter(|c| !c.is_control())
            .take(14)
            .collect();
        let s = Slot {
            label: format!("{} {t}", self.chapter),
            at: self.msg_at,
            stack: self.stack.clone(),
            vars: self
                .vars
                .iter()
                .filter(|(k, v)| self.consts.get(*k) != Some(*v))
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
            layers: self
                .layers
                .iter()
                .filter_map(|(k, l)| Some((*k, l.cur.clone()?)))
                .map(|(k, p)| {
                    let v = [p.x, p.y, p.ax, p.ay, p.oy, p.sx, p.sy, p.alpha];
                    (k, v, p.src)
                })
                .collect(),
            bgm: self.bgm.clone(),
            loops: self.loops.clone(),
        };
        self.files
            .write_save(&format!("slot{i:02}.sav"), s.write().as_bytes());
    }

    fn load(&mut self, i: usize) {
        let Some(s) = self.read_slot(i) else {
            return;
        };
        self.pc = s.at;
        self.stack = s.stack;
        self.vars = self.consts.clone();
        self.vars.extend(s.vars);
        self.layers.clear();
        for (k, v, src) in s.layers {
            if let Some(mut p) = self.spr(&src) {
                [p.x, p.y, p.ax, p.ay, p.oy, p.sx, p.sy, p.alpha] = v;
                self.set_layer(k, Some(p), 0, false);
            }
        }
        for c in ["se0", "se1", "se2"] {
            self.stop(c, 0.0);
        }
        self.stop_voices();
        self.loops.clear();
        for (c, n) in s.loops {
            self.play(&c, &format!("se/{n}.ogg"), true, 0.8);
            self.loops.insert(c, n);
        }
        self.stop("bgm", 0.0);
        self.bgm = None;
        if let Some(b) = s.bgm {
            self.play_bgm(&b);
        }
        self.text.clear();
        self.name = None;
        self.choices.clear();
        self.window = false;
        self.skip = false;
        self.wait = Wait::None;
        self.view = View::Game;
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
    at: usize,
    stack: Vec<usize>,
    vars: Vec<(String, i64)>,
    layers: Vec<(i32, [f32; 8], String)>,
    bgm: Option<String>,
    loops: BTreeMap<String, String>,
}

impl Slot {
    fn write(&self) -> String {
        let mut s = String::from("NONCOLORSAV1\n");
        s += &format!("label {}\n", self.label.replace('\n', " "));
        s += &format!("at {}\n", self.at);
        for pc in &self.stack {
            s += &format!("call {pc}\n");
        }
        for (k, v) in &self.vars {
            s += &format!("var {k} {v}\n");
        }
        for (k, v, src) in &self.layers {
            let v: Vec<String> = v.iter().map(|f| f.to_string()).collect();
            s += &format!("layer {k} {} {src}\n", v.join(" "));
        }
        if let Some(b) = &self.bgm {
            s += &format!("bgm {b}\n");
        }
        for (c, n) in &self.loops {
            s += &format!("loop {c} {n}\n");
        }
        s
    }

    fn parse(t: &str) -> Option<Slot> {
        let mut lines = t.lines();
        if lines.next()? != "NONCOLORSAV1" {
            return None;
        }
        let mut s = Slot {
            label: String::new(),
            at: usize::MAX,
            stack: Vec::new(),
            vars: Vec::new(),
            layers: Vec::new(),
            bgm: None,
            loops: BTreeMap::new(),
        };
        for l in lines {
            let (k, rest) = l.split_once(' ').unwrap_or((l, ""));
            match k {
                "label" => s.label = rest.to_string(),
                "at" => s.at = rest.parse().ok()?,
                "call" => s.stack.push(rest.parse().ok()?),
                "var" => {
                    let (k, v) = rest.split_once(' ')?;
                    s.vars.push((k.to_string(), v.parse().ok()?));
                }
                "layer" => {
                    let w: Vec<&str> = rest.splitn(10, ' ').collect();
                    let mut v = [0f32; 8];
                    for (i, x) in v.iter_mut().enumerate() {
                        *x = w.get(i + 1)?.parse().ok()?;
                    }
                    s.layers
                        .push((w[0].parse().ok()?, v, w.get(9)?.to_string()));
                }
                "bgm" => s.bgm = Some(rest.to_string()),
                "loop" => {
                    let (c, n) = rest.split_once(' ')?;
                    s.loops.insert(c.to_string(), n.to_string());
                }
                _ => {}
            }
        }
        (s.at != usize::MAX).then_some(s)
    }
}
