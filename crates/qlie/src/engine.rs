//! Runs a QLIE game: the game thread (startup.s) and the system thread (sysstartup.s, for the
//! menus), their variables and call stacks, waits, story text and the screen.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nexas::files::Files;
use nexas::gfx::{Draw, Image};

use crate::abmp::Abmp;
use crate::expr::{self, Env, Expr, Value};
use crate::game::{Game, norm};
use crate::obj::{OKind, Obj};
use crate::script::{self, Arg, Block, Kind, Unit};
use crate::text::Font;

pub const GAME: usize = 0;
pub const SYS: usize = 1;

pub struct SoundEvent {
    pub channel: String,
    /// None: stop
    pub data: Option<Rc<Vec<u8>>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: u32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum VKind {
    Int,
    Str,
    Obj,
}

#[derive(Clone, Debug)]
pub struct Var {
    pub kind: VKind,
    pub vals: Vec<Value>,
}

pub struct Frame {
    pub unit: Rc<Unit>,
    pub pc: usize,
    pub locals: HashMap<String, Var>,
    pub params: Vec<Value>,
    /// the objects `\scp` chose
    pub scp: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Wait {
    None,
    /// until this time (ms); a click ends it too when `click`
    Until {
        end: f64,
        click: bool,
    },
    Click,
    /// a button (`\btn`)
    Btn,
    /// `\stop`: the handler runs once a frame until it says `\play`
    Stop,
}

pub struct Thread {
    pub frames: Vec<Frame>,
    pub wait: Wait,
    pub globals: HashMap<String, Var>,
    /// ResultInt, ResultStr, ... by (name, index)
    pub results: HashMap<(String, i64), Value>,
    /// the `\stop` handler: unit and op
    pub stop: Option<(Rc<Unit>, usize)>,
    /// the call depth below the running handler
    pub handler: Option<usize>,
    pub played: bool,
    /// `^name` -> commands; story text and empty lines
    pub rep: HashMap<String, String>,
    pub et: String,
    pub nt: String,
    pub on_frame: Option<(Rc<Unit>, usize)>,
}

impl Thread {
    fn new() -> Thread {
        Thread {
            frames: Vec::new(),
            wait: Wait::None,
            globals: HashMap::new(),
            results: HashMap::new(),
            stop: None,
            handler: None,
            played: false,
            rep: HashMap::new(),
            et: String::new(),
            nt: String::new(),
            on_frame: None,
        }
    }
}

/// Screen effects: a captured screen and the change from it to the live one.
#[derive(Default)]
pub struct Effect {
    /// the screen as `\c` caught it (below `zmax`)
    pub snap: Option<Rc<Image>>,
    pub zmax: i32,
    /// start, length (ms), mask (grey levels), softness
    pub trans: Option<(f64, f64, Option<Rc<Image>>, f32)>,
    /// a colour over everything, fading: colour, from, to, start, length
    pub cover: Option<([u8; 3], f32, f32, f64, f64)>,
    /// the camera shakes until
    pub quake: f64,
    pub quake_px: f32,
}

/// Where a command is: its unit and op (for relative paths and blocks).
#[derive(Clone)]
pub struct At {
    pub unit: Rc<Unit>,
    pub op: usize,
}

impl At {
    pub fn file(&self) -> &str {
        let f = self.unit.ops.get(self.op).map_or(0, |o| o.file as usize);
        &self.unit.files[f]
    }
}

/// A `rep` template's commands.
type Template = Vec<(String, Vec<Arg>)>;

pub struct Engine {
    pub g: Game,
    pub width: u32,
    pub height: u32,
    units: HashMap<String, Rc<Unit>>,
    pub objs: Vec<Option<Obj>>,
    pub threads: [Thread; 2],
    /// the running thread
    pub cur: usize,
    /// whose variables and objects `\srct` refers to
    pub refer: usize,
    pub sysvars: HashMap<String, Var>,
    /// sys, eff, snd, ...
    builtins: HashMap<String, Var>,
    pub now: f64,
    pub mouse: (i32, i32),
    /// draw only the objects of this thread
    only: Option<usize>,
    clicked: bool,
    rclicked: bool,
    wheel: i32,
    /// this frame's click went to no button
    sys_click: bool,
    /// the button clicked this frame
    btn_clicked: Option<usize>,
    pub font: Font,
    pub(crate) sounds: Vec<SoundEvent>,
    images: HashMap<String, Rc<Image>>,
    abmps: HashMap<String, Rc<Abmp>>,
    fills: HashMap<[u8; 3], Rc<Image>>,
    pub fx: Effect,
    pub bg: [u8; 3],
    /// skey: key code -> action, label
    pub skey: Vec<(i64, String, String)>,
    /// the log (`\scp,log`), oldest first: name, text, voice
    pub backlog: Vec<[String; 3]>,
    pub(crate) ended: bool,
    pub log: u32,
    warned: HashSet<String>,
    /// no waiting (tests)
    pub fast: bool,
    /// the story line the game thread showed last: its script and op
    pub last_text: Option<(String, usize)>,
    /// skipping to a saved line
    pub loading: Option<crate::save::Loading>,
    /// now, for the dates of saves: year, month, day, hour, minute, second
    pub today: [i64; 6],
    pub savetext: [String; 4],
    pub play_mode: i64,
    rng: u64,
    /// the snapshot during a transition, with the mask applied, per frame
    overlay: Option<(u64, Rc<Image>)>,
    pub frame_no: u64,
    /// templates of `rep` parsed: text -> commands
    templates: HashMap<String, Rc<Template>>,
}

fn decl_name(raw: &str) -> (String, Option<&str>) {
    match raw.find('[') {
        Some(i) => (
            raw[..i].trim().to_lowercase(),
            Some(raw[i + 1..].trim_end().trim_end_matches(']')),
        ),
        None => (raw.trim().to_lowercase(), None),
    }
}

fn dir_of(path: &str) -> String {
    let p = path.replace('/', "\\");
    match p.rfind('\\') {
        Some(i) => p[..=i].to_string(),
        None => String::new(),
    }
}

/// Literal arguments (story lines): quotes taken off, nothing evaluated.
fn literal_args(s: &str) -> Vec<Arg> {
    script::split_top(s, ',')
        .into_iter()
        .map(|a| {
            let t = a.trim();
            let t = t
                .strip_prefix('"')
                .and_then(|t| t.strip_suffix('"'))
                .unwrap_or(t);
            Arg {
                raw: t.to_string(),
                expr: Expr::Str(t.to_string()),
            }
        })
        .collect()
}

struct Cx<'a> {
    e: &'a mut Engine,
}

impl Env for Cx<'_> {
    fn get(&mut self, name: &str, idx: Option<i64>) -> Option<Value> {
        self.e.get_var(name, idx)
    }
    fn set(&mut self, name: &str, idx: Option<i64>, v: Value) {
        self.e.set_var(name, idx, v)
    }
}

impl Engine {
    pub fn open(files: Box<dyn Files>) -> Result<Engine, String> {
        let g = Game::open(files)?;
        let mut e = Engine {
            g,
            width: 800,
            height: 600,
            units: HashMap::new(),
            objs: Vec::new(),
            threads: [Thread::new(), Thread::new()],
            cur: GAME,
            refer: GAME,
            sysvars: HashMap::new(),
            builtins: HashMap::new(),
            now: 0.0,
            mouse: (-1, -1),
            only: None,
            clicked: false,
            rclicked: false,
            wheel: 0,
            sys_click: false,
            btn_clicked: None,
            font: Font::default(),
            sounds: Vec::new(),
            images: HashMap::new(),
            abmps: HashMap::new(),
            fills: HashMap::new(),
            fx: Effect::default(),
            bg: [0, 0, 0],
            skey: Vec::new(),
            backlog: Vec::new(),
            ended: false,
            log: 0,
            warned: HashSet::new(),
            fast: false,
            last_text: None,
            loading: None,
            today: [2000, 1, 1, 0, 0, 0],
            savetext: Default::default(),
            play_mode: 0,
            rng: 0x2545_f491_4f6c_dd1d,
            overlay: None,
            frame_no: 0,
            templates: HashMap::new(),
        };
        e.screen_size();
        e.load_system_vars();
        // the built-in objects
        for (name, kind) in [
            ("sys", OKind::Sys),
            ("eff", OKind::Eff),
            ("cam", OKind::Cam),
            ("rep", OKind::Rep),
            ("skey", OKind::SKey),
            ("inp", OKind::Inp),
            ("misc", OKind::Misc),
            ("cfg", OKind::Cfg),
            ("log", OKind::Log),
            ("llog", OKind::Flags),
            ("flog", OKind::Flags),
            ("dat", OKind::Dat),
            ("math", OKind::Math),
            ("show", OKind::Movie),
            ("csr", OKind::Other),
            ("scsr", OKind::Other),
        ] {
            let id = e.new_obj(Obj::new(kind, GAME));
            e.builtins.insert(
                name.into(),
                Var {
                    kind: VKind::Obj,
                    vals: vec![Value::Obj(id)],
                },
            );
        }
        let snd: Vec<Value> = (0..32)
            .map(|_| Value::Obj(e.new_obj(Obj::new(OKind::Snd, GAME))))
            .collect();
        e.builtins.insert(
            "snd".into(),
            Var {
                kind: VKind::Obj,
                vals: snd,
            },
        );
        e.start(GAME, "startup.s", "@@TOP")?;
        Ok(e)
    }

