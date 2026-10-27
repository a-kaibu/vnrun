//! Runs the charts: threads of calls (the main one, plus one for each event handler a component
//! fires), variables, components on the front and back screens, and drawing them.
//!
//! LiveMaker games build their message window, menus and effects out of the charts of the
//! "novel system" library they ship with, so the engine itself stays small: it plays text into
//! message boxes and fires the boxes' events (ONNOTIFY for `<EVENT ...>` in the text, ONPLAYEND at
//! a page end, ONIDLE every frame), and the library's charts do the rest.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use nexas::files::Files;
use nexas::gfx::{Draw, Image, Place};

use crate::archive::{Archive, key};
use crate::compo::*;
use crate::lsb::{self, Cmd, Expr, LabelRef, Param, Script};
use crate::names;
use crate::project::{MenuItem, Project, parse_lpm};
use crate::text::Fonts;
use crate::tpword::Glyph;
use crate::value::{Value, Var};

pub const FRAME_MS: f64 = 1000.0 / 60.0;
/// The parent name of the back screen.
const BACK: &str = "\u{1}";

/// A sound to start (`data`) or stop (`None`) on a channel named after its component.
pub struct SoundEvent {
    pub channel: String,
    pub data: Option<Rc<Vec<u8>>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: u32,
}

struct Frame {
    page: String,
    script: Rc<Script>,
    pc: usize,
    params: Vec<Value>,
    locals: HashMap<String, Var>,
    result: Value,
    result_var: String,
    sender: String,
}

/// An event handler to run: the handler ("chart.lsc:label"), its @ParamStr and @Sender.
type Handler = (String, Vec<Value>, String);

#[derive(Default)]
struct Thread {
    frames: Vec<Frame>,
    /// the current command (Wait, Flip, TextIns) has begun and is waiting
    started: bool,
    deadline: f64,
    /// (component, event) for handlers that run one at a time
    key: Option<(String, u8)>,
    /// handlers to run after this one
    then: VecDeque<Handler>,
    main: bool,
    done: bool,
}

struct Motion {
    obj: String,
    prop: u8,
    from: f64,
    to: f64,
    t0: f64,
    dur: f64,
}

struct FlipFx {
    old: Vec<Draw>,
    t0: f64,
    dur: f64,
}

enum Go {
    Next,
    Jumped,
    /// back to the top of a loop
    Loop,
    Yield,
}

pub struct Engine {
    files: Box<dyn Files>,
    arc: Archive,
    pub width: u32,
    pub height: u32,
    scripts: HashMap<String, Option<Rc<Script>>>,
    globals: HashMap<String, Var>,
    sys: HashMap<String, Value>,
    threads: Vec<Thread>,
    objs: Vec<Obj>,
    seq: u64,
    now: f64,
    mouse: (i32, i32),
    click: bool,
    rclick: bool,
    lclick_now: bool,
    rclick_now: bool,
    sounds: Vec<SoundEvent>,
    images: HashMap<String, Option<Rc<Image>>>,
    datas: HashMap<String, Option<Rc<Vec<u8>>>>,
    solids: HashMap<u32, Rc<Image>>,
    fonts: Fonts,
    flip: Option<FlipFx>,
    motions: Vec<Motion>,
    ended: bool,
    log: u8,
    rand: u64,
}

fn split_list(s: &str) -> Vec<String> {
    s.split(['\r', '\n', ','])
        .filter(|x| !x.is_empty())
        .map(|x| x.to_string())
        .collect()
}

/// "chart.lsc:label" as the chart and the label.
fn handler_target(h: &str) -> (String, String) {
    let (page, label) = match h.rfind(':') {
        Some(i) if i > 1 => (&h[..i], &h[i + 1..]),
        _ => (h, ""),
    };
    let page = if page.to_lowercase().ends_with(".lsc") {
        format!("{}.lsb", &page[..page.len() - 4])
    } else {
        page.to_string()
    };
    (page, label.to_string())
}

fn sjis_len(s: &str) -> usize {
    nexas::sjis::encode(s).len()
}

/// Byte-wise Copy on the Shift_JIS form (1-based).
fn sjis_copy(s: &str, start: i64, count: i64) -> String {
    let b = nexas::sjis::encode(s);
    let a = (start.max(1) - 1) as usize;
    if a >= b.len() || count <= 0 {
        return String::new();
    }
    let e = (a + count as usize).min(b.len());
    nexas::sjis::decode(&b[a..e])
}

fn sjis_pos(sub: &str, s: &str) -> i64 {
    let (b, n) = (nexas::sjis::encode(s), nexas::sjis::encode(sub));
    if n.is_empty() {
        return 0;
    }
    b.windows(n.len())
        .position(|w| w == &n[..])
        .map_or(0, |i| i as i64 + 1)
}

fn file_ext(p: &str) -> String {
    let name = &p[p.rfind(['\\', '/']).map_or(0, |i| i + 1)..];
    name.rfind('.')
        .map_or(String::new(), |i| name[i..].to_string())
}

fn binary(op: u8, a: &Value, b: &Value) -> Value {
    let num = |v: &Value| !v.is_str() || v.to_string().trim().parse::<f64>().is_ok();
    let float = a.is_float() || b.is_float();
    match op {
        2 => {
            if a.is_str() && b.is_str() || (a.is_str() || b.is_str()) && !(num(a) && num(b)) {
                Value::Str(format!("{a}{b}"))
            } else if float || a.to_string().contains('.') || b.to_string().contains('.') {
                Value::Float(a.float() + b.float())
            } else {
                Value::Int(a.int().wrapping_add(b.int()))
            }
        }
        3..=6 => {
            if float {
                let (x, y) = (a.float(), b.float());
                Value::Float(match op {
                    3 => x - y,
                    4 => x * y,
                    5 => {
                        if y == 0.0 {
                            0.0
                        } else {
                            x / y
                        }
                    }
                    _ => {
                        if y == 0.0 {
                            0.0
                        } else {
                            x % y
                        }
                    }
                })
            } else {
                let (x, y) = (a.int(), b.int());
                Value::Int(match op {
                    3 => x.wrapping_sub(y),
                    4 => x.wrapping_mul(y),
                    5 => {
                        if y == 0 {
                            0
                        } else {
                            x / y
                        }
                    }
                    _ => {
                        if y == 0 {
                            0
                        } else {
                            x % y
                        }
                    }
                })
            }
        }
        7..=9 => {
            if matches!(a, Value::Flag(_)) || matches!(b, Value::Flag(_)) {
                let (x, y) = (a.truthy(), b.truthy());
                Value::Flag(match op {
                    7 => x | y,
                    8 => x & y,
                    _ => x ^ y,
                })
            } else {
                let (x, y) = (a.int(), b.int());
                Value::Int(match op {
                    7 => x | y,
                    8 => x & y,
                    _ => x ^ y,
                })
            }
        }
        12..=16 | 20 => {
            let ord = if a.is_str() && b.is_str() {
                a.to_string().cmp(&b.to_string())
            } else if (a.is_str() && !num(a)) || (b.is_str() && !num(b)) {
                let (x, y) = (a.to_string(), b.to_string());
                // TRUE == "TRUE"
                x.to_uppercase().cmp(&y.to_uppercase())
            } else {
                a.float()
                    .partial_cmp(&b.float())
                    .unwrap_or(std::cmp::Ordering::Equal)
            };
            use std::cmp::Ordering::*;
            Value::Flag(match op {
                12 => ord == Equal,
                13 => ord == Greater,
                14 => ord == Less,
                15 => ord != Less,
                16 => ord != Greater,
                _ => ord != Equal,
            })
        }
        17 => Value::Int(a.int().wrapping_shl(b.int() as u32)),
        18 => Value::Int(a.int().wrapping_shr(b.int() as u32)),
        19 => Value::Str(format!("{a}{b}")),
        _ => b.clone(),
    }
}

impl Engine {
    pub fn open(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let arc = Archive::open(&mut *files)?;
        let lpb = arc
            .read(&mut *files, "live.lpb")
            .ok_or("live.lpb is missing from the archive")?;
        let project = Project::parse(&lpb)?;
        let mut e = Engine {
            files,
            arc,
            width: project.width,
            height: project.height,
            scripts: HashMap::new(),
            globals: HashMap::new(),
            sys: HashMap::new(),
            threads: Vec::new(),
            objs: Vec::new(),
            seq: 0,
            now: 0.0,
            mouse: (-1, -1),
            click: false,
            rclick: false,
            lclick_now: false,
            rclick_now: false,
            sounds: Vec::new(),
            images: HashMap::new(),
            datas: HashMap::new(),
            solids: HashMap::new(),
            fonts: Fonts::new(),
            flip: None,
            motions: Vec::new(),
            ended: false,
            log: 0,
            rand: 0x2545_f491_4f6c_dd1d,
        };
        for (name, ty, v) in project.vars {
            e.globals.insert(name, Var::new(ty, v));
        }
        let script = e
            .script(&project.init)
            .ok_or(format!("{}: missing", project.init))?;
        e.threads.push(Thread {
            frames: vec![Frame {
                page: project.init.clone(),
                script,
                pc: 0,
                params: Vec::new(),
                locals: HashMap::new(),
                result: Value::default(),
                result_var: String::new(),
                sender: String::new(),
            }],
            main: true,
            ..Thread::default()
        });
        Ok(e)
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        self.fonts.set(ttf)
    }

    /// 0: quiet, 1: commands (not the per-frame idle handlers), 2: everything
    pub fn set_log(&mut self, level: u8) {
        self.log = level;
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn point(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if x >= 0 {
            self.mouse = (x, y);
        }
        self.click = true;
    }

    pub fn cancel(&mut self) {
        self.rclick = true;
    }

    pub fn wheel(&mut self, up: bool) {
        if !up {
            self.click = true;
        }
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    // ---- files

    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        if let Some(b) = self.arc.read(&mut *self.files, path) {
            return Some(b);
        }
        let p = path.replace('\\', "/");
        let n = self.files.size(&p)?;
        self.files.read_range(&p, 0, n as usize)
    }

    fn data(&mut self, path: &str) -> Option<Rc<Vec<u8>>> {
        let k = key(path);
        if let Some(d) = self.datas.get(&k) {
            return d.clone();
        }
        let d = self.read(path).map(Rc::new);
        self.datas.insert(k, d.clone());
        d
    }

    fn script(&mut self, page: &str) -> Option<Rc<Script>> {
        let k = key(page);
        if let Some(s) = self.scripts.get(&k) {
            return s.clone();
        }
        let s = match self.read(page) {
            Some(b) => match Script::parse(&b) {
                Ok(s) => Some(Rc::new(s)),
                Err(err) => {
                    eprintln!("livemaker: {page}: {err}");
                    None
                }
            },
            None => {
                eprintln!("livemaker: {page}: missing");
                None
            }
        };
        self.scripts.insert(k, s.clone());
        s
    }

    fn image(&mut self, path: &str) -> Option<Rc<Image>> {
        if path.is_empty() {
            return None;
        }
        let k = key(path);
        if let Some(i) = self.images.get(&k) {
            return i.clone();
        }
        let mut found = None;
        let tries = if file_ext(path).is_empty() {
            vec![
                format!("{path}.gal"),
                format!("{path}.png"),
                format!("{path}.jpg"),
            ]
        } else {
            vec![path.to_string()]
        };
        for p in tries {
            if let Some(b) = self.read(&p) {
                let img = if crate::gal::is_gal(&b) {
                    crate::gal::decode(&b)
                } else {
                    Image::decode(&b)
                };
                match img {
                    Ok(i) => found = Some(Rc::new(i)),
                    Err(err) => eprintln!("livemaker: {p}: {err}"),
                }
                break;
            }
        }
        if found.is_none() {
            eprintln!("livemaker: picture {path}: missing");
        }
        self.images.insert(k, found.clone());
        found
    }

    // ---- components

    fn find(&self, name: &str) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        self.objs.iter().position(|o| o.name() == name)
    }

