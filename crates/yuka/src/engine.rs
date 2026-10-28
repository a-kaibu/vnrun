//! Runs a Yuka game: the scripts from Start.yks on, with the engine functions they call.
//!
//! The screen is a stack of numbered layers (`GraphicLoad(n, file, x, y)`), drawn from the
//! lowest number up. `DrawStop()` keeps the screen as it is until `Transition(kind, ms)` fades
//! to the new one. Text goes into the window the scripts set up (`TextWindowSet` on a layer,
//! `StrOut`, `LF`, `KeyWait`, `PF`), the speaker's name into another layer (`StrOutNWC`).
//! Buttons are `MouseSelect() { MouseCase(rect, ...) { ... } ... }`: the engine waits for a
//! click, then runs the block, where a case whose rectangle has the click runs its own block.
//! Choices are `Select() { Case(text, x, y, rect...) { ... } }` the same way.
//! Saves are our own (ten slots from the right-click menu): the call stack, the variables and
//! the layers.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use nexas::files::Files;
use nexas::font::Font;
use nexas::gfx::{Draw, Image, Place};

use crate::ykc;
use crate::ykg;
use crate::yks::{Elem, Script};

/// A sound to start or stop.
pub struct Sound {
    pub data: Vec<u8>,
    pub volume: f32,
    pub channel: i32,
    pub stop: bool,
    pub fade_ms: u32,
    pub looped: bool,
}

const FRAME_MS: f64 = 1000.0 / 60.0;
const SLOTS: usize = 10;
/// the most statements run in one frame before the engine gives up on the script
const STEPS: usize = 200_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Str(String),
    /// a label: its code position
    Label(usize),
}

impl Value {
    fn n(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            Value::Str(s) => s.trim().parse().unwrap_or(0),
            Value::Label(p) => *p as i64,
        }
    }
    fn s(&self) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Str(s) => s.clone(),
            Value::Label(_) => String::new(),
        }
    }
}

#[derive(Clone)]
struct Layer {
    file: String,
    mask: Option<String>,
    image: Rc<Image>,
    frames: Vec<Vec<([i32; 4], u32)>>,
    x: f32,
    y: f32,
    shown: bool,
    /// 0 opaque .. 1000 clear
    clear: f32,
    /// moving: from, to, start, ms
    moving: Option<([f32; 2], [f32; 2], u64, u64)>,
    fading: Option<(f32, f32, u64, u64)>,
    /// patterns of other layers drawn on this one: number -> (x, y, picture layer, pattern,
    /// when it started)
    sprites: BTreeMap<i32, (f32, f32, i32, i32, u64)>,
}

struct TextWin {
    rect: [i32; 4],
    lines: Vec<String>,
    size: i32,
    rgb: [u8; 3],
    /// a pattern of the GraphicFontSet picture shown instead of text (`@gf(n)`, a name)
    gf: Option<i32>,
}

/// The layer that keeps the GraphicFontSet picture (never shown itself).
const GRAPHIC_FONT: i32 = -1000;

struct Fade {
    from: Vec<Draw>,
    start: u64,
    ms: u64,
}

#[derive(Clone, PartialEq)]
enum Wait {
    None,
    Click,
    Until(u64),
    /// a MouseSelect or Select statement and its `{`
    Cases(usize, usize),
    Moving,
}

/// A button or choice of the waiting MouseSelect / Select.
#[derive(Clone)]
struct Case {
    rect: [i32; 4],
    /// the picture layer and where its frames go: normal, lit
    src: Option<(i32, f32, f32, i32, i32)>,
    text: Option<(String, f32, f32)>,
    /// only lights up (IndicateCase), or a case for another button (right click)
    passive: bool,
    /// taken only when no other case has the click (a MouseCase with a 13th number, such as
    /// one over the whole screen before the buttons)
    fallback: bool,
}

#[derive(Clone)]
struct SoundState {
    file: String,
    looped: bool,
}

pub struct Engine {
    files: Box<dyn Files>,
    index: ykc::Index,
    scripts: HashMap<String, Script>,
    /// the owner of each `{` of a script (the statement before it and where it starts)
    owners: HashMap<String, HashMap<usize, (String, usize)>>,
    /// each script's `{` -> its `}` (the `}` links back; an if's `{` can link to its else)
    closes: HashMap<String, HashMap<usize, usize>>,
    cur: String,
    pc: usize,
    stack: Vec<(String, usize)>,
    vars: HashMap<(String, i32), Value>,
    tmps: HashMap<(String, usize), Value>,
    layers: BTreeMap<i32, Layer>,
    frozen: Option<Vec<Draw>>,
    fade: Option<Fade>,
    wait: Wait,
    /// the click being matched against the cases
    hit: Option<(i32, i32)>,
    cases: Vec<Case>,
    windows: HashMap<i32, TextWin>,
    text_layer: i32,
    /// the layer the last text went to (where LF breaks the line)
    last_layer: i32,
    fonts: HashMap<i32, i32>,
    window_font: HashMap<i32, i32>,
    loaded_sounds: HashMap<i32, String>,
    playing: BTreeMap<i32, SoundState>,
    sounds: Vec<Sound>,
    font: Option<Rc<fontdue::Font>>,
    text_cache: HashMap<(String, [u8; 3], i32), Rc<Image>>,
    cache: HashMap<String, Rc<ykg::Ykg>>,
    pub screen: (u32, u32),
    now: u64,
    frames: u64,
    pointer: (i32, i32),
    menu: Option<Menu>,
    ended: bool,
    pub log: bool,
    seed: u64,
    message: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Menu {
    Main,
    Save,
    Load,
}

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Rc<Image> {
    let mut i = Image::new(w, h);
    i.fill(rgba);
    Rc::new(i)
}

impl Engine {
    pub fn new(mut files: Box<dyn Files>) -> Result<Engine, String> {
        let mut index = ykc::Index::default();
        let mut names: Vec<String> = files
            .list()
            .into_iter()
            .filter(|n| {
                let l = n.to_lowercase();
                l.ends_with(".ykc") || l.ends_with(".dat")
            })
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        for n in &names {
            let mut read = |o: u64, len: usize| files.read_range(n, o, len);
            index.add(n, &mut read);
        }
        if index.find("Start.yks").is_none() {
            return Err("Yuka: no Start.yks in the archives".into());
        }
        let mut e = Engine {
            files,
            index,
            scripts: HashMap::new(),
            owners: HashMap::new(),
            closes: HashMap::new(),
            cur: String::new(),
            pc: 0,
            stack: Vec::new(),
            vars: HashMap::new(),
            tmps: HashMap::new(),
            layers: BTreeMap::new(),
            frozen: None,
            fade: None,
            wait: Wait::None,
            hit: None,
            cases: Vec::new(),
            windows: HashMap::new(),
            text_layer: -1,
            last_layer: -1,
            fonts: HashMap::new(),
            window_font: HashMap::new(),
            loaded_sounds: HashMap::new(),
            playing: BTreeMap::new(),
            sounds: Vec::new(),
            font: None,
            text_cache: HashMap::new(),
            cache: HashMap::new(),
            screen: (800, 600),
            now: 0,
            frames: 0,
            pointer: (-1, -1),
            menu: None,
            ended: false,
            log: false,
            seed: 12345,
            message: String::new(),
        };
        if let Some(ini) = e.file("default.ini") {
            let t = nexas::sjis::decode(&ini);
            for l in t.lines() {
                let l = l.trim();
                if let Some(v) = l.strip_prefix("WindowSize") {
                    let v: Vec<u32> = v
                        .trim_start_matches([' ', '='])
                        .split(',')
                        .filter_map(|x| x.trim().parse().ok())
                        .collect();
                    if v.len() == 2 && v[0] > 0 && v[1] > 0 {
                        e.screen = (v[0], v[1]);
                    }
                }
            }
        }
        e.read_globals();
        e.enter("Start.yks", None)?;
        Ok(e)
    }