    /// The screen size: system/etc/game.txt says "ScreenWidth=1280", or the layouts say
    /// "BackGroundWidth=800" in their comments.
    fn screen_size(&mut self) {
        let mut names: Vec<String> = self
            .g
            .names()
            .filter(|n| n.ends_with("etc/game.txt"))
            .cloned()
            .collect();
        names.extend(
            self.g
                .names()
                .filter(|n| n.ends_with("layout.s") || n.contains("layout"))
                .take(8)
                .cloned(),
        );
        for n in names {
            if let Some(b) = self.g.read(&n) {
                let t = script::decode(&b);
                let num = |k: &str| {
                    t.find(k).and_then(|i| {
                        t[i + k.len()..]
                            .chars()
                            .take_while(|c| c.is_ascii_digit())
                            .collect::<String>()
                            .parse::<u32>()
                            .ok()
                    })
                };
                for (kw, kh) in [
                    ("ScreenWidth=", "ScreenHeight="),
                    ("BackGroundWidth=", "BackGroundHeight="),
                ] {
                    if let (Some(w), Some(h)) = (num(kw), num(kh)) {
                        self.width = w;
                        self.height = h;
                        return;
                    }
                }
            }
        }
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        self.font.set(ttf)
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn point(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
        self.clicked = true;
    }

    /// A right click.
    pub fn cancel(&mut self) {
        self.rclicked = true;
    }

    pub fn wheel(&mut self, up: bool) {
        self.wheel = if up { 1 } else { -1 };
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    pub fn warn(&mut self, s: String) {
        if self.warned.insert(s.clone()) {
            eprintln!("qlie: {s}");
        }
    }

    fn new_obj(&mut self, o: Obj) -> usize {
        if let Some(i) = self.objs.iter().position(|o| o.is_none()) {
            self.objs[i] = Some(o);
            return i;
        }
        self.objs.push(Some(o));
        self.objs.len() - 1
    }

    fn free_obj(&mut self, id: usize) {
        if let Some(Some(o)) = self.objs.get(id)
            && o.kind == OKind::Snd
            && o.snd.playing
        {
            self.sounds.push(SoundEvent {
                channel: format!("snd{id}"),
                data: None,
                volume: 0.0,
                repeat: false,
                fade_ms: 0,
            });
        }
        if let Some(s) = self.objs.get_mut(id) {
            *s = None;
        }
    }

    pub fn obj(&mut self, id: usize) -> Option<&mut Obj> {
        self.objs.get_mut(id).and_then(|o| o.as_mut())
    }

    // ---- scripts

    /// The unit of a script file (its includes read in).
    pub fn unit(&mut self, path: &str) -> Option<Rc<Unit>> {
        let key = norm(path);
        if let Some(u) = self.units.get(&key) {
            return Some(u.clone());
        }
        let text = script::decode(&self.g.read(&key)?);
        let g = &mut self.g;
        let mut load = |from: &str, inc: &str| {
            let near = format!("{}{}", dir_of(from), inc);
            for p in [near.as_str(), inc] {
                if let Some(b) = g.read(p) {
                    return Some((p.to_string(), script::decode(&b)));
                }
            }
            None
        };
        let u = Rc::new(Unit::load(path, &text, &mut load));
        self.units.insert(key, u.clone());
        Some(u)
    }

    /// Starts thread `t` over at a label of a file.
    pub fn start(&mut self, t: usize, file: &str, label: &str) -> Result<(), String> {
        let u = self.unit(file).ok_or(format!("{file}: no such script"))?;
        let pc = u.label(label).ok_or(format!("{file}: no {label}"))?;
        let th = &mut self.threads[t];
        th.frames.clear();
        th.wait = Wait::None;
        th.stop = None;
        th.handler = None;
        th.played = false;
        th.frames.push(Frame {
            unit: u,
            pc: pc + 1,
            locals: HashMap::new(),
            params: Vec::new(),
            scp: Vec::new(),
        });
        Ok(())
    }

    fn top(&mut self) -> Option<&mut Frame> {
        self.threads[self.cur].frames.last_mut()
    }

    /// A file named by a script: next to the command's file, next to the running script, or
    /// from the top of the game's files.
    pub fn resolve(&mut self, at: &At, path: &str) -> Option<String> {
        if path.is_empty() {
            return None;
        }
        let p = path.replace('/', "\\");
        let mut tries = vec![format!("{}{}", dir_of(at.file()), p)];
        tries.push(format!("{}{}", dir_of(&at.unit.files[0]), p));
        tries.push(p);
        tries.into_iter().find(|t| self.g.exists(t))
    }

    pub fn image(&mut self, path: &str) -> Option<Rc<Image>> {
        let key = norm(path);
        if let Some(i) = self.images.get(&key) {
            return Some(i.clone());
        }
        let b = self.g.read(&key)?;
        let im = Rc::new(decode_image(&b)?);
        if self.images.len() > 256 {
            self.images.clear();
        }
        self.images.insert(key, im.clone());
        Some(im)
    }

    pub fn abmp(&mut self, path: &str) -> Option<Rc<Abmp>> {
        let key = norm(path);
        if let Some(a) = self.abmps.get(&key) {
            return Some(a.clone());
        }
        let b = self.g.read(&key)?;
        let a = match Abmp::parse(&b) {
            Ok(a) => Rc::new(a),
            Err(e) => {
                self.warn(format!("{path}: {e}"));
                return None;
            }
        };
        if self.abmps.len() > 128 {
            self.abmps.clear();
        }
        self.abmps.insert(key, a.clone());
        Some(a)
    }

    pub fn fill(&mut self, c: [u8; 3]) -> Rc<Image> {
        self.fills
            .entry(c)
            .or_insert_with(|| {
                let mut im = Image::new(1, 1);
                im.fill([c[0], c[1], c[2], 255]);
                Rc::new(im)
            })
            .clone()
    }

    pub fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    // ---- variables

    fn result(&self, name: &str, i: i64) -> Value {
        let t = &self.threads[self.cur];
        if let Some(v) = t.results.get(&(name.to_string(), i)) {
            return v.clone();
        }
        match name {
            "resultstr" | "resultbtnstr" | "resultfilename" => Value::Str(String::new()),
            "resultobj" | "resultbtn" => Value::Null,
            _ => Value::Int(0),
        }
    }

    pub fn set_result(&mut self, name: &str, i: i64, v: Value) {
        self.threads[self.cur]
            .results
            .insert((name.to_string(), i), v);
    }

    pub fn res_int(&mut self, i: i64, v: i64) {
        self.set_result("resultint", i, Value::Int(v));
    }

    pub fn res_str(&mut self, i: i64, v: String) {
        self.set_result("resultstr", i, Value::Str(v));
    }

    fn get_var(&mut self, name: &str, idx: Option<i64>) -> Option<Value> {
        let i = idx.unwrap_or(0);
        match name {
            "resultint" | "resultstr" | "resultobj" | "resultbtn" | "resultbtnint"
            | "resultbtnstr" | "resultclkint" | "resultfilename" => {
                return Some(self.result(name, i));
            }
            "paramint" | "paramstr" | "paramcount" => {
                let f = self.threads[self.cur].frames.last()?;
                let p = f.params.get(i as usize).cloned();
                return Some(match name {
                    "paramint" => Value::Int(p.map_or(0, |v| v.int())),
                    "paramstr" => Value::Str(p.map_or(String::new(), |v| v.str())),
                    _ => Value::Int(f.params.len() as i64),
                });
            }
            "selffilename" | "selffilepath" => {
                let f = self.threads[self.cur].frames.last()?;
                let file = f.unit.files[0].replace('/', "\\");
                return Some(Value::Str(if name == "selffilename" {
                    file
                } else {
                    dir_of(&file)
                }));
            }
            _ => {}
        }
        let get = |v: &Var| {
            Some(v.vals.get(i as usize).cloned().unwrap_or(match v.kind {
                VKind::Int => Value::Int(0),
                VKind::Str => Value::Str(String::new()),
                VKind::Obj => Value::Null,
            }))
        };
        if let Some(f) = self.threads[self.cur].frames.last()
            && let Some(v) = f.locals.get(name)
        {
            return get(v);
        }
        if let Some(v) = self.threads[self.refer].globals.get(name) {
            return get(v);
        }
        if self.refer != self.cur
            && let Some(v) = self.threads[self.cur].globals.get(name)
        {
            return get(v);
        }
        if let Some(v) = self.sysvars.get(name) {
            return get(v);
        }
        if let Some(v) = self.builtins.get(name) {
            return get(v);
        }
        None
    }

    fn convert(&mut self, kind: VKind, v: Value) -> Value {
        match kind {
            VKind::Int => Value::Int(v.int()),
            VKind::Str => Value::Str(v.str()),
            VKind::Obj => match v {
                Value::Obj(_) | Value::Null => v,
                Value::Str(s) => {
                    let (n, i) = decl_name(&s);
                    let i = i.map(|s| expr::eval(&expr::parse_arg(s), &mut Cx { e: self }).int());
                    self.get_var(&n, i).unwrap_or(Value::Null)
                }
                Value::Int(_) => Value::Null,
            },
        }
    }

    fn set_var(&mut self, name: &str, idx: Option<i64>, v: Value) {
        let i = idx.unwrap_or(0).max(0) as usize;
        if name.starts_with("result") {
            self.set_result(name, i as i64, v);
            return;
        }
        let put = |var: &mut Var, v: Value| {
            if var.vals.len() <= i {
                let d = match var.kind {
                    VKind::Int => Value::Int(0),
                    VKind::Str => Value::Str(String::new()),
                    VKind::Obj => Value::Null,
                };
                var.vals.resize(i + 1, d);
            }
            var.vals[i] = v;
        };
        let cur = self.cur;
        let refer = self.refer;
        let kind = if let Some(f) = self.threads[cur].frames.last()
            && let Some(var) = f.locals.get(name)
        {
            Some((0, var.kind))
        } else if let Some(var) = self.threads[refer].globals.get(name) {
            Some((1, var.kind))
        } else if let Some(var) = self.threads[cur].globals.get(name) {
            Some((2, var.kind))
        } else if let Some(var) = self.sysvars.get(name) {
            Some((3, var.kind))
        } else {
            self.builtins.get(name).map(|var| (4, var.kind))
        };
        match kind {
            Some((place, k)) => {
                let v = self.convert(k, v);
                let var = match place {
                    0 => self.threads[cur]
                        .frames
                        .last_mut()
                        .unwrap()
                        .locals
                        .get_mut(name),
                    1 => self.threads[refer].globals.get_mut(name),
                    2 => self.threads[cur].globals.get_mut(name),
                    3 => self.sysvars.get_mut(name),
                    _ => self.builtins.get_mut(name),
                };
                if let Some(var) = var {
                    put(var, v);
                }
                if place == 3 {
                    self.save_system_vars();
                }
            }
            None => {
                // not declared: a global of the value's kind
                let kind = match v {
                    Value::Int(_) => VKind::Int,
                    Value::Obj(_) | Value::Null => VKind::Obj,
                    Value::Str(_) => VKind::Str,
                };
                let mut var = Var {
                    kind,
                    vals: Vec::new(),
                };
                put(&mut var, v);
                if name.starts_with('#') {
                    self.sysvars.insert(name.to_string(), var);
                    self.save_system_vars();
                } else {
                    self.threads[refer].globals.insert(name.to_string(), var);
                }
            }
        }
    }

    pub fn eval(&mut self, e: &Expr) -> Value {
        expr::eval(e, &mut Cx { e: self })
    }

    pub fn eval_args(&mut self, args: &[Arg]) -> Vec<Value> {
        args.iter().map(|a| self.eval(&a.expr)).collect()
    }

    pub(crate) fn builtin_ids(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for b in self.builtins.values() {
            for v in &b.vals {
                if let Value::Obj(id) = v {
                    out.push(*id);
                }
            }
        }
        out
    }

    pub(crate) fn free_game_var(&mut self, v: Var) {
        self.free_var(v);
    }

    pub(crate) fn drop_obj(&mut self, id: usize) {
        self.free_obj(id);
    }

    fn free_var(&mut self, v: Var) {
        if v.kind == VKind::Obj {
            for x in v.vals {
                if let Value::Obj(id) = x {
                    // a built-in object (snd swapped in) stays
                    let builtin = self
                        .builtins
                        .values()
                        .any(|b| b.vals.contains(&Value::Obj(id)));
                    if !builtin {
                        self.free_obj(id);
                    }
                }
            }
        }
    }

    /// `\var`, `\gvar`, `\svar`.
    fn declare(&mut self, scope: u8, args: &[Arg]) {
        let Some(ty) = args.first() else { return };
        let ty = ty.raw.trim().to_lowercase();
        let kind = match ty.as_str() {
            "int" => VKind::Int,
            "str" => VKind::Str,
            _ => VKind::Obj,
        };
        let okind = OKind::of(&ty);
        for a in &args[1..] {
            if a.raw.trim().is_empty() {
                continue;
            }
            let (name, size) = decl_name(&a.raw);
            let n = size.map_or(1, |s| {
                (self.eval(&expr::parse_arg(s)).int().clamp(1, 4096)) as usize
            });
            // a system variable keeps its value
            if scope == 2 && self.sysvars.contains_key(&name) {
                continue;
            }
            let vals: Vec<Value> = match kind {
                VKind::Int => vec![Value::Int(0); n],
                VKind::Str => vec![Value::Str(String::new()); n],
                VKind::Obj => (0..n)
                    .map(|_| Value::Obj(self.new_obj(Obj::new(okind, self.refer))))
                    .collect(),
            };
            let var = Var { kind, vals };
            let old = match scope {
                0 => self.top().and_then(|f| f.locals.insert(name.clone(), var)),
                1 => self.threads[self.refer].globals.insert(name.clone(), var),
                _ => {
                    self.sysvars.insert(name.clone(), var);
                    None
                }
            };
            if let Some(old) = old {
                self.free_var(old);
            }
        }
    }

    fn delete(&mut self, args: &[Arg]) {
        for a in args {
            let (name, _) = decl_name(&a.raw);
            let local = self.top().and_then(|f| f.locals.remove(&name));
            let v = local
                .or_else(|| self.threads[self.refer].globals.remove(&name))
                .or_else(|| self.threads[self.cur].globals.remove(&name));
            if let Some(v) = v {
                self.free_var(v);
            }
        }
    }

    /// The number of values of a variable (`\garr`).
    fn var_len(&mut self, name: &str) -> usize {
        let (name, _) = decl_name(name);
        let t = &self.threads[self.cur];
        t.frames
            .last()
            .and_then(|f| f.locals.get(&name))
            .or_else(|| self.threads[self.refer].globals.get(&name))
            .or_else(|| t.globals.get(&name))
            .or_else(|| self.sysvars.get(&name))
            .or_else(|| self.builtins.get(&name))
            .map_or(0, |v| v.vals.len())
    }

    fn exists(&mut self, name: &str) -> bool {
        let (name, _) = decl_name(name);
        self.threads[self.cur]
            .frames
            .last()
            .is_some_and(|f| f.locals.contains_key(&name))
            || self.threads[self.refer].globals.contains_key(&name)
            || self.threads[self.cur].globals.contains_key(&name)
            || self.sysvars.contains_key(&name)
    }

    fn load_system_vars(&mut self) {
        let Some(b) = self.g.files.read_save("qlie-system.txt") else {
            return;
        };
        for line in String::from_utf8_lossy(&b).lines() {
            let mut p = line.splitn(3, '\t');
            let (Some(n), Some(k), Some(v)) = (p.next(), p.next(), p.next()) else {
                continue;
            };
            let vals: Vec<Value> = v
                .split('\u{1f}')
                .map(|s| {
                    if k == "i" {
                        Value::Int(s.parse().unwrap_or(0))
                    } else {
                        Value::Str(s.to_string())
                    }
                })
                .collect();
            let kind = if k == "i" { VKind::Int } else { VKind::Str };
            self.sysvars.insert(n.to_string(), Var { kind, vals });
        }
    }

    fn save_system_vars(&mut self) {
        let mut out = String::new();
        let mut names: Vec<&String> = self.sysvars.keys().collect();
        names.sort();
        for n in names {
            let v = &self.sysvars[n];
            let k = match v.kind {
                VKind::Int => "i",
                VKind::Str => "s",
                VKind::Obj => continue,
            };
            let vals: Vec<String> = v.vals.iter().map(|x| x.str()).collect();
            out += &format!("{n}\t{k}\t{}\n", vals.join("\u{1f}"));
        }
        self.g.files.write_save("qlie-system.txt", out.as_bytes());
    }

    // ---- running

    /// One frame (60 a second).
    pub fn frame(&mut self) -> Result<(), String> {
        let dt = 1000.0 / 60.0;
        self.now += dt;
        self.frame_no += 1;
        self.input();
        self.advance_objs(dt);
        // the system thread runs while it has work; the game thread waits for it
        let t = if self.threads[SYS].frames.is_empty() {
            GAME
        } else {
            SYS
        };
        self.run(t)?;
        if t == SYS && self.threads[SYS].frames.is_empty() {
            // the menu closed: the game goes on in the same frame
            self.run(GAME)?;
        }
        self.update_texts();
        self.clicked = false;
        self.rclicked = false;
        self.wheel = 0;
        self.sys_click = false;
        self.btn_clicked = None;
        Ok(())
    }

    /// A frame of the game thread alone, skipping to a saved line.
    pub(crate) fn skip_frame(&mut self) -> Result<(), String> {
        let dt = 1000.0 / 60.0;
        self.now += dt;
        self.advance_objs(dt);
        self.run(GAME)
    }

    fn advance_objs(&mut self, dt: f64) {
        for id in 0..self.objs.len() {
            let now = self.now;
            let Some(o) = self.objs[id].as_mut() else {
                continue;
            };
            if !o.kind.shows() {
                continue;
            }
            o.advance(dt as f32);
            if o.hiding && o.settled() {
                o.hiding = false;
                o.shown = false;
            }
            if o.kind.button() && o.shown && !o.hiding && o.entering && o.settled() {
                o.entering = false;
                o.play_named("OffMouse");
            }
            if let Some(t) = o.text.as_mut() {
                let total = crate::text::count(&t.items) as f32;
                if t.shown < total {
                    t.shown = (t.shown + dt as f32 / 30.0).min(total);
                    t.dirty = true;
                }
            }
            let _ = now;
            let cues = std::mem::take(&mut o.cues);
            if !cues.is_empty() {
                self.cue_sounds(id, cues);
            }
        }
    }

    /// Pointer over buttons, clicks on them, and the keys the game set (skey).
    fn input(&mut self) {
        let mouse = self.mouse;
        // the topmost button under the pointer (of the thread that runs)
        let running = if self.threads[SYS].frames.is_empty() {
            GAME
        } else {
            SYS
        };
        let mut top: Option<(i32, usize)> = None;
        // behind a menu of the system thread, the game's buttons are out of reach
        let sys_only = self.system_screen();
        for (id, o) in self.objs.iter().enumerate() {
            let Some(o) = o else { continue };
            if !o.kind.button() || !o.shown || o.hiding || o.lock || (sys_only && o.thread != SYS) {
                continue;
            }
            if (o.kind == OKind::Btn && o.thread != running) || !o.inside(mouse) || o.away(mouse) {
                continue;
            }
            if top.is_none_or(|(z, _)| o.z >= z) {
                top = Some((o.z, id));
            }
        }
        let over = top.map(|t| t.1);
        for id in 0..self.objs.len() {
            let Some(o) = self.objs[id].as_mut() else {
                continue;
            };
            if !o.kind.button() {
                continue;
            }
            let h = over == Some(id);
            if h != o.hover && o.shown && !o.hiding && !o.entering {
                o.play_named(if h { "OnMouse" } else { "OffMouse" });
            }
            o.hover = h;
        }
        if self.clicked {
            match over {
                Some(id) => {
                    let o = self.objs[id].as_mut().unwrap();
                    o.play_named("OnClick");
                    let (kind, us) = (o.kind, o.us.clone());
                    if kind == OKind::SBtn {
                        if let Some(l) = us.get(1).filter(|l| l.starts_with("@@")) {
                            self.system_label(l);
                        }
                    } else {
                        self.btn_clicked = Some(id);
                    }
                }
                None => self.sys_click = true,
            }
        }
        if self.rclicked {
            self.key(0x10002);
        }
        if self.wheel > 0 {
            self.key(0xf0);
        } else if self.wheel < 0 {
            self.key(0xf1);
        }
    }

    /// A key the game maps (skey) to a label of the system thread.
    fn key(&mut self, code: i64) {
        let hit = self
            .skey
            .iter()
            .find(|(c, _, _)| *c & 0xffff == code & 0xffff)
            .cloned();
        if let Some((_, _, label)) = hit
            && label.starts_with("@@")
            && self.threads[SYS].frames.is_empty()
            && self.threads[GAME].stop.is_some()
        {
            self.system_label(&label);
        }
    }

    /// Runs a label of sysstartup.s on the system thread (a menu).
    pub fn system_label(&mut self, label: &str) {
        if !self.threads[SYS].frames.is_empty() {
            return;
        }
        if self.log > 0 {
            eprintln!("qlie: system {label}");
        }
        if let Err(e) = self.start(SYS, "sysstartup.s", label) {
            self.warn(e);
        }
    }

    /// The button clicked this frame (once).
    pub(crate) fn take_button(&mut self) -> Option<usize> {
        self.btn_clicked.take()
    }

    /// A click this frame that no button took.
    pub(crate) fn system_click(&self) -> bool {
        self.sys_click
    }

    /// The key codes of this frame (skey numbering: $10001 left click, $10002 right click,
    /// $400F0/$400F1 the wheel).
    pub(crate) fn keys_fired(&self) -> Vec<i64> {
        let mut v = Vec::new();
        if self.rclicked {
            v.push(0x10002);
        }
        if self.sys_click {
            v.push(0x10001);
        }
        if self.wheel > 0 {
            v.push(0x400f0);
        }
        if self.wheel < 0 {
            v.push(0x400f1);
        }
        v
    }

    /// Runs thread `t` until it waits.
    fn run(&mut self, t: usize) -> Result<(), String> {
        self.cur = t;
        self.refer = t;
        let mut budget = 400_000u32;
        loop {
            budget -= 1;
            if budget == 0 {
                self.warn("a script ran 400000 commands in one frame".into());
                return Ok(());
            }
            let th = &mut self.threads[t];
            match th.wait {
                Wait::None => {}
                Wait::Until { end, click } => {
                    let clicked = click && self.clicked;
                    if self.now >= end || clicked || self.fast {
                        th.wait = Wait::None;
                        if clicked {
                            self.set_result("resultclkint", 0, Value::Int(1));
                        }
                    } else {
                        return Ok(());
                    }
                }
                Wait::Click => {
                    if self.clicked || self.fast {
                        th.wait = Wait::None;
                        self.set_result("resultclkint", 0, Value::Int(1));
                    } else {
                        return Ok(());
                    }
                }
                Wait::Btn => match self.btn_clicked.take() {
                    Some(id) => {
                        th.wait = Wait::None;
                        self.button_results(id);
                    }
                    None => return Ok(()),
                },
                Wait::Stop => {
                    if th.played {
                        th.wait = Wait::None;
                        th.stop = None;
                        th.played = false;
                        continue;
                    }
                    let Some((u, pc)) = th.stop.clone() else {
                        th.wait = Wait::Click;
                        continue;
                    };
                    // the handler: once a frame
                    th.handler = Some(th.frames.len());
                    th.wait = Wait::None;
                    th.frames.push(Frame {
                        unit: u,
                        pc: pc + 1,
                        locals: HashMap::new(),
                        params: Vec::new(),
                        scp: Vec::new(),
                    });
                }
            }
            if self.ended {
                return Ok(());
            }
            let th = &mut self.threads[t];
            let Some(f) = th.frames.last_mut() else {
                return Ok(());
            };
            let (unit, pc) = (f.unit.clone(), f.pc);
            if pc >= unit.ops.len() {
                if self.ret() {
                    return Ok(());
                }
                continue;
            }
            f.pc = pc + 1;
            let at = At {
                unit: unit.clone(),
                op: pc,
            };
            let op = &unit.ops[pc];
            if self.log > 1 {
                eprintln!(
                    "[{t}] {}:{} {:?}",
                    unit.files[op.file as usize], op.line, op.kind
                );
            }
            let r = match &op.kind {
                Kind::Label(_) => Ok(()),
                Kind::Blank => {
                    let nt = self.threads[t].nt.clone();
                    if op.file == 0 && !nt.is_empty() {
                        self.template(&at, &nt, Vec::new())
                    } else {
                        Ok(())
                    }
                }
                Kind::Text(s) => {
                    if t == GAME && op.file == 0 {
                        let file = unit.files[0].clone();
                        if self.loading.is_some() {
                            self.load_reached(&file, pc);
                        }
                        self.last_text = Some((file, pc));
                    }
                    let et = self.threads[t].et.clone();
                    if op.file == 0 && !et.is_empty() {
                        self.template(&at, &et, literal_args(s))
                    } else {
                        Ok(())
                    }
                }
                Kind::Story(name, rest) => {
                    let tpl = self.threads[t].rep.get(name).cloned();
                    match tpl {
                        Some(tpl) => self.template(&at, &tpl, literal_args(rest)),
                        None => {
                            self.warn(format!("no story command {name}"));
                            Ok(())
                        }
                    }
                }
                Kind::Cmd(name, args) => self.command(&at, name, args),
            };
            if let Err(e) = r {
                let op = &unit.ops[pc];
                self.warn(format!("{}:{}: {e}", unit.files[op.file as usize], op.line));
            }
            if self.threads[t].wait != Wait::None {
                return Ok(());
            }
        }
    }

    /// Returns from the running call; true when the thread's turn ends (its stop handler
    /// finished without `\play`, or nothing is left).
    fn ret(&mut self) -> bool {
        let t = self.cur;
        let Some(f) = self.threads[t].frames.pop() else {
            return true;
        };
        for (_, v) in f.locals {
            self.free_var(v);
        }
        let th = &mut self.threads[t];
        if th.handler == Some(th.frames.len()) {
            th.handler = None;
            if th.played {
                th.played = false;
                th.stop = None;
                th.wait = Wait::None;
                return false;
            }
            th.wait = Wait::Stop;
            return true;
        }
        if th.frames.is_empty() {
            if t == GAME {
                self.warn("the game thread ended".into());
            }
            return true;
        }
        false
    }

    /// Runs a `rep`/`\et`/`\nt` template with extra (literal) arguments added to its last
    /// command.
    fn template(&mut self, at: &At, tpl: &str, extra: Vec<Arg>) -> Result<(), String> {
        let cmds = match self.templates.get(tpl) {
            Some(c) => c.clone(),
            None => {
                let c: Vec<(String, Vec<Arg>)> = script::split_cmds(tpl)
                    .into_iter()
                    .map(|(n, mut a)| {
                        // "\sub,label,self," ends with an empty argument for the rest
                        if a.last().is_some_and(|s| s.is_empty()) {
                            a.pop();
                        }
                        (n, script::make_args(a))
                    })
                    .collect();
                let c = Rc::new(c);
                self.templates.insert(tpl.to_string(), c.clone());
                c
            }
        };
        let n = cmds.len();
        for (k, (name, args)) in cmds.iter().enumerate() {
            if k + 1 == n {
                let mut all = args.clone();
                all.extend(extra.iter().cloned());
                self.command(at, name, &all)?;
            } else {
                self.command(at, name, args)?;
            }
        }
        Ok(())
    }

    fn jump_target(
        &mut self,
        at: &At,
        label: &Value,
        file: Option<&Value>,
    ) -> Result<(Rc<Unit>, usize), String> {
        let label = label.str();
        let unit = match file.map(|f| f.str()) {
            Some(f) if !f.is_empty() && !f.eq_ignore_ascii_case("self") => {
                let path = self.resolve(at, &f).ok_or(format!("no script {f}"))?;
                self.unit(&path).ok_or(format!("no script {f}"))?
            }
            _ => at.unit.clone(),
        };
        let pc = unit
            .label(&label)
            .ok_or(format!("no label {label} in {}", unit.files[0]))?;
        Ok((unit, pc))
    }

    fn button_results(&mut self, id: usize) {
        let (us, ui) = self.objs[id]
            .as_ref()
            .map(|o| (o.us.clone(), o.ui))
            .unwrap_or_default();
        self.set_result("resultbtn", 0, Value::Obj(id));
        self.set_result("resultbtnint", 0, Value::Int(ui));
        for (i, s) in us.iter().enumerate() {
            self.set_result("resultbtnstr", i as i64, Value::Str(s.clone()));
        }
    }

    // ---- commands

    fn command(&mut self, at: &At, name: &str, args: &[Arg]) -> Result<(), String> {
        let t = self.cur;
        match name {
            "then" | "draw" | "zero" | "tmp" => {}
            "if" => {
                let c = args
                    .first()
                    .map(|a| self.eval(&a.expr).truth())
                    .unwrap_or(false);
                if !c && let Some(Block::If { otherwise, .. }) = at.unit.blocks.get(&at.op) {
                    self.jump(*otherwise);
                }
            }
            "else" | "ans" | "of" => {
                if let Some(Block::Skip { after }) = at.unit.blocks.get(&at.op) {
                    self.jump(*after);
                }
            }
            "end" => {
                if let Some(Block::Loop { head }) = at.unit.blocks.get(&at.op) {
                    let head = *head;
                    let hargs = match &at.unit.ops[head].kind {
                        Kind::Cmd(_, a) => a.clone(),
                        _ => Vec::new(),
                    };
                    if let Some(step) = hargs.get(2) {
                        self.eval(&step.expr);
                    }
                    let go = hargs.get(1).is_none_or(|c| {
                        matches!(c.expr, Expr::Empty) || self.eval(&c.expr).truth()
                    });
                    if go {
                        self.jump(head + 1);
                    }
                }
            }
            "for" => {
                if let Some(init) = args.first() {
                    self.eval(&init.expr);
                }
                let go = args
                    .get(1)
                    .is_none_or(|c| matches!(c.expr, Expr::Empty) || self.eval(&c.expr).truth());
                if !go && let Some(Block::For { after }) = at.unit.blocks.get(&at.op) {
                    self.jump(*after);
                }
            }
            "cnt" => {
                if let Some(Block::Continue { end }) = at.unit.blocks.get(&at.op) {
                    // the `\end` runs the step and the test
                    self.jump(*end);
                }
            }
            "brk" => {
                if let Some(Block::Break { after }) = at.unit.blocks.get(&at.op) {
                    self.jump(*after);
                }
            }
            "case" => {
                let v = args
                    .first()
                    .map(|a| self.eval(&a.expr))
                    .unwrap_or(Value::Null);
                let arms = at.unit.arms.get(&at.op).cloned().unwrap_or_default();
                let mut to = None;
                let mut default = None;
                'arms: for arm in arms {
                    let Kind::Cmd(n, aargs) = &at.unit.ops[arm].kind else {
                        continue;
                    };
                    if n == "else" {
                        default = Some(arm + 1);
                        continue;
                    }
                    for a in aargs {
                        let x = self.eval(&a.expr);
                        let eq = match (&v, &x) {
                            (Value::Str(_), _) | (_, Value::Str(_)) => {
                                v.str().to_lowercase() == x.str().to_lowercase()
                            }
                            _ => v.int() == x.int(),
                        };
                        if eq {
                            to = Some(arm + 1);
                            break 'arms;
                        }
                    }
                }
                let after = match at.unit.blocks.get(&at.op) {
                    Some(Block::Case { after }) => *after,
                    _ => at.op + 1,
                };
                self.jump(to.or(default).unwrap_or(after));
            }
            "cal" => {
                for a in args {
                    self.eval(&a.expr);
                }
            }
            "var" => self.declare(0, args),
            "gvar" => self.declare(1, args),
            "svar" => self.declare(2, args),
            "del" => self.delete(args),
            "init" => {
                // the variables back to 0 / ""
                for a in args {
                    let (name, _) = decl_name(&a.raw);
                    let t = self.cur;
                    let refer = self.refer;
                    self.threads[t].results.retain(|(n, _), _| *n != name);
                    let var = match self.threads[t].frames.last_mut() {
                        Some(f) if f.locals.contains_key(&name) => f.locals.get_mut(&name),
                        _ => None,
                    };
                    let var = match var {
                        Some(v) => Some(v),
                        None if self.threads[refer].globals.contains_key(&name) => {
                            self.threads[refer].globals.get_mut(&name)
                        }
                        None => self.sysvars.get_mut(&name),
                    };
                    if let Some(v) = var {
                        let blank = match v.kind {
                            VKind::Int => Value::Int(0),
                            VKind::Str => Value::Str(String::new()),
                            VKind::Obj => continue,
                        };
                        for x in v.vals.iter_mut() {
                            *x = blank.clone();
                        }
                    }
                }
            }
            "garr" => {
                let n = args.first().map_or(0, |a| self.var_len(&a.raw));
                self.res_int(0, n as i64);
            }
            "vexs" => {
                let e = args.first().is_some_and(|a| self.exists(&a.raw));
                self.res_int(0, e as i64);
            }
            "fexs" => {
                let p = args
                    .first()
                    .map(|a| self.eval(&a.expr).str())
                    .unwrap_or_default();
                let e = self.resolve(at, &p).is_some();
                self.res_int(0, e as i64);
            }
            "bsub" if args.len() >= 2 && !args[0].raw.trim_start().starts_with("\"@@") => {
                // a regular expression's replacement: "s/pat/rep/flags",text or text,pat,rep
                let v: Vec<String> = self.eval_args(args).iter().map(|v| v.str()).collect();
                let (text, pat, rep, all) = if v.len() >= 3 {
                    (v[0].clone(), v[1].clone(), v[2].clone(), true)
                } else {
                    let s = v[0].strip_prefix('s').unwrap_or(&v[0]);
                    let parts: Vec<&str> = s.splitn(4, '/').collect();
                    let flags = parts.get(3).copied().unwrap_or("");
                    (
                        v[1].clone(),
                        parts.get(1).copied().unwrap_or("").to_string(),
                        parts.get(2).copied().unwrap_or("").to_string(),
                        flags.contains('g'),
                    )
                };
                let out = crate::cmds::regex_replace(&text, &pat, &rep, all).unwrap_or(text);
                self.res_str(0, out);
            }
            "sub" | "bsub" | "jmp" | "go" => {
                let v = self.eval_args(args);
                let Some(label) = v.first() else {
                    return Err(format!("\\{name} without a label"));
                };
                let (unit, pc) = self.jump_target(at, label, v.get(1))?;
                let params: Vec<Value> = v.iter().skip(2).cloned().collect();
                match name {
                    "jmp" => {
                        let f = self.top().unwrap();
                        f.unit = unit;
                        f.pc = pc + 1;
                        if v.len() > 2 {
                            f.params = params;
                        }
                    }
                    "go" => {
                        let th = &mut self.threads[t];
                        let frames: Vec<Frame> = th.frames.drain(..).collect();
                        th.handler = None;
                        th.stop = None;
                        for f in frames {
                            for (_, v) in f.locals {
                                self.free_var(v);
                            }
                        }
                        self.threads[t].frames.push(Frame {
                            unit,
                            pc: pc + 1,
                            locals: HashMap::new(),
                            params,
                            scp: Vec::new(),
                        });
                    }
                    _ => self.threads[t].frames.push(Frame {
                        unit,
                        pc: pc + 1,
                        locals: HashMap::new(),
                        params,
                        scp: Vec::new(),
                    }),
                }
            }
            "ret" => {
                if self.ret() && self.threads[t].wait == Wait::None {
                    // the turn ends: a handler waits for the next frame
                    self.threads[t].wait = if self.threads[t].frames.is_empty() {
                        Wait::Until {
                            end: f64::MAX,
                            click: false,
                        }
                    } else {
                        Wait::Stop
                    };
                }
            }
            "clk" => {
                let v = self.eval_args(args);
                self.set_result("resultclkint", 0, Value::Int(0));
                match v.first() {
                    None => self.threads[t].wait = Wait::Click,
                    Some(ms) => {
                        let click = v.get(1).is_none_or(|c| c.str().is_empty() || c.int() != 0);
                        self.threads[t].wait = Wait::Until {
                            end: self.now + ms.int() as f64,
                            click,
                        };
                    }
                }
            }
            "wait" => {
                let ms = args.first().map_or(0, |a| self.eval(&a.expr).int());
                self.threads[t].wait = Wait::Until {
                    end: self.now + ms as f64,
                    click: false,
                };
            }
            "stop" => {
                let v = self.eval_args(args);
                let handler = match v.first() {
                    Some(l) if !l.str().is_empty() => Some(self.jump_target(at, l, v.get(1))?),
                    _ => None,
                };
                let th = &mut self.threads[t];
                th.played = false;
                th.stop = handler;
                // the handler starts next frame
                th.wait = Wait::Stop;
                th.handler = None;
            }
            "play" => self.threads[t].played = true,
            "btn" => {
                let v = self.eval_args(args);
                self.set_result("resultbtn", 0, Value::Null);
                self.threads[t].wait = Wait::Btn;
                let _ = v;
            }
            "scp" => {
                let mut sel = Vec::new();
                for a in args {
                    match self.eval(&a.expr) {
                        Value::Obj(id) => sel.push(id),
                        Value::Null => {}
                        v => {
                            // a whole array: every object in it
                            let (n, _) = decl_name(&a.raw);
                            match self.get_all(&n) {
                                Some(ids) if !ids.is_empty() => sel.extend(ids),
                                _ => {
                                    self.warn(format!("\\scp: {} is not an object ({v:?})", a.raw))
                                }
                            }
                        }
                    }
                }
                if let Some(f) = self.top() {
                    f.scp = sel;
                }
            }
            "srct" => {
                let n = args.first().map_or(0, |a| self.eval(&a.expr).int());
                // 1: the game thread, 0: this one
                self.refer = if n == 1 { GAME } else { self.cur };
            }
            "thrd" => {
                // back to the game: the system thread ends
                if t == SYS {
                    self.end_system();
                }
            }
            "swap" => {
                if args.len() >= 2 {
                    let a = self.lvalue(&args[0]);
                    let b = self.lvalue(&args[1]);
                    if let (Some((an, ai)), Some((bn, bi))) = (a, b) {
                        let va = self.get_var(&an, ai).unwrap_or(Value::Null);
                        let vb = self.get_var(&bn, bi).unwrap_or(Value::Null);
                        self.set_raw(&an, ai, vb);
                        self.set_raw(&bn, bi, va);
                    }
                }
            }
            "div" => {
                let v = self.eval_args(args);
                let sep = v.first().map(|v| v.str()).unwrap_or_default();
                let s = v.get(1).map(|v| v.str()).unwrap_or_default();
                let parts: Vec<String> = if sep.is_empty() {
                    vec![s]
                } else {
                    s.split(sep.as_str()).map(String::from).collect()
                };
                // clear a few past the end
                for i in 0..(parts.len() + 8) {
                    let p = parts.get(i).cloned().unwrap_or_default();
                    self.res_str(i as i64, p);
                }
                self.res_int(0, parts.len() as i64);
            }
            "copy" | "cpy" => {
                let v = self.eval_args(args);
                let s: Vec<char> = v
                    .first()
                    .map(|v| v.str())
                    .unwrap_or_default()
                    .chars()
                    .collect();
                let from = (v.get(1).map_or(1, |v| v.int()) - 1).max(0) as usize;
                let n = v.get(2).map_or(s.len() as i64, |v| v.int()).max(0) as usize;
                let r: String = s.iter().skip(from).take(n).collect();
                self.res_str(0, r);
            }
            "hcpy" => {
                let v = self.eval_args(args);
                let b = nexas::sjis::encode(&v.first().map(|v| v.str()).unwrap_or_default());
                let from = (v.get(1).map_or(1, |v| v.int()) - 1).max(0) as usize;
                let n = v.get(2).map_or(b.len() as i64, |v| v.int()).max(0) as usize;
                let r: Vec<u8> = b.iter().skip(from).take(n).copied().collect();
                self.res_str(0, nexas::sjis::decode(&r));
            }
            "len" => {
                let s = args
                    .first()
                    .map(|a| self.eval(&a.expr).str())
                    .unwrap_or_default();
                self.res_int(0, s.chars().count() as i64);
            }
            "hlen" => {
                let s = args
                    .first()
                    .map(|a| self.eval(&a.expr).str())
                    .unwrap_or_default();
                self.res_int(0, nexas::sjis::encode(&s).len() as i64);
            }
            "pos" => {
                let v = self.eval_args(args);
                let sub = v.first().map(|v| v.str()).unwrap_or_default();
                let s = v.get(1).map(|v| v.str()).unwrap_or_default();
                let p = if sub.is_empty() {
                    0
                } else {
                    s.find(&sub)
                        .map_or(0, |i| s[..i].chars().count() as i64 + 1)
                };
                self.res_int(0, p);
            }
            "rep" => {
                if args.len() >= 3
                    && let Some((n, i)) = self.lvalue(&args[0])
                {
                    let s = self.get_var(&n, i).map(|v| v.str()).unwrap_or_default();
                    let from = self.eval(&args[1].expr).str();
                    let to = self.eval(&args[2].expr).str();
                    if !from.is_empty() {
                        self.set_var(&n, i, Value::Str(s.replace(&from, &to)));
                    }
                }
            }
            "fmt" => {
                // format, its values, and again: another format and its values
                let v = self.eval_args(args);
                let mut out = String::new();
                let mut k = 0;
                while k < v.len() {
                    let f = v[k].str();
                    k += 1;
                    let mut cs = f.chars().peekable();
                    while let Some(c) = cs.next() {
                        if c != '%' {
                            out.push(c);
                            continue;
                        }
                        // %s %d %3d %03d %.4d ...
                        let mut spec = String::new();
                        while let Some(&d) = cs.peek() {
                            if d.is_ascii_digit() || d == '-' || d == '.' {
                                spec.push(d);
                                cs.next();
                            } else {
                                break;
                            }
                        }
                        match cs.next() {
                            Some('%') => out.push('%'),
                            Some(ty) => {
                                let a = v.get(k).cloned().unwrap_or(Value::Str(String::new()));
                                k += 1;
                                out += &if ty == 'd' || ty == 'x' {
                                    let n = a.int();
                                    let s = if ty == 'x' {
                                        format!("{n:x}")
                                    } else {
                                        n.to_string()
                                    };
                                    let zero = spec.starts_with('0') || spec.starts_with('.');
                                    let w: usize = spec
                                        .trim_start_matches(['0', '.', '-'])
                                        .parse()
                                        .unwrap_or(0);
                                    if zero {
                                        format!("{s:0>w$}")
                                    } else {
                                        format!("{s:>w$}")
                                    }
                                } else {
                                    a.str()
                                };
                            }
                            None => {}
                        }
                    }
                }
                self.res_str(0, out);
            }
            "lowc" => {
                let s = args
                    .first()
                    .map(|a| self.eval(&a.expr).str())
                    .unwrap_or_default();
                self.res_str(0, s.to_lowercase());
            }
            "zenc" => {
                // to full width
                let s = args
                    .first()
                    .map(|a| self.eval(&a.expr).str())
                    .unwrap_or_default();
                let z = s
                    .chars()
                    .map(|c| match c {
                        ' ' => '\u{3000}',
                        '!'..='~' => char::from_u32(c as u32 + 0xfee0).unwrap_or(c),
                        _ => c,
                    })
                    .collect();
                self.res_str(0, z);
            }
            "fint" => {
                let v = self.eval_args(args);
                let s = v.first().map(|v| v.str()).unwrap_or_default();
                let d = v.get(1).map_or(0, |v| v.int());
                let t = s.trim();
                let ok = !t.is_empty()
                    && (t.starts_with('$')
                        || t.trim_start_matches('-')
                            .chars()
                            .all(|c| c.is_ascii_digit()));
                self.res_int(0, if ok { expr::str_int(t) } else { d });
            }
            "bmat" => self.res_int(0, 0),
            "rnd" => {
                let n = args.first().map_or(0, |a| self.eval(&a.expr).int());
                let r = if n > 0 {
                    (self.random() % n as u64) as i64
                } else {
                    0
                };
                self.res_int(0, r);
            }
            "ddlg" => {
                let s = args
                    .first()
                    .map(|a| self.eval(&a.expr).str())
                    .unwrap_or_default();
                self.warn(format!("{}: {s}", at.file()));
            }
            _ => return self.obj_command(at, name, args),
        }
        Ok(())
    }

    fn jump(&mut self, pc: usize) {
        if let Some(f) = self.top() {
            f.pc = pc;
        }
    }

    /// The variable an argument names, and its index.
    fn lvalue(&mut self, a: &Arg) -> Option<(String, Option<i64>)> {
        match &a.expr {
            Expr::Var(n, idx, _) => {
                let i = idx.as_ref().map(|e| self.eval(e).int());
                Some((n.clone(), i))
            }
            _ => None,
        }
    }

    /// Sets a variable without converting (for \swap of objects).
    fn set_raw(&mut self, name: &str, idx: Option<i64>, v: Value) {
        let i = idx.unwrap_or(0).max(0) as usize;
        let (cur, refer) = (self.cur, self.refer);
        let var = if let Some(var) = self.threads[cur]
            .frames
            .last_mut()
            .and_then(|f| f.locals.get_mut(name))
        {
            Some(var)
        } else if self.threads[refer].globals.contains_key(name) {
            self.threads[refer].globals.get_mut(name)
        } else if self.threads[cur].globals.contains_key(name) {
            self.threads[cur].globals.get_mut(name)
        } else {
            self.builtins.get_mut(name)
        };
        if let Some(var) = var {
            if var.vals.len() <= i {
                var.vals.resize(i + 1, Value::Null);
            }
            var.vals[i] = v;
        }
    }

    /// Every object of an array variable.
    fn get_all(&mut self, name: &str) -> Option<Vec<usize>> {
        let f = self.threads[self.cur].frames.last();
        let var = f
            .and_then(|f| f.locals.get(name))
            .or_else(|| self.threads[self.refer].globals.get(name))
            .or_else(|| self.threads[self.cur].globals.get(name))
            .or_else(|| self.builtins.get(name))?;
        Some(
            var.vals
                .iter()
                .filter_map(|v| match v {
                    Value::Obj(i) => Some(*i),
                    _ => None,
                })
                .collect(),
        )
    }

    /// Ends the system thread (a menu closed).
    pub fn end_system(&mut self) {
        let frames: Vec<Frame> = self.threads[SYS].frames.drain(..).collect();
        for f in frames {
            for (_, v) in f.locals {
                self.free_var(v);
            }
        }
        let th = &mut self.threads[SYS];
        th.wait = Wait::Until {
            end: f64::MAX,
            click: false,
        };
        th.stop = None;
        th.handler = None;
        // a new turn starts clean
        th.wait = Wait::None;
    }

    /// Resets the game (`\scp,sys \r`): variables and objects of the game go, and the game
    /// thread starts at a label (startup.s @@RESET by default).
    pub fn reset(&mut self, file: &str, label: &str) {
        let globals: Vec<Var> = self.threads[GAME].globals.drain().map(|(_, v)| v).collect();
        for v in globals {
            self.free_var(v);
        }
        self.end_system();
        self.threads[GAME].rep.clear();
        self.threads[GAME].et.clear();
        self.threads[GAME].nt.clear();
        self.fx = Effect::default();
        if let Err(e) = self.start(GAME, file, label) {
            self.warn(e);
        }
    }

    fn cue_sounds(&mut self, id: usize, cues: Vec<u8>) {
        let Some(o) = self.objs[id].as_ref() else {
            return;
        };
        let Some(a) = o.abmp().cloned() else {
            return;
        };
        let own = o.sounds.clone();
        for c in cues {
            let d = own.get(c as usize).cloned().flatten().or_else(|| {
                a.sounds
                    .get(c as usize)
                    .filter(|d| !d.is_empty())
                    .map(|d| Rc::new(d.clone()))
            });
            if let Some(d) = d {
                self.sounds.push(SoundEvent {
                    channel: format!("cue{id}"),
                    data: Some(d),
                    volume: 1.0,
                    repeat: false,
                    fade_ms: 0,
                });
            }
        }
    }

    /// Makes the pictures of texts that changed.
    fn update_texts(&mut self) {
        for id in 0..self.objs.len() {
            let Some(o) = self.objs[id].as_mut() else {
                continue;
            };
            let Some(t) = o.text.as_mut() else { continue };
            if !t.dirty {
                continue;
            }
            t.dirty = false;
            let items = t.items.clone();
            let shown = t.shown as usize;
            let (w, h) = (t.rect[2].max(1) as u32, t.rect[3].max(1) as u32);
            let im = self.font.render(&items, shown, w, h);
            if let Some(t) = self.objs[id].as_mut().and_then(|o| o.text.as_mut()) {
                t.image = Some((shown, Rc::new(im)));
            }
        }
    }

    pub fn push_sound(&mut self, ev: SoundEvent) {
        self.sounds.push(ev);
    }

    // ---- the screen

    fn obj_draws(&mut self, below: Option<i32>, above: Option<i32>) -> Vec<Draw> {
        let (sw, sh) = (self.width as f32, self.height as f32);
        let mut list: Vec<(i32, i32, usize, Draw)> = Vec::new();
        let now = self.now;
        let mut fills = std::mem::take(&mut self.fills);
        for (id, o) in self.objs.iter().enumerate() {
            let Some(o) = o else { continue };
            if !o.kind.shows()
                || !o.shown
                || o.away(self.mouse)
                || self.only.is_some_and(|t| o.thread != t)
            {
                continue;
            }
            if below.is_some_and(|z| o.z >= z) || above.is_some_and(|z| o.z < z) {
                continue;
            }
            let mut out = Vec::new();
            let mut fill = |c: [u8; 3]| {
                fills
                    .entry(c)
                    .or_insert_with(|| {
                        let mut im = Image::new(1, 1);
                        im.fill([c[0], c[1], c[2], 255]);
                        Rc::new(im)
                    })
                    .clone()
            };
            crate::obj::draws(o, now, (sw, sh), &mut fill, &mut out);
            for (lz, d) in out {
                list.push((o.z, lz, id, d));
            }
        }
        self.fills = fills;
        list.sort_by_key(|(z, lz, id, _)| (*z, *lz, *id));
        let mut out: Vec<Draw> = list.into_iter().map(|x| x.3).collect();
        if self.now < self.fx.quake {
            let r = self.random();
            let a = self.fx.quake_px;
            let dx = ((r & 0xff) as f32 / 255.0 - 0.5) * 2.0 * a;
            let dy = (((r >> 8) & 0xff) as f32 / 255.0 - 0.5) * 2.0 * a;
            for d in &mut out {
                d.dst = [d.dst[0] + dx, d.dst[1] + dy, d.dst[2] + dx, d.dst[3] + dy];
                d.place.cx += dx;
                d.place.cy += dy;
            }
        }
        out
    }

    /// The system thread shows a screen of its own (a menu): the game's objects are behind it,
    /// seen only through `$$$GameThreadImage`.
    fn system_screen(&self) -> bool {
        !self.threads[SYS].frames.is_empty()
            && self
                .objs
                .iter()
                .flatten()
                .any(|o| o.thread == SYS && o.shown && o.kind.shows() && o.pic.is_some())
    }

    /// `$$$GameThreadImage,z`: the game thread's screen below z (all when 0).
    pub fn game_image(&mut self, zmax: i32) -> Image {
        let zmax = if zmax <= 0 { i32::MAX } else { zmax };
        let (w, h) = (self.width as f32, self.height as f32);
        let bg = self.fill(self.bg);
        let mut ds = vec![Draw {
            image: bg,
            src: [0, 0, 1, 1],
            dst: [0.0, 0.0, w, h],
            place: crate::obj::place(1.0, w, h),
        }];
        let keep = self.only.replace(GAME);
        ds.extend(self.obj_draws(Some(zmax), None));
        self.only = keep;
        self.compose(&ds)
    }

    /// The pictures of the screen, bottom first.
    pub fn draws(&mut self) -> Vec<Draw> {
        if self.system_screen() {
            self.only = Some(SYS);
            let out = self.screen_draws();
            self.only = None;
            return out;
        }
        self.screen_draws()
    }

    fn screen_draws(&mut self) -> Vec<Draw> {
        let (w, h) = (self.width as f32, self.height as f32);
        let mut out = Vec::new();
        let bg = self.fill(self.bg);
        out.push(Draw {
            image: bg,
            src: [0, 0, 1, 1],
            dst: [0.0, 0.0, w, h],
            place: crate::obj::place(1.0, w, h),
        });
        match self.fx.snap.clone() {
            None => out.extend(self.obj_draws(None, None)),
            Some(snap) => {
                let z = self.fx.zmax;
                out.extend(self.obj_draws(Some(z), None));
                if let Some(d) = self.snap_draw(&snap) {
                    out.push(d);
                }
                out.extend(self.obj_draws(None, Some(z)));
            }
        }
        if let Some((c, from, to, start, ms)) = self.fx.cover {
            let f = if ms <= 0.0 {
                1.0
            } else {
                ((self.now - start) / ms).clamp(0.0, 1.0) as f32
            };
            let a = from + (to - from) * f;
            if a > 0.0 {
                let im = self.fill(c);
                out.push(Draw {
                    image: im,
                    src: [0, 0, 1, 1],
                    dst: [0.0, 0.0, w, h],
                    place: crate::obj::place(a, w, h),
                });
            }
        }
        out
    }

    /// The captured screen over the live one: whole while nothing changes it, fading or
    /// wiping away during a transition.
    fn snap_draw(&mut self, snap: &Rc<Image>) -> Option<Draw> {
        let (w, h) = (self.width as f32, self.height as f32);
        let mut image = snap.clone();
        let mut alpha = 1.0;
        if let Some((start, ms, mask, vague)) = self.fx.trans.clone() {
            let f = if ms <= 0.0 {
                1.0
            } else {
                ((self.now - start) / ms).clamp(0.0, 1.0) as f32
            };
            if f >= 1.0 {
                self.fx.snap = None;
                self.fx.trans = None;
                return None;
            }
            match mask {
                None => alpha = 1.0 - f,
                Some(m) => {
                    if let Some((n, im)) = &self.overlay
                        && *n == self.frame_no
                    {
                        image = im.clone();
                    } else {
                        let im = Rc::new(mask_wipe(snap, &m, f, vague));
                        self.overlay = Some((self.frame_no, im.clone()));
                        image = im;
                    }
                }
            }
        }
        Some(Draw {
            src: [0, 0, image.w as i32, image.h as i32],
            image,
            dst: [0.0, 0.0, w, h],
            place: crate::obj::place(alpha, w, h),
        })
    }

    /// The screen as one picture.
    pub fn render(&mut self) -> Image {
        let ds = self.draws();
        self.compose(&ds)
    }

    pub fn compose(&self, ds: &[Draw]) -> Image {
        let mut out = Image::new(self.width, self.height);
        out.fill([0, 0, 0, 255]);
        for d in ds {
            nexas::gfx::draw(&mut out, &d.image, d.src, d.dst, &d.place);
        }
        out
    }

    /// `\scp,eff \c`: catches the screen below z `zmax` (all when 0).
    pub fn capture(&mut self, zmax: i32) {
        let zmax = if zmax <= 0 { i32::MAX } else { zmax };
        let (w, h) = (self.width as f32, self.height as f32);
        let mut ds = Vec::new();
        let bg = self.fill(self.bg);
        ds.push(Draw {
            image: bg,
            src: [0, 0, 1, 1],
            dst: [0.0, 0.0, w, h],
            place: crate::obj::place(1.0, w, h),
        });
        // what shows now: the old capture if one is still up
        match self.fx.snap.clone() {
            Some(snap) => {
                let z = self.fx.zmax.min(zmax);
                ds.extend(self.obj_draws(Some(z), None));
                if let Some(d) = self.snap_draw(&snap) {
                    ds.push(d);
                }
                if zmax > z {
                    let mut rest = self.obj_draws(Some(zmax), Some(z));
                    ds.append(&mut rest);
                }
            }
            None => ds.extend(self.obj_draws(Some(zmax), None)),
        }
        let im = self.compose(&ds);
        self.fx.snap = Some(Rc::new(im));
        self.fx.zmax = zmax;
        self.fx.trans = None;
    }
}

/// The captured screen with the mask's dark parts gone first: `f` (0..1) of the way.
fn mask_wipe(snap: &Image, mask: &Image, f: f32, vague: f32) -> Image {
    let mut out = snap.clone();
    let v = vague.max(1.0);
    let th = f * (255.0 + v);
    for y in 0..snap.h {
        let my = (y * mask.h / snap.h.max(1)).min(mask.h.saturating_sub(1));
        for x in 0..snap.w {
            let mx = (x * mask.w / snap.w.max(1)).min(mask.w.saturating_sub(1));
            let m = mask.px[((my * mask.w + mx) * 4) as usize] as f32;
            let a = ((m + v - th) / v).clamp(0.0, 1.0);
            let i = ((y * snap.w + x) * 4 + 3) as usize;
            out.px[i] = (out.px[i] as f32 * a) as u8;
        }
    }
    out
}

/// A picture file: PNG, BMP, JPEG, or DPNG (PNG tiles).
pub fn decode_image(b: &[u8]) -> Option<Image> {
    if b.starts_with(b"DPNG") {
        let n = u32::from_le_bytes(b.get(8..12)?.try_into().ok()?) as usize;
        let w = u32::from_le_bytes(b.get(12..16)?.try_into().ok()?);
        let h = u32::from_le_bytes(b.get(16..20)?.try_into().ok()?);
        let mut out = Image::new(w, h);
        let mut at = 0x14;
        for _ in 0..n {
            let rd = |o: usize| -> Option<u32> {
                Some(u32::from_le_bytes(
                    b.get(at + o..at + o + 4)?.try_into().ok()?,
                ))
            };
            let (x, y, size) = (rd(0)?, rd(4)?, rd(16)? as usize);
            let data = b.get(at + 28..at + 28 + size)?;
            at += 28 + size;
            if size == 0 {
                continue;
            }
            let Ok(tile) = Image::decode(data) else {
                continue;
            };
            for ty in 0..tile.h {
                for tx in 0..tile.w {
                    let (ox, oy) = (x + tx, y + ty);
                    if ox < w && oy < h {
                        let s = ((ty * tile.w + tx) * 4) as usize;
                        let d = ((oy * w + ox) * 4) as usize;
                        out.px[d..d + 4].copy_from_slice(&tile.px[s..s + 4]);
                    }
                }
            }
        }
        return Some(out);
    }
    Image::decode(b).ok()
}
