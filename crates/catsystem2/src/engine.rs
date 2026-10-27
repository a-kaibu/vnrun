//! Runs a CatSystem2 game: the screen scripts (.fes, from flow.fes, which cs2's main.kcs starts)
//! with their objects, and the story (.cst scenes, which the games play with sscript.kcs; this
//! side plays them itself, see [`crate::adv`]).
//!
//! A .fes runs its current block (`#LABEL`) from the top every frame until `next` moves on;
//! `wait` stops it for some frames or until its objects stop moving. `execfes` starts a child
//! script beside it; the child's `exit n` sets the parent's `_EXITCODE_` (-1 while it runs) and
//! `endfes` removes the child and its objects.

use std::collections::HashMap;
use std::rc::Rc;

use nexas::files::Files;
use nexas::gfx::{Draw, Image, Place};

use crate::adv::Adv;
use crate::expr::{self, Env};
use crate::fes::{Def, Fes};
use crate::game::Game;
use crate::text::Font;

/// A sound to start (`data`) or stop (`None`) on a channel.
pub struct SoundEvent {
    pub channel: String,
    pub data: Option<Rc<Vec<u8>>>,
    pub volume: f32,
    pub repeat: bool,
    pub fade_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plane,
    /// a plane or picture of one colour
    Color,
    Image,
    Button,
    Sound,
    Other,
}

#[derive(Debug, Clone, Copy)]
pub enum Move {
    Fade(f32, f32),
    /// to an absolute position
    Pos((f32, f32), (f32, f32)),
    /// along an ellipse: angles in degrees, radii; the position moves by the change
    Arc {
        a0: f32,
        a1: f32,
        r: (f32, f32),
        from: (f32, f32),
    },
    Scale((f32, f32), (f32, f32)),
    Rot(f32, f32),
    /// takes time but changes nothing we draw (shakes, quakes)
    Idle,
}

#[derive(Debug, Clone, Copy)]
pub struct Motion {
    pub m: Move,
    pub t0: u64,
    pub dur: u64,
    /// accelerate (1), decelerate (2) or neither (0)
    pub ease: u8,
}

#[derive(Debug, Clone)]
pub struct Obj {
    pub owner: u32,
    pub kind: Kind,
    pub file: String,
    pub ids: Vec<i64>,
    pub cur: usize,
    pub color: [u8; 3],
    pub size: (f32, f32),
    pub plane: Option<usize>,
    pub pos: (f32, f32),
    pub base: (f32, f32),
    pub scale: (f32, f32),
    pub rot: f32,
    /// 0..=255
    pub alpha: f32,
    pub disp: bool,
    pub pri: i32,
    pub enable: bool,
    /// story pictures are placed by their base point, screen pictures by their canvas
    pub anchor: bool,
    /// `coordmode 1`: a screen picture scales and turns about its frame's base point
    pub center: bool,
    /// the class whose handlers the object also has (`DEFAULT defbtn`)
    pub class: String,
    pub add: bool,
    pub motions: Vec<Motion>,
    /// a story picture made of parts (files), drawn in order
    pub parts: Vec<String>,
    /// sounds: the loaded file and the volume (0..=1)
    pub sound: String,
    pub volume: f32,
}

impl Obj {
    pub fn new(owner: u32, kind: Kind) -> Obj {
        Obj {
            owner,
            kind,
            file: String::new(),
            ids: Vec::new(),
            cur: 0,
            color: [0, 0, 0],
            size: (0.0, 0.0),
            plane: None,
            pos: (0.0, 0.0),
            base: (0.0, 0.0),
            scale: (1.0, 1.0),
            rot: 0.0,
            alpha: 255.0,
            disp: false,
            pri: 0,
            enable: true,
            anchor: false,
            center: false,
            class: String::new(),
            add: false,
            motions: Vec::new(),
            parts: Vec::new(),
            sound: String::new(),
            volume: 1.0,
        }
    }

    pub fn id(&self) -> i64 {
        self.ids.get(self.cur).copied().unwrap_or(-1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wait {
    None,
    Until(u64),
    /// until the script's objects stop moving
    Motions,
    Obj(usize),
}

/// A running screen script.
pub struct Run {
    pub id: u32,
    pub name: String,
    fes: Rc<Fes>,
    pc: usize,
    /// the block `pc` is in, rerun from its top when it ends
    block: usize,
    calls: Vec<usize>,
    wait: Wait,
    locals: HashMap<i64, i64>,
    /// the child's exit code, -1 while it runs
    pub exit_code: i64,
    /// exited: it no longer runs, its objects stay until the parent's endfes
    pub(crate) done: bool,
    /// started by the story (`fex sel`): its exit code is its own, the story waits on it
    pub(crate) story: bool,
    names: HashMap<String, usize>,
    groups: HashMap<String, Vec<usize>>,
    /// the button under the pointer
    focus: Option<usize>,
}

pub struct Engine {
    pub g: Game,
    pub width: u32,
    pub height: u32,
    pub now: u64,
    pub objs: Vec<Option<Obj>>,
    pub runs: Vec<Run>,
    next_id: u32,
    pub flags: HashMap<i64, i64>,
    pub strs: HashMap<i64, String>,
    /// the picture under everything (`baseimg`)
    base: Vec<Draw>,
    pub font: Font,
    pub sounds: Vec<SoundEvent>,
    pub mouse: (i32, i32),
    clicked: bool,
    rclicked: bool,
    /// this frame's click for the scripts: the button's number, -1 elsewhere, 0 none
    click_l: i64,
    click_r: i64,
    pub adv: Option<Adv>,
    ended: bool,
    pub log: u8,
    /// skip the story (no waits, no clicks) until this scene and line (for testing)
    pub skip_to: Option<(String, usize)>,
    colors: HashMap<[u8; 3], Rc<Image>>,
    /// the event number (`@`) while a handler runs
    at: i64,
    /// the object (`@ setid 1`) while a handler runs
    at_obj: Option<usize>,
    /// the variables a load sets once its skip reaches the saved page
    pub(crate) pending_vars: Option<crate::adv::Vars>,
    /// our save/load screen or backlog, over everything
    pub(crate) menu: Option<crate::menu::Menu>,
    wheel_up: bool,
}

fn num(s: &str) -> f32 {
    s.trim_end_matches('%').parse().unwrap_or(0.0)
}

fn pair(s: &str) -> (f32, f32) {
    let mut it = s.split(',');
    (num(it.next().unwrap_or("0")), num(it.next().unwrap_or("0")))
}

/// Arguments split by spaces, keeping a parenthesised part whole.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c.is_whitespace() && depth <= 0 {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `if (cond) rest`: the condition and what follows it.
pub fn split_if(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix("if")?.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let mut depth = 0;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&rest[1..i], rest[i + 1..].trim()));
                }
            }
            _ => {}
        }
    }
    None
}