    pub fn set_font(&mut self, b: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(b, fontdue::FontSettings::default())?;
        self.font = Some(Rc::new(f));
        self.text_cache.clear();
        Ok(())
    }

    /// Runs Start.yks until its first wait, then jumps to a script (for testing).
    pub fn start_script(&mut self, name: &str) -> Result<(), String> {
        for _ in 0..600 {
            if self.cur == ykc::key("yks/title/title_main.yks") || self.wait == Wait::Click {
                break;
            }
            self.wait = Wait::None;
            self.run()?;
        }
        self.frozen = None;
        self.fade = None;
        self.wait = Wait::None;
        self.stack.clear();
        self.enter(name, None)
    }

    fn file(&mut self, name: &str) -> Option<Vec<u8>> {
        let (arc, off, len) = self.index.find(name)?.clone();
        self.files.read_range(&arc, off, len)
    }

    fn trace(&self, s: &str) {
        if self.log {
            eprintln!("{s}");
        }
    }

    // ---- scripts ----

    fn load_script(&mut self, name: &str) -> Result<String, String> {
        let key = ykc::key(name);
        if !self.scripts.contains_key(&key) {
            let b = self
                .file(name)
                .ok_or_else(|| format!("Yuka: no script {name}"))?;
            let s = Script::parse(name, &b)?;
            let (owners, closes) = scan(&s);
            self.owners.insert(key.clone(), owners);
            self.closes.insert(key.clone(), closes);
            self.scripts.insert(key.clone(), s);
        }
        Ok(key)
    }

    /// Starts a script (at a label).
    fn enter(&mut self, name: &str, label: Option<&Value>) -> Result<(), String> {
        let key = self.load_script(name)?;
        self.cur = key;
        self.pc = 0;
        if let Some(l) = label {
            self.goto_label(l);
        }
        Ok(())
    }

    fn goto_label(&mut self, l: &Value) {
        match l {
            Value::Label(p) => self.pc = *p,
            Value::Str(n) => {
                if let Some(p) = self.script().label(n) {
                    self.pc = p;
                }
            }
            _ => {}
        }
    }

    fn script(&self) -> &Script {
        &self.scripts[&self.cur]
    }

    fn word(&self, p: usize) -> usize {
        self.script().code.get(p).map_or(0, |&w| w as usize)
    }

    fn elem(&self, p: usize) -> Elem {
        self.script().elem(p).cloned().unwrap_or(Elem::Other)
    }

    /// The value of the element at a code position.
    fn value(&self, p: usize) -> Value {
        let s = self.script();
        match s.elem(p) {
            Some(Elem::Int(c)) => Value::Int(s.int(*c) as i64),
            Some(Elem::Str(t)) => Value::Str(t.clone()),
            Some(Elem::Var(k, c)) => self.var(k, s.int(*c)),
            Some(Elem::Tmp) => self
                .tmps
                .get(&(self.cur.clone(), self.word(p)))
                .cloned()
                .unwrap_or(Value::Int(0)),
            Some(Elem::Ctrl(n, l)) => {
                if *l >= 0 && (*l as usize) < s.code.len() {
                    Value::Label(*l as usize)
                } else {
                    Value::Str(n.clone())
                }
            }
            _ => Value::Int(0),
        }
    }

    fn var(&self, kind: &str, n: i32) -> Value {
        match self.vars.get(&(kind.to_string(), n)) {
            Some(v) => v.clone(),
            None if kind.contains("String") => Value::Str(String::new()),
            None => Value::Int(0),
        }
    }

    fn set(&mut self, p: usize, v: Value) {
        let w = self.word(p);
        match self.elem(p) {
            Elem::Var(k, c) => {
                let n = self.script().int(c);
                let v = if k.contains("String") {
                    Value::Str(v.s())
                } else {
                    Value::Int(v.n())
                };
                let global = k.starts_with("Global");
                self.vars.insert((k, n), v);
                if global {
                    self.write_globals();
                }
            }
            Elem::Tmp => {
                self.tmps.insert((self.cur.clone(), w), v);
            }
            Elem::Int(c) => {
                let key = self.cur.clone();
                if let Some(s) = self.scripts.get_mut(&key) {
                    s.set_int(c, v.n() as i32);
                }
            }
            _ => {}
        }
    }

    /// Operands with operators between them.
    fn eval(&self, from: usize, n: usize) -> Value {
        let mut vals: Vec<Value> = Vec::new();
        let mut ops: Vec<String> = Vec::new();
        for p in from..from + n {
            match self.elem(p) {
                Elem::Ctrl(op, _) if is_op(&op) => ops.push(op),
                _ => vals.push(self.value(p)),
            }
        }
        if vals.is_empty() {
            return Value::Int(0);
        }
        // by precedence: * / %, then + -, then comparisons, then && ||
        for level in 0..4 {
            let mut i = 0;
            while i < ops.len() && i + 1 < vals.len() {
                if prec(&ops[i]) == level {
                    let b = vals.remove(i + 1);
                    let a = vals[i].clone();
                    vals[i] = apply(&ops[i], a, b);
                    ops.remove(i);
                } else {
                    i += 1;
                }
            }
        }
        vals.swap_remove(0)
    }

    /// The `}` that closes the block opened at `open`, and where to go after it (past an else
    /// block).
    fn close(&self, open: usize) -> usize {
        if let Some(&c) = self.closes.get(&self.cur).and_then(|m| m.get(&open)) {
            return c;
        }
        match self.elem(open) {
            Elem::Ctrl(_, l) => l.max(0) as usize,
            _ => open,
        }
    }

    fn after_block(&self, open: usize, enter_else: bool) -> usize {
        let p = self.close(open) + 1;
        match self.elem(p) {
            Elem::Ctrl(n, _) if n == "else" => {
                if enter_else {
                    p + 2
                } else {
                    self.close(p + 1) + 1
                }
            }
            _ => p,
        }
    }

    pub fn frame(&mut self) -> Result<(), String> {
        self.frames += 1;
        self.now = (self.frames as f64 * FRAME_MS) as u64;
        self.animate();
        if let Some(f) = &self.fade
            && self.now >= f.start + f.ms
        {
            self.fade = None;
        }
        match self.wait.clone() {
            Wait::Until(t) if self.now < t => return Ok(()),
            Wait::Moving if self.layers.values().any(|l| l.moving.is_some()) => return Ok(()),
            Wait::Click | Wait::Cases(..) => return Ok(()),
            _ => self.wait = Wait::None,
        }
        if self.menu.is_some() || self.ended {
            return Ok(());
        }
        self.run()
    }