    fn on_back(&self, mut i: usize) -> bool {
        for _ in 0..32 {
            let p = self.objs[i].parent();
            if p == BACK {
                return true;
            }
            match self.find(p) {
                Some(j) if j != i => i = j,
                _ => return false,
            }
        }
        false
    }

    fn abs_pos(&self, mut i: usize) -> (i32, i32) {
        let (mut x, mut y) = (0, 0);
        for _ in 0..32 {
            x += self.objs[i].int(LEFT) as i32;
            y += self.objs[i].int(TOP) as i32;
            match self.find(self.objs[i].parent()) {
                Some(j) if j != i => i = j,
                _ => break,
            }
        }
        (x, y)
    }

    fn rename(&mut self, i: usize, to: &str) {
        let old = self.objs[i].name().to_string();
        for o in self.objs.iter_mut() {
            if o.parent() == old {
                o.props.insert(PARENT, Value::Str(to.to_string()));
            }
        }
        self.objs[i].props.insert(NAME, Value::Str(to.to_string()));
    }

    fn delete(&mut self, name: &str) {
        let Some(i) = self.find(name) else { return };
        let children: Vec<String> = self
            .objs
            .iter()
            .filter(|o| o.parent() == name)
            .map(|o| o.name().to_string())
            .collect();
        let o = self.objs.remove(i);
        if o.playing && matches!(o.kind, lsb::SOUND | lsb::MOVIE) {
            self.sounds.push(SoundEvent {
                channel: name.to_string(),
                data: None,
                volume: 0.0,
                repeat: false,
                fade_ms: 0,
            });
        }
        for c in children {
            if c != name {
                self.delete(&c);
            }
        }
    }

    fn get_prop(&self, name: &str, p: u8) -> Value {
        let Some(i) = self.find(name) else {
            if name.is_empty() || name == BACK {
                return match p {
                    WIDTH => Value::Int(self.width as i64),
                    HEIGHT => Value::Int(self.height as i64),
                    VISIBLE => Value::Flag(true),
                    _ => Value::Int(0),
                };
            }
            return Obj::new(0, "", 0).get(p).clone();
        };
        let o = &self.objs[i];
        match (p, o.kind) {
            (PLAYING, lsb::SOUND | lsb::MOVIE) => {
                Value::Flag(o.playing && (o.flag(REPEAT) || self.now < o.t0 + o.dur))
            }
            (PLAYING, lsb::MES_NEW) => {
                let m = o.mes.as_ref().unwrap();
                Value::Flag(!m.queue.is_empty() || m.waiting)
            }
            (PLAYING, lsb::TIMER) => Value::Flag(o.playing),
            (PLAYING, _) => Value::Flag(false),
            (COUNT, lsb::SOUND | lsb::MOVIE) => Value::Int(o.dur as i64),
            (INDEX, lsb::SOUND | lsb::MOVIE) => {
                Value::Int(((self.now - o.t0).max(0.0) as i64).min(o.dur as i64))
            }
            (COUNT, lsb::PREV_MENU_NEW | lsb::MENU) => Value::Int(o.items.len() as i64),
            (COUNT, lsb::MES_NEW) => Value::Int(1),
            (TEXT, lsb::MES_NEW) => Value::Str(o.mes.as_ref().unwrap().text.clone()),
            (CARETX, lsb::MES_NEW) => Value::Int(o.mes.as_ref().unwrap().x as i64),
            (CARETY, lsb::MES_NEW) => Value::Int(o.mes.as_ref().unwrap().y as i64),
            (CARETHEIGHT, lsb::MES_NEW) => Value::Int(o.get(FONTHEIGHT).int().max(1)),
            (COMPOTYPE, k) => Value::Int(k as i64),
            (91, _) => Value::Int(o.img.as_ref().map_or(o.int(WIDTH), |m| m.w as i64)),
            (92, _) => Value::Int(o.img.as_ref().map_or(o.int(HEIGHT), |m| m.h as i64)),
            _ => o.get(p),
        }
    }

    fn set_prop(&mut self, name: &str, p: u8, v: Value) {
        let Some(i) = self.find(name) else { return };
        let v = coerce(p, v);
        match p {
            NAME => {
                let to = v.to_string();
                if to != name {
                    self.delete(&to);
                    let i = self.find(name).unwrap();
                    self.rename(i, &to);
                }
                return;
            }
            SOURCE => {
                self.objs[i].props.insert(p, v.clone());
                self.load_source(i, &v.to_string());
                return;
            }
            TEXT if self.objs[i].kind == lsb::MES_NEW => {
                let m = self.objs[i].mes.as_mut().unwrap();
                clear(m);
                m.queue.clear();
                let s = v.to_string();
                m.queue.extend(s.chars().map(|ch| Glyph::Char {
                    ch,
                    style: -1,
                    link: 0,
                    speed: 0,
                }));
                return;
            }
            PAUSED if matches!(self.objs[i].kind, lsb::SOUND | lsb::MOVIE) => {
                let was = self.objs[i].flag(PAUSED);
                self.objs[i].props.insert(p, v.clone());
                if was && !v.truthy() {
                    self.play(i);
                } else if v.truthy() && self.objs[i].playing {
                    self.stop(i, 0);
                }
                return;
            }
            PAUSED if self.objs[i].kind == lsb::TIMER => {
                self.objs[i].playing = !v.truthy();
                self.objs[i].t0 = self.now;
            }
            _ => {}
        }
        let o = &mut self.objs[i];
        o.props.insert(p, v);
        if matches!(p, TEXT | 16..=24 | 142..=144) {
            o.label = None;
            if o.kind == lsb::CAPTION {
                self.label(i);
            }
        }
    }

    fn load_source(&mut self, i: usize, src: &str) {
        let kind = self.objs[i].kind;
        match kind {
            lsb::PREV_MENU_NEW => {
                let Some(b) = self.read(src) else {
                    eprintln!("livemaker: menu {src}: missing");
                    return;
                };
                let text = nexas::sjis::decode(&b);
                let dir = src.rfind('\\').map_or("", |n| &src[..n + 1]).to_string();
                let mut items = parse_lpm(&text);
                for it in items.iter_mut() {
                    for p in [&mut it.path, &mut it.hover, &mut it.down, &mut it.disabled] {
                        if !p.is_empty() {
                            *p = format!("{dir}{p}");
                        }
                    }
                }
                let tag = |t: &str| {
                    let open = format!("<{t}>");
                    text.find(&open)
                        .and_then(|a| text[a + open.len()..].split('<').next())
                        .and_then(|s| s.trim().parse::<i64>().ok())
                        .unwrap_or(0)
                };
                let (w, h) = (tag("Width"), tag("Height"));
                let o = &mut self.objs[i];
                o.disabled = vec![false; items.len()];
                o.items = items;
                o.props.insert(WIDTH, Value::Int(w));
                o.props.insert(HEIGHT, Value::Int(h));
            }
            lsb::SOUND | lsb::MOVIE | lsb::CINEMA | lsb::MES_NEW | lsb::TIMER => {}
            _ => {
                let img = self.image(src);
                let o = &mut self.objs[i];
                if let Some(im) = &img {
                    o.props.insert(WIDTH, Value::Int(im.w as i64));
                    o.props.insert(HEIGHT, Value::Int(im.h as i64));
                }
                o.img = img;
            }
        }
    }

    fn play(&mut self, i: usize) {
        let src = self.objs[i].str(SOURCE);
        let data = self.data(&src);
        let o = &mut self.objs[i];
        o.playing = true;
        o.t0 = self.now;
        o.dur = data.as_ref().map_or(0.0, |d| duration(d));
        if data.is_none() {
            eprintln!("livemaker: sound {src}: missing");
        }
        self.sounds.push(SoundEvent {
            channel: o.name().to_string(),
            data,
            volume: (o.int(VOLUME) as f32 / 1000.0).clamp(0.0, 1.0),
            repeat: o.flag(REPEAT),
            fade_ms: 0,
        });
    }

    fn stop(&mut self, i: usize, fade: u32) {
        let o = &mut self.objs[i];
        o.playing = false;
        self.sounds.push(SoundEvent {
            channel: o.name().to_string(),
            data: None,
            volume: 0.0,
            repeat: false,
            fade_ms: fade,
        });
    }

    fn create(&mut self, th: &mut Thread, kind: u8, props: &[(u8, Expr)]) {
        let mut name = String::new();
        let mut parent = String::new();
        for (p, e) in props {
            if *p == NAME && !e.0.is_empty() {
                name = self.eval(th, e).to_string();
            } else if *p == PARENT && !e.0.is_empty() {
                parent = self.eval(th, e).to_string();
            }
        }
        self.seq += 1;
        if name.is_empty() {
            name = format!("__lm{}", self.seq);
        }
        let back = parent == BACK || self.find(&parent).is_some_and(|j| self.on_back(j));
        if let Some(i) = self.find(&name) {
            if self.on_back(i) != back && self.objs[i].cab.is_none() {
                // The other screen's one stays, as name_2, until a Flip deletes it.
                let other = format!("{name}_2");
                self.delete(&other);
                let i = self.find(&name).unwrap();
                self.rename(i, &other);
            } else {
                // Replaced: its children stay, under the new one.
                let o = self.objs.remove(i);
                if o.playing && matches!(o.kind, lsb::SOUND | lsb::MOVIE) {
                    self.sounds.push(SoundEvent {
                        channel: name.clone(),
                        data: None,
                        volume: 0.0,
                        repeat: false,
                        fade_ms: 0,
                    });
                }
            }
        }
        let mut o = Obj::new(kind, &name, self.seq);
        o.props.insert(PARENT, Value::Str(parent));
        if kind == lsb::MES_NEW {
            o.mes = Some(Box::default());
        }
        self.objs.push(o);
        for (p, e) in props {
            if *p == NAME || *p == PARENT || e.0.is_empty() {
                continue;
            }
            let v = self.eval(th, e);
            if *p == PAUSED {
                // sounds start below, once everything is set
                if let Some(i) = self.find(&name) {
                    self.objs[i].props.insert(PAUSED, coerce(PAUSED, v));
                }
                continue;
            }
            self.set_prop(&name, *p, v);
        }
        // Places worked out from the object's own size see it once it is set.
        for (p, e) in props {
            if (*p == LEFT || *p == TOP) && e.0.len() > 1 {
                let v = self.eval(th, e);
                self.set_prop(&name, *p, v);
            }
        }
        let Some(i) = self.find(&name) else { return };
        match kind {
            lsb::SOUND | lsb::MOVIE if !self.objs[i].flag(PAUSED) => self.play(i),
            lsb::TIMER => {
                let o = &mut self.objs[i];
                o.t0 = self.now;
                o.playing = !o.flag(PAUSED);
            }
            lsb::MENU => {
                let lines: Vec<MenuItem> = self.objs[i]
                    .str(TEXT)
                    .split("\r\n")
                    .filter(|s| !s.is_empty())
                    .map(|s| MenuItem {
                        name: s.to_string(),
                        ..MenuItem::default()
                    })
                    .collect();
                let o = &mut self.objs[i];
                o.disabled = vec![false; lines.len()];
                o.items = lines;
                self.menu_setup(i);
                let h = self.objs[i].str(75);
                self.spawn(&h, Vec::new(), &name, None);
            }
            _ => {}
        }
        if self.log > 0 {
            eprintln!("  new {name} ({})", names::COMMANDS[kind as usize]);
        }
    }