pub fn ease(t: f32, kind: u8) -> f32 {
    match kind {
        1 => t * t,
        2 => 1.0 - (1.0 - t) * (1.0 - t),
        _ => t,
    }
}

struct Ctx<'a> {
    e: &'a Engine,
    run: Option<usize>,
    depth: u8,
}

impl Env for Ctx<'_> {
    fn get(&mut self, n: &str) -> i64 {
        let e = self.e;
        if let Some(v) = n.strip_prefix('$').or_else(|| n.strip_prefix('#')) {
            // `$700`, or `$g_TEMP_0` with a macro for the number
            let k = match v.parse::<i64>() {
                Ok(k) => k,
                Err(_) if self.depth < 8 => {
                    self.depth += 1;
                    let k = self.get(v);
                    self.depth -= 1;
                    k
                }
                Err(_) => return 0,
            };
            return self.e.flags.get(&k).copied().unwrap_or(0);
        }
        if let Some(v) = n.strip_prefix('\\') {
            let Some(r) = self.run else { return 0 };
            return v
                .parse::<i64>()
                .map(|k| e.runs[r].locals.get(&k).copied().unwrap_or(0))
                .unwrap_or(0);
        }
        match n {
            "@" => return e.at,
            "_EXITCODE_" => return self.run.map_or(-1, |r| e.runs[r].exit_code),
            "_CLICK_L_" => return e.click_l,
            "_CLICK_R_" => return e.click_r,
            "_MOUSE_X_" => return e.mouse.0 as i64,
            "_MOUSE_Y_" => return e.mouse.1 as i64,
            _ => {}
        }
        if let Some(r) = self.run {
            let run = &e.runs[r];
            if let Some(m) = run.fes.macros.get(n)
                && self.depth < 8
            {
                let m = m.clone();
                let mut c = Ctx {
                    e,
                    run: self.run,
                    depth: self.depth + 1,
                };
                return expr::eval(&m, &mut c).unwrap_or(0);
            }
            let name = n.replace("[@]", &format!("[{}]", e.at));
            if let Some(&i) = run.names.get(&name) {
                return i as i64 + 1;
            }
        }
        0
    }
}

impl Engine {
    pub fn open(files: Box<dyn Files>) -> Result<Engine, String> {
        let mut g = Game::open(files)?;
        let startup = g
            .files
            .read("config/startup.xml")
            .map(|b| crate::sjis(&b))
            .unwrap_or_default();
        let dim = |k: &str, d: u32| {
            crate::game::xml_path(&startup, &["SCREEN", k])
                .and_then(|v| v.parse().ok())
                .unwrap_or(d)
        };
        let (width, height) = (dim("width", 800), dim("height", 600));
        let mut e = Engine {
            g,
            width,
            height,
            now: 0,
            objs: Vec::new(),
            runs: Vec::new(),
            next_id: 1,
            flags: HashMap::new(),
            strs: HashMap::new(),
            base: Vec::new(),
            font: Font::default(),
            sounds: Vec::new(),
            mouse: (-1, -1),
            clicked: false,
            rclicked: false,
            click_l: 0,
            click_r: 0,
            adv: None,
            ended: false,
            log: 0,
            skip_to: None,
            colors: HashMap::new(),
            at: 0,
            at_obj: None,
            pending_vars: None,
            menu: None,
            wheel_up: false,
        };
        e.exec_fes("flow")?;
        Ok(e)
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
        if x >= 0 {
            self.mouse = (x, y);
        }
        self.clicked = true;
    }

    pub fn cancel(&mut self) {
        self.rclicked = true;
    }

