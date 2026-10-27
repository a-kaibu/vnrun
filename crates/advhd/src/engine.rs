//! Running a game: the scenario scripts (.ws2) one command after another, the Lua interface
//! called the way AdvHD.exe calls it, and the screen 60 times a second.
//!
//! The scenario's layers are the set named "" (drawn first); times in scripts are seconds.

use crate::exe::Formats;
use crate::files::Files;
use crate::host::{self, St};
use crate::layers::{Content, Effect, Snap};
use crate::state::State;
use crate::ws2::{Arg, Ins, Script};
use luavm::{Lua, Value};
use nexas::gfx::Draw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub const FRAME_MS: f64 = 1000.0 / 60.0;
const SCENE: &str = "";
/// ms per character of message text
const CHAR_MS: f64 = 30.0;

#[derive(Debug, Clone, PartialEq)]
enum Wait {
    None,
    Timer(String),
    Transition,
    Effects(String),
    /// message text coming out
    Typing,
    Click,
    /// choices on screen until the interface reports one (LegacyGame__lua_SelectItem)
    Select(Vec<Arg>),
    /// a menu the scenario opened (0x1c, the function said true) until its sets are gone
    Menu(Vec<String>),
}

pub struct Engine {
    st: St,
    lua: Lua,
    fm: Formats,
    scripts: HashMap<String, Rc<Script>>,
    cur: Rc<Script>,
    pc: usize,
    stack: Vec<(Rc<Script>, usize)>,
    wait: Wait,
    timers: HashMap<String, f64>,
    clicks: Vec<(f32, f32, bool)>,
    moved: Option<(f32, f32)>,
    /// where message text goes (getMessageWindowKey) and since when it comes out
    msg: Option<(String, String, f64)>,
    window_open: bool,
    order: Vec<String>,
    ended: bool,
    trace: bool,
    /// the choice the interface reported
    chosen: Option<i64>,
}

fn lua_str(v: &Value) -> String {
    v.as_bytes()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default()
}

impl Engine {
    pub fn new(files: Box<dyn Files>) -> Result<Engine, String> {
        let st: St = Rc::new(RefCell::new(State::new(files, 1280, 720)));
        let exe = {
            let mut s = st.borrow_mut();
            let name = s
                .files
                .list()
                .into_iter()
                .find(|n| n.eq_ignore_ascii_case("advhd.exe"));
            name.and_then(|n| s.files.read(&n)).ok_or("no AdvHD.exe")?
        };
        let fm = crate::exe::formats(&exe);
        st.borrow_mut().screen.add(SCENE);
        let mut lua = Lua::new();
        host::install(&mut lua, &st);
        let mut e = Engine {
            st,
            lua,
            fm,
            scripts: HashMap::new(),
            cur: Rc::new(Script::new("", &[])),
            pc: 0,
            stack: Vec::new(),
            wait: Wait::None,
            timers: HashMap::new(),
            clicks: Vec::new(),
            moved: None,
            msg: None,
            window_open: false,
            order: Vec::new(),
            ended: false,
            trace: false,
            chosen: None,
        };
        e.init_lua();
        e.cur = e.script("start").ok_or("no start.ws2")?;
        Ok(e)
    }