    /// Objects to draw, front screen, in drawing order, with where they are and how opaque.
    fn order(&self) -> Vec<(usize, i32, i32, f32)> {
        let mut out = Vec::new();
        let mut roots: Vec<usize> = (0..self.objs.len())
            .filter(|&i| self.objs[i].parent().is_empty())
            .collect();
        self.sort(&mut roots);
        for i in roots {
            self.order_from(i, 0, 0, 1.0, &mut out, 0);
        }
        out
    }

    fn sort(&self, v: &mut [usize]) {
        v.sort_by_key(|&i| (self.objs[i].int(PRIORITY), self.objs[i].seq));
    }

    fn order_from(
        &self,
        i: usize,
        ox: i32,
        oy: i32,
        alpha: f32,
        out: &mut Vec<(usize, i32, i32, f32)>,
        depth: u32,
    ) {
        let o = &self.objs[i];
        if !o.flag(VISIBLE) || o.cab.is_some() || depth > 16 {
            return;
        }
        let x = ox + o.int(LEFT) as i32;
        let y = oy + o.int(TOP) as i32;
        let a = alpha * (o.int(ALPHA).clamp(0, 255) as f32 / 255.0);
        out.push((i, x, y, a));
        let name = o.name();
        let mut kids: Vec<usize> = (0..self.objs.len())
            .filter(|&j| j != i && self.objs[j].parent() == name)
            .collect();
        self.sort(&mut kids);
        for j in kids {
            self.order_from(j, x, y, a, out, depth + 1);
        }
    }

    fn solid(&mut self, color: i64) -> Rc<Image> {
        let c = crate::text::rgb(color);
        let k = u32::from_le_bytes([c[0], c[1], c[2], 0]);
        self.solids
            .entry(k)
            .or_insert_with(|| {
                let mut i = Image::new(1, 1);
                i.fill([c[0], c[1], c[2], 255]);
                Rc::new(i)
            })
            .clone()
    }

    fn screen_draws(&mut self) -> Vec<Draw> {
        let order = self.order();
        let (sw, sh) = (self.width as f32, self.height as f32);
        let mut out = Vec::new();
        let put = |out: &mut Vec<Draw>, img: Rc<Image>, x: i32, y: i32, w: i32, h: i32, a: f32| {
            if w <= 0 || h <= 0 || a <= 0.0 {
                return;
            }
            let (x, y) = (x as f32, y as f32);
            out.push(Draw {
                src: [0, 0, img.w as i32, img.h as i32],
                dst: [x, y, x + w as f32, y + h as f32],
                image: img,
                place: Place {
                    x,
                    y,
                    sx: 1.0,
                    sy: 1.0,
                    cx: 0.0,
                    cy: 0.0,
                    rot: 0.0,
                    alpha: a,
                    add: false,
                    flip: (false, false),
                    clip: [0.0, 0.0, sw, sh],
                },
            });
        };
        for (i, x, y, a) in order {
            let kind = self.objs[i].kind;
            match kind {
                lsb::BOX_NEW => {
                    let c = self.objs[i].int(COLOR);
                    if c >= 0 {
                        let img = self.solid(c);
                        let o = &self.objs[i];
                        put(
                            &mut out,
                            img,
                            x,
                            y,
                            o.int(WIDTH) as i32,
                            o.int(HEIGHT) as i32,
                            a,
                        );
                    }
                }
                lsb::MES_NEW => {
                    let img = self.mes_image(i);
                    if let Some(img) = img {
                        let (w, h) = (img.w as i32, img.h as i32);
                        put(&mut out, img, x, y, w, h, a);
                    }
                }
                lsb::CAPTION | lsb::BUTTON | lsb::EDIT_NEW => {
                    let o = &self.objs[i];
                    let src = if kind == lsb::BUTTON && o.hover.is_some() {
                        o.str(HOVERSOURCE)
                    } else {
                        o.str(SOURCE)
                    };
                    let (w, h) = (o.int(WIDTH) as i32, o.int(HEIGHT) as i32);
                    if kind == lsb::BUTTON {
                        if let Some(img) = self.image(&src) {
                            put(&mut out, img, x, y, w, h, a);
                        }
                    } else if kind == lsb::EDIT_NEW {
                        let img = self.solid(0xffffff);
                        put(&mut out, img, x, y, w, h, a * 0.8);
                    }
                    let label = self.label(i);
                    if let Some(img) = label {
                        let (lw, lh) = (img.w as i32, img.h as i32);
                        let (lx, ly) = if kind == lsb::CAPTION {
                            (x, y)
                        } else if kind == lsb::EDIT_NEW {
                            (x + 4, y + (h - lh) / 2)
                        } else {
                            (x + (w - lw) / 2, y + (h - lh) / 2)
                        };
                        put(&mut out, img, lx, ly, lw, lh, a);
                    }
                }
                lsb::PREV_MENU_NEW => {
                    let items = self.objs[i].items.clone();
                    let hover = self.objs[i].hover;
                    for (n, it) in items.iter().enumerate() {
                        let src = if self.objs[i].disabled.get(n) == Some(&true) {
                            &it.disabled
                        } else if hover == Some(n) {
                            &it.hover
                        } else {
                            &it.path
                        };
                        if let Some(img) = self.image(src) {
                            let (w, h) = (img.w as i32, img.h as i32);
                            put(&mut out, img, x + it.left, y + it.top, w, h, a);
                        }
                    }
                }
                lsb::MENU => {
                    let o = &self.objs[i];
                    let [l, t, r, b, line] = o.geom;
                    let (w, h) = (o.int(WIDTH) as i32, o.int(HEIGHT) as i32);
                    let (cw, ch) = (w - l - r, h - t - b);
                    let cells = [
                        (x, y, l, t),
                        (x + l, y, cw, t),
                        (x + w - r, y, r, t),
                        (x, y + t, l, ch),
                        (x + l, y + t, cw, ch),
                        (x + w - r, y + t, r, ch),
                        (x, y + h - b, l, b),
                        (x + l, y + h - b, cw, b),
                        (x + w - r, y + h - b, r, b),
                    ];
                    let srcs: Vec<String> = (55..=63).map(|p| o.str(p)).collect();
                    let (align, my) = (o.int(54), o.int(53) as i32);
                    let (lines, hover) = (o.lines.clone(), o.hover);
                    if srcs.iter().all(|s| s.is_empty()) {
                        let bg = self.solid(0);
                        put(&mut out, bg, x, y, w, h, a * 0.7);
                    }
                    for (src, c) in srcs.iter().zip(cells) {
                        if let Some(img) = self.image(src) {
                            put(&mut out, img, c.0, c.1, c.2, c.3, a);
                        }
                    }
                    for (k, pics) in lines.into_iter().enumerate() {
                        let img = pics[(hover == Some(k)) as usize].clone();
                        let lw = img.w as i32;
                        let lx = match align {
                            1 => x + (w - lw) / 2,
                            2 => x + w - r - lw,
                            _ => x + l,
                        };
                        let (iw, ih) = (img.w as i32, img.h as i32);
                        put(&mut out, img, lx, y + t + my + k as i32 * line, iw, ih, a);
                    }
                }
                lsb::SOUND | lsb::MOVIE | lsb::TIMER | lsb::CINEMA => {}
                _ => {
                    let o = &self.objs[i];
                    if let Some(img) = o.img.clone() {
                        let (mut w, mut h) = (img.w as i32, img.h as i32);
                        if kind == lsb::TILE_NEW {
                            w = o.int(WIDTH) as i32;
                            h = o.int(HEIGHT) as i32;
                        }
                        put(&mut out, img, x, y, w, h, a);
                    }
                }
            }
        }
        out
    }

    /// Sizes a menu from its border pictures and lines, and draws the lines.
    fn menu_setup(&mut self, i: usize) {
        let o = &self.objs[i];
        let size = match o.int(FONTHEIGHT) {
            n if n > 0 => n as u32,
            _ => 20,
        };
        let space = o.int(LINESPACE) as i32;
        let line = size as i32 + space;
        let (color, hover) = (o.int(FONTCOLOR), o.int(23));
        let (mx, my) = (o.int(52) as i32, o.int(53) as i32);
        let (tl, br) = (o.str(55), o.str(63));
        let names: Vec<String> = o.items.iter().map(|m| m.name.clone()).collect();
        let tl = self.image(&tl);
        let br = self.image(&br);
        let (l, t) = tl.map_or((8, 8), |m| (m.w as i32, m.h as i32));
        let (r, b) = br.map_or((8, 8), |m| (m.w as i32, m.h as i32));
        let mut lines = Vec::new();
        let mut tw = 0;
        for n in &names {
            let (chars, w, h) = layout(&mut self.fonts, n, size, line);
            tw = tw.max(w);
            let pic = |fonts: &mut Fonts, c| {
                Rc::new(render_text(
                    fonts,
                    w as u32 + 2,
                    h as u32 + 2,
                    &chars,
                    size,
                    c,
                    None,
                ))
            };
            lines.push([pic(&mut self.fonts, color), pic(&mut self.fonts, hover)]);
        }
        let w = l + r + tw + 2 * mx;
        let h = t + b + names.len() as i32 * line - space + 2 * my;
        let o = &mut self.objs[i];
        o.geom = [l + mx, t, r + mx, b, line];
        o.lines = lines;
        for (p, v) in [(WIDTH, w), (HEIGHT, h), (91, w), (92, h)] {
            o.props.insert(p, Value::Int(v as i64));
        }
    }

    /// The line of a menu at (x, y) under the pointer.
    fn menu_row(&self, i: usize, x: i32, y: i32) -> Option<usize> {
        let o = &self.objs[i];
        let (mx, my) = self.mouse;
        let [_, t, _, _, line] = o.geom;
        let top = y + t + o.int(53) as i32;
        if mx < x || mx >= x + o.int(WIDTH) as i32 || my < top || line <= 0 {
            return None;
        }
        let n = ((my - top) / line) as usize;
        (n < o.items.len() && o.disabled.get(n) != Some(&true)).then_some(n)
    }

    /// The text picture of a caption, button, edit box or menu.
    fn label(&mut self, i: usize) -> Option<Rc<Image>> {
        if let Some(l) = &self.objs[i].label {
            return Some(l.clone());
        }
        let o = &self.objs[i];
        let text = o.str(TEXT);
        if text.is_empty() {
            return None;
        }
        let size = match o.int(FONTHEIGHT) {
            n if n > 0 => n as u32,
            _ => 20,
        };
        let color = if o.kind == lsb::EDIT_NEW {
            0
        } else if o.props.contains_key(&FONTCOLOR) {
            o.int(FONTCOLOR)
        } else {
            0xffffff
        };
        let border = (o.int(FONTBORDER) > 0).then(|| o.int(FONTBORDERCOLOR));
        let line = size as i32 + o.int(LINESPACE) as i32;
        let (chars, w, h) = layout(&mut self.fonts, &text, size, line);
        let img = Rc::new(render_text(
            &mut self.fonts,
            w as u32 + 2,
            h as u32 + 2,
            &chars,
            size,
            color,
            border,
        ));
        let o = &mut self.objs[i];
        if o.kind == lsb::CAPTION {
            o.props.insert(WIDTH, Value::Int(w as i64));
            o.props.insert(HEIGHT, Value::Int(h as i64));
        }
        o.label = Some(img.clone());
        Some(img)
    }