    pub fn wheel(&mut self, up: bool) {
        if up {
            self.wheel_up = true;
        } else {
            self.clicked = true;
        }
    }

    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    pub fn new_owner(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn add_obj(&mut self, o: Obj) -> usize {
        self.objs.push(Some(o));
        self.objs.len() - 1
    }

    pub fn obj(&mut self, i: usize) -> Option<&mut Obj> {
        self.objs.get_mut(i)?.as_mut()
    }

    /// Removes every object of an owner.
    pub fn drop_owner(&mut self, owner: u32) {
        for o in self.objs.iter_mut() {
            if o.as_ref().is_some_and(|o| o.owner == owner) {
                *o = None;
            }
        }
        while self.objs.last().is_some_and(Option::is_none) {
            self.objs.pop();
        }
    }

    // ---- screen scripts

    pub(crate) fn exec_fes(&mut self, name: &str) -> Result<(), String> {
        let data = self
            .g
            .read(&format!("{name}.fes"))
            .ok_or(format!("{name}.fes: not found"))?;
        let fes = Rc::new(Fes::parse_packed(&data)?);
        let id = self.new_owner();
        let pc = fes.labels.get("START").copied().unwrap_or(fes.lines.len());
        self.runs.push(Run {
            id,
            name: name.to_string(),
            fes: fes.clone(),
            pc,
            block: pc,
            calls: Vec::new(),
            wait: Wait::None,
            locals: HashMap::new(),
            exit_code: -1,
            done: false,
            story: false,
            names: HashMap::new(),
            groups: HashMap::new(),
            focus: None,
        });
        let r = self.runs.len() - 1;
        for d in &fes.defs {
            match d {
                Def::Define(kind, name) => self.define(r, kind, name),
                Def::Object(cols, rows) => {
                    for row in rows {
                        for i in self.resolve(r, &row[0]) {
                            for (c, v) in cols.iter().zip(&row[1..]) {
                                self.set_attr(i, c, v, r);
                            }
                        }
                    }
                }
            }
        }
        // A plane the script never shows itself (it may hide it) is shown whatever its DISP
        // column says (ニュートンと林檎の樹's title puts its picture on such a plane).
        for d in &fes.defs {
            let Def::Define(kind, plane) = d else {
                continue;
            };
            if kind != "PLANE" {
                continue;
            }
            let base = plane.split('[').next().unwrap_or(plane);
            let shown = fes.lines.iter().any(|l| {
                let Some(rest) = l.strip_prefix(base) else {
                    return false;
                };
                let w: Vec<&str> = rest.split_whitespace().collect();
                w.windows(2).any(|p| p[0] == "disp" && p[1] != "0")
            });
            if !shown {
                for i in self.resolve(r, base) {
                    if let Some(o) = self.obj(i) {
                        o.disp = true;
                    }
                }
            }
        }
        if self.log > 0 {
            eprintln!("execfes {name}");
        }
        Ok(())
    }

    fn define(&mut self, r: usize, kind: &str, name: &str) {
        let k = match kind {
            "PLANE" => Kind::Plane,
            "CPLANE" | "CIMAGE" => Kind::Color,
            "IMAGE" | "DIFIMAGE" => Kind::Image,
            "BUTTON" => Kind::Button,
            "SOUND" => Kind::Sound,
            _ => Kind::Other,
        };
        let id = self.runs[r].id;
        let (base, count) = match name.split_once('[') {
            Some((b, n)) => (b, n.trim_end_matches(']').parse::<usize>().unwrap_or(1)),
            None => (name, 0),
        };
        let mut o = Obj::new(id, k);
        if k == Kind::Color {
            o.size = (self.width as f32, self.height as f32);
            o.color = [255, 255, 255];
        }
        if count == 0 {
            let i = self.add_obj(o);
            self.runs[r].names.insert(base.to_string(), i);
            self.runs[r].groups.insert(base.to_string(), vec![i]);
        } else {
            let mut all = Vec::new();
            for k in 0..count {
                let i = self.add_obj(o.clone());
                self.runs[r].names.insert(format!("{base}[{k}]"), i);
                all.push(i);
            }
            self.runs[r].groups.insert(base.to_string(), all);
        }
    }

    /// The objects a name means in a script: `pl[1]`, `btn1[@]`, or a whole array `pl`.
    fn resolve(&self, r: usize, name: &str) -> Vec<usize> {
        if name == "@" {
            return self.at_obj.into_iter().collect();
        }
        let run = &self.runs[r];
        let name = if name.contains('[') {
            // the index may be an expression
            let (b, idx) = name.split_once('[').unwrap();
            let idx = idx.trim_end_matches(']');
            let v = expr::eval(
                idx,
                &mut Ctx {
                    e: self,
                    run: Some(r),
                    depth: 0,
                },
            )
            .unwrap_or(0);
            format!("{b}[{v}]")
        } else {
            name.to_string()
        };
        if let Some(&i) = run.names.get(&name)
            && !run.groups.get(&name).is_some_and(|g| g.len() > 1)
        {
            return vec![i];
        }
        run.groups.get(&name).cloned().unwrap_or_default()
    }

    fn eval(&self, r: Option<usize>, s: &str) -> i64 {
        match expr::eval(
            s,
            &mut Ctx {
                e: self,
                run: r,
                depth: 0,
            },
        ) {
            Ok(v) => v,
            Err(err) => {
                if self.log > 0 {
                    eprintln!("expression {s:?}: {err}");
                }
                0
            }
        }
    }

    fn set_attr(&mut self, i: usize, col: &str, v: &str, r: usize) {
        // "file:id" -> a frame of a picture
        let frame_of = |e: &mut Engine, v: &str| {
            let (f, id) = v.split_once(':')?;
            e.g.frame(f, id.parse().ok()?)
        };
        match col {
            "FILE" => {
                if let Some(o) = self.obj(i) {
                    o.file = v.to_string();
                }
            }
            "POS2" => {
                if let Some(f) = frame_of(self, v)
                    && let Some(o) = self.obj(i)
                {
                    o.pos = (f.x as f32, f.y as f32);
                }
            }
            "SIZE2" => {
                if let Some(f) = frame_of(self, v)
                    && let Some(o) = self.obj(i)
                {
                    o.size = (f.image.w as f32, f.image.h as f32);
                }
            }
            "PL" | "PARENT" => {
                let p = self.resolve(r, v).first().copied();
                if let Some(o) = self.obj(i) {
                    o.plane = p;
                }
            }
            _ => {
                let Some(o) = self.obj(i) else { return };
                match col {
                    "DISP" => o.disp = num(v) != 0.0,
                    "PRI" => o.pri = num(v) as i32,
                    "POS" => o.pos = pair(v),
                    "BASE" => o.base = pair(v),
                    "SIZE" => o.size = pair(v),
                    "ENABLE" => o.enable = num(v) != 0.0,
                    "BLEND" => o.alpha = num(v),
                    "DEFAULT" => o.class = v.to_string(),
                    "COLOR" => {
                        let c = u32::from_str_radix(v.trim_start_matches("0x"), 16).unwrap_or(0);
                        o.color = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                    }
                    _ => {
                        if let Some(k) = col.strip_prefix("ID.") {
                            let k: usize = k.parse().unwrap_or(0);
                            if o.ids.len() <= k {
                                o.ids.resize(k + 1, -1);
                            }
                            o.ids[k] = v.parse().unwrap_or(-1);
                        }
                    }
                }
            }
        }
    }

    /// Runs the scripts for one frame.
    fn run_scripts(&mut self) -> Result<(), String> {
        let mut r = 0;
        while r < self.runs.len() {
            if !self.runs[r].done {
                self.run_one(r)?;
            }
            r += 1;
        }
        Ok(())
    }

    fn motions_done(&self, owner: u32) -> bool {
        self.objs
            .iter()
            .flatten()
            .filter(|o| o.owner == owner)
            .all(|o| o.motions.is_empty())
    }

    fn run_one(&mut self, r: usize) -> Result<(), String> {
        match self.runs[r].wait {
            Wait::Until(t) if self.now < t => return Ok(()),
            Wait::Motions if !self.motions_done(self.runs[r].id) => return Ok(()),
            Wait::Obj(i)
                if self
                    .objs
                    .get(i)
                    .and_then(|o| o.as_ref())
                    .is_some_and(|o| !o.motions.is_empty()) =>
            {
                return Ok(());
            }
            _ => self.runs[r].wait = Wait::None,
        }
        let mut budget = 4000;
        let mut reruns = 0;
        while budget > 0 && r < self.runs.len() && !self.runs[r].done {
            budget -= 1;
            let run = &self.runs[r];
            let line = run.fes.lines.get(run.pc).cloned();
            match line {
                None => {
                    // the end of the script: stay in the block
                    if !self.block_end(r, &mut reruns) {
                        return Ok(());
                    }
                }
                Some(l) if l.starts_with('#') => {
                    if !self.block_end(r, &mut reruns) {
                        return Ok(());
                    }
                }
                Some(l) => {
                    self.runs[r].pc += 1;
                    self.command(r, &l)?;
                    if r >= self.runs.len() || self.runs[r].wait != Wait::None {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    /// At the end of a block: return from a call, or rerun the block next frame.
    fn block_end(&mut self, r: usize, reruns: &mut u32) -> bool {
        let run = &mut self.runs[r];
        if let Some(pc) = run.calls.pop() {
            run.pc = pc;
            return true;
        }
        run.pc = run.block;
        *reruns += 1;
        false
    }

    fn goto(&mut self, r: usize, label: &str) {
        let run = &mut self.runs[r];
        match run.fes.labels.get(label) {
            Some(&pc) => {
                run.pc = pc;
                run.block = pc;
                run.calls.clear();
            }
            None => eprintln!("{}: no label {label}", run.name),
        }
    }

    /// Skips to the matching `else`/`endif` (`to_else`) or `endif`.
    fn skip_if(&mut self, r: usize, to_else: bool) {
        let run = &mut self.runs[r];
        let mut depth = 0;
        while let Some(l) = run.fes.lines.get(run.pc) {
            if l.starts_with('#') {
                return;
            }
            run.pc += 1;
            let w = l.split_whitespace().next().unwrap_or("");
            if w == "if"
                || (w.starts_with("if(") && split_if(l).is_some_and(|(_, rest)| rest.is_empty()))
            {
                depth += 1;
            } else if w == "endif" {
                if depth == 0 {
                    return;
                }
                depth -= 1;
            } else if w == "else" && depth == 0 && to_else {
                return;
            }
        }
    }

    /// `$str599` and the like replaced by the string variables.
    fn strings(&self, s: &str) -> String {
        if !s.contains("$str") {
            return s.to_string();
        }
        let mut out = String::new();
        let mut rest = s;
        while let Some(i) = rest.find("$str") {
            out.push_str(&rest[..i]);
            let tail = &rest[i + 4..];
            let n = tail.bytes().take_while(u8::is_ascii_digit).count();
            if n == 0 {
                out.push_str("$str");
                rest = tail;
                continue;
            }
            let k: i64 = tail[..n].parse().unwrap_or(0);
            out.push_str(self.strs.get(&k).map_or("", String::as_str));
            rest = &tail[n..];
        }
        out.push_str(rest);
        out
    }

    fn command(&mut self, r: usize, line: &str) -> Result<(), String> {
        if self.log > 1 {
            eprintln!("[{}] {line}", self.runs[r].name);
        }
        if let Some((cond, rest)) = split_if(line) {
            let v = self.eval(Some(r), cond) != 0;
            if rest.is_empty() {
                if !v {
                    self.skip_if(r, true);
                }
            } else if v {
                return self.command(r, rest);
            }
            return Ok(());
        }
        let mut a = split_args(&self.strings(line));
        // `$128=1` without spaces
        if let Some(first) = a.first().cloned()
            && (first.starts_with('$') || first.starts_with('\\'))
            && let Some(eq) = first.find('=')
        {
            let op_at = if eq > 0 && "+-*/".contains(&first[eq - 1..eq]) {
                eq - 1
            } else {
                eq
            };
            let rest: Vec<String> = a.drain(1..).collect();
            let rhs = [vec![first[eq + 1..].to_string()], rest].concat().join(" ");
            a = vec![
                first[..op_at].to_string(),
                first[op_at..=eq].to_string(),
                rhs,
            ];
        }
        let Some(cmd) = a.first().map(String::as_str) else {
            return Ok(());
        };
        let arg = |k: usize| a.get(k).map_or("", String::as_str);
        // assignments: $1 = 2, \0 += 1
        if (cmd.starts_with('$') || cmd.starts_with('\\')) && a.len() >= 3 {
            let rhs = a[2..].join(" ");
            let v = self.eval(Some(r), &rhs);
            let old = self.eval(Some(r), cmd);
            let v = match arg(1) {
                "+=" => old + v,
                "-=" => old - v,
                "*=" => old * v,
                "/=" => {
                    if v == 0 {
                        0
                    } else {
                        old / v
                    }
                }
                _ => v,
            };
            // `$700` or `$g_TEMP_0` (a macro for the number)
            let k: i64 = cmd[1..]
                .parse()
                .unwrap_or_else(|_| self.eval(Some(r), &cmd[1..]));
            if cmd.starts_with('$') {
                self.flags.insert(k, v);
            } else {
                self.runs[r].locals.insert(k, v);
            }
            return Ok(());
        }
        match cmd {
            "else" => self.skip_if(r, false),
            "endif" | "keyskip" | "dbgstr" | "deletebgbutton" | "setconf" | "lockactive"
            | "resetactive" | "decode" => {}
            "next" => self.goto(r, arg(1)),
            "call" => {
                let run = &mut self.runs[r];
                if let Some(&pc) = run.fes.labels.get(arg(1)) {
                    run.calls.push(run.pc);
                    run.pc = pc;
                }
            }
            "wait" => {
                let w = if a.len() < 2 {
                    Wait::Motions
                } else if let Ok(n) = arg(1).parse::<u64>() {
                    Wait::Until(self.now + n)
                } else {
                    match self.resolve(r, arg(1)).first() {
                        Some(&i) => Wait::Obj(i),
                        None => Wait::None,
                    }
                };
                self.runs[r].wait = w;
            }
            "exit" => {
                let v = self.eval(Some(r), arg(1));
                self.runs[r].done = true;
                if self.runs[r].story {
                    self.runs[r].exit_code = v;
                } else if r == 0 {
                    self.ended = true;
                } else {
                    self.runs[r - 1].exit_code = v;
                }
                if self.log > 0 {
                    eprintln!("{} exits {v}", self.runs[r].name);
                }
            }
            "execfes" if arg(1).eq_ignore_ascii_case("saveload") => {
                // the game's save screen (SAVEPANEL objects): ours instead, answering the
                // script with 1000 + slot, or -2 for back
                self.end_children(r);
                self.runs[r].exit_code = -1;
                self.open_menu(crate::menu::Mode::Load, Some(r));
            }
            "execfes" => {
                self.end_children(r);
                self.runs[r].exit_code = -1;
                if let Err(e) = self.exec_fes(arg(1)) {
                    eprintln!("{e}");
                    self.runs[r].exit_code = 0;
                }
            }
            "endfes" => self.end_children(r),
            "execkcs" => {
                self.runs[r].exit_code = -1;
                self.start_story(r);
            }
            "endkcs" => self.end_story(),
            "strvar" => {
                let k = self.eval(Some(r), arg(1));
                let fmt = arg(2).to_string();
                let args: Vec<i64> = a[3.min(a.len())..]
                    .iter()
                    .map(|x| self.eval(Some(r), x))
                    .collect();
                self.strs.insert(k, format_c(&fmt, &args));
            }
            "getconf" => {
                let k = self.eval(Some(r), arg(1));
                let v = match arg(2) {
                    "messpd" => 50,
                    _ => 0,
                };
                self.flags.insert(k, v);
            }
            "input_text" | "messagebox" => {
                // Windows dialogs (a name to type, a yes/no box): take the default and OK
                let k = self.eval(Some(r), arg(1));
                self.flags.insert(k, 1);
            }
            "sysvoice" => {
                // no system voices
                let k = self.eval(Some(r), arg(1));
                self.flags.insert(k, -1);
            }
            "baseimg" => {
                self.base = match a.get(1) {
                    Some(n) => {
                        let mut out = Vec::new();
                        for i in self.resolve(r, n) {
                            self.draw_obj(i, None, &mut out);
                        }
                        out
                    }
                    None => Vec::new(),
                };
            }
            _ => {
                let objs = self.resolve(r, cmd);
                if objs.is_empty() {
                    if self.log > 0 {
                        eprintln!("[{}] ? {line}", self.runs[r].name);
                    }
                    return Ok(());
                }
                let sub = arg(1).to_string();
                let vals: Vec<String> = a[2.min(a.len())..].to_vec();
                // the event number of the object, for `btnimg1[@]` in its handlers
                for i in objs {
                    self.obj_command(Some(r), i, &sub, &vals);
                }
            }
        }
        Ok(())
    }

    /// Ends the scripts started by script `r` and removes their objects.
    pub(crate) fn end_children(&mut self, r: usize) {
        while self.runs.len() > r + 1 {
            let c = self.runs.pop().unwrap();
            self.drop_owner(c.id);
        }
    }

    /// An object's command (`disp 1`, `fade 30 0 255`, `pos x y`, ...).
    pub fn obj_command(&mut self, r: Option<usize>, i: usize, sub: &str, vals: &[String]) {
        // a bare `@` in a coordinate is its current value (`pos @+10 0`, `move 8 @-10 0`)
        let cur = self
            .objs
            .get(i)
            .and_then(|o| o.as_ref())
            .map(|o| match sub {
                "pos" => vec![o.pos.0, o.pos.1],
                "move" => vec![0.0, o.pos.0, o.pos.1],
                "base" => vec![o.base.0, o.base.1],
                _ => Vec::new(),
            });
        let ev: Vec<f32> = vals
            .iter()
            .enumerate()
            .map(|(k, v)| match cur.as_ref().and_then(|c| c.get(k)) {
                Some(c) if v.starts_with('@') => {
                    let rest = &v[1..];
                    if rest.is_empty() {
                        *c
                    } else {
                        *c + self.eval(r, &format!("0{rest}")) as f32
                    }
                }
                _ => self.eval(r, v) as f32,
            })
            .collect();
        let n = |k: usize| ev.get(k).copied().unwrap_or(0.0);
        let now = self.now;
        let (cur_frame, getpos) = (sub == "getpos2", sub == "getpos");
        if sub == "getsize" {
            let Some(o) = self.objs.get(i).and_then(|o| o.clone()) else {
                return;
            };
            let (w, h) = self
                .g
                .frame(&o.file, o.id())
                .map_or((0, 0), |f| (f.image.w as i64, f.image.h as i64));
            self.flags.insert(n(0) as i64, w);
            self.flags.insert(n(1) as i64, h);
            return;
        }
        if cur_frame || getpos {
            let Some(o) = self.objs.get(i).and_then(|o| o.clone()) else {
                return;
            };
            let (x, y) = if cur_frame {
                self.g
                    .frame(&o.file, o.id())
                    .map_or(o.pos, |f| (f.x as f32 + o.pos.0, f.y as f32 + o.pos.1))
            } else {
                o.pos
            };
            self.flags.insert(n(0) as i64, x as i64);
            self.flags.insert(n(1) as i64, y as i64);
            return;
        }
        if (sub == "load" || sub == "play" || sub == "stop" || sub == "blend" || sub == "fade")
            && self
                .objs
                .get(i)
                .and_then(|o| o.as_ref())
                .is_some_and(|o| o.kind == Kind::Sound)
        {
            self.sound_command(i, sub, vals, &ev);
            return;
        }
        let Some(o) = self.obj(i) else { return };
        let motion = |m: Move, dur: f32| Motion {
            m,
            t0: now,
            dur: dur.max(0.0) as u64,
            ease: 0,
        };
        match sub {
            "disp" => o.disp = n(0) != 0.0,
            "enable" => o.enable = n(0) != 0.0,
            "pri" => o.pri = n(0) as i32,
            "pos" => o.pos = (n(0), n(1)),
            "base" => o.base = (n(0), n(1)),
            "setid" => o.cur = n(0).max(0.0) as usize,
            "settbl" => {
                let k = n(0).max(0.0) as usize;
                if o.ids.len() <= k {
                    o.ids.resize(k + 1, -1);
                }
                o.ids[k] = n(1) as i64;
            }
            "scale" => o.scale = (n(0) / 100.0, n(1) / 100.0),
            "rot" => o.rot = n(2),
            "stop" => {
                // finish what moves
                let ms = std::mem::take(&mut o.motions);
                for m in ms {
                    apply(o, &m, 1.0);
                }
            }
            "fade" | "fade2" => {
                o.alpha = n(1);
                o.motions.push(motion(Move::Fade(n(1), n(2)), n(0)));
            }
            "move" => {
                let from = o.pos;
                o.motions.push(motion(Move::Pos(from, (n(1), n(2))), n(0)));
            }
            "arc" => {
                let from = o.pos;
                o.motions.push(motion(
                    Move::Arc {
                        a0: n(1),
                        a1: n(2),
                        r: (n(3), n(4)),
                        from,
                    },
                    n(0),
                ));
            }
            "mrot" => {
                let from = o.rot;
                o.motions.push(motion(Move::Rot(from, n(3)), n(0)));
            }
            "mscale" => {
                let from = o.scale;
                o.motions.push(motion(
                    Move::Scale(from, (n(1) / 100.0, n(2) / 100.0)),
                    n(0),
                ));
            }
            "coordmode" => o.center = n(0) != 0.0,
            "draw" => {}
            _ => {
                if self.log > 0 {
                    eprintln!("? object command {sub} {vals:?}");
                }
            }
        }
    }

    fn sound_command(&mut self, i: usize, sub: &str, vals: &[String], ev: &[f32]) {
        let n = |k: usize| ev.get(k).copied().unwrap_or(0.0);
        let channel = format!("fes{i}");
        let Some(o) = self.obj(i) else { return };
        match sub {
            "load" => o.sound = vals.first().cloned().unwrap_or_default(),
            "blend" => o.volume = n(0) / 100.0,
            "play" => {
                let (name, volume) = (o.sound.clone(), o.volume);
                let repeat = n(0) == 0.0;
                self.play(&channel, &name, volume, repeat);
            }
            "stop" => self.stop_sound(&channel, 0),
            "fade" if n(2) <= 0.0 => {
                self.stop_sound(&channel, (n(0) * 1000.0 / 60.0) as u32);
            }
            _ => {}
        }
    }

    /// Plays a sound file (name without extension) on a channel.
    pub fn play(&mut self, channel: &str, name: &str, volume: f32, repeat: bool) {
        let Some(file) = self.g.find(name, &["ogg", "wav"]) else {
            if self.log > 0 {
                eprintln!("sound {name}: not found");
            }
            return;
        };
        let Some(data) = self.g.read(&file) else {
            return;
        };
        self.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data: Some(Rc::new(data)),
            volume,
            repeat,
            fade_ms: 0,
        });
    }

    pub fn stop_sound(&mut self, channel: &str, fade_ms: u32) {
        self.sounds.push(SoundEvent {
            channel: channel.to_string(),
            data: None,
            volume: 0.0,
            repeat: false,
            fade_ms,
        });
    }

    // ---- input

    /// Where a button is on the screen.
    fn button_rect(&mut self, i: usize) -> Option<[f32; 4]> {
        let o = self.objs.get(i)?.clone()?;
        let f = self.g.frame(&o.file, o.id())?;
        let (mut x, mut y) = (o.pos.0 + f.x as f32, o.pos.1 + f.y as f32);
        if let Some(p) = o.plane.and_then(|p| self.objs.get(p)?.as_ref()) {
            x += p.pos.0 - p.base.0;
            y += p.pos.1 - p.base.1;
        }
        Some([x, y, x + f.image.w as f32, y + f.image.h as f32])
    }

    /// The enabled button under the pointer in the topmost script: (script, object).
    fn hover(&mut self) -> Option<(usize, usize)> {
        let (mx, my) = (self.mouse.0 as f32, self.mouse.1 as f32);
        for r in (0..self.runs.len()).rev() {
            if self.runs[r].done {
                continue;
            }
            let mut names: Vec<(String, usize)> = self.runs[r]
                .names
                .iter()
                .map(|(n, &i)| (n.clone(), i))
                .collect();
            names.sort_by_key(|(_, i)| *i);
            let mut best: Option<(i32, usize)> = None;
            for (_, i) in names {
                let Some(o) = self.objs.get(i).and_then(|o| o.as_ref()) else {
                    continue;
                };
                if o.kind != Kind::Button || !o.enable {
                    continue;
                }
                let pri = o.pri;
                if let Some(rc) = self.button_rect(i)
                    && mx >= rc[0]
                    && my >= rc[1]
                    && mx < rc[2]
                    && my < rc[3]
                    && best.is_none_or(|b| pri >= b.0)
                {
                    best = Some((pri, i));
                }
            }
            if let Some((_, i)) = best {
                return Some((r, i));
            }
        }
        None
    }

    /// Runs an event block (`#btn1.FOCUS`) of a script for an object, to its end.
    fn event(&mut self, r: usize, obj: usize, ev: &str) {
        let Some((group, index)) =
            self.runs[r]
                .names
                .iter()
                .find(|(_, i)| **i == obj)
                .map(|(n, _)| match n.split_once('[') {
                    Some((b, k)) => (b.to_string(), k.trim_end_matches(']').parse().unwrap_or(0)),
                    None => (n.clone(), 0),
                })
        else {
            return;
        };
        // the handlers of the object's default class (`TYPE DEFAULT defbtn`), then its own
        let class = self.objs[obj]
            .as_ref()
            .map_or(String::new(), |o| o.class.clone());
        for label in [format!("{class}.{ev}"), format!("{group}.{ev}")] {
            let Some(&start) = self.runs[r].fes.labels.get(&label) else {
                continue;
            };
            if self.log > 0 {
                eprintln!("[{}] event {label} @{index}", self.runs[r].name);
            }
            self.at_obj = Some(obj);
            let jumped = self.handler(r, start, index);
            self.at_obj = None;
            if jumped || r >= self.runs.len() {
                return;
            }
        }
    }

    /// Runs a handler block from `start` to its end; true when it went on to another block.
    fn handler(&mut self, r: usize, start: usize, index: i64) -> bool {
        let saved = (
            self.runs[r].pc,
            self.runs[r].block,
            self.runs[r].calls.clone(),
            self.runs[r].wait,
        );
        self.at = index;
        self.runs[r].pc = start;
        self.runs[r].calls.clear();
        let mut steps = 0;
        let mut jumped = false;
        while steps < 2000 && r < self.runs.len() && !self.runs[r].done {
            steps += 1;
            let run = &self.runs[r];
            let Some(l) = run.fes.lines.get(run.pc).cloned() else {
                break;
            };
            if l.starts_with('#') {
                if let Some(pc) = self.runs[r].calls.pop() {
                    self.runs[r].pc = pc;
                    continue;
                }
                break;
            }
            self.runs[r].pc += 1;
            let block = self.runs[r].block;
            let _ = self.command(r, &l);
            if r >= self.runs.len() {
                return true;
            }
            // waits in handlers do not stop them
            self.runs[r].wait = Wait::None;
            if self.runs[r].block != block {
                jumped = true;
                break;
            }
        }
        if !jumped && r < self.runs.len() {
            let run = &mut self.runs[r];
            (run.pc, run.block, run.calls, run.wait) = saved;
        }
        jumped
    }

    fn input(&mut self) {
        self.click_l = 0;
        self.click_r = 0;
        let clicked = std::mem::take(&mut self.clicked);
        let rclicked = std::mem::take(&mut self.rclicked);
        let wheel_up = std::mem::take(&mut self.wheel_up);
        if self.menu_input(clicked, rclicked, wheel_up) {
            return;
        }
        // the story takes the clicks, unless it waits on a screen script
        if self.adv.as_ref().is_some_and(|a| !a.waits_fes()) {
            self.adv_input(clicked, rclicked);
            return;
        }
        let hover = self.hover();
        // focus changes
        for r in 0..self.runs.len() {
            let now = hover.filter(|h| h.0 == r).map(|h| h.1);
            let old = self.runs[r].focus;
            if old != now {
                self.runs[r].focus = now;
                if let Some(o) = old {
                    self.event(r, o, "UNFOCUS");
                }
                if let Some(o) = now
                    && r < self.runs.len()
                {
                    self.event(r, o, "FOCUS");
                }
            }
            if r >= self.runs.len() {
                break;
            }
        }
        if clicked {
            self.click_l = hover.map_or(-1, |h| h.1 as i64 + 1);
            if let Some((r, o)) = hover {
                self.event(r, o, "PUSH_L");
            }
        }
        if rclicked {
            self.click_r = -1;
        }
    }

    // ---- frames

    /// Runs one frame (the scripts count on 60 a second).
    pub fn frame(&mut self) -> Result<(), String> {
        if self.ended {
            return Ok(());
        }
        self.now += 1;
        if self.adv.is_some() {
            let shown = self.story_draws();
            self.adv.as_mut().unwrap().prev = shown;
        }
        self.input();
        self.step_motions();
        self.run_scripts()?;
        if self.adv.is_some() {
            self.run_story()?;
        }
        Ok(())
    }

    fn step_motions(&mut self) {
        let now = self.now;
        for o in self.objs.iter_mut().flatten() {
            if o.motions.is_empty() {
                continue;
            }
            let ms = std::mem::take(&mut o.motions);
            let mut keep = Vec::new();
            for m in ms {
                let t = if m.dur == 0 {
                    1.0
                } else {
                    ((now.saturating_sub(m.t0)) as f32 / m.dur as f32).min(1.0)
                };
                apply(o, &m, ease(t, m.ease));
                if t < 1.0 {
                    keep.push(m);
                }
            }
            o.motions = keep;
        }
    }

    // ---- drawing

    pub(crate) fn color_image(&mut self, c: [u8; 3]) -> Rc<Image> {
        self.colors
            .entry(c)
            .or_insert_with(|| {
                let mut i = Image::new(1, 1);
                i.fill([c[0], c[1], c[2], 255]);
                Rc::new(i)
            })
            .clone()
    }

    /// The draws of an object (and of a plane's pictures), with its plane's place.
    fn draw_obj(&mut self, i: usize, parent: Option<(Place, (f32, f32))>, out: &mut Vec<Draw>) {
        let Some(o) = self.objs.get(i).and_then(|o| o.clone()) else {
            return;
        };
        if !o.disp || o.alpha <= 0.0 {
            return;
        }
        let full = [0.0, 0.0, self.width as f32, self.height as f32];
        // (place, offset added to positions)
        let (mut place, off) = parent.unwrap_or((
            Place {
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
                clip: full,
            },
            (0.0, 0.0),
        ));
        place.alpha *= o.alpha / 255.0;
        place.add |= o.add;
        match o.kind {
            Kind::Plane => {
                let (mut p, mut off) = (place, off);
                if parent.is_none() {
                    // screen = pos + R S (P - base)
                    p.cx = o.pos.0;
                    p.cy = o.pos.1;
                    p.sx = o.scale.0;
                    p.sy = o.scale.1;
                    p.rot = o.rot.to_radians();
                    off = (o.pos.0 - o.base.0, o.pos.1 - o.base.1);
                } else {
                    off = (off.0 + o.pos.0 - o.base.0, off.1 + o.pos.1 - o.base.1);
                }
                let mut kids: Vec<(i32, usize)> = self
                    .objs
                    .iter()
                    .enumerate()
                    .filter_map(|(k, c)| {
                        c.as_ref()
                            .filter(|c| c.plane == Some(i))
                            .map(|c| (c.pri, k))
                    })
                    .collect();
                kids.sort();
                for (_, k) in kids {
                    self.draw_obj(k, Some((p, off)), out);
                }
            }
            Kind::Color => {
                let img = self.color_image(o.color);
                let (x, y) = (o.pos.0 + off.0, o.pos.1 + off.1);
                out.push(Draw {
                    image: img,
                    src: [0, 0, 1, 1],
                    dst: [x, y, x + o.size.0, y + o.size.1],
                    place,
                });
            }
            Kind::Image | Kind::Button => {
                // a button with one picture is a hit area under separate images (春風); one
                // with a picture per state shows them
                if o.kind == Kind::Button && o.ids.len() < 2 {
                    return;
                }
                let files: Vec<(String, i64)> = if o.parts.is_empty() {
                    vec![(o.file.clone(), o.id())]
                } else {
                    o.parts.iter().map(|p| (p.clone(), -1)).collect()
                };
                for (file, id) in files {
                    let Some(f) = self.g.frame(&file, id) else {
                        continue;
                    };
                    let (mut x, mut y) =
                        (o.pos.0 + off.0 + f.x as f32, o.pos.1 + off.1 + f.y as f32);
                    if o.anchor {
                        x -= f.base_x as f32;
                        y -= f.base_y as f32;
                    }
                    let (mut w, mut h) = (f.image.w as f32, f.image.h as f32);
                    // a picture's own scale and turn keep its position (base point) in place
                    let (mut ax, mut ay) = (o.pos.0 + off.0, o.pos.1 + off.1);
                    if o.center {
                        ax += f.base_x as f32;
                        ay += f.base_y as f32;
                    }
                    let mut place = place;
                    if o.rot != 0.0 && place.rot == 0.0 && (place.sx, place.sy) == (1.0, 1.0) {
                        place.cx = ax;
                        place.cy = ay;
                        place.rot = o.rot.to_radians();
                    }
                    if o.scale != (1.0, 1.0) {
                        x = ax + (x - ax) * o.scale.0;
                        y = ay + (y - ay) * o.scale.1;
                        w *= o.scale.0;
                        h *= o.scale.1;
                    }
                    out.push(Draw {
                        src: [0, 0, f.image.w as i32, f.image.h as i32],
                        image: f.image,
                        dst: [x, y, x + w, y + h],
                        place,
                    });
                }
            }
            _ => {}
        }
    }

    /// Draws of the top-level objects of some owners (all when `adv` is None; else only the
    /// story's or only the others').
    pub fn draw_objs(&mut self, adv: Option<(bool, u32)>, out: &mut Vec<Draw>) {
        let mut top: Vec<(i32, usize)> = self
            .objs
            .iter()
            .enumerate()
            .filter_map(|(k, o)| {
                let o = o.as_ref()?;
                if o.plane.is_some() {
                    return None;
                }
                if let Some((mine, owner)) = adv
                    && (o.owner == owner) != mine
                {
                    return None;
                }
                Some((o.pri, k))
            })
            .collect();
        top.sort();
        for (_, k) in top {
            self.draw_obj(k, None, out);
        }
    }

    /// The pictures of the screen, bottom first.
    pub fn draws(&mut self) -> Vec<Draw> {
        let mut out = self.base.clone();
        let story = self.adv.as_ref().map(|a| a.owner);
        match story {
            Some(owner) => {
                let story = self.story_draws();
                out.extend(story);
                self.draw_objs(Some((false, owner)), &mut out);
                self.window_draws(&mut out);
            }
            None => self.draw_objs(None, &mut out),
        }
        self.menu_draws(&mut out);
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
}

/// Moves an object `t` (0..=1) of the way through a motion.
pub fn apply(o: &mut Obj, m: &Motion, t: f32) {
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    match m.m {
        Move::Fade(a, b) => o.alpha = lerp(a, b),
        Move::Pos(a, b) => o.pos = (lerp(a.0, b.0), lerp(a.1, b.1)),
        Move::Arc { a0, a1, r, from } => {
            let at = |deg: f32| {
                let rad = deg.to_radians();
                (r.0 * rad.cos(), -r.1 * rad.sin())
            };
            let (s, c) = (at(a0), at(lerp(a0, a1)));
            o.pos = (from.0 + c.0 - s.0, from.1 + c.1 - s.1);
        }
        Move::Scale(a, b) => o.scale = (lerp(a.0, b.0), lerp(a.1, b.1)),
        Move::Rot(a, b) => o.rot = lerp(a, b),
        Move::Idle => {}
    }
}

/// printf-style formatting of integers: `se_sys_%02d`, `%03d`, `%d`.
pub fn format_c(fmt: &str, args: &[i64]) -> String {
    let mut out = String::new();
    let mut it = fmt.chars().peekable();
    let mut k = 0;
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&d) = it.peek() {
            it.next();
            if d.is_ascii_alphabetic() {
                spec.push(d);
                break;
            }
            spec.push(d);
        }
        if spec == "%" {
            out.push('%');
            continue;
        }
        let v = args.get(k).copied().unwrap_or(0);
        k += 1;
        let width: usize = spec
            .trim_end_matches(|c: char| c.is_ascii_alphabetic())
            .parse()
            .unwrap_or(0);
        let zero = spec.starts_with('0');
        let s = v.to_string();
        if s.len() < width {
            let pad = if zero { '0' } else { ' ' };
            out.extend(std::iter::repeat_n(pad, width - s.len()));
        }
        out.push_str(&s);
    }
    out
}