    fn animate(&mut self) {
        let now = self.now;
        for l in self.layers.values_mut() {
            if let Some((a, b, t0, ms)) = l.moving {
                let k = if ms == 0 {
                    1.0
                } else {
                    ((now.saturating_sub(t0)) as f32 / ms as f32).min(1.0)
                };
                l.x = a[0] + (b[0] - a[0]) * k;
                l.y = a[1] + (b[1] - a[1]) * k;
                if k >= 1.0 {
                    l.moving = None;
                }
            }
            if let Some((a, b, t0, ms)) = l.fading {
                let k = if ms == 0 {
                    1.0
                } else {
                    ((now.saturating_sub(t0)) as f32 / ms as f32).min(1.0)
                };
                l.clear = a + (b - a) * k;
                if k >= 1.0 {
                    l.fading = None;
                }
            }
        }
    }

    fn run(&mut self) -> Result<(), String> {
        for _ in 0..STEPS {
            if self.wait != Wait::None || self.ended || self.menu.is_some() {
                return Ok(());
            }
            self.step()?;
        }
        Err(format!(
            "Yuka: {} runs on without stopping (at {})",
            self.cur, self.pc
        ))
    }

    fn step(&mut self) -> Result<(), String> {
        let len = self.script().code.len();
        if self.pc >= len {
            return self.ret();
        }
        let pc = self.pc;
        match self.elem(pc) {
            Elem::Func(name) => {
                let argc = self.word(pc + 1);
                self.pc = pc + 2 + argc;
                self.call(&name, pc, pc + 2, argc)?;
            }
            Elem::Var(..) | Elem::Tmp | Elem::Int(_) => {
                // target "=" count operands
                let argc = self.word(pc + 2);
                if argc == 0 {
                    // the result of the call that follows
                    let at = pc + 3;
                    if let Elem::Func(name) = self.elem(at) {
                        let n = self.word(at + 1);
                        self.pc = at + 2 + n;
                        let v = self.call(&name, at, at + 2, n)?;
                        self.set(pc, v);
                    } else {
                        self.pc = pc + 3;
                    }
                } else {
                    let v = self.eval(pc + 3, argc);
                    self.pc = pc + 3 + argc;
                    self.set(pc, v);
                }
            }
            Elem::Ctrl(n, link) => match n.as_str() {
                "}" => {
                    let open = link.max(0) as usize;
                    let owner = self
                        .owners
                        .get(&self.cur)
                        .and_then(|o| o.get(&open))
                        .cloned();
                    match owner {
                        Some((f, at)) if f == "MouseSelect" || f == "Select" => {
                            // no case took the click: wait again
                            self.hit = None;
                            self.pc = at;
                        }
                        _ => self.pc = self.after_block(open, false),
                    }
                }
                "else" => self.pc = self.close(pc + 1) + 1,
                "{" => self.pc = pc + 1,
                _ => self.pc = pc + 1,
            },
            _ => self.pc = pc + 1,
        }
        Ok(())
    }

    fn ret(&mut self) -> Result<(), String> {
        match self.stack.pop() {
            Some((s, p)) => {
                self.cur = s;
                self.pc = p;
            }
            None => {
                self.trace("the scripts ended");
                self.ended = true;
            }
        }
        Ok(())
    }