    /// Loads the interface: LegacyGame.lua, then what LegacyGame.inc includes, then asks it to
    /// set itself up.
    fn init_lua(&mut self) {
        let read = |st: &St, n: &str| st.borrow_mut().read(n);
        for n in [
            "VersionInfo.lua",
            "GameInfo.lua",
            "VariableSize.lua",
            "LegacyGame.lua",
        ] {
            if let Some(c) = read(&self.st, n)
                && let Err(e) = self.lua.exec(&c, n)
            {
                eprintln!("advhd: {n}: {e}");
            }
        }
        let inc = read(&self.st, "LegacyGame.inc").unwrap_or_default();
        for line in String::from_utf8_lossy(&inc).lines() {
            if let Some(name) = line
                .trim()
                .strip_prefix("include")
                .map(|r| r.trim().trim_matches('"').to_string())
                && !name.is_empty()
            {
                let file = format!("{name}.lua");
                match read(&self.st, &file) {
                    Some(c) => {
                        if let Err(e) = self.lua.exec(&c, &file) {
                            eprintln!("advhd: {file}: {e}");
                        }
                    }
                    None => eprintln!("advhd: no {file}"),
                }
            }
        }
        // getAppInfo: (?, width, height, name, ...)
        let info = self.call("getAppInfo", vec![]);
        let size = |i: usize| {
            info.get(i)
                .and_then(|v| v.to_num())
                .filter(|&n| n >= 1.0)
                .map(|n| n as u32)
        };
        if let (Some(w), Some(h)) = (size(1), size(2)) {
            let mut s = self.st.borrow_mut();
            s.screen.w = w;
            s.screen.h = h;
        }
        self.call("checkInitVersion", vec![]);
        self.call("initLegacyGame", vec![]);
        self.call("initSystemScreen", vec![]);
    }

    /// Calls a global Lua function, reporting errors; nothing when it doesn't exist.
    fn set_names(&self) -> Vec<String> {
        self.st
            .borrow()
            .screen
            .sets
            .iter()
            .map(|x| x.name.clone())
            .collect()
    }