    fn mes_image(&mut self, i: usize) -> Option<Rc<Image>> {
        let o = &self.objs[i];
        let m = o.mes.as_ref()?;
        if !m.dirty
            && let Some(img) = &m.image
        {
            return Some(img.clone());
        }
        let (w, h) = (o.int(WIDTH).max(1) as u32, o.int(HEIGHT).max(1) as u32);
        let size = mes_size(o);
        let color = o.int(FONTCOLOR);
        let border = (o.int(FONTBORDER) > 0).then(|| o.int(FONTBORDERCOLOR));
        let chars = m.chars.clone();
        let img = Rc::new(render_text(
            &mut self.fonts,
            w,
            h + size,
            &chars,
            size,
            color,
            border,
        ));
        let m = self.objs[i].mes.as_mut().unwrap();
        m.image = Some(img.clone());
        m.dirty = false;
        Some(img)
    }

    /// The pictures of the screen, bottom first.
    pub fn draws(&mut self) -> Vec<Draw> {
        let mut new = self.screen_draws();
        let Some(f) = &self.flip else { return new };
        let t = ((self.now - f.t0) / f.dur).clamp(0.0, 1.0) as f32;
        let mut out = f.old.clone();
        for d in new.iter_mut() {
            d.place.alpha *= t;
        }
        out.extend(new);
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

    // ---- threads

    fn spawn(
        &mut self,
        handler: &str,
        params: Vec<Value>,
        sender: &str,
        key: Option<(String, u8)>,
    ) {
        if handler.is_empty() {
            return;
        }
        if let Some(k) = &key
            && self
                .threads
                .iter()
                .any(|t| !t.done && t.key.as_ref() == Some(k))
        {
            return;
        }
        let mut th = Thread {
            key,
            ..Thread::default()
        };
        if self.push_handler(&mut th, (handler.to_string(), params, sender.to_string())) {
            self.threads.push(th);
        }
    }

    fn push_handler(&mut self, th: &mut Thread, (handler, params, sender): Handler) -> bool {
        let (page, label) = handler_target(&handler);
        let Some(script) = self.script(&page) else {
            return false;
        };
        let pc = if label.is_empty() {
            0
        } else {
            script
                .commands
                .iter()
                .position(|c| matches!(&c.cmd, Cmd::Label(l) if *l == label))
                .unwrap_or(0)
        };
        th.frames.push(Frame {
            page,
            script,
            pc,
            params,
            locals: HashMap::new(),
            result: Value::default(),
            result_var: String::new(),
            sender,
        });
        true
    }

    /// Runs one frame (the scripts count on 60 a second).
    pub fn frame(&mut self) -> Result<(), String> {
        if self.ended {
            return Ok(());
        }
        self.now += FRAME_MS;
        self.lclick_now = false;
        self.rclick_now = std::mem::take(&mut self.rclick);
        if std::mem::take(&mut self.click) && !self.hit() {
            self.lclick_now = true;
        }
        self.hover();
        self.animate();
        // timers and sounds
        let mut fire = Vec::new();
        let mut gone = Vec::new();
        for o in self.objs.iter_mut() {
            match o.kind {
                lsb::TIMER if o.playing && o.cab.is_none() => {
                    if self.now - o.t0 >= o.int(DELAY) as f64 {
                        fire.push((o.str(ONNOTIFY), o.name().to_string()));
                        o.t0 = self.now;
                        if o.flag(AUTOSCRAP) {
                            o.playing = false;
                            gone.push(o.name().to_string());
                        }
                    }
                }
                lsb::SOUND | lsb::MOVIE
                    if o.playing && !o.flag(REPEAT) && self.now >= o.t0 + o.dur =>
                {
                    o.playing = false;
                    if o.flag(AUTOSCRAP) {
                        gone.push(o.name().to_string());
                    }
                }
                _ => {}
            }
        }
        for (h, name) in fire {
            self.spawn(&h, Vec::new(), &name, Some((name.clone(), ONNOTIFY)));
        }
        for n in gone {
            self.delete(&n);
        }
        self.play_text();
        let idle: Vec<(String, String)> = self
            .objs
            .iter()
            .filter(|o| o.cab.is_none() && !o.str(ONIDLE).is_empty())
            .map(|o| (o.str(ONIDLE), o.name().to_string()))
            .collect();
        for (h, name) in idle {
            self.spawn(&h, Vec::new(), &name, Some((name.clone(), ONIDLE)));
        }
        let mut t = 0;
        while t < self.threads.len() {
            if !self.threads[t].done {
                let mut th = std::mem::take(&mut self.threads[t]);
                self.run(&mut th);
                self.threads[t] = th;
            }
            t += 1;
        }
        let main_done = self.threads.iter().any(|t| t.main && t.done);
        self.threads.retain(|t| !t.done);
        if main_done {
            eprintln!("livemaker: the main chart ended");
            self.ended = true;
        }
        Ok(())
    }

    /// A click on a menu or button; false when it hit none.
    fn hit(&mut self) -> bool {
        let (mx, my) = self.mouse;
        if mx < 0 {
            return false;
        }
        let order = self.order();
        for &(i, x, y, _) in order.iter().rev() {
            let o = &self.objs[i];
            if o.int(IGNOREMOUSE) == 1 {
                continue;
            }
            let name = o.name().to_string();
            match o.kind {
                lsb::PREV_MENU_NEW => {
                    let mut hit = None;
                    for (n, r) in self.item_rects(i, x, y).into_iter().enumerate().rev() {
                        let o = &self.objs[i];
                        if o.disabled.get(n) == Some(&true) {
                            continue;
                        }
                        if mx >= r[0] && my >= r[1] && mx < r[2] && my < r[3] {
                            hit = Some((n, o.items[n].name.clone()));
                            break;
                        }
                    }
                    if let Some((n, item)) = hit {
                        let params = vec![Value::Str(item), Value::Int(n as i64)];
                        let o = &self.objs[i];
                        let chain = [o.str(ONSELECT), o.str(ONPLAYEND)];
                        self.spawn_chain(&chain, params, &name);
                        return true;
                    }
                }
                lsb::MENU => {
                    if let Some(n) = self.menu_row(i, x, y) {
                        let o = &self.objs[i];
                        let params =
                            vec![Value::Str(o.items[n].name.clone()), Value::Int(n as i64)];
                        let chain = [o.str(ONSELECT)];
                        let scrap = o.flag(AUTOSCRAP);
                        self.spawn_chain(&chain, params, &name);
                        if scrap {
                            self.delete(&name);
                        }
                        return true;
                    }
                }
                lsb::BUTTON => {
                    let (w, h) = (o.int(WIDTH) as i32, o.int(HEIGHT) as i32);
                    if mx >= x && my >= y && mx < x + w && my < y + h {
                        let chain = [o.str(ONNOTIFY)];
                        self.spawn_chain(&chain, Vec::new(), &name);
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Where the buttons of a preview menu at (x, y) are: left, top, right, bottom.
    fn item_rects(&mut self, i: usize, x: i32, y: i32) -> Vec<[i32; 4]> {
        let items = self.objs[i].items.clone();
        items
            .iter()
            .map(|it| {
                let (w, h) = self
                    .image(&it.path)
                    .map_or((0, 0), |m| (m.w as i32, m.h as i32));
                [x + it.left, y + it.top, x + it.left + w, y + it.top + h]
            })
            .collect()
    }

    fn spawn_chain(&mut self, chain: &[String], params: Vec<Value>, sender: &str) {
        let mut th = Thread::default();
        for h in chain.iter().filter(|h| !h.is_empty()) {
            th.then
                .push_back((h.clone(), params.clone(), sender.to_string()));
        }
        if !th.then.is_empty() {
            // before the chart that waits for the choice looks again
            self.threads.insert(0, th);
        }
    }

    fn hover(&mut self) {
        let (mx, my) = self.mouse;
        let order = self.order();
        let mut found: Option<(usize, usize)> = None;
        for &(i, x, y, _) in order.iter().rev() {
            let o = &self.objs[i];
            if o.int(IGNOREMOUSE) == 1 || found.is_some() {
                continue;
            }
            match o.kind {
                lsb::PREV_MENU_NEW => {
                    for (n, r) in self.item_rects(i, x, y).into_iter().enumerate() {
                        if mx >= r[0] && my >= r[1] && mx < r[2] && my < r[3] {
                            found = Some((i, n));
                        }
                    }
                }
                lsb::MENU => {
                    if let Some(n) = self.menu_row(i, x, y) {
                        found = Some((i, n));
                    }
                }
                lsb::BUTTON => {
                    let (w, h) = (o.int(WIDTH) as i32, o.int(HEIGHT) as i32);
                    if mx >= x && my >= y && mx < x + w && my < y + h {
                        found = Some((i, 0));
                    }
                }
                _ => {}
            }
        }
        for (i, o) in self.objs.iter_mut().enumerate() {
            o.hover = found.filter(|f| f.0 == i).map(|f| f.1);
        }
    }

    fn animate(&mut self) {
        let now = self.now;
        let mut done = Vec::new();
        let mut sets = Vec::new();
        for (k, m) in self.motions.iter().enumerate() {
            let t = if m.dur <= 0.0 {
                1.0
            } else {
                ((now - m.t0) / m.dur).clamp(0.0, 1.0)
            };
            sets.push((m.obj.clone(), m.prop, m.from + (m.to - m.from) * t));
            if t >= 1.0 {
                done.push(k);
            }
        }
        for (o, p, v) in sets {
            self.set_prop(&o, p, Value::Int(v.round() as i64));
        }
        for k in done.into_iter().rev() {
            self.motions.remove(k);
        }
        if let Some(f) = &self.flip
            && now >= f.t0 + f.dur
        {
            self.flip = None;
        }
    }

    /// Plays the message boxes' text and fires their events.
    fn play_text(&mut self) {
        for i in 0..self.objs.len() {
            if self.objs[i].kind != lsb::MES_NEW || self.objs[i].cab.is_some() {
                continue;
            }
            let o = &self.objs[i];
            let m = o.mes.as_ref().unwrap();
            if o.flag(TEXTPAUSED) || m.waiting || m.queue.is_empty() {
                continue;
            }
            let name = o.name().to_string();
            let delay = (o.int(TEXTDELAY) as f64).min(50.0);
            let width = o.int(WIDTH) as i32;
            let size = mes_size(o);
            let line = size as i32 + o.int(LINESPACE) as i32;
            let (on_char, on_end, on_event) = (o.str(ONPLAYING), o.str(ONPLAYEND), o.str(ONNOTIFY));
            let mut spawned = Vec::new();
            {
                let mut m = self.objs[i].mes.take().unwrap();
                m.acc += FRAME_MS;
                let mut chars = 0;
                while let Some(g) = m.queue.pop_front() {
                    match g {
                        Glyph::Char { ch, .. } => {
                            if delay > 0.0 {
                                if m.acc < delay {
                                    m.queue.push_front(g);
                                    break;
                                }
                                m.acc -= delay;
                            }
                            place(&mut m, &mut self.fonts, ch, size, width, line);
                            chars += 1;
                        }
                        Glyph::Var { name: v, .. } => {
                            let s = self
                                .globals
                                .get(&v)
                                .map_or(String::new(), |x| x.v.to_string());
                            for ch in s.chars().rev() {
                                m.queue.push_front(Glyph::Char {
                                    ch,
                                    style: -1,
                                    link: 0,
                                    speed: 0,
                                });
                            }
                        }
                        Glyph::Return(0) => newline(&mut m, line),
                        Glyph::Return(3) => clear(&mut m),
                        Glyph::Return(_) => {
                            m.waiting = true;
                            spawned.push((on_end.clone(), Vec::new(), ONPLAYEND));
                            break;
                        }
                        Glyph::Indent => m.indent = m.x,
                        Glyph::Undent => m.indent = 0,
                        Glyph::Event(s) => {
                            let s = s.strip_prefix('\u{1}').unwrap_or(&s).to_string();
                            let params =
                                s.split("\r\n").map(|p| Value::Str(p.to_string())).collect();
                            if self.log > 0 {
                                eprintln!("  event {}", s.replace("\r\n", " | "));
                            }
                            spawned.push((on_event.clone(), params, ONNOTIFY));
                            break;
                        }
                        _ => {}
                    }
                }
                if m.acc > delay.max(FRAME_MS) {
                    m.acc = 0.0;
                }
                if chars > 0 {
                    spawned.insert(0, (on_char.clone(), Vec::new(), ONPLAYING));
                }
                if m.queue.is_empty() && !m.waiting && !m.text.is_empty() && spawned.is_empty() {
                    m.waiting = true;
                    spawned.push((on_end.clone(), Vec::new(), ONPLAYEND));
                }
                self.objs[i].mes = Some(m);
            }
            for (h, params, ev) in spawned {
                // text events run one after another, each in its own thread
                let k = if ev == ONNOTIFY {
                    None
                } else {
                    Some((name.clone(), ev))
                };
                self.spawn(&h, params, &name, k);
            }
        }
    }

    fn run(&mut self, th: &mut Thread) {
        let mut budget = 200_000u32;
        let mut loops = 0u32;
        loop {
            if th.frames.is_empty() {
                match th.then.pop_front() {
                    Some(h) => {
                        if !self.push_handler(th, h) {
                            continue;
                        }
                    }
                    None => {
                        th.done = true;
                        return;
                    }
                }
            }
            if self.ended {
                return;
            }
            let f = th.frames.last().unwrap();
            let script = f.script.clone();
            let pc = f.pc;
            let Some(c) = script.commands.get(pc) else {
                self.ret(th);
                continue;
            };
            budget -= 1;
            if budget == 0 {
                eprintln!("livemaker: {}:{pc} runs too long", f.page);
                return;
            }
            if c.mute {
                th.frames.last_mut().unwrap().pc += 1;
                continue;
            }
            // a loop that spins shows only its first round each frame
            if (self.log > 1 || (self.log == 1 && !th.key.as_ref().is_some_and(|k| k.1 == ONIDLE)))
                && !th.started
                && loops < 1
            {
                eprintln!(
                    "{}:{pc} {}{}",
                    f.page,
                    "  ".repeat(c.indent as usize),
                    c.cmd
                );
            }
            match self.exec(th, &script, pc) {
                Go::Next => {
                    if let Some(f) = th.frames.last_mut()
                        && Rc::ptr_eq(&f.script, &script)
                        && f.pc == pc
                    {
                        f.pc += 1;
                    }
                }
                Go::Jumped => {}
                Go::Loop => {
                    loops += 1;
                    if loops > 1000 {
                        return;
                    }
                }
                Go::Yield => return,
            }
        }
    }

    fn ret(&mut self, th: &mut Thread) {
        th.started = false;
        let Some(f) = th.frames.pop() else { return };
        if !f.result_var.is_empty() && !th.frames.is_empty() {
            let target = f.result_var.clone();
            if let Some(open) = target.find('[') {
                let name = &target[..open];
                let idx = target[open + 1..].trim_end_matches(']');
                let n = idx
                    .parse::<i64>()
                    .unwrap_or_else(|_| self.var(th, idx).int());
                self.set_element(th, name, &[n], f.result);
            } else {
                self.assign(th, &target, f.result);
            }
        }
    }

    fn goto(&mut self, th: &mut Thread, to: &LabelRef) {
        let f = th.frames.last().unwrap();
        if key(&f.page) == key(&to.page) {
            th.frames.last_mut().unwrap().pc = to.label as usize;
            return;
        }
        match self.script(&to.page) {
            Some(s) => {
                let f = th.frames.last_mut().unwrap();
                f.page = to.page.clone();
                f.script = s;
                f.pc = to.label as usize;
            }
            None => {
                th.frames.last_mut().unwrap().pc = usize::MAX;
            }
        }
    }

    fn cond(&mut self, th: &mut Thread, e: &Expr) -> bool {
        e.0.is_empty() || self.eval(th, e).truthy()
    }

    fn exec(&mut self, th: &mut Thread, script: &Rc<Script>, pc: usize) -> Go {
        let cmds = &script.commands;
        let c = &cmds[pc];
        let set_pc = |th: &mut Thread, n: usize| th.frames.last_mut().unwrap().pc = n;
        match &c.cmd {
            Cmd::If(e) => {
                if self.eval(th, e).truthy() {
                    return Go::Next;
                }
                let d = c.indent;
                let mut j = pc + 1;
                loop {
                    while j < cmds.len() && cmds[j].indent > d {
                        j += 1;
                    }
                    if j < cmds.len() && cmds[j].indent == d {
                        match &cmds[j].cmd {
                            Cmd::Elseif(e2) => {
                                if self.eval(th, e2).truthy() {
                                    set_pc(th, j + 1);
                                    return Go::Jumped;
                                }
                                j += 1;
                                continue;
                            }
                            Cmd::Else => {
                                set_pc(th, j + 1);
                                return Go::Jumped;
                            }
                            _ => {}
                        }
                    }
                    set_pc(th, j);
                    return Go::Jumped;
                }
            }
            Cmd::Elseif(_) | Cmd::Else => {
                // the branch before ran: skip the rest of the chain
                let d = c.indent;
                let mut j = pc + 1;
                loop {
                    while j < cmds.len() && cmds[j].indent > d {
                        j += 1;
                    }
                    if j < cmds.len()
                        && cmds[j].indent == d
                        && matches!(cmds[j].cmd, Cmd::Elseif(_) | Cmd::Else)
                    {
                        j += 1;
                        continue;
                    }
                    set_pc(th, j);
                    return Go::Jumped;
                }
            }
            Cmd::Label(_) | Cmd::Comment | Cmd::Ifdef | Cmd::Ifndef | Cmd::Endif => {}
            Cmd::Jump(to, cond) => {
                if self.cond(th, cond) {
                    self.goto(th, to);
                    return Go::Jumped;
                }
            }
            Cmd::Call {
                to,
                result,
                cond,
                params,
            } => {
                if !self.cond(th, cond) {
                    return Go::Next;
                }
                let params: Vec<Value> = params.iter().map(|e| self.eval(th, e)).collect();
                th.frames.last_mut().unwrap().pc = pc + 1;
                let Some(script) = self.script(&to.page) else {
                    return Go::Jumped;
                };
                let sender = th.frames.last().unwrap().sender.clone();
                th.frames.push(Frame {
                    page: to.page.clone(),
                    script,
                    pc: to.label as usize,
                    params,
                    locals: HashMap::new(),
                    result: Value::default(),
                    result_var: result.clone(),
                    sender,
                });
                if th.frames.len() > 200 {
                    eprintln!("livemaker: calls too deep at {}", to.page);
                    th.frames.truncate(1);
                }
                return Go::Jumped;
            }
            Cmd::Exit(cond) => {
                if self.cond(th, cond) {
                    self.ret(th);
                    return Go::Jumped;
                }
            }
            Cmd::Wait { cond, time, stop } => {
                let t = self.eval(th, time).int();
                if cond.0.is_empty() && t <= 0 {
                    return Go::Next;
                }
                if !th.started {
                    th.started = true;
                    th.deadline = if t > 0 { self.now + t as f64 } else { f64::MAX };
                }
                let c = cond.0.is_empty() || self.eval(th, cond).truthy();
                let stop = !stop.0.is_empty() && self.eval(th, stop).truthy() && self.lclick_now;
                if !c || self.now >= th.deadline || stop {
                    th.started = false;
                    return Go::Next;
                }
                return Go::Yield;
            }
            Cmd::New(kind, props) => self.create(th, *kind, props),
            Cmd::Flip {
                wipe,
                time,
                targets,
                delete,
                stop,
                ..
            } => {
                if th.started {
                    let stop = self.cond(th, stop) && self.lclick_now;
                    if self.now >= th.deadline || stop {
                        th.started = false;
                        self.flip = None;
                        return Go::Next;
                    }
                    return Go::Yield;
                }
                let wipe = self.eval(th, wipe).int();
                let t = self.eval(th, time).int();
                let mut names = Vec::new();
                for e in targets {
                    names.extend(split_list(&self.eval(th, e).to_string()));
                }
                let delete = self.cond(th, delete);
                let old = if wipe != 0 && t > 0 {
                    self.screen_draws()
                } else {
                    Vec::new()
                };
                for n in names {
                    if delete {
                        self.delete(&n);
                    } else {
                        self.set_prop(&n, VISIBLE, Value::Flag(false));
                    }
                }
                for o in self.objs.iter_mut() {
                    if o.parent() == BACK {
                        o.props.insert(PARENT, Value::Str(String::new()));
                    }
                }
                if wipe != 0 && t > 0 {
                    self.flip = Some(FlipFx {
                        old,
                        t0: self.now,
                        dur: t as f64,
                    });
                    th.started = true;
                    th.deadline = self.now + t as f64;
                    return Go::Yield;
                }
                self.flip = None;
            }
            Cmd::Calc(e) | Cmd::WhileInit(e) => {
                self.eval(th, e);
            }
            Cmd::VarNew {
                name,
                ty,
                init,
                scope,
            } => {
                let v = if init.0.is_empty() {
                    Value::zero(*ty)
                } else {
                    self.eval(th, init)
                };
                let var = Var::new(*ty, v);
                if matches!(scope, 0 | 3) {
                    self.globals.insert(name.clone(), var);
                } else {
                    th.frames
                        .last_mut()
                        .unwrap()
                        .locals
                        .insert(name.clone(), var);
                }
            }
            Cmd::VarDel(name) => {
                if let Some(f) = th
                    .frames
                    .iter_mut()
                    .rev()
                    .find(|f| f.locals.contains_key(name))
                {
                    f.locals.remove(name);
                } else {
                    self.globals.remove(name);
                }
            }
            Cmd::GetProp { obj, prop, var } => {
                let o = self.eval(th, obj).to_string();
                let p = self.eval(th, prop).int() + 1;
                let v = self.get_prop(&o, p as u8);
                self.assign(th, var, v);
            }
            Cmd::SetProp { obj, prop, value } => {
                let o = self.eval(th, obj).to_string();
                let p = self.eval(th, prop).int() + 1;
                let v = self.eval(th, value);
                self.set_prop(&o, p as u8, v);
            }
            Cmd::ObjDel(e) => {
                let n = self.eval(th, e).to_string();
                self.delete(&n);
            }
            Cmd::TextIns {
                text, target, wait, ..
            } => {
                let n = self.eval(th, target).to_string();
                let Some(i) = self.find(&n).filter(|&i| self.objs[i].mes.is_some()) else {
                    if self.log > 0 {
                        eprintln!("livemaker: TextIns into {n:?}, which is no message box");
                    }
                    return Go::Next;
                };
                if !th.started {
                    if self.log > 0 {
                        eprintln!("  text {}", text.plain().replace('\n', " / "));
                    }
                    let o = &mut self.objs[i];
                    let m = o.mes.as_mut().unwrap();
                    m.queue.extend(text.body.iter().cloned());
                    let h = o.str(ONPLAYSTART);
                    self.spawn(&h, vec![Value::Str("0".into())], &n, None);
                    if !self.cond(th, wait) || wait.0.is_empty() {
                        return Go::Next;
                    }
                    th.started = true;
                }
                if self.get_prop(&n, PLAYING).truthy() {
                    return Go::Yield;
                }
                th.started = false;
            }
            Cmd::TextClr(e) => {
                let n = self.eval(th, e).to_string();
                if let Some(i) = self.find(&n)
                    && let Some(m) = self.objs[i].mes.as_mut()
                {
                    clear(m);
                }
            }
            Cmd::MovieStop { target, time, .. } => {
                let n = self.eval(th, target).to_string();
                let t = self.eval(th, time).int().max(0) as u32;
                if let Some(i) = self.find(&n)
                    && self.objs[i].playing
                {
                    self.stop(i, t);
                }
            }
            Cmd::MediaPlay(e) => {
                let n = self.eval(th, e).to_string();
                if let Some(i) = self.find(&n)
                    && matches!(self.objs[i].kind, lsb::SOUND | lsb::MOVIE)
                {
                    self.play(i);
                }
            }
            Cmd::While { cond, end } => {
                if !self.eval(th, cond).truthy() {
                    set_pc(th, *end as usize);
                    return Go::Jumped;
                }
            }
            Cmd::WhileLoop { calc, start } => {
                self.eval(th, calc);
                set_pc(th, *start as usize);
                return Go::Loop;
            }
            Cmd::Break { cond, end } => {
                if self.cond(th, cond) {
                    set_pc(th, *end as usize);
                    return Go::Jumped;
                }
            }
            Cmd::Continue { cond, start } => {
                if self.cond(th, cond) {
                    set_pc(th, *start as usize);
                    return Go::Loop;
                }
            }
            Cmd::DoEvent => {
                if !th.started {
                    th.started = true;
                    return Go::Yield;
                }
                th.started = false;
            }
            Cmd::Terminate => {
                self.ended = true;
                return Go::Yield;
            }
            Cmd::PcReset { to, .. } | Cmd::Reset { to, .. } => {
                if matches!(c.cmd, Cmd::Reset { .. }) {
                    self.objs.clear();
                    for t in self.threads.iter_mut() {
                        t.done = true;
                    }
                }
                th.frames.truncate(1);
                th.started = false;
                self.goto(th, to);
                return Go::Jumped;
            }
            Cmd::PropMotion {
                obj,
                prop,
                value,
                time,
                ..
            } => {
                let o = self.eval(th, obj).to_string();
                let p = (self.eval(th, prop).int() + 1) as u8;
                let to = self.eval(th, value).float();
                let dur = self.eval(th, time).float();
                let from = self.get_prop(&o, p).float();
                self.motions.retain(|m| !(m.obj == o && m.prop == p));
                self.motions.push(Motion {
                    obj: o,
                    prop: p,
                    from,
                    to,
                    t0: self.now,
                    dur,
                });
            }
            Cmd::Cabinet {
                load,
                props,
                targets,
                ..
            } => {
                let mut cab = String::new();
                for (p, e) in props {
                    if *p == NAME && !e.0.is_empty() {
                        cab = self.eval(th, e).to_string();
                    }
                }
                if *load {
                    for o in self.objs.iter_mut() {
                        if o.cab.as_deref() == Some(cab.as_str()) {
                            o.cab = None;
                        }
                    }
                } else {
                    let mut names = Vec::new();
                    for e in targets {
                        names.extend(split_list(&self.eval(th, e).to_string()));
                    }
                    for n in names {
                        if let Some(i) = self.find(&n) {
                            self.objs[i].cab = Some(cab.clone());
                        }
                    }
                }
            }
            Cmd::ClrHist
            | Cmd::MenuClose(_)
            | Cmd::CallHist(_)
            | Cmd::GameSave { .. }
            | Cmd::GameLoad(_)
            | Cmd::ClrRead
            | Cmd::FormatHist => {}
        }
        Go::Next
    }

    // ---- Calc("...")

    fn calc_str(&mut self, th: &mut Thread, src: &str) -> Value {
        let t = tokens(src);
        let assign = t.iter().position(|x| *x == Tok::Op("=".into()));
        let (lhs, rhs) = match assign {
            Some(k) => (&t[..k], &t[k + 1..]),
            None => (&t[..0], &t[..]),
        };
        let mut at = 0;
        let v = self.calc_expr(th, rhs, &mut at, 0);
        match lhs {
            [Tok::Id(n)] => self.assign(th, n, v.clone()),
            [Tok::Id(n), Tok::Op(o), rest @ ..] if o == "[" => {
                let mut k = 0;
                let i = self.calc_expr(th, rest, &mut k, 0).int();
                self.set_element(th, n, &[i], v.clone());
            }
            _ => {}
        }
        v
    }

    fn calc_expr(&mut self, th: &mut Thread, t: &[Tok], at: &mut usize, level: u8) -> Value {
        const LEVELS: [&[&str]; 4] = [
            &["|", "&", "^"],
            &["==", "!=", "<", ">", "<=", ">="],
            &["+", "-", "++"],
            &["*", "/", "%"],
        ];
        if level as usize >= LEVELS.len() {
            return self.calc_atom(th, t, at);
        }
        let mut v = self.calc_expr(th, t, at, level + 1);
        while let Some(Tok::Op(o)) = t.get(*at) {
            if !LEVELS[level as usize].contains(&o.as_str()) {
                break;
            }
            let op = match o.as_str() {
                "+" => 2,
                "-" => 3,
                "*" => 4,
                "/" => 5,
                "%" => 6,
                "|" => 7,
                "&" => 8,
                "^" => 9,
                "==" => 12,
                ">" => 13,
                "<" => 14,
                ">=" => 15,
                "<=" => 16,
                "!=" => 20,
                _ => 19,
            };
            *at += 1;
            let b = self.calc_expr(th, t, at, level + 1);
            v = binary(op, &v, &b);
        }
        v
    }

    fn calc_atom(&mut self, th: &mut Thread, t: &[Tok], at: &mut usize) -> Value {
        let Some(tok) = t.get(*at).cloned() else {
            return Value::default();
        };
        *at += 1;
        let word = |id: &str| match id.to_uppercase().as_str() {
            "TRUE" => Some(Value::Flag(true)),
            "FALSE" => Some(Value::Flag(false)),
            _ => None,
        };
        match tok {
            Tok::Num(x, true) => Value::Float(x),
            Tok::Num(x, false) => Value::Int(x as i64),
            Tok::Str(s) => Value::Str(s),
            Tok::Op(o) if o == "(" => {
                let v = self.calc_expr(th, t, at, 0);
                *at += 1;
                v
            }
            Tok::Op(o) if o == "-" => {
                let v = self.calc_atom(th, t, at);
                binary(3, &Value::Int(0), &v)
            }
            Tok::Op(_) => Value::default(),
            Tok::Id(id) => match t.get(*at) {
                Some(Tok::Op(o)) if o == "(" => {
                    *at += 1;
                    let mut args = Vec::new();
                    while *at < t.len() && t[*at] != Tok::Op(")".into()) {
                        args.push(self.calc_expr(th, t, at, 0));
                        if t.get(*at) == Some(&Tok::Op(",".into())) {
                            *at += 1;
                        }
                    }
                    *at += 1;
                    let Some(f) = names::FUNCS
                        .iter()
                        .position(|f| f.eq_ignore_ascii_case(&id))
                    else {
                        return Value::default();
                    };
                    let mut temps = HashMap::new();
                    let params: Vec<Param> = args
                        .into_iter()
                        .enumerate()
                        .map(|(k, v)| {
                            let n = format!("____c{k}");
                            temps.insert(n.clone(), v);
                            Param::Var(n)
                        })
                        .collect();
                    self.func(th, f as u8, &params, &temps, &HashMap::new())
                }
                Some(Tok::Op(o)) if o == "[" => {
                    *at += 1;
                    let i = self.calc_expr(th, t, at, 0).int();
                    *at += 1;
                    self.element(th, &id, &[i])
                }
                _ => word(&id).unwrap_or_else(|| self.var(th, &id)),
            },
        }
    }

    // ---- variables and expressions

    fn frame_ref(th: &Thread) -> Option<&Frame> {
        th.frames.last()
    }

    fn var(&self, th: &Thread, name: &str) -> Value {
        if name.starts_with('@') {
            return self.sys_var(th, name);
        }
        for f in th.frames.iter().rev() {
            if let Some(v) = f.locals.get(name) {
                return v.v.clone();
            }
        }
        self.globals
            .get(name)
            .map_or(Value::Str(String::new()), |v| v.v.clone())
    }

    fn var_ref<'a>(&'a self, th: &'a Thread, name: &str) -> Option<&'a Var> {
        for f in th.frames.iter().rev() {
            if let Some(v) = f.locals.get(name) {
                return Some(v);
            }
        }
        self.globals.get(name)
    }

    fn var_mut<'a>(
        globals: &'a mut HashMap<String, Var>,
        th: &'a mut Thread,
        name: &str,
    ) -> &'a mut Var {
        if let Some(f) = th
            .frames
            .iter_mut()
            .rev()
            .find(|f| f.locals.contains_key(name))
        {
            return f.locals.get_mut(name).unwrap();
        }
        globals
            .entry(name.to_string())
            .or_insert_with(|| Var::new(0, Value::default()))
    }

    fn sys_var(&self, th: &Thread, name: &str) -> Value {
        let f = Self::frame_ref(th);
        match name {
            "@ParamStr" => f
                .and_then(|f| f.params.first().cloned())
                .unwrap_or_default(),
            "@ParamCount" => Value::Int(f.map_or(0, |f| f.params.len() as i64)),
            "@Sender" => Value::Str(f.map_or(String::new(), |f| f.sender.clone())),
            "@Result" => f.map_or(Value::default(), |f| f.result.clone()),
            "@ScrWidth" => Value::Int(self.width as i64),
            "@ScrHeight" => Value::Int(self.height as i64),
            "@LClick" | "@LPush" => Value::Flag(self.lclick_now),
            "@RClick" | "@RPush" => Value::Flag(self.rclick_now),
            "@MouseX" => Value::Int(self.mouse.0 as i64),
            "@MouseY" => Value::Int(self.mouse.1 as i64),
            "@ActiveCompo" => self.sys.get(name).cloned().unwrap_or_default(),
            "@HistoryCount" => Value::Int(0),
            "@TickCount" => Value::Int(self.now as i64),
            "@KeyClick" | "@KeyPush" => Value::Flag(false),
            "@Year" => Value::Int(2026),
            "@Month" | "@Day" => Value::Int(1),
            "@Hour" | "@Min" | "@Sec" => Value::Int(0),
            _ => self.sys.get(name).cloned().unwrap_or(Value::Int(0)),
        }
    }

    fn element(&self, th: &Thread, name: &str, idx: &[i64]) -> Value {
        match name {
            "@ParamStr" => {
                let i = idx.first().copied().unwrap_or(0);
                return Self::frame_ref(th)
                    .and_then(|f| f.params.get(i.max(0) as usize).cloned())
                    .unwrap_or_default();
            }
            "@KeyClick" => {
                let k = idx.first().copied().unwrap_or(0);
                let _ = k;
                return Value::Flag(false);
            }
            "@KeyPush" => return Value::Flag(false),
            _ => {}
        }
        match self.var_ref(th, name) {
            Some(v) => {
                if v.arr.is_none() && idx.len() == 1 && idx[0] == 0 && v.v.is_str() {
                    // a plain string read as an array of lines
                    return Value::Str(v.v.to_string().split("\r\n").next().unwrap_or("").into());
                }
                v.get(v.flat(idx))
            }
            None => Value::default(),
        }
    }

    fn assign(&mut self, th: &mut Thread, name: &str, v: Value) {
        if name.is_empty() {
            return;
        }
        if name == "@Result" {
            if let Some(f) = th.frames.last_mut() {
                f.result = v;
            }
            return;
        }
        if name.starts_with('@') {
            self.sys.insert(name.to_string(), v);
            return;
        }
        Self::var_mut(&mut self.globals, th, name).set(v);
    }

    fn set_element(&mut self, th: &mut Thread, name: &str, idx: &[i64], v: Value) {
        if name == "@ParamStr" {
            if let Some(f) = th.frames.last_mut() {
                let i = idx.first().copied().unwrap_or(0).max(0) as usize;
                if f.params.len() <= i {
                    f.params.resize(i + 1, Value::default());
                }
                f.params[i] = v;
            }
            return;
        }
        let var = Self::var_mut(&mut self.globals, th, name);
        let at = var.flat(idx);
        var.set_at(at, v);
    }

    fn array(&self, th: &Thread, name: &str) -> Vec<Value> {
        if name == "@ParamStr" {
            return Self::frame_ref(th).map_or(Vec::new(), |f| f.params.clone());
        }
        match self.var_ref(th, name) {
            Some(v) => v.arr.clone().unwrap_or_default(),
            None => Vec::new(),
        }
    }

    fn set_array(&mut self, th: &mut Thread, name: &str, vals: Vec<Value>) {
        let var = Self::var_mut(&mut self.globals, th, name);
        let ty = var.ty;
        var.arr = Some(
            vals.into_iter()
                .map(|v| if ty == 0 { v } else { v.to_type(ty) })
                .collect(),
        );
        var.dims.clear();
    }

    fn param(&self, th: &Thread, p: &Param, temps: &HashMap<String, Value>) -> Value {
        match p {
            Param::Var(n) => match temps.get(n) {
                Some(v) => v.clone(),
                None => self.var(th, n),
            },
            Param::Int(n) => Value::Int(*n as i64),
            Param::Float(x) => Value::Float(*x),
            Param::Flag(b) => Value::Flag(*b != 0),
            Param::Str(s) => Value::Str(s.clone()),
        }
    }

    fn eval(&mut self, th: &mut Thread, e: &Expr) -> Value {
        let mut temps: HashMap<String, Value> = HashMap::new();
        let mut refs: HashMap<String, (String, Vec<i64>)> = HashMap::new();
        let mut last = Value::default();
        for s in &e.0 {
            let v = match s.op {
                1 => s
                    .args
                    .last()
                    .map(|p| self.param(th, p, &temps))
                    .unwrap_or_default(),
                10 => {
                    let base = match s.args.first() {
                        Some(Param::Var(n)) => refs.get(n).map_or(n.clone(), |r| r.0.clone()),
                        _ => String::new(),
                    };
                    let idx: Vec<i64> = s.args[1.min(s.args.len())..]
                        .iter()
                        .map(|p| self.param(th, p, &temps).int())
                        .collect();
                    let v = self.element(th, &base, &idx);
                    refs.insert(s.name.clone(), (base, idx));
                    temps.insert(s.name.clone(), v.clone());
                    last = v;
                    continue;
                }
                11 => self.func(th, s.func, &s.args, &temps, &refs),
                op => {
                    let a = s
                        .args
                        .first()
                        .map(|p| self.param(th, p, &temps))
                        .unwrap_or_default();
                    let b = s
                        .args
                        .get(1)
                        .map(|p| self.param(th, p, &temps))
                        .unwrap_or_default();
                    binary(op, &a, &b)
                }
            };
            if let Some((base, idx)) = refs.get(&s.name).cloned() {
                self.set_element(th, &base, &idx, v.clone());
                temps.insert(s.name.clone(), v.clone());
            } else if s.name.starts_with("____") || s.name.is_empty() {
                temps.insert(s.name.clone(), v.clone());
            } else {
                self.assign(th, &s.name, v.clone());
            }
            last = v;
        }
        last
    }

    /// Where a function writes its result through an argument (a variable or an element).
    fn write_param(
        &mut self,
        th: &mut Thread,
        p: Option<&Param>,
        refs: &HashMap<String, (String, Vec<i64>)>,
        v: Value,
    ) {
        let Some(Param::Var(n)) = p else { return };
        if let Some((base, idx)) = refs.get(n) {
            self.set_element(th, base, idx, v);
        } else if !n.starts_with("____") {
            self.assign(th, n, v);
        }
    }

    fn param_name(p: Option<&Param>, refs: &HashMap<String, (String, Vec<i64>)>) -> String {
        match p {
            Some(Param::Var(n)) => refs.get(n).map_or(n.clone(), |r| r.0.clone()),
            _ => String::new(),
        }
    }

    fn list_compo(&self, parent: Option<&str>, deep: bool) -> Vec<Value> {
        let Some(parent) = parent else {
            return self
                .objs
                .iter()
                .filter(|o| o.cab.is_none())
                .map(|o| Value::Str(o.name().to_string()))
                .collect();
        };
        if self.objs.iter().any(|o| o.cab.as_deref() == Some(parent)) {
            return self
                .objs
                .iter()
                .filter(|o| {
                    o.cab.as_deref() == Some(parent)
                        && !self
                            .find(o.parent())
                            .is_some_and(|j| self.objs[j].cab.as_deref() == Some(parent))
                })
                .map(|o| Value::Str(o.name().to_string()))
                .collect();
        }
        let mut out = Vec::new();
        let mut todo = vec![parent.to_string()];
        while let Some(p) = todo.pop() {
            for o in &self.objs {
                if o.parent() == p && !p.is_empty() {
                    out.push(Value::Str(o.name().to_string()));
                    if deep {
                        todo.push(o.name().to_string());
                    }
                }
            }
        }
        out
    }

    fn menu_index(&self, menu: &str, item: &Value) -> Option<(usize, usize)> {
        let i = self.find(menu)?;
        let items = &self.objs[i].items;
        let n = match item {
            Value::Int(n) => *n as usize,
            v => {
                let s = v.to_string();
                match items.iter().position(|it| it.name == s) {
                    Some(n) => n,
                    None => s.parse::<usize>().ok()?,
                }
            }
        };
        (n < items.len()).then_some((i, n))
    }

    fn func(
        &mut self,
        th: &mut Thread,
        f: u8,
        args: &[Param],
        temps: &HashMap<String, Value>,
        refs: &HashMap<String, (String, Vec<i64>)>,
    ) -> Value {
        let v: Vec<Value> = args.iter().map(|p| self.param(th, p, temps)).collect();
        let arg = |i: usize| v.get(i).cloned().unwrap_or_default();
        let s = |i: usize| arg(i).to_string();
        let n = |i: usize| arg(i).int();
        let name = names::func(f);
        match name {
            "IntToStr" => {
                let t = n(0).to_string();
                let w = n(1).max(0) as usize;
                Value::Str(if v.len() > 1 && t.len() < w {
                    format!("{}{t}", "0".repeat(w - t.len()))
                } else {
                    t
                })
            }
            "IntToHex" => Value::Str(format!("{:0w$X}", n(0), w = n(1).max(0) as usize)),
            "GetProp" => self.get_prop(&s(0), (n(1) + 1) as u8),
            "SetProp" => {
                self.set_prop(&s(0), (n(1) + 1) as u8, arg(2));
                Value::Flag(true)
            }
            "GetArraySize" => {
                Value::Int(self.array(th, &Self::param_name(args.first(), refs)).len() as i64)
            }
            "Length" => Value::Int(sjis_len(&s(0)) as i64),
            "JLength" => Value::Int(s(0).chars().count() as i64),
            "Copy" => Value::Str(sjis_copy(&s(0), n(1), n(2))),
            "JCopy" => Value::Str(
                s(0).chars()
                    .skip((n(1).max(1) - 1) as usize)
                    .take(n(2).max(0) as usize)
                    .collect(),
            ),
            "Delete" | "JDelete" => {
                let t = s(0);
                let out = if name == "Delete" {
                    let len = sjis_len(&t) as i64;
                    format!(
                        "{}{}",
                        sjis_copy(&t, 1, n(1) - 1),
                        sjis_copy(&t, n(1) + n(2), len)
                    )
                } else {
                    let c: Vec<char> = t.chars().collect();
                    let a = (n(1).max(1) - 1) as usize;
                    let b = (a + n(2).max(0) as usize).min(c.len());
                    c[..a.min(c.len())].iter().chain(c[b..].iter()).collect()
                };
                self.write_param(th, args.first(), refs, Value::Str(out.clone()));
                Value::Str(out)
            }
            "Insert" | "JInsert" => {
                // Insert(source, var, index)
                let t = s(1);
                let c: Vec<char> = t.chars().collect();
                let at = ((n(2).max(1) - 1) as usize).min(c.len());
                let out: String = c[..at].iter().collect::<String>()
                    + &s(0)
                    + &c[at..].iter().collect::<String>();
                self.write_param(th, args.get(1), refs, Value::Str(out.clone()));
                Value::Str(out)
            }
            "CompareStr" => Value::Int(s(0).cmp(&s(1)) as i64),
            "CompareText" => Value::Int(s(0).to_lowercase().cmp(&s(1).to_lowercase()) as i64),
            "Pos" => Value::Int(sjis_pos(&s(0), &s(1))),
            "JPos" => Value::Int(
                s(1).find(&s(0))
                    .map_or(0, |b| s(1)[..b].chars().count() as i64 + 1),
            ),
            "Trim" | "JTrim" => Value::Str(s(0).trim_matches(|c: char| c <= ' ').to_string()),
            "Exists" => {
                let t = s(0);
                Value::Flag(
                    self.find(&t).is_some()
                        || self
                            .objs
                            .iter()
                            .any(|o| o.cab.as_deref() == Some(t.as_str())),
                )
            }
            "Not" => Value::Flag(!arg(0).truthy()),
            "SetArray" => {
                let nm = Self::param_name(args.first(), refs);
                let dims: Vec<usize> = (1..v.len()).map(|i| n(i).max(0) as usize).collect();
                let total = dims
                    .iter()
                    .product::<usize>()
                    .max(if dims.is_empty() { 0 } else { 1 });
                let mut a = self.array(th, &nm);
                let ty = self.var_ref(th, &nm).map_or(0, |x| x.ty);
                a.resize(total, Value::zero(ty));
                self.set_array(th, &nm, a);
                let var = Self::var_mut(&mut self.globals, th, &nm);
                var.dims = if dims.len() > 1 { dims } else { Vec::new() };
                Value::Flag(true)
            }
            "FillMem" => {
                let nm = Self::param_name(args.first(), refs);
                let len = self.array(th, &nm).len();
                self.set_array(th, &nm, vec![arg(1); len]);
                Value::Flag(true)
            }
            "Random" => {
                self.rand ^= self.rand << 13;
                self.rand ^= self.rand >> 7;
                self.rand ^= self.rand << 17;
                let m = n(0);
                Value::Int(if m > 0 {
                    (self.rand % m as u64) as i64
                } else {
                    0
                })
            }
            "GetSaveCaption" | "ListSaveCaption" => Value::Str(String::new()),
            "ArrayToString" => {
                let a = self.array(th, &Self::param_name(args.first(), refs));
                let d = if v.len() > 1 { s(1) } else { "\r\n".into() };
                Value::Str(a.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(&d))
            }
            "StringToArray" => {
                let t = s(0);
                let d = if v.len() > 2 { s(2) } else { "\r\n".into() };
                let parts: Vec<Value> = if t.is_empty() {
                    Vec::new()
                } else if d.is_empty() {
                    t.chars().map(|c| Value::Str(c.to_string())).collect()
                } else {
                    t.split(d.as_str())
                        .map(|x| Value::Str(x.to_string()))
                        .collect()
                };
                let count = parts.len();
                let nm = Self::param_name(args.get(1), refs);
                self.set_array(th, &nm, parts);
                Value::Int(count as i64)
            }
            "IndexOfStr" => {
                let a = self.array(th, &Self::param_name(args.first(), refs));
                let t = s(1);
                Value::Int(
                    a.iter()
                        .position(|x| x.to_string() == t)
                        .map_or(-1, |i| i as i64),
                )
            }
            "ListCompo" => {
                let parent = (v.len() > 2).then(|| s(2));
                let list = self.list_compo(parent.as_deref(), v.len() > 3 && arg(3).truthy());
                let count = list.len();
                let nm = Self::param_name(args.first(), refs);
                self.set_array(th, &nm, list);
                Value::Int(count as i64)
            }
            "ToClientX" | "ToClientY" | "ToScreenX" | "ToScreenY" => {
                let (x, y) = self.find(&s(0)).map_or((0, 0), |i| self.abs_pos(i));
                let base = if name.ends_with('X') { x } else { y } as i64;
                Value::Int(if name.starts_with("ToScreen") {
                    n(1) + base
                } else {
                    n(1) - base
                })
            }
            "Int" => Value::Int(arg(0).float() as i64),
            "Float" => Value::Float(arg(0).float()),
            "Sin" => Value::Float(arg(0).float().sin()),
            "Cos" => Value::Float(arg(0).float().cos()),
            "Tan" => Value::Float(arg(0).float().tan()),
            "ArcTan" => Value::Float(arg(0).float().atan()),
            "ArcTan2" => Value::Float(arg(0).float().atan2(arg(1).float())),
            "Hypot" => Value::Float(arg(0).float().hypot(arg(1).float())),
            "Abs" => Value::Int(n(0).abs()),
            "Fabs" => Value::Float(arg(0).float().abs()),
            "IndexOfMenu" => Value::Int(
                self.menu_index(&s(0), &Value::Str(s(1)))
                    .map_or(-1, |m| m.1 as i64),
            ),
            "VarExists" => {
                let t = s(0);
                Value::Flag(self.var_ref(th, &t).is_some())
            }
            "AddArray" => {
                let nm = Self::param_name(args.first(), refs);
                let mut a = self.array(th, &nm);
                match args.get(1) {
                    Some(Param::Var(x))
                        if !x.starts_with("____")
                            && self.var_ref(th, x).is_some_and(|v| v.arr.is_some()) =>
                    {
                        a.extend(self.array(th, x))
                    }
                    _ => a.push(arg(1)),
                }
                self.set_array(th, &nm, a);
                Value::Flag(true)
            }
            "InsertArray" => {
                let nm = Self::param_name(args.first(), refs);
                let mut a = self.array(th, &nm);
                let at = (n(1).max(0) as usize).min(a.len());
                a.insert(at, arg(2));
                self.set_array(th, &nm, a);
                Value::Flag(true)
            }
            "DeleteArray" => {
                let nm = Self::param_name(args.first(), refs);
                let mut a = self.array(th, &nm);
                let at = (n(1).max(0) as usize).min(a.len());
                let end = (at + if v.len() > 2 { n(2).max(0) as usize } else { 1 }).min(a.len());
                a.drain(at..end);
                self.set_array(th, &nm, a);
                Value::Flag(true)
            }
            "CopyArray" => {
                let src = self.array(th, &Self::param_name(args.get(1), refs));
                let nm = Self::param_name(args.first(), refs);
                self.set_array(th, &nm, src);
                Value::Flag(true)
            }
            "UniqueArray" => {
                let nm = Self::param_name(args.first(), refs);
                let mut a = self.array(th, &nm);
                let mut seen = Vec::new();
                a.retain(|x| {
                    let t = x.to_string();
                    if seen.contains(&t) {
                        false
                    } else {
                        seen.push(t);
                        true
                    }
                });
                self.set_array(th, &nm, a);
                Value::Flag(true)
            }
            "TrimArray" => {
                let nm = Self::param_name(args.first(), refs);
                let mut a = self.array(th, &nm);
                a.retain(|x| !x.to_string().is_empty());
                self.set_array(th, &nm, a);
                Value::Flag(true)
            }
            "SortStr" => {
                let nm = Self::param_name(args.first(), refs);
                let mut a = self.array(th, &nm);
                a.sort_by_key(|x| x.to_string());
                self.set_array(th, &nm, a);
                Value::Flag(true)
            }
            "InPrimary" => Value::Flag(self.find(&s(0)).is_some_and(|i| !self.on_back(i))),
            "InCabinet" => {
                Value::Flag(self.find(&s(0)).is_some_and(|i| self.objs[i].cab.is_some()))
            }
            "FileExists" => {
                let p = s(0);
                Value::Flag(self.arc.has(&p) || self.files.size(&p.replace('\\', "/")).is_some())
            }
            "LoadTextFile" => {
                let p = s(1);
                let text = self
                    .read(&p)
                    .map(|b| nexas::sjis::decode(&b))
                    .unwrap_or_default();
                let lines: Vec<Value> = text
                    .lines()
                    .map(|l| Value::Str(l.trim_end_matches('\r').to_string()))
                    .collect();
                let nm = Self::param_name(args.first(), refs);
                self.set_array(th, &nm, lines);
                Value::Flag(true)
            }
            "LowerCase" => Value::Str(s(0).to_lowercase()),
            "UpperCase" => Value::Str(s(0).to_uppercase()),
            "ExtractFilePath" => {
                let p = s(0);
                Value::Str(
                    p.rfind('\\')
                        .map_or(String::new(), |i| p[..i + 1].to_string()),
                )
            }
            "ExtractFileName" => {
                let p = s(0);
                Value::Str(p.rfind('\\').map_or(p.clone(), |i| p[i + 1..].to_string()))
            }
            "ExtractFileExt" => Value::Str(file_ext(&s(0))),
            "ChangeFileExt" => {
                let p = s(0);
                let e = file_ext(&p);
                Value::Str(format!("{}{}", &p[..p.len() - e.len()], s(1)))
            }
            "IsPathDelimiter" => {
                Value::Flag(s(0).chars().nth((n(1).max(1) - 1) as usize) == Some('\\'))
            }
            "AddBackSlash" => {
                let p = s(0);
                Value::Str(if p.is_empty() || p.ends_with('\\') {
                    p
                } else {
                    p + "\\"
                })
            }
            "IsDelimiter" => {
                let t = sjis_copy(&s(1), n(2), 2);
                let c = t.chars().next();
                Value::Flag(c.is_some_and(|c| s(0).contains(c)))
            }
            "StringOfChar" => Value::Str(s(0).repeat(n(1).max(0) as usize)),
            "StringReplace" => Value::Str(s(0).replace(&s(1), &s(2))),
            "HanToZen" => Value::Str(s(0).chars().map(nexas::sjis::to_zen).collect()),
            "ZenToHan" => Value::Str(s(0).chars().map(nexas::sjis::to_han).collect()),
            "Min" => Value::Int(n(0).min(n(1))),
            "Max" => Value::Int(n(0).max(n(1))),
            "Fmin" => Value::Float(arg(0).float().min(arg(1).float())),
            "Fmax" => Value::Float(arg(0).float().max(arg(1).float())),
            "GetVarType" => Value::Int(
                self.var_ref(th, &Self::param_name(args.first(), refs))
                    .map_or(0, |x| x.ty as i64),
            ),
            "GetEnabled" | "GetVisible" => Value::Flag(
                self.menu_index(&s(0), &arg(1))
                    .is_some_and(|(i, k)| name == "GetVisible" || !self.objs[i].disabled[k]),
            ),
            "SetEnabled" => {
                if let Some((i, k)) = self.menu_index(&s(0), &arg(1)) {
                    self.objs[i].disabled[k] = !arg(2).truthy();
                }
                Value::Flag(true)
            }
            "GetGroupIndex" => Value::Int(
                self.menu_index(&s(0), &arg(1))
                    .map_or(0, |(i, k)| self.objs[i].items[k].group),
            ),
            "GetSelected" | "GetCheck" => Value::Flag(false),
            "AddDelimiter" => {
                let t = s(1);
                Value::Str(if t.is_empty() || t.ends_with(&s(0)) {
                    t
                } else {
                    t + &s(0)
                })
            }
            "StrToIntDef" => {
                let t = s(0);
                let t = t.trim();
                let parsed = if let Some(h) = t.strip_prefix('$') {
                    i64::from_str_radix(h, 16).ok()
                } else {
                    t.parse::<i64>().ok()
                };
                Value::Int(parsed.unwrap_or(n(1)))
            }
            "StrToFloatDef" => Value::Float(s(0).trim().parse::<f64>().unwrap_or(arg(1).float())),
            "GetHistoryCount" | "GetHistoryMaxCount" => Value::Int(0),
            "Chr" => {
                let c = n(0) as u32;
                let b = if c > 0xff {
                    vec![(c >> 8) as u8, c as u8]
                } else {
                    vec![c as u8]
                };
                Value::Str(nexas::sjis::decode(&b))
            }
            "Ord" => {
                let b = nexas::sjis::encode(&s(0));
                Value::Int(match b.len() {
                    0 => 0,
                    1 => b[0] as i64,
                    _ if b[0] >= 0x80 => ((b[0] as i64) << 8) | b[1] as i64,
                    _ => b[0] as i64,
                })
            }
            "SetSelected" | "SetCheck" | "SetHistoryMaxCount" | "ClrHist" => Value::Flag(true),
            "GetItem" if self.find(&s(0)).is_some() => Value::Str(
                self.menu_index(&s(0), &Value::Int(n(1)))
                    .map_or(String::new(), |(i, k)| self.objs[i].items[k].name.clone()),
            ),
            "GetItem" => {
                let a = self.array(th, &Self::param_name(args.first(), refs));
                a.get(n(1).max(0) as usize).cloned().unwrap_or_default()
            }
            "DBGetStr" | "DBDirectGetStr" => Value::Str(String::new()),
            "Calc" => {
                let src = s(0);
                self.calc_str(th, &src)
            }
            _ => {
                if self.log > 0 {
                    eprintln!("livemaker: function {name} is not done");
                }
                Value::Int(0)
            }
        }
    }
}

/// A token of the expressions that Calc("...") evaluates.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64, bool),
    Str(String),
    Id(String),
    Op(String),
}

const OPS: &str = "+-*/%()[],=<>!&|^";

fn tokens(src: &str) -> Vec<Tok> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch == '"' {
            let mut t = String::new();
            i += 1;
            while i < c.len() {
                if c[i] == '"' {
                    if c.get(i + 1) == Some(&'"') {
                        t.push('"');
                        i += 2;
                        continue;
                    }
                    break;
                }
                t.push(c[i]);
                i += 1;
            }
            i += 1;
            out.push(Tok::Str(t));
        } else if ch.is_ascii_digit() {
            let mut t = String::new();
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                t.push(c[i]);
                i += 1;
            }
            out.push(Tok::Num(t.parse().unwrap_or(0.0), t.contains('.')));
        } else if OPS.contains(ch) {
            let two: String = c[i..(i + 2).min(c.len())].iter().collect();
            if ["++", "==", "!=", "<=", ">=", "<<", ">>"].contains(&two.as_str()) {
                out.push(Tok::Op(two));
                i += 2;
            } else {
                out.push(Tok::Op(ch.to_string()));
                i += 1;
            }
        } else {
            let mut t = String::new();
            while i < c.len() && !c[i].is_whitespace() && !OPS.contains(c[i]) && c[i] != '"' {
                t.push(c[i]);
                i += 1;
            }
            out.push(Tok::Id(t));
        }
    }
    out
}

fn mes_size(o: &Obj) -> u32 {
    match o.int(FONTHEIGHT) {
        n if n > 0 => n as u32,
        _ => 16,
    }
}