    /// Runs a function; `pc` is already past its arguments.
    fn call(&mut self, name: &str, at: usize, args: usize, argc: usize) -> Result<Value, String> {
        let a: Vec<Value> = (args..args + argc).map(|p| self.value(p)).collect();
        if self.log {
            eprintln!("{}:{at} {name}{:?}", self.cur, a);
        }
        let n = |i: usize| a.get(i).map_or(0, |v| v.n());
        let s = |i: usize| a.get(i).map_or(String::new(), |v| v.s());
        let mut ret = Value::Int(0);
        match name {
            "if" => {
                let open = self.pc;
                self.pc = if n(0) != 0 {
                    open + 1
                } else {
                    self.after_block(open, true)
                };
            }
            "ScriptCall" | "ScriptJump" => {
                let call = name == "ScriptCall";
                let file = s(0).to_lowercase().replace('\\', "/");
                // the game's own save and load screens: ours instead
                let base = file.rsplit('/').next().unwrap_or("");
                let load = matches!(base, "m_load.yks" | "loadmenu.yks");
                if call && (load || matches!(base, "m_save.yks" | "savemenu.yks")) {
                    self.menu = Some(if load { Menu::Load } else { Menu::Save });
                    return Ok(ret);
                }
                match a.first() {
                    Some(Value::Str(file)) => {
                        if call {
                            self.stack.push((self.cur.clone(), self.pc));
                        }
                        let file = file.clone();
                        if let Err(e) = self.enter(&file, a.get(1)) {
                            self.trace(&e);
                            if call {
                                self.ret()?;
                            }
                        }
                    }
                    Some(l @ Value::Label(_)) => {
                        if call {
                            self.stack.push((self.cur.clone(), self.pc));
                        }
                        let l = l.clone();
                        self.goto_label(&l);
                        self.pc += 1;
                    }
                    _ => {}
                }
            }
            "Return" => self.ret()?,
            "ScriptExit" | "Restart" => {
                self.stack.clear();
                self.enter("Start.yks", None)?;
            }
            "MouseSelect" | "Select" => {
                // waiting for the player shows the screen as it is
                self.frozen = None;
                let open = self.pc;
                self.cases = self.collect_cases(open);
                self.wait = Wait::Cases(at, open);
                self.pc = at;
            }
            "MouseCase" | "MouseCase2" | "IndicateCase" | "Case" | "MouseKeyCase"
            | "KeyTableCase" => {
                let open = self.pc;
                let case = case_of(name, &a);
                let took = match (self.hit, &case) {
                    (Some((x, y)), Some(c)) if !c.passive => {
                        inside(c.rect, x, y)
                            && !(c.fallback
                                && self
                                    .cases
                                    .iter()
                                    .any(|o| !o.passive && !o.fallback && inside(o.rect, x, y)))
                    }
                    _ => false,
                };
                if took {
                    self.hit = None;
                    let click = if name == "Case" {
                        n(8)
                    } else {
                        n(argc_click(name))
                    };
                    self.play_loaded(click as i32);
                    self.pc = open + 1;
                } else {
                    self.pc = self.after_block(open, true);
                }
            }
            "GraphicLoad" | "GraphicLoadHide" => {
                let id = n(0) as i32;
                let file = s(1);
                let (mask, xy) = match a.get(2) {
                    Some(Value::Str(m)) => (Some(m.clone()), 3),
                    _ => (None, 2),
                };
                let (x, y) = (n(xy) as f32, n(xy + 1) as f32);
                self.load_layer(id, &file, mask, x, y, name == "GraphicLoad");
            }
            "SpriteCreate" => {
                // (layer, sprite, x, y, picture layer, frame)
                let now = self.now;
                if let Some(l) = self.layers.get_mut(&(n(0) as i32)) {
                    l.sprites.insert(
                        n(1) as i32,
                        (n(2) as f32, n(3) as f32, n(4) as i32, n(5) as i32, now),
                    );
                }
            }
            "SpriteDelete" => {
                if let Some(l) = self.layers.get_mut(&(n(0) as i32)) {
                    if a.len() >= 2 {
                        l.sprites.remove(&(n(1) as i32));
                    } else {
                        l.sprites.clear();
                    }
                }
            }
            "SpriteMove" => {
                if let Some(l) = self.layers.get_mut(&(n(0) as i32))
                    && let Some(sp) = l.sprites.get_mut(&(n(1) as i32))
                {
                    sp.0 = n(2) as f32;
                    sp.1 = n(3) as f32;
                }
            }
            "GraphicDelete" => {
                self.layers.remove(&(n(0) as i32));
            }
            "GraphicShow" | "GraphicHide" => {
                if let Some(l) = self.layers.get_mut(&(n(0) as i32)) {
                    l.shown = name == "GraphicShow";
                }
            }
            "GraphicMove" => {
                if let Some(l) = self.layers.get_mut(&(n(0) as i32)) {
                    l.x = n(1) as f32;
                    l.y = n(2) as f32;
                    l.moving = None;
                }
            }
            "GraphicScroll" => {
                let now = self.now;
                if let Some(l) = self.layers.get_mut(&(n(0) as i32)) {
                    l.moving = Some((
                        [l.x, l.y],
                        [n(2) as f32, n(3) as f32],
                        now,
                        n(1).max(0) as u64,
                    ));
                }
            }
            "GraphicScrollWait" | "GraphicTransmittanceScrollWait" | "SpriteMotionWait" => {
                if self
                    .layers
                    .values()
                    .any(|l| l.moving.is_some() || l.fading.is_some())
                {
                    self.wait = Wait::Moving;
                }
            }
            "GraphicTransmittance" => {
                if let Some(l) = self.layers.get_mut(&(n(0) as i32)) {
                    l.clear = n(1).clamp(0, 1000) as f32;
                    l.fading = None;
                }
            }
            "GraphicTransmittanceScroll" => {
                let now = self.now;
                if let Some(l) = self.layers.get_mut(&(n(0) as i32)) {
                    l.fading = Some((l.clear, n(1).clamp(0, 1000) as f32, now, n(2).max(0) as u64));
                }
            }
            "DrawStop" => {
                if self.frozen.is_none() {
                    self.frozen = Some(self.scene());
                }
            }
            "DrawStart" => self.frozen = None,
            "Transition" | "UserTransition" | "TTE" | "SelectTTE" => {
                let ms = if name == "UserTransition" { n(2) } else { n(1) };
                if let Some(from) = self.frozen.take() {
                    let ms = ms.clamp(0, 10000) as u64;
                    if ms > 0 {
                        self.fade = Some(Fade {
                            from,
                            start: self.now,
                            ms,
                        });
                        self.wait = Wait::Until(self.now + ms);
                    }
                }
            }
            "Wait" | "TimerWait" => {
                self.wait = Wait::Until(self.now + n(0).clamp(0, 60000) as u64);
            }
            "KeyWait" => {
                self.frozen = None;
                self.wait = Wait::Click;
            }
            "GraphicTextRect" | "TextWindowSet" => {
                // a text area on a layer (left, top, right, bottom in the layer); TextWindowSet
                // also makes it the window the story's text goes to
                let id = n(0) as i32;
                if a.len() >= 5 {
                    let mut rect = [n(1) as i32, n(2) as i32, n(3) as i32, n(4) as i32];
                    // some games give a width and height instead of the right and bottom
                    if rect[2] <= rect[0] || rect[3] <= rect[1] {
                        rect[2] += rect[0];
                        rect[3] += rect[1];
                    }
                    self.window(id).rect = rect;
                } else {
                    self.window(id);
                }
                if name == "TextWindowSet" {
                    self.text_layer = id;
                }
            }
            "DefaultTextWindowGroup" => {
                // the name's layer, then the text's
                if a.len() >= 2 {
                    self.text_layer = n(1) as i32;
                    self.window(n(1) as i32);
                }
            }
            "DefaultFontSet" => {
                self.fonts.insert(n(0) as i32, n(2) as i32);
            }
            "FontSet" => {
                self.window_font.insert(n(0) as i32, n(1) as i32);
            }
            "TextColor" => {
                let c = n(1);
                self.window(n(0) as i32).rgb = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
            }
            "RGB" => {
                ret = Value::Int(if a.len() >= 3 {
                    (n(0) & 255) << 16 | (n(1) & 255) << 8 | (n(2) & 255)
                } else {
                    n(0)
                });
            }
            "StrOut" | "StrOutNWC" | "StrOutNW" => {
                let (layer, text) = if a.len() >= 2 {
                    (n(0) as i32, s(1))
                } else {
                    (self.text_layer, s(0))
                };
                self.str_out(layer, &text);
            }
            "LF" => {
                let l = self.last_layer;
                self.window(l).lines.push(String::new());
            }
            "PF" => {
                for w in self.windows.values_mut() {
                    w.lines.clear();
                    w.gf = None;
                }
            }
            "GraphicFontSet" => {
                // a picture of names, whose patterns `@gf(n)` text shows
                let file = s(0);
                self.load_layer(GRAPHIC_FONT, &file, None, 0.0, 0.0, false);
            }
            "SoundLoad" | "SoundLoadStream" => {
                self.loaded_sounds.insert(n(0) as i32, s(1));
            }
            "SoundPlay" | "SoundPlayLoop" | "SoundPlayWait" if a.len() >= 2 => {
                let (ch, file) = (n(0) as i32, s(1));
                self.loaded_sounds.insert(ch, file.clone());
                self.play(ch, &file, name == "SoundPlayLoop");
            }
            "SoundPlay" | "SoundPlayLoop" => {
                let ch = n(0) as i32;
                if let Some(f) = self.loaded_sounds.get(&ch).cloned() {
                    self.play(ch, &f, name == "SoundPlayLoop");
                }
            }
            "SoundPlayWait" => {
                let ch = n(0) as i32;
                let ms = self
                    .playing
                    .get(&ch)
                    .map(|p| p.file.clone())
                    .and_then(|f| self.file(&f))
                    .map_or(0, |b| ogg_ms(&b));
                self.wait = Wait::Until(self.now + ms.min(20000));
            }
            "SoundStop" => self.stop(n(0) as i32, 0),
            "SoundDeleteToFade" => self.stop(n(0) as i32, n(1).clamp(0, 10000) as u32),
            "SoundStopALL" => {
                let chs: Vec<i32> = self.playing.keys().copied().collect();
                for c in chs {
                    self.stop(c, 0);
                }
            }
            "Random" => {
                self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let m = n(0).max(1);
                ret = Value::Int(((self.seed >> 33) as i64) % m);
            }
            "TextSpeedGet" | "AutoSpeedGet" => ret = Value::Int(20),
            "SoundGroupVolumeGet" => ret = Value::Int(1000),
            "MouseXPointGet" => ret = Value::Int(self.pointer.0 as i64),
            "MouseYPointGet" => ret = Value::Int(self.pointer.1 as i64),
            "FlagReset" => {
                self.vars.retain(|k, _| k.0.starts_with("Global"));
            }
            _ => {}
        }
        Ok(ret)
    }

    /// The cases in a MouseSelect / Select block, for drawing them while it waits.
    fn collect_cases(&self, open: usize) -> Vec<Case> {
        let end = self.close(open);
        let mut out = Vec::new();
        let mut p = open + 1;
        while p < end {
            match self.elem(p) {
                Elem::Func(name) => {
                    let argc = self.word(p + 1);
                    if matches!(
                        name.as_str(),
                        "MouseCase"
                            | "MouseCase2"
                            | "IndicateCase"
                            | "Case"
                            | "MouseKeyCase"
                            | "KeyTableCase"
                    ) {
                        let a: Vec<Value> = (p + 2..p + 2 + argc).map(|q| self.value(q)).collect();
                        if let Some(c) = case_of(&name, &a) {
                            out.push(c);
                        }
                        // past its block
                        let o = p + 2 + argc;
                        p = if matches!(self.elem(o), Elem::Ctrl(ref b, _) if b == "{") {
                            self.close(o) + 1
                        } else {
                            o
                        };
                        continue;
                    }
                    p += 2 + argc;
                }
                Elem::Var(..) | Elem::Tmp | Elem::Int(_) => p += 3 + self.word(p + 2),
                _ => p += 1,
            }
        }
        out
    }