    fn call(&mut self, name: &str, args: Vec<Value>) -> Vec<Value> {
        let f = self.lua.global(name);
        if f.is_nil() {
            return vec![];
        }
        match self.lua.call(&f, args) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("advhd: Lua {name}: {e}");
                vec![]
            }
        }
    }

    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())?;
        self.st.borrow_mut().font = Some(Rc::new(f));
        Ok(())
    }

    /// Seconds since 1970, for Lua's os.time where the engine can't tell (the browser).
    pub fn set_time(&mut self, secs: f64) {
        self.lua.epoch = secs - self.lua.clock;
    }

    pub fn set_trace(&mut self, on: bool) {
        self.trace = on;
        self.lua.trace = on && std::env::var_os("ADVHD_LUA").is_some();
    }

    /// Whether a text field waits for the player's text (a name to enter).
    /// The sounds to start and stop since the last call.
    pub fn take_sounds(&mut self) -> Vec<crate::state::Sound> {
        std::mem::take(&mut self.st.borrow_mut().sounds)
    }

    pub fn input_wanted(&self) -> bool {
        self.st.borrow().input.is_some()
    }

    /// Gives the waiting text field its text, as if typed and confirmed.
    pub fn enter_text(&mut self, text: &str) {
        let Some(set) = self.st.borrow_mut().input.take() else {
            return;
        };
        // the last argument says the entry was cancelled
        self.call(
            "onIMEnd",
            vec![Value::str(&set), Value::str(text), Value::Bool(false)],
        );
    }

    /// Starts the scenario over at the top of a script (for trying out a part of the game).
    pub fn goto(&mut self, script: &str) -> Result<(), String> {
        self.stack.clear();
        self.wait = Wait::None;
        self.jump_file(script)
    }

    pub fn width(&self) -> u32 {
        self.st.borrow().screen.w
    }

    pub fn height(&self) -> u32 {
        self.st.borrow().screen.h
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn click(&mut self, x: f32, y: f32) {
        self.clicks.push((x, y, false));
    }

    pub fn right_click(&mut self, x: f32, y: f32) {
        self.clicks.push((x, y, true));
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) {
        self.moved = Some((x, y));
    }

    /// Where the scenario is and what it waits for.
    pub fn describe(&self) -> String {
        let s = self.st.borrow();
        let sets: Vec<String> = s
            .screen
            .sets
            .iter()
            .map(|x| {
                let ls: Vec<&str> = x
                    .layers
                    .iter()
                    .filter(|l| l.visible)
                    .map(|l| l.name.as_str())
                    .collect();
                format!(
                    "[{}{}: {}]",
                    x.name,
                    if x.snap.is_some() { " snap" } else { "" },
                    ls.join(" ")
                )
            })
            .collect();
        format!(
            "{}:{:#x} wait {}{} {}",
            self.cur.name,
            self.pc,
            match &self.wait {
                Wait::Select(c) => format!("Select({} choices)", c.len()),
                w => format!("{w:?}"),
            },
            if s.paused { " (paused by Lua)" } else { "" },
            sets.join(" ")
        )
    }

    pub fn draws(&self) -> Vec<Draw> {
        let s = self.st.borrow();
        s.screen.draws(s.now, None)
    }

    /// The screen as one picture.
    pub fn render(&self) -> nexas::gfx::Image {
        let s = self.st.borrow();
        s.screen.render(s.now, None)
    }

    fn script(&mut self, name: &str) -> Option<Rc<Script>> {
        let key = name.to_lowercase();
        let key = if key.ends_with(".ws2") {
            key
        } else {
            format!("{key}.ws2")
        };
        if let Some(s) = self.scripts.get(&key) {
            return Some(s.clone());
        }
        let raw = self.st.borrow_mut().read(&key)?;
        let s = Rc::new(Script::new(&key, &raw));
        self.scripts.insert(key, s.clone());
        Some(s)
    }

    /// Runs one frame.
    pub fn frame(&mut self) -> Result<(), String> {
        let now = {
            let mut s = self.st.borrow_mut();
            s.now += FRAME_MS;
            let now = s.now;
            s.screen.step(now);
            now
        };
        self.lua.clock = now / 1000.0;
        // the interface's effects move on when asked whether they are running
        // the interface finishes its effects when asked about them
        self.call("isSystemEffect", vec![]);
        self.call("isMessageWindowEffect", vec![]);
        self.input();
        self.requests();
        let mut n = 0;
        while !self.ended && self.ready(now) {
            self.step()?;
            n += 1;
            if n > 10000 {
                return Err(format!(
                    "{}:{:#x}: the script runs without waiting",
                    self.cur.name, self.pc
                ));
            }
        }
        self.st.borrow_mut().draw_texts();
        Ok(())
    }

    /// The set taking the mouse, and what is under it there.
    fn hit(&mut self, x: f32, y: f32) -> (String, Value, i64) {
        let set = self
            .call("getSystemSet", vec![])
            .first()
            .map(lua_str)
            .unwrap_or_else(|| "system".into());
        let s = self.st.borrow();
        let h = s
            .screen
            .sets
            .iter()
            .find(|s| s.name == set)
            .and_then(|s| s.hit(x, y));
        let (n, layer) = h.map_or((-1, Value::Nil), |(n, l)| (n as i64, Value::str(l)));
        drop(s);
        (set, layer, n)
    }

    fn mouse_args(&mut self, x: f32, y: f32) -> Vec<Value> {
        let (set, layer, n) = self.hit(x, y);
        vec![
            Value::str(&set),
            layer,
            Value::Int(n),
            Value::Num(x as f64),
            Value::Num(y as f64),
            Value::Int(0),
        ]
    }

    fn input(&mut self) {
        if let Some((x, y)) = self.moved.take() {
            self.st.borrow_mut().cursor = (x, y);
            let a = self.mouse_args(x, y);
            self.call("onMouseMove", a);
        }
        for (x, y, right) in std::mem::take(&mut self.clicks) {
            let paused = self.st.borrow().paused;
            if x >= 0.0 {
                self.st.borrow_mut().cursor = (x, y);
                let a = self.mouse_args(x, y);
                if self.trace {
                    eprintln!("click {x},{y}: {:?}", &a[..3]);
                }
                self.call("onMouseMove", a.clone());
                if right {
                    self.call("onRButtonDown", a.clone());
                    self.call("onRButtonUp", a);
                } else {
                    self.call("onLButtonDown", a.clone());
                    self.call("onLButtonUp", a);
                }
            }
            if paused || right {
                continue;
            }
            match self.wait {
                Wait::Typing => {
                    self.show_all();
                    self.wait = Wait::Click;
                    self.call("startClickWaitAnimation", vec![]);
                }
                Wait::Click => {
                    self.call("stopClickWaitAnimation", vec![]);
                    self.wait = Wait::None;
                }
                Wait::Timer(_) => self.wait = Wait::None,
                _ => {}
            }
        }
    }

    /// Acts on what the interface asked for.
    fn requests(&mut self) {
        let reqs = std::mem::take(&mut self.st.borrow_mut().requests);
        for (k, v) in reqs {
            match k {
                6 => {
                    // back to the title
                    self.stack.clear();
                    if let Some(s) = self.script("mainmenu") {
                        self.cur = s;
                        self.pc = 0;
                        self.wait = Wait::None;
                    }
                }
                2 => self.ended = true,
                -1 => self.chosen = Some(v),
                _ => self
                    .st
                    .borrow_mut()
                    .unknown(&format!("NeedProcess {k} {v}")),
            }
        }
    }

    fn show_all(&mut self) {
        if let Some((set, layer, _)) = &self.msg {
            let key = (set.clone(), layer.clone());
            let mut s = self.st.borrow_mut();
            if let Some(t) = s.texts.get_mut(&key) {
                t.shown = t.chars.len();
            }
        }
    }

    /// Whether the scenario may go on.
    fn ready(&mut self, now: f64) -> bool {
        if self.st.borrow().paused {
            return false;
        }
        match self.wait.clone() {
            Wait::None => true,
            Wait::Timer(n) => {
                if self.timers.get(&n).is_none_or(|&t| now >= t) {
                    self.wait = Wait::None;
                    true
                } else {
                    false
                }
            }
            Wait::Transition => {
                let busy = self
                    .st
                    .borrow_mut()
                    .screen
                    .set(SCENE)
                    .is_some_and(|s| s.fading());
                if !busy {
                    self.wait = Wait::None;
                }
                !busy
            }
            Wait::Effects(l) => {
                let busy = self
                    .st
                    .borrow_mut()
                    .screen
                    .layer(SCENE, &l)
                    .is_some_and(|l| l.busy());
                if !busy {
                    self.wait = Wait::None;
                }
                !busy
            }
            Wait::Typing => {
                let Some((set, layer, start)) = self.msg.clone() else {
                    self.wait = Wait::None;
                    return true;
                };
                let n = ((now - start) / CHAR_MS).max(0.0) as usize;
                let done = {
                    let mut s = self.st.borrow_mut();
                    match s.texts.get_mut(&(set, layer)) {
                        Some(t) => {
                            t.shown = n.min(t.chars.len());
                            t.shown == t.chars.len()
                        }
                        None => true,
                    }
                };
                if done {
                    self.wait = Wait::Click;
                    self.call("startClickWaitAnimation", vec![]);
                }
                false
            }
            Wait::Click => false,
            Wait::Menu(sets) => {
                let open = self.set_names();
                let done = !sets.iter().any(|n| open.contains(n));
                if done {
                    self.wait = Wait::None;
                }
                done
            }
            Wait::Select(c) => {
                // asked every frame: the interface finishes its effects here
                let busy = self
                    .call("isSelectEffect", vec![])
                    .first()
                    .is_some_and(|v| v.truthy());
                let Some(i) = self.chosen.filter(|_| !busy) else {
                    return false;
                };
                self.chosen = None;
                self.wait = Wait::None;
                match c.get(i as usize) {
                    Some(Arg::List(c)) => self.take_choice(c),
                    _ => eprintln!("advhd: no choice {i}"),
                }
                true
            }
        }
    }

    /// Goes where a choice of command 0x0f leads: [id, text, ?, ?, ?, kind, target].
    fn take_choice(&mut self, c: &[Arg]) {
        match c.get(5).map(|x| x.int()) {
            Some(6) => self.pc = c[6].int() as usize,
            Some(7) => {
                if let Err(e) = self.jump_file(c[6].text()) {
                    eprintln!("advhd: {e}");
                }
            }
            _ => {}
        }
    }

    fn jump_file(&mut self, name: &str) -> Result<(), String> {
        self.cur = self
            .script(name)
            .ok_or_else(|| format!("no script {name}"))?;
        self.pc = 0;
        Ok(())
    }

    fn step(&mut self) -> Result<(), String> {
        if self.pc >= self.cur.code.len() || self.cur.code[self.pc] == 0xff {
            return self.ret();
        }
        let ins = self.cur.read(&self.fm, self.pc)?;
        if self.trace {
            eprintln!(
                "{}:{:06x} {:02x} {:?}",
                self.cur.name, ins.at, ins.op, ins.args
            );
        }
        self.pc = ins.next;
        let ins = self.resolve(ins);
        self.exec(&ins)
    }

    /// Puts string variables' values in for "@NAME" strings and for <@NAME> inside text, but not
    /// where a command names the variable it sets (the first string of 0x6e, 0x73 and 0x75).
    fn resolve(&self, mut ins: Ins) -> Ins {
        let st = self.st.borrow();
        if st.strings.is_empty() {
            return ins;
        }
        let skip = matches!(ins.op, 0x6e | 0x73 | 0x75) as usize;
        for a in ins.args.iter_mut().skip(skip) {
            if let Arg::Str(s) = a {
                if let Some(v) = st.strings.get(s.as_str()) {
                    *s = v.clone();
                } else if s.contains("<@") {
                    *s = embed(s, &st.strings);
                }
            }
        }
        ins
    }

    fn ret(&mut self) -> Result<(), String> {
        match self.stack.pop() {
            Some((s, pc)) => {
                self.cur = s;
                self.pc = pc;
            }
            None => self.ended = true,
        }
        Ok(())
    }

    fn exec(&mut self, i: &Ins) -> Result<(), String> {
        let a = |k: usize| i.arg(k);
        let now = self.st.borrow().now;
        match i.op {
            0x00 => {}
            0x01 => {
                if !self.condition(i) {
                    let t = a(3).int();
                    self.pc = if t != 0 {
                        t as usize
                    } else {
                        a(4).int() as usize
                    };
                }
            }
            0x02 | 0x06 => self.pc = a(0).int() as usize,
            0x04 => {
                let back = (self.cur.clone(), self.pc);
                self.jump_file(a(0).text())?;
                self.stack.push(back);
            }
            0x05 => self.ret()?,
            0x07 => self.jump_file(a(0).text())?,
            0x08 => self.ended = true,
            0x09 | 0x0a => {
                // mode (bit 7: the value is a variable), variable, value
                let (mode, var) = if i.op == 0x09 {
                    (a(0).int(), a(1).int())
                } else {
                    (0x7f, a(0).int())
                };
                let raw = if i.op == 0x09 { a(2).num() } else { a(1).num() };
                let mut s = self.st.borrow_mut();
                let v = if mode & 0x80 != 0 && i.op == 0x09 {
                    s.var(raw as i64)
                } else {
                    raw
                };
                let x = s.var(var);
                let r = match (i.op, mode & 0x7f) {
                    (0x0a, _) => {
                        if v == 0.0 {
                            x
                        } else {
                            x % v
                        }
                    }
                    (_, 0) => v,
                    (_, 1) => x + v,
                    (_, 2) => x - v,
                    (_, 3) => x * v,
                    (_, 4) => {
                        if v == 0.0 {
                            x
                        } else {
                            x / v
                        }
                    }
                    _ => x,
                };
                s.set_var(var, r);
            }
            0x0b => self.st.borrow_mut().set_flag(a(0).int(), a(1).int() != 0),
            0x0c => {
                let mut s = self.st.borrow_mut();
                let want = a(1).int() != 0;
                let all = match a(2) {
                    Arg::List(l) => l.iter().all(|f| s.flag(f.int()) == want),
                    _ => true,
                };
                s.set_flag(a(0).int(), all);
            }
            0x0d => {
                let mut s = self.st.borrow_mut();
                for k in 0..a(1).int().max(0) {
                    s.set_var(a(0).int() + k, a(2).num());
                }
            }
            0x0e => {
                let mut s = self.st.borrow_mut();
                for k in 0..a(1).int().max(0) {
                    s.set_flag(a(0).int() + k, a(2).int() != 0);
                }
            }
            0x0f => {
                let n = i.args.len();
                self.call("initSelect", vec![Value::Int(n as i64)]);
                for (k, c) in i.args.iter().enumerate() {
                    let key = self.call("getSelectTextKey", vec![Value::Int(k as i64)]);
                    let (set, layer) = (
                        key.first().map(lua_str).unwrap_or_default(),
                        key.get(1).map(lua_str).unwrap_or_default(),
                    );
                    let text = match c {
                        Arg::List(c) => c.get(1).map(|t| t.text().to_string()).unwrap_or_default(),
                        _ => String::new(),
                    };
                    self.st.borrow_mut().set_text(&set, &layer, &text, None);
                    self.call(
                        "setSelectTextColor",
                        vec![Value::Int(k as i64), Value::Int(0)],
                    );
                }
                self.chosen = None;
                self.wait = Wait::Select(i.args.clone());
            }
            0x11 => {
                self.timers
                    .insert(a(0).text().to_string(), now + a(2).num() as f64 * 1000.0);
            }
            0x12 => self.wait = Wait::Timer(a(0).text().to_string()),
            0x14 => self.message(a(2).text()),
            0x15 => {
                let key = self.call("getNameWindowKey", vec![]);
                let (set, layer) = (
                    key.first().map(lua_str).unwrap_or_default(),
                    key.get(1).map(lua_str).unwrap_or_default(),
                );
                let name = a(0).text().to_string();
                self.st.borrow_mut().set_text(&set, &layer, &name, None);
                let shown: String = crate::text::plain(&name).into_iter().collect();
                self.call("nameWindowVisible", vec![Value::Bool(!shown.is_empty())]);
            }
            0x16 => {
                if a(0).int() == 0 && self.window_open {
                    self.call("closeMessageWindow", vec![Value::Bool(true)]);
                    self.window_open = false;
                }
                // Early builds (yobai 2013) open the window on a scene frozen by 0x64 and never
                // fade it in with 0x65: opening the window shows the scene as it is.
                if a(0).int() == 1 {
                    let mut s = self.st.borrow_mut();
                    if let Some(set) = s.screen.set(SCENE)
                        && set.snap.as_ref().is_some_and(|p| p.fade.is_none())
                    {
                        set.snap = None;
                    }
                }
            }
            0x1a => {
                self.call(a(0).text(), vec![]);
            }
            0x1c => {
                let args = vec![
                    Value::str(a(1).text()),
                    Value::Int(a(2).int()),
                    Value::Int(a(3).int()),
                ];
                let before = self.set_names();
                let r = self.call(a(0).text(), args);
                // a menu (openCharSelect, openGallery) returns true and the scenario waits
                // until it closes, which is when the sets it made are removed
                if r.first().is_some_and(|v| v.truthy()) {
                    let new: Vec<String> = self
                        .set_names()
                        .into_iter()
                        .filter(|n| !before.contains(n))
                        .collect();
                    if !new.is_empty() {
                        self.wait = Wait::Menu(new);
                    }
                }
            }
            0x33 | 0x34 => {
                let mut s = self.st.borrow_mut();
                let l = s.picture_layer(a(0).text(), a(1).text());
                s.screen.add(SCENE).insert(l);
                drop(s);
                self.reorder();
            }
            0x37 => {
                let mut s = self.st.borrow_mut();
                let set = s.screen.add(SCENE);
                if a(0).text() == "*" {
                    set.layers.clear();
                } else {
                    set.remove(a(0).text());
                }
            }
            0x39 => {
                let ids: Vec<i64> = match a(3) {
                    Arg::List(l) => l.iter().map(|x| x.int()).collect(),
                    _ => vec![],
                };
                if let Some(l) = self.st.borrow_mut().screen.layer(SCENE, a(0).text()) {
                    for sub in &mut l.subs {
                        sub.visible = ids.contains(&(sub.id as i64));
                    }
                }
            }
            0x3f => {
                if let Arg::List(l) = a(0) {
                    self.order = l.iter().map(|x| x.text().to_lowercase()).collect();
                }
                self.reorder();
            }
            0x45 => {
                // layer, parameter, 4 variables to put it in
                let mut s = self.st.borrow_mut();
                let v = s
                    .screen
                    .layer(SCENE, a(0).text())
                    .map_or([0.0; 4], |l| l.p.get(a(1).int() as i32));
                for (k, x) in v.into_iter().enumerate() {
                    s.set_var(a(2 + k).num() as i64, x);
                }
            }
            0x46 => {
                // layer, parameter, mask (bit 7 down: that value is a variable), 4 values
                let mut s = self.st.borrow_mut();
                let v = self.values(&s, i, 3, a(2).int(), 4);
                if let Some(l) = s.screen.layer(SCENE, a(0).text()) {
                    l.p.set(a(1).int() as i32, [v[0], v[1], v[2], v[3]]);
                }
            }
            0x47 => {
                // layer, effect, kind, ease, mask, 4 values, seconds, ...
                let mut s = self.st.borrow_mut();
                let v = self.values(&s, i, 5, a(4).int(), 5);
                let e = Effect::new(
                    a(1).text(),
                    a(2).int() as i32,
                    [v[0], v[1], v[2], v[3]],
                    a(3).int() as i32,
                    v[4] as f64 * 1000.0,
                    0.0,
                );
                if let Some(l) = s.screen.layer(SCENE, a(0).text()) {
                    l.effects.push(e);
                }
            }
            0x48 => {
                // layer, effect, repeats, wait (0: no, 1: a click ends it, 2: yes), ...
                if let Some(l) = self.st.borrow_mut().screen.layer(SCENE, a(0).text()) {
                    l.start_effect(a(1).text(), now);
                }
                if a(3).int() != 0 {
                    self.wait = Wait::Effects(a(0).text().to_string());
                }
            }
            0x57 => {
                // forgets the layer's effects that were set and not started
                if let Some(l) = self.st.borrow_mut().screen.layer(SCENE, a(0).text()) {
                    l.effects.retain(|e| e.start.is_some());
                }
            }
            0x64 => {
                if a(0).int() < 0xfe {
                    let mut s = self.st.borrow_mut();
                    let image = Rc::new(s.screen.render(now, Some(SCENE)));
                    s.screen.add(SCENE).snap = Some(Snap {
                        image,
                        fade: None,
                        under: false,
                    });
                }
            }
            0x65 => {
                let len = a(2).num() as f64 * 1000.0;
                let mut s = self.st.borrow_mut();
                let set = s.screen.add(SCENE);
                if len <= 0.0 {
                    set.snap = None;
                } else if let Some(p) = &mut set.snap {
                    p.fade = Some((now, len));
                    if a(4).int() != 0 {
                        self.wait = Wait::Transition;
                    }
                }
            }
            // play on a channel: channel, file, fade in (s), ?, loops (65535: for ever), volume
            // (of 10), ...; 0x1e music, 0x28 sound effects and voices
            0x1e | 0x28 => {
                let looped = a(4).int() != 0;
                let volume = a(5).int() as f32 / 10.0;
                let fade = a(2).num() as f64 * 1000.0;
                self.st
                    .borrow_mut()
                    .play(a(0).text(), a(1).text(), looped, volume, fade);
            }
            // stop a channel ("*": all): channel, fade out (s)
            0x1f | 0x29 => {
                let fade = a(1).num() as f64 * 1000.0;
                self.st.borrow_mut().stop(a(0).text(), fade);
            }
            0x20 | 0x2a | 0x30 => {} // volume changes and the like: not yet
            0x6e => {
                if a(0).text().starts_with('@') {
                    self.st
                        .borrow_mut()
                        .strings
                        .insert(a(0).text().to_string(), a(1).text().to_string());
                }
            }
            0x73 => {
                // flag = (string variable == text)
                let same = self
                    .st
                    .borrow()
                    .strings
                    .get(a(0).text())
                    .map_or("", String::as_str)
                    .eq_ignore_ascii_case(a(1).text());
                self.st.borrow_mut().set_flag(a(2).int(), same);
            }
            0x75 => {
                // string variable = the picture file of a scene layer
                let file = self
                    .st
                    .borrow_mut()
                    .screen
                    .layer(SCENE, a(1).text())
                    .map(|l| l.file.clone())
                    .unwrap_or_default();
                self.st
                    .borrow_mut()
                    .strings
                    .insert(a(0).text().to_string(), file);
            }
            0x2e | 0x13 | 0x17 | 0x18 | 0x1b | 0x32 | 0x38 | 0x53 | 0x66 | 0x68 | 0x6f | 0x71
            | 0xfb | 0xfc | 0xfd => {}
            op => self.st.borrow_mut().unknown(&format!("command {op:#04x}")),
        }
        Ok(())
    }

    /// `n` float arguments from `at`, those marked in `mask` (0x80 first) read as variables.
    fn values(&self, s: &State, i: &Ins, at: usize, mask: i64, n: usize) -> Vec<f32> {
        (0..n)
            .map(|k| {
                let v = i.arg(at + k).num();
                if mask & (0x80 >> k) != 0 {
                    s.var(v as i64)
                } else {
                    v
                }
            })
            .collect()
    }

    /// Command 0x01: kind (bit 7: a variable, else a flag; bit 6: the value is a variable or
    /// flag too; low bits: 0 >=, 1 <=, 2 ==, 3 !=, 4 >, 5 <), which, value.
    fn condition(&self, i: &Ins) -> bool {
        let s = self.st.borrow();
        let kind = i.arg(0).int();
        let which = i.arg(1).int();
        let raw = i.arg(2).num();
        let (x, v) = if kind & 0x80 != 0 {
            let v = if kind & 0x40 != 0 {
                s.var(raw as i64)
            } else {
                raw
            };
            (s.var(which), v)
        } else {
            let v = if kind & 0x40 != 0 {
                s.flag(raw as i64) as i32 as f32
            } else {
                (raw != 0.0) as i32 as f32
            };
            (s.flag(which) as i32 as f32, v)
        };
        match kind & 0x3f {
            0 => x >= v,
            1 => x <= v,
            2 => x == v,
            3 => x != v,
            4 => x > v,
            5 => x < v,
            _ => false,
        }
    }

    fn reorder(&mut self) {
        if self.order.is_empty() {
            return;
        }
        let order = self.order.clone();
        let mut s = self.st.borrow_mut();
        let set = s.screen.add(SCENE);
        set.layers.sort_by_key(|l| {
            order
                .iter()
                .position(|n| *n == l.name.to_lowercase())
                .unwrap_or(usize::MAX)
        });
    }

    fn message(&mut self, text: &str) {
        if !self.window_open {
            self.call("openMessageWindow", vec![Value::Bool(true)]);
            self.window_open = true;
        }
        let key = self.call("getMessageWindowKey", vec![]);
        let set = key.first().map(lua_str).unwrap_or_else(|| "system".into());
        let layer = key.get(1).map(lua_str).unwrap_or_else(|| "Txt01".into());
        let now = self.st.borrow().now;
        {
            let mut s = self.st.borrow_mut();
            if s.screen.layer(&set, &layer).is_none() {
                // no text layer from the interface: one of our own over the bottom of the screen
                let (w, h) = (s.screen.w as i32, s.screen.h as i32);
                let b = crate::layers::TextBox {
                    w: w - 200,
                    h: 160,
                    margin: [10, 10, 10, 10],
                    line_gap: 0.2,
                    image: None,
                };
                s.text_layer(&set, &layer, b);
                if let Some(l) = s.screen.layer(&set, &layer) {
                    l.p.pos = [100.0, (h - 180) as f32, 0.0];
                }
            }
            s.set_text(&set, &layer, text, Some(0));
        }
        self.msg = Some((set, layer, now));
        self.wait = Wait::Typing;
    }
}

impl Engine {
    /// A layer of the scenario (for tests and tools).
    pub fn scene_layer(&self, name: &str) -> bool {
        self.st
            .borrow_mut()
            .screen
            .layer(SCENE, name)
            .is_some_and(|l| !matches!(l.content, Content::None))
    }
}

/// Text with each <@NAME> replaced by the string variable "@NAME" (empty when unset).
fn embed(s: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("<@") {
        let Some(j) = rest[i..].find('>') else { break };
        out.push_str(&rest[..i]);
        out.push_str(vars.get(&rest[i + 1..i + j]).map_or("", String::as_str));
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out
}