    fn window(&mut self, id: i32) -> &mut TextWin {
        let w = self.screen.0 as i32;
        self.windows.entry(id).or_insert(TextWin {
            rect: [0, 0, w, 10000],
            lines: Vec::new(),
            size: 24,
            rgb: [255, 255, 255],
            gf: None,
        })
    }

    fn str_out(&mut self, layer: i32, text: &str) {
        self.last_layer = layer;
        self.window(layer);
        let size = self
            .window_font
            .get(&layer)
            .and_then(|f| self.fonts.get(f))
            .copied()
            .unwrap_or(24)
            .clamp(10, 64);
        let Some(w) = self.windows.get_mut(&layer) else {
            return;
        };
        w.size = size;
        if let Some(k) = text.strip_prefix("@gf(").and_then(|t| t.strip_suffix(')')) {
            w.lines.clear();
            w.gf = k.trim().parse().ok();
            return;
        }
        let width = ((w.rect[2] - w.rect[0]).max(size * 4) / size - 1) as usize;
        if w.lines.is_empty() {
            w.lines.push(String::new());
        }
        for c in plain(text).chars() {
            let last = w.lines.last_mut().unwrap();
            if last.chars().count() >= width && !"、。」』）！？…".contains(c) {
                w.lines.push(String::new());
            }
            w.lines.last_mut().unwrap().push(c);
        }
        self.message = text.chars().take(40).collect();
    }

    // ---- pictures ----

    fn ykg(&mut self, file: &str) -> Option<Rc<ykg::Ykg>> {
        let key = ykc::key(file);
        if let Some(y) = self.cache.get(&key) {
            return Some(y.clone());
        }
        let b = self.file(file)?;
        let y = match ykg::parse(&b) {
            Ok(y) => Rc::new(y),
            Err(e) => {
                self.trace(&format!("{file}: {e}"));
                return None;
            }
        };
        if self.cache.len() > 64 {
            self.cache.clear();
        }
        self.cache.insert(key, y.clone());
        Some(y)
    }

    fn load_layer(
        &mut self,
        id: i32,
        file: &str,
        mask: Option<String>,
        x: f32,
        y: f32,
        shown: bool,
    ) {
        let Some(g) = self.ykg(file) else {
            self.trace(&format!("no picture {file}"));
            self.layers.remove(&id);
            return;
        };
        let mut img = match (&g.image, &g.mask) {
            (Some(i), _) => i.clone(),
            (None, Some(m)) => m.clone(),
            _ => Image::new(1, 1),
        };
        let mask_img = match &mask {
            Some(m) => self
                .ykg(m)
                .and_then(|mg| mg.mask.clone().or_else(|| mg.image.clone())),
            None if g.image.is_some() && img.px.chunks_exact(4).all(|p| p[3] == 255) => {
                g.mask.clone()
            }
            None => None,
        };
        if let Some(m) = mask_img {
            ykg::apply_mask(&mut img, &m);
        }
        img.renew();
        self.layers.insert(
            id,
            Layer {
                file: file.to_string(),
                mask,
                image: Rc::new(img),
                frames: g.frames.clone(),
                x,
                y,
                shown,
                clear: 0.0,
                moving: None,
                fading: None,
                sprites: BTreeMap::new(),
            },
        );
    }

    /// The layers as they are (without the text).
    fn scene(&mut self) -> Vec<Draw> {
        let mut out = Vec::new();
        for (&id, l) in &self.layers {
            if !l.shown || l.clear >= 1000.0 {
                continue;
            }
            let alpha = 1.0 - l.clear / 1000.0;
            let _ = id;
            out.push(draw(&l.image, l.x, l.y, alpha));
            for &(x, y, src, f, t0) in l.sprites.values() {
                let Some(sl) = self.layers.get(&src) else {
                    continue;
                };
                if let Some(r) = frame_at(&sl.frames, f, self.now.saturating_sub(t0)) {
                    let mut d = part(&sl.image, r, l.x + x, l.y + y);
                    d.place.alpha = alpha;
                    out.push(d);
                }
            }
        }
        let texts: Vec<(i32, f32, f32)> = self
            .layers
            .iter()
            .filter(|(_, l)| l.shown)
            .map(|(&id, l)| (id, l.x, l.y))
            .collect();
        for (id, x, y) in texts {
            let gf = self.windows.get(&id).and_then(|w| Some((w.gf?, w.rect)));
            if let (Some((k, rect)), Some(g)) = (gf, self.layers.get(&GRAPHIC_FONT))
                && let Some(r) = frame_at(&g.frames, k, 0)
            {
                out.push(part(&g.image, r, x + rect[0] as f32, y + rect[1] as f32));
            }
            if let Some((lines, rect, size, rgb)) = self
                .windows
                .get(&id)
                .map(|w| (w.lines.clone(), w.rect, w.size, w.rgb))
            {
                let mut ty = y + rect[1] as f32;
                for l in lines {
                    if !l.is_empty() {
                        let t = self.text(&l, rgb, size);
                        out.push(draw(&t, x + rect[0] as f32 - 2.0, ty - 2.0, 1.0));
                    }
                    ty += size as f32 * 1.35;
                }
            }
        }
        out
    }

    fn case_draws(&mut self, out: &mut Vec<Draw>) {
        let cases = self.cases.clone();
        for c in cases {
            let lit = inside(c.rect, self.pointer.0, self.pointer.1);
            if let Some((src, x, y, normal, hover)) = c.src {
                let f = if lit { hover } else { normal };
                if f >= 0
                    && let Some(l) = self.layers.get(&src)
                    && let Some(r) = frame_at(&l.frames, f, 0)
                {
                    out.push(part(&l.image, r, x, y));
                }
            }
            if let Some((t, x, y)) = &c.text {
                let rgb = if lit {
                    [255, 220, 120]
                } else {
                    [255, 255, 255]
                };
                let img = self.text(t, rgb, 26);
                out.push(draw(&img, *x, *y, 1.0));
            }
        }
    }

    pub fn draws(&mut self) -> Vec<Draw> {
        if let Some(m) = self.menu {
            return self.menu_draws(m);
        }
        let mut out = match &self.frozen {
            Some(f) => f.clone(),
            None => self.scene(),
        };
        if let Some(f) = &self.fade {
            let k = ((self.now.saturating_sub(f.start)) as f32 / f.ms.max(1) as f32).min(1.0);
            let mut from = f.from.clone();
            for d in &mut from {
                d.place.alpha *= 1.0 - k;
            }
            out.extend(from);
        }
        if matches!(self.wait, Wait::Cases(..)) && self.frozen.is_none() {
            self.case_draws(&mut out);
        }
        out
    }

    // ---- sounds ----

    fn play(&mut self, ch: i32, file: &str, looped: bool) {
        let Some(data) = self.file(file) else {
            self.trace(&format!("no sound {file}"));
            return;
        };
        self.playing.insert(
            ch,
            SoundState {
                file: file.to_string(),
                looped,
            },
        );
        self.sounds.push(Sound {
            data,
            volume: 1.0,
            channel: ch,
            stop: false,
            fade_ms: 0,
            looped,
        });
    }

    fn play_loaded(&mut self, ch: i32) {
        if ch < 0 {
            return;
        }
        if let Some(f) = self.loaded_sounds.get(&ch).cloned() {
            self.play(ch, &f, false);
        }
    }

    fn stop(&mut self, ch: i32, fade_ms: u32) {
        self.playing.remove(&ch);
        self.sounds.push(Sound {
            data: Vec::new(),
            volume: 0.0,
            channel: ch,
            stop: true,
            fade_ms,
            looped: false,
        });
    }

    pub fn take_sounds(&mut self) -> Vec<Sound> {
        std::mem::take(&mut self.sounds)
    }

    // ---- text ----

    fn text(&mut self, s: &str, rgb: [u8; 3], size: i32) -> Rc<Image> {
        let key = (s.to_string(), rgb, size);
        if let Some(i) = self.text_cache.get(&key) {
            return i.clone();
        }
        let dark = rgb[0] as u32 * 3 + rgb[1] as u32 * 6 + rgb[2] as u32 <= 1000;
        let img = match &self.font {
            Some(f) if dark => {
                // dark text gets a light, fainter border: drawn white, then recoloured
                let mut i = Font::outline(f.clone(), size).render(
                    &nexas::sjis::encode(s),
                    [255, 255, 255],
                    2,
                );
                for p in i.px.chunks_exact_mut(4) {
                    let v = p[0] as u32;
                    for (c, &t) in p.iter_mut().zip(rgb.iter()) {
                        *c = ((t as u32 * v + 250 * (255 - v)) / 255) as u8;
                    }
                    p[3] = ((p[3] as u32 * (128 + v / 2)) / 255) as u8;
                }
                i.renew();
                Rc::new(i)
            }
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

    // ---- input ----

    pub fn point(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
    }

    pub fn click(&mut self, x: i32, y: i32) {
        if x >= 0 {
            self.pointer = (x, y);
        }
        if let Some(m) = self.menu {
            self.menu_click(m, x, y);
            return;
        }
        if self.fade.is_some() {
            self.fade = None;
        }
        match self.wait.clone() {
            Wait::Click | Wait::Until(_) => self.wait = Wait::None,
            Wait::Moving => {
                for l in self.layers.values_mut() {
                    if let Some((_, b, _, _)) = l.moving.take() {
                        l.x = b[0];
                        l.y = b[1];
                    }
                    if let Some((_, b, _, _)) = l.fading.take() {
                        l.clear = b;
                    }
                }
                self.wait = Wait::None;
            }
            Wait::Cases(_, open) if x >= 0 => {
                self.hit = Some((x, y));
                self.wait = Wait::None;
                self.pc = open + 1;
            }
            _ => {}
        }
    }

    /// A right click: our menu (save, load, title).
    pub fn cancel(&mut self) {
        self.menu = match self.menu {
            None if matches!(self.wait, Wait::Click | Wait::Cases(..)) => Some(Menu::Main),
            None => None,
            Some(Menu::Main) => None,
            Some(_) => Some(Menu::Main),
        };
    }

    pub fn wheel(&mut self, _up: bool) {}

    pub fn ended(&self) -> bool {
        self.ended
    }

    // ---- our menu and saves ----

    fn menu_items(&self) -> Vec<(&'static str, [i32; 4])> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        let names = ["セーブ", "ロード", "タイトルへ", "戻る"];
        let top = h / 2 - 2 * 60;
        names
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                (
                    n,
                    [
                        w / 2 - 150,
                        top + i as i32 * 60,
                        w / 2 + 150,
                        top + i as i32 * 60 + 48,
                    ],
                )
            })
            .collect()
    }

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        let rh = (h - 80) / SLOTS as i32;
        (0..SLOTS as i32)
            .map(|i| [60, 40 + i * rh, w - 60, 40 + i * rh + rh - 6])
            .collect()
    }

    fn button(&mut self, label: &str, r: [i32; 4], out: &mut Vec<Draw>) {
        let lit = inside(r, self.pointer.0, self.pointer.1);
        let mut d = draw(
            &solid(
                1,
                1,
                if lit {
                    [70, 90, 140, 230]
                } else {
                    [20, 24, 40, 220]
                },
            ),
            0.0,
            0.0,
            1.0,
        );
        d.dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        out.push(d);
        let t = self.text(label, [255, 255, 255], 22);
        let mut d = draw(
            &t,
            (r[0] + 14) as f32,
            (r[1] + (r[3] - r[1] - 26) / 2) as f32,
            1.0,
        );
        d.place.clip = [r[0] as f32, r[1] as f32, (r[2] - 6) as f32, r[3] as f32];
        out.push(d);
    }

    fn menu_draws(&mut self, m: Menu) -> Vec<Draw> {
        let mut out = match &self.frozen {
            Some(f) => f.clone(),
            None => self.scene(),
        };
        let mut d = draw(&solid(1, 1, [0, 0, 0, 150]), 0.0, 0.0, 1.0);
        d.dst = [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32];
        out.push(d);
        match m {
            Menu::Main => {
                for (label, r) in self.menu_items() {
                    self.button(label, r, &mut out);
                }
            }
            Menu::Save | Menu::Load => {
                for (i, r) in self.slot_rects().into_iter().enumerate() {
                    let label = match self.read_slot(i) {
                        Some(s) => format!("{:02}  {}", i + 1, s.title),
                        None => format!("{:02}  ----", i + 1),
                    };
                    self.button(&label, r, &mut out);
                }
            }
        }
        out
    }

    fn menu_click(&mut self, m: Menu, x: i32, y: i32) {
        match m {
            Menu::Main => {
                let hit = self
                    .menu_items()
                    .into_iter()
                    .position(|(_, r)| inside(r, x, y));
                match hit {
                    Some(0) => self.menu = Some(Menu::Save),
                    Some(1) => self.menu = Some(Menu::Load),
                    Some(2) => {
                        self.menu = None;
                        self.back_to_title();
                    }
                    Some(_) => self.menu = None,
                    None => {}
                }
            }
            Menu::Save | Menu::Load => {
                if let Some(i) = self.slot_rects().into_iter().position(|r| inside(r, x, y)) {
                    if m == Menu::Save {
                        self.save(i);
                        self.menu = None;
                    } else if self.read_slot(i).is_some() {
                        self.load(i);
                        self.menu = None;
                    }
                }
            }
        }
    }

    fn back_to_title(&mut self) {
        self.stack.clear();
        self.layers.clear();
        self.frozen = None;
        self.fade = None;
        self.wait = Wait::None;
        for w in self.windows.values_mut() {
            w.lines.clear();
        }
        self.sounds.push(Sound {
            data: Vec::new(),
            volume: 0.0,
            channel: -1,
            stop: true,
            fade_ms: 300,
            looped: false,
        });
        self.playing.clear();
        if self.enter("Start.yks", None).is_err() {
            self.ended = true;
        }
    }

    fn read_slot(&mut self, i: usize) -> Option<SaveData> {
        let b = self.files.read_save(&format!("yuka{:02}.sav", i + 1))?;
        SaveData::parse(&String::from_utf8_lossy(&b))
    }

    fn read_globals(&mut self) {
        if let Some(b) = self.files.read_save("yuka-global.sav") {
            let t = String::from_utf8_lossy(&b).to_string();
            for l in t.lines() {
                if let Some((k, n, v)) = parse_var(l) {
                    self.vars.insert((k, n), v);
                }
            }
        }
    }

    fn write_globals(&mut self) {
        let mut t = String::new();
        let mut g: Vec<_> = self
            .vars
            .iter()
            .filter(|(k, _)| k.0.starts_with("Global"))
            .collect();
        g.sort_by(|a, b| a.0.cmp(b.0));
        for ((k, n), v) in g {
            t.push_str(&var_line(k, *n, v));
        }
        self.files.write_save("yuka-global.sav", t.as_bytes());
    }

    fn save(&mut self, i: usize) {
        let mut t = String::new();
        t.push_str(&format!("title\t{}\n", esc(&self.message)));
        t.push_str(&format!("at\t{}\t{}\n", self.cur, self.pc));
        for (s, p) in &self.stack {
            t.push_str(&format!("stack\t{s}\t{p}\n"));
        }
        for ((k, n), v) in &self.vars {
            if !k.starts_with("Global") {
                t.push_str(&var_line(k, *n, v));
            }
        }
        for (id, l) in &self.layers {
            t.push_str(&format!(
                "layer\t{id}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                esc(&l.file),
                esc(l.mask.as_deref().unwrap_or("")),
                l.x,
                l.y,
                l.shown as u8,
                l.clear
            ));
        }
        for (id, l) in &self.layers {
            for (no, &(x, y, src, k, _)) in &l.sprites {
                t.push_str(&format!("sprite\t{id}\t{no}\t{x}\t{y}\t{src}\t{k}\n"));
            }
        }
        for (ch, s) in &self.playing {
            if s.looped {
                t.push_str(&format!("sound\t{ch}\t{}\n", esc(&s.file)));
            }
        }
        for (id, w) in &self.windows {
            if let Some(k) = w.gf {
                t.push_str(&format!("gf\t{id}\t{k}\n"));
            }
            for l in &w.lines {
                t.push_str(&format!("line\t{id}\t{}\n", esc(l)));
            }
        }
        self.files
            .write_save(&format!("yuka{:02}.sav", i + 1), t.as_bytes());
    }

    fn load(&mut self, i: usize) {
        let Some(b) = self.files.read_save(&format!("yuka{:02}.sav", i + 1)) else {
            return;
        };
        let t = String::from_utf8_lossy(&b).to_string();
        self.back_to_title();
        self.layers.clear();
        self.stack.clear();
        self.vars.retain(|k, _| k.0.starts_with("Global"));
        let mut at = None;
        for l in t.lines() {
            let f: Vec<&str> = l.split('\t').collect();
            match f.first().copied() {
                Some("at") if f.len() >= 3 => {
                    at = Some((f[1].to_string(), f[2].parse().unwrap_or(0)))
                }
                Some("stack") if f.len() >= 3 => {
                    let s = f[1].to_string();
                    if self.load_script(&s).is_ok() {
                        self.stack.push((s, f[2].parse().unwrap_or(0)));
                    }
                }
                Some("var") => {
                    if let Some((k, n, v)) = parse_var(l) {
                        self.vars.insert((k, n), v);
                    }
                }
                Some("layer") if f.len() >= 8 => {
                    let id = f[1].parse().unwrap_or(0);
                    let mask = unesc(f[3]);
                    let mask = (!mask.is_empty()).then_some(mask);
                    let (x, y) = (f[4].parse().unwrap_or(0.0), f[5].parse().unwrap_or(0.0));
                    self.load_layer(id, &unesc(f[2]), mask, x, y, f[6] == "1");
                    if let Some(l) = self.layers.get_mut(&id) {
                        l.clear = f[7].parse().unwrap_or(0.0);
                    }
                }
                Some("sound") if f.len() >= 3 => {
                    let ch = f[1].parse().unwrap_or(0);
                    let file = unesc(f[2]);
                    self.loaded_sounds.insert(ch, file.clone());
                    self.play(ch, &file, true);
                }
                Some("sprite") if f.len() >= 7 => {
                    let v: Vec<f32> = f[1..7].iter().map(|x| x.parse().unwrap_or(0.0)).collect();
                    let now = self.now;
                    if let Some(l) = self.layers.get_mut(&(v[0] as i32)) {
                        l.sprites
                            .insert(v[1] as i32, (v[2], v[3], v[4] as i32, v[5] as i32, now));
                    }
                }
                Some("gf") if f.len() >= 3 => {
                    let id = f[1].parse().unwrap_or(0);
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.gf = f[2].parse().ok();
                    }
                }
                Some("line") if f.len() >= 3 => {
                    let id = f[1].parse().unwrap_or(0);
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.lines.push(unesc(f[2]));
                    }
                }
                _ => {}
            }
        }
        if let Some((s, p)) = at
            && self.load_script(&s).is_ok()
        {
            self.cur = s;
            self.pc = p;
        }
        self.frozen = None;
        self.fade = None;
        self.wait = Wait::Click;
    }

    /// The frame as one picture (for the native runner).
    pub fn render(&mut self) -> Image {
        let mut img = Image::new(self.screen.0, self.screen.1);
        img.fill([0, 0, 0, 255]);
        for d in self.draws() {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }
}

struct SaveData {
    title: String,
}

impl SaveData {
    fn parse(t: &str) -> Option<SaveData> {
        let title = t.lines().find_map(|l| l.strip_prefix("title\t"))?;
        Some(SaveData {
            title: unesc(title),
        })
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn var_line(k: &str, n: i32, v: &Value) -> String {
    match v {
        Value::Str(s) => format!("var\t{k}\t{n}\ts\t{}\n", esc(s)),
        o => format!("var\t{k}\t{n}\ti\t{}\n", o.n()),
    }
}

fn parse_var(l: &str) -> Option<(String, i32, Value)> {
    let f: Vec<&str> = l.split('\t').collect();
    if f.len() < 5 || f[0] != "var" {
        return None;
    }
    let n = f[2].parse().ok()?;
    let v = if f[3] == "s" {
        Value::Str(unesc(f[4]))
    } else {
        Value::Int(f[4].parse().ok()?)
    };
    Some((f[1].to_string(), n, v))
}

/// The statement before each `{` of a script and where it starts.
type Blocks = (HashMap<usize, (String, usize)>, HashMap<usize, usize>);

/// The call or assignment owning each `{`, and each `{`'s `}`.
fn scan(s: &Script) -> Blocks {
    let mut out = HashMap::new();
    let mut closes = HashMap::new();
    let mut last: Option<(String, usize)> = None;
    let mut p = 0;
    let word = |p: usize| s.code.get(p).map_or(0, |&w| w as usize);
    while p < s.code.len() {
        match s.elem(p) {
            Some(Elem::Func(name)) => {
                last = Some((name.clone(), p));
                p += 2 + word(p + 1);
            }
            Some(Elem::Var(..)) | Some(Elem::Tmp) | Some(Elem::Int(_)) => {
                last = Some(("=".into(), p));
                p += 3 + word(p + 2);
            }
            Some(Elem::Ctrl(n, link)) => {
                if n == "{"
                    && let Some(l) = &last
                {
                    out.insert(p, l.clone());
                }
                if n == "}" && *link >= 0 {
                    closes.insert(*link as usize, p);
                }
                last = None;
                p += 1;
            }
            _ => p += 1,
        }
    }
    (out, closes)
}

/// Text without its inline commands (`@r(5,reading)` ruby and the like).
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('@') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        let name = tail
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(tail.len());
        match (tail[name..].starts_with('('), tail[name..].find(')')) {
            (true, Some(e)) if name > 0 => rest = &tail[name + e + 1..],
            _ => {
                out.push('@');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_op(s: &str) -> bool {
    matches!(
        s,
        "+" | "-"
            | "*"
            | "/"
            | "%"
            | "<"
            | ">"
            | "<="
            | ">="
            | "="
            | "=="
            | "!="
            | "&&"
            | "||"
            | "&"
            | "|"
    )
}

fn prec(op: &str) -> u8 {
    match op {
        "*" | "/" | "%" => 0,
        "+" | "-" => 1,
        "&&" | "||" | "&" | "|" => 3,
        _ => 2,
    }
}

fn apply(op: &str, a: Value, b: Value) -> Value {
    let str_op = matches!(a, Value::Str(_)) || matches!(b, Value::Str(_));
    match op {
        "+" if str_op => Value::Str(a.s() + &b.s()),
        "=" | "==" if str_op => Value::Int((a.s() == b.s()) as i64),
        "!=" if str_op => Value::Int((a.s() != b.s()) as i64),
        _ => {
            let (x, y) = (a.n(), b.n());
            Value::Int(match op {
                "+" => x.wrapping_add(y),
                "-" => x.wrapping_sub(y),
                "*" => x.wrapping_mul(y),
                "/" => x.checked_div(y).unwrap_or(0),
                "%" => x.checked_rem(y).unwrap_or(0),
                "<" => (x < y) as i64,
                ">" => (x > y) as i64,
                "<=" => (x <= y) as i64,
                ">=" => (x >= y) as i64,
                "=" | "==" => (x == y) as i64,
                "!=" => (x != y) as i64,
                "&&" | "&" => (x != 0 && y != 0) as i64,
                _ => (x != 0 || y != 0) as i64,
            })
        }
    }
}

/// Where the click sound is among a case's arguments.
fn argc_click(name: &str) -> usize {
    match name {
        "MouseCase2" => 13,
        _ => 11,
    }
}

/// MouseCase(x1, y1, x2, y2, target layer, picture layer, x, y, normal frame, lit frame,
/// lit sound, click sound[, button]); MouseCase2 has whether it is on first and a third
/// frame after the lit one; IndicateCase only
/// lights up; Case(text, x, y, x1, y1, x2, y2, lit sound, click sound) is a choice.
fn case_of(name: &str, a: &[Value]) -> Option<Case> {
    let n = |i: usize| a.get(i).map_or(-1, |v| v.n()) as i32;
    match name {
        "Case" => Some(Case {
            rect: [n(3), n(4), n(5), n(6)],
            src: None,
            text: Some((
                a.first().map(|v| v.s()).unwrap_or_default(),
                n(1) as f32,
                n(2) as f32,
            )),
            passive: false,
            fallback: false,
        }),
        _ => {
            let k = (name == "MouseCase2") as usize;
            if a.len() < k + 4 {
                return None;
            }
            let src = (n(k + 5) >= 0).then(|| {
                (
                    n(k + 5),
                    n(k + 6) as f32,
                    n(k + 7) as f32,
                    n(k + 8),
                    n(k + 9),
                )
            });
            Some(Case {
                rect: [n(k), n(k + 1), n(k + 2), n(k + 3)],
                src,
                text: None,
                // MouseCase2 has a third frame before its sounds, and 0 first when it is off;
                // MouseKeyCase / KeyTableCase are keys or the right button (our menu has that)
                passive: matches!(name, "IndicateCase" | "MouseKeyCase" | "KeyTableCase")
                    || (k == 1 && n(0) == 0),
                fallback: name == "MouseCase" && a.len() > 12,
            })
        }
    }
}

/// How long an Ogg Vorbis sound plays (its last granule position over its rate), in ms.
pub fn ogg_ms(b: &[u8]) -> u64 {
    let rate = b
        .windows(7)
        .position(|w| w == b"\x01vorbis")
        .and_then(|i| b.get(i + 12..i + 16))
        .map_or(44100, |r| u32::from_le_bytes(r.try_into().unwrap()).max(1)) as u64;
    let Some(last) = b.windows(4).rposition(|w| w == b"OggS") else {
        return 0;
    };
    let g = b
        .get(last + 6..last + 14)
        .map_or(0, |g| u64::from_le_bytes(g.try_into().unwrap()));
    if g == u64::MAX {
        return 0;
    }
    g * 1000 / rate
}

/// The frame of a pattern showing `ms` after it started (the last one stays).
fn frame_at(patterns: &[Vec<([i32; 4], u32)>], k: i32, ms: u64) -> Option<[i32; 4]> {
    let frames = patterns.get(usize::try_from(k).ok()?)?;
    let mut t = ms;
    for &(r, d) in frames {
        if t < d as u64 {
            return Some(r);
        }
        t -= d as u64;
    }
    frames.last().map(|f| f.0)
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
}

/// Part of a picture (`src`: x, y, w, h) at (x, y).
fn part(image: &Rc<Image>, src: [i32; 4], x: f32, y: f32) -> Draw {
    let mut d = draw(image, x - src[0] as f32, y - src[1] as f32, 1.0);
    d.place.clip = [x, y, x + src[2] as f32, y + src[3] as f32];
    d
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(v: &[i64]) -> Vec<Value> {
        v.iter().map(|&n| Value::Int(n)).collect()
    }

    #[test]
    fn cases() {
        // a whole-screen MouseCase with a 13th number only takes clicks no button has
        let a = [0, 0, 800, 600, -1, -1, -1, -1, -1, -1, -1, -1, 1];
        let c = case_of("MouseCase", &ints(&a)).unwrap();
        assert!(c.fallback && !c.passive);
        // MouseCase2: on/off first, the rect after it
        let a = [
            1, 315, 272, 470, 326, 21, 25, 315, 272, -1, 5, -1, 101, 100, 128,
        ];
        let c = case_of("MouseCase2", &ints(&a)).unwrap();
        assert_eq!(c.rect, [315, 272, 470, 326]);
        assert!(!c.passive && !c.fallback);
        let mut off = a;
        off[0] = 0;
        assert!(case_of("MouseCase2", &ints(&off)).unwrap().passive);
        assert_eq!(argc_click("MouseCase2"), 13);
    }

    #[test]
    fn inline_commands() {
        assert_eq!(
            plain("五光館学園@r(5,ごこうかんがくえん)、か"),
            "五光館学園、か"
        );
        assert_eq!(plain("a@b"), "a@b");
    }

    #[test]
    fn patterns() {
        let p = vec![
            vec![([0, 0, 10, 10], 100), ([10, 0, 10, 10], 100)],
            vec![([0, 10, 5, 5], 0)],
        ];
        assert_eq!(frame_at(&p, 0, 50), Some([0, 0, 10, 10]));
        assert_eq!(frame_at(&p, 0, 150), Some([10, 0, 10, 10]));
        assert_eq!(frame_at(&p, 0, 5000), Some([10, 0, 10, 10]));
        assert_eq!(frame_at(&p, 1, 0), Some([0, 10, 5, 5]));
        assert_eq!(frame_at(&p, 2, 0), None);
    }
}
