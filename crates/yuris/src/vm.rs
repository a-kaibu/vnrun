//! Runs the compiled scripts: variables, labels, control flow (IF/LOOP/GOSUB/GO) and tasks.
//! Every other command goes to the [`Host`].
//!
//! Tasks take turns: each runs until it waits (WAIT, text) and then the next one runs; the
//! screen is drawn once all of them wait (one frame).

use std::collections::HashMap;

use crate::expr::{self, Kind, Place, Value, Var, VarId};
use crate::sjis;
use crate::sysvar as sys;
use crate::ybn::{self, Command, Instr, Script, VarDecl, VarType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub script: usize,
    pub ip: usize,
}

pub struct Program {
    pub commands: Vec<Command>,
    pub scripts: Vec<Script>,
    pub script_names: Vec<String>,
    labels: HashMap<String, Pos>,
    pub decls: Vec<VarDecl>,
    pub config: ybn::Config,
    /// per script and instruction: the instruction a block command goes to (see `link`)
    partner: Vec<Vec<u32>>,
    op: HashMap<&'static str, u8>,
}

// Commands the VM runs itself.
const OPS: [&str; 26] = [
    "IF", "ELSE", "IFEND", "IFBLEND", "IFBREAK", "IFCONTINUE", "LOOP", "LOOPEND", "LOOPBREAK", "LOOPCONTINUE",
    "GO", "GOSUB", "RETURN", "RETURNBREAK", "LET", "INT", "FLT", "STR", "S_INT", "S_FLT", "S_STR", "G_INT", "G_FLT",
    "G_STR", "WAIT", "END",
];

impl Program {
    /// Reads ysbin/: `read(name)` returns a file of it, e.g. "ysc.ybn".
    pub fn load(read: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Result<Program, String> {
        let need = |read: &mut dyn FnMut(&str) -> Option<Vec<u8>>, name: &str| {
            read(name).ok_or_else(|| format!("ysbin/{name} is missing"))
        };
        let ctx = |name: &'static str| move |e: String| format!("{name}: {e}");
        let commands = ybn::commands(&need(read, "ysc.ybn")?).map_err(ctx("ysc.ybn"))?;
        let script_names = ybn::script_names(&need(read, "yst_list.ybn")?).map_err(ctx("yst_list.ybn"))?;
        let decls = ybn::variables(&need(read, "ysv.ybn")?).map_err(ctx("ysv.ybn"))?;
        let files: Vec<_> = (0..script_names.len()).map(|i| read(&format!("yst{i:05}.ybn"))).collect();
        let key = files.iter().flatten().find_map(|b| ybn::script_key(b));
        let mut scripts = Vec::with_capacity(script_names.len());
        for (i, f) in files.into_iter().enumerate() {
            scripts.push(match f {
                Some(b) => ybn::script(&b, key).map_err(|e| format!("yst{i:05}.ybn: {e}"))?,
                None => Script::default(), // files that only declare (global.yst, macro.yst)
            });
        }
        let mut op = HashMap::new();
        for name in OPS {
            if let Some(i) = commands.iter().position(|c| c.name == name) {
                op.insert(name, i as u8);
            }
        }
        // the text decides how every string reads, the title included
        if let Some(word) = commands.iter().position(|c| c.name == "WORD").map(|i| i as u8) {
            let text: Vec<u8> = scripts
                .iter()
                .flat_map(|s: &Script| &s.code)
                .filter(|i| i.op == word)
                .filter_map(|i| i.args.first())
                .flat_map(|a| a.value.iter().copied())
                .collect();
            sjis::set_big5(sjis::looks_big5(&text));
        }
        let config = ybn::config(&need(read, "yscfg.ybn")?).map_err(ctx("yscfg.ybn"))?;
        let mut labels = HashMap::new();
        for l in ybn::labels(&need(read, "ysl.ybn")?).map_err(ctx("ysl.ybn"))? {
            labels.insert(l.name.to_uppercase(), Pos { script: l.script as usize, ip: l.ip as usize });
        }
        let mut p = Program { commands, scripts, script_names, labels, decls, config, partner: Vec::new(), op };
        p.partner = p.scripts.iter().map(|s| p.link(s)).collect();
        Ok(p)
    }

    fn is(&self, i: &Instr, name: &str) -> bool {
        self.op.get(name) == Some(&i.op)
    }

    pub fn name(&self, i: &Instr) -> &str {
        self.commands.get(i.op as usize).map_or("?", |c| &c.name)
    }

    /// Matches block commands: ELSE/IFBLEND/IFBREAK -> their IFEND, LOOPEND -> its LOOP,
    /// LOOPBREAK/LOOPCONTINUE -> the LOOPEND of their loop.
    fn link(&self, s: &Script) -> Vec<u32> {
        let mut out = vec![u32::MAX; s.code.len()];
        let mut ifs: Vec<Vec<usize>> = Vec::new();
        let mut loops: Vec<(usize, Vec<usize>)> = Vec::new();
        for (i, ins) in s.code.iter().enumerate() {
            if self.is(ins, "IF") {
                ifs.push(Vec::new());
            } else if self.is(ins, "ELSE") || self.is(ins, "IFBLEND") || self.is(ins, "IFBREAK") {
                if let Some(t) = ifs.last_mut() {
                    t.push(i);
                }
            } else if self.is(ins, "IFEND") {
                for j in ifs.pop().unwrap_or_default() {
                    out[j] = i as u32;
                }
            } else if self.is(ins, "LOOP") {
                loops.push((i, Vec::new()));
            } else if self.is(ins, "LOOPBREAK") || self.is(ins, "LOOPCONTINUE") {
                if let Some((_, t)) = loops.last_mut() {
                    t.push(i);
                }
            } else if self.is(ins, "LOOPEND") {
                if let Some((start, waiting)) = loops.pop() {
                    out[i] = start as u32;
                    for j in waiting {
                        out[j] = i as u32;
                    }
                }
            }
        }
        out
    }

    pub fn label(&self, name: &str) -> Option<Pos> {
        self.labels.get(&name.to_uppercase()).copied()
    }
}

/// An instruction's arguments as the host sees them.
pub struct Call<'a> {
    pub name: &'a str,
    /// keyword, how it is assigned, expression
    pub args: Vec<(&'a str, Assign, &'a [u8])>,
    pub pos: Pos,
}

impl Call<'_> {
    /// The expression given for a keyword.
    pub fn arg(&self, keyword: &str) -> Option<&[u8]> {
        self.args.iter().find(|(k, _, _)| k.eq_ignore_ascii_case(keyword)).map(|(_, _, e)| *e)
    }
    /// The expression and its assignment (`X+=10` is relative to the current X).
    pub fn arg_op(&self, keyword: &str) -> Option<(Assign, &[u8])> {
        self.args.iter().find(|(k, _, _)| k.eq_ignore_ascii_case(keyword)).map(|(_, a, e)| (*a, *e))
    }
}

/// `=`, `+=`, ... : the high byte of an argument's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assign {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    Xor,
}

impl Assign {
    pub fn from_type(ty: u16) -> Assign {
        match ty >> 8 {
            1 => Assign::Add,
            2 => Assign::Sub,
            3 => Assign::Mul,
            4 => Assign::Div,
            5 => Assign::Rem,
            6 => Assign::And,
            7 => Assign::Or,
            8 => Assign::Xor,
            _ => Assign::Set,
        }
    }
    /// The new value from the old one and the given one.
    pub fn apply(self, old: Value, v: Value) -> Value {
        let op = match self {
            Assign::Set => return v,
            Assign::Add => b'+',
            Assign::Sub => b'-',
            Assign::Mul => b'*',
            Assign::Div => b'/',
            Assign::Rem => b'%',
            Assign::And => b'A',
            Assign::Or => b'O',
            Assign::Xor => b'^',
        };
        expr::binary(op, old, v).unwrap_or_default()
    }
}

/// What a command does to its task.
pub enum Flow {
    Next,
    /// wait this many milliseconds
    Wait(f64),
    /// wait this many frames
    Frames(u32),
    /// let the other tasks run and come back next frame
    Yield,
    /// stop the whole engine
    End,
}

pub trait Host {
    fn command(&mut self, vm: &mut State, call: &Call) -> Result<Flow, String>;
    /// Text written in the script (WORD) and `_[...]`, in Shift_JIS.
    fn text(&mut self, vm: &mut State, text: &[u8]) -> Result<Flow, String>;
}

#[derive(Debug, Clone, Default)]
struct Loop {
    at: usize,
    count: Option<i64>,
    n: i64,
}

#[derive(Debug, Clone)]
struct Frame {
    ret: Option<Pos>,
    loops: Vec<Loop>,
    /// @_PINT etc. of the caller, put back on RETURN
    params: Vec<Var>,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub enabled: bool,
    pub z: i64,
    pos: Pos,
    frames: Vec<Frame>,
    wake: f64,
    frames_left: u32,
    pub done: bool,
    /// stopped at RETURNCODE: the text it gave is shown, and goes when the task goes on
    text_shown: bool,
}

/// Variables and tasks: what commands may read and change.
pub struct State {
    pub vars: Vec<Option<Var>>,
    pub tasks: Vec<Task>,
    /// index of the task running now
    pub current: usize,
    /// milliseconds since start
    pub now: f64,
    pub trace: bool,
    /// TASK[RESTART=1]: run the tasks once more this frame
    restart: bool,
}

impl expr::Vars for State {
    fn get(&self, p: &Place) -> Value {
        let Some(Some(v)) = self.vars.get(p.var.id as usize) else {
            return if p.var.str { Value::Str(Vec::new()) } else { Value::Int(0) };
        };
        v.slot(&p.index).and_then(|i| v.vals.get(i)).cloned().unwrap_or_else(|| v.kind.zero())
    }
}

fn kind_of(ty: VarType, id: VarId) -> Kind {
    match ty {
        VarType::Int => Kind::Int,
        VarType::Flt => Kind::Flt,
        VarType::Str => Kind::Str,
        VarType::None if id.str => Kind::Str,
        VarType::None => Kind::Int,
    }
}

impl State {
    pub fn eval(&self, e: &[u8]) -> Result<Value, String> {
        expr::eval(e, self)
    }
    pub fn int(&self, e: &[u8]) -> Result<i64, String> {
        Ok(self.eval(e)?.int())
    }
    pub fn str(&self, e: &[u8]) -> Result<String, String> {
        Ok(self.eval(e)?.str())
    }
    pub fn place(&self, e: &[u8]) -> Result<Place, String> {
        expr::place(e, self)
    }

    fn var_mut(&mut self, id: VarId) -> &mut Var {
        let i = id.id as usize;
        if self.vars.len() <= i {
            self.vars.resize(i + 1, None);
        }
        self.vars[i].get_or_insert_with(|| {
            let kind = if id.str { Kind::Str } else { Kind::Int };
            Var::new(kind, Vec::new(), kind.zero())
        })
    }

    /// Gives a variable the size and value ysv.ybn declares.
    fn declare_from(&mut self, d: &VarDecl) -> Result<(), String> {
        let id = VarId { id: d.id, str: d.ty == VarType::Str };
        let kind = kind_of(d.ty, id);
        let init = match d.ty {
            VarType::Int => Value::Int(i64::from_le_bytes(d.init[..8].try_into().unwrap())),
            VarType::Flt => Value::Flt(f64::from_le_bytes(d.init[..8].try_into().unwrap())),
            VarType::Str if d.init.is_empty() => Value::Str(Vec::new()),
            VarType::Str => self.eval(&d.init)?,
            VarType::None => return Ok(()),
        };
        self.declare(id, kind, d.dims.iter().map(|&n| n as usize).collect(), init);
        Ok(())
    }

    pub fn declare(&mut self, id: VarId, kind: Kind, dims: Vec<usize>, init: Value) {
        let i = id.id as usize;
        if self.vars.len() <= i {
            self.vars.resize(i + 1, None);
        }
        self.vars[i] = Some(Var::new(kind, dims, init));
    }

    pub fn set(&mut self, p: &Place, v: Value) {
        let var = self.var_mut(p.var);
        if p.whole {
            let v = var.kind.coerce(v);
            var.vals.iter_mut().for_each(|x| *x = v.clone());
        } else if let Some(i) = var.slot(&p.index) {
            if let Some(x) = var.vals.get_mut(i) {
                *x = var.kind.coerce(v);
            }
        }
    }

    /// Sets a system variable element (e.g. @_RINT(1)).
    pub fn set_sys(&mut self, id: u16, str: bool, index: &[i64], v: Value) {
        self.set(&Place { var: VarId { id, str }, index: index.to_vec(), whole: false }, v);
    }

    pub fn get_sys(&self, id: u16, str: bool, index: &[i64]) -> Value {
        expr::Vars::get(self, &Place { var: VarId { id, str }, index: index.to_vec(), whole: false })
    }

    /// Stores into the place an argument names (LET=@A etc.).
    pub fn assign(&mut self, e: &[u8], v: Value) -> Result<(), String> {
        let p = self.place(e)?;
        self.set(&p, v);
        Ok(())
    }

    pub fn task(&self) -> &Task {
        &self.tasks[self.current]
    }
}

pub struct Vm {
    pub prog: Program,
    pub state: State,
    pub ended: bool,
}

const NO_JUMP: u32 = 0;

impl Vm {
    pub fn new(prog: Program) -> Result<Vm, String> {
        let mut state = State { vars: Vec::new(), tasks: Vec::new(), current: 0, now: 0.0, trace: false, restart: false };
        for d in &prog.decls {
            state.declare_from(d)?;
        }
        sys::init(&mut state);
        let start = prog.label("SYSTEM_START").ok_or("no SYSTEM_START label")?;
        state.tasks.push(Task::new("SYSTEM", start));
        Ok(Vm { prog, state, ended: false })
    }

    /// Runs every task until it waits: one frame of the game.
    pub fn frame(&mut self, now: f64, host: &mut dyn Host) -> Result<(), String> {
        self.state.now = now;
        // TASK[RESTART=1] asks for another round without waiting a frame; ERIS uses it after
        // a line of text so the scenario gives the next one at once
        for _ in 0..8 {
            self.state.restart = false;
            self.round(now, host)?;
            if !self.state.restart || self.ended {
                break;
            }
            for t in &mut self.state.tasks {
                t.frames_left = 0;
            }
        }
        Ok(())
    }

    fn round(&mut self, now: f64, host: &mut dyn Host) -> Result<(), String> {
        let mut order: Vec<usize> = (0..self.state.tasks.len()).collect();
        order.sort_by_key(|&i| self.state.tasks[i].z);
        for i in order {
            if self.ended {
                break;
            }
            let t = &mut self.state.tasks[i];
            if t.done || !t.enabled {
                continue;
            }
            if t.frames_left > 0 {
                t.frames_left -= 1;
                continue;
            }
            if t.wake > now {
                continue;
            }
            self.state.current = i;
            self.run_task(host)?;
        }
        self.state.tasks.retain(|t| !t.done);
        Ok(())
    }

    fn run_task(&mut self, host: &mut dyn Host) -> Result<(), String> {
        // A task that runs this long without waiting is stuck in a loop. ERIS 554 sets up its
        // buttons in one go, a few million steps.
        for _ in 0..50_000_000 {
            let t = &self.state.tasks[self.state.current];
            if t.done || !t.enabled {
                return Ok(());
            }
            let pos = t.pos;
            if t.text_shown {
                self.task_mut().text_shown = false;
                self.state.set_sys(sys::TEXT, true, &[], Value::Str(Vec::new()));
                self.state.set_sys(sys::TEXT_FLAG, true, &[], Value::Str(Vec::new()));
            }
            self.sync_lc();
            let Some(ins) = self.prog.scripts.get(pos.script).and_then(|s| s.code.get(pos.ip)) else {
                // running off the end of a file ends the task
                self.state.tasks[self.state.current].done = true;
                return Ok(());
            };
            let ins = ins.clone();
            self.task_mut().pos.ip += 1;
            let name = self.prog.name(&ins).to_string();
            if self.state.trace {
                let call = Self::call(&self.prog, &ins, &name, pos);
                let args: Vec<String> = call
                    .args
                    .iter()
                    .map(|(k, _, e)| format!("{k}={}", self.state.eval(e).map_or("?".into(), |v| v.str())))
                    .collect();
                eprintln!("[{}] {}:{} {} {}", self.task_mut().id, pos.script, pos.ip, name, args.join(" "));
            }
            let flow = self
                .exec(&ins, &name, pos, host)
                .map_err(|e| format!("{} line {} ({name}): {e}", self.prog.script_names[pos.script], ins.line))?;
            match flow {
                Flow::Next => {}
                Flow::Wait(ms) => {
                    let now = self.state.now;
                    self.task_mut().wake = now + ms;
                    return Ok(());
                }
                Flow::Frames(n) => {
                    self.task_mut().frames_left = n.saturating_sub(1);
                    return Ok(());
                }
                Flow::Yield => return Ok(()),
                Flow::End => {
                    self.ended = true;
                    return Ok(());
                }
            }
        }
        Err(format!("task {} runs without waiting", self.state.task().id))
    }

    fn task_mut(&mut self) -> &mut Task {
        let c = self.state.current;
        &mut self.state.tasks[c]
    }

    fn frame_mut(&mut self) -> &mut Frame {
        self.task_mut().frames.last_mut().expect("a task always has a frame")
    }

    fn sync_lc(&mut self) {
        let n = self.state.task().frames.last().and_then(|f| f.loops.last()).map_or(0, |l| l.n);
        self.state.set_sys(sys::LC, false, &[], Value::Int(n));
    }

    fn jump(&mut self, ip: u32) {
        self.task_mut().pos.ip = ip as usize;
    }

    fn goto_label(&mut self, name: &str) -> Result<(), String> {
        let to = self.prog.label(name).ok_or_else(|| format!("no label {name:?}"))?;
        self.task_mut().pos = to;
        Ok(())
    }

    fn exec(&mut self, ins: &Instr, name: &str, pos: Pos, host: &mut dyn Host) -> Result<Flow, String> {
        let st = &mut self.state;
        let a = |k: usize| ins.args.get(k).map_or(&[][..], |a| &a.value[..]);
        match name {
            "IF" | "ELSE" => {
                let has_cond = !ins.args.is_empty() && !a(0).is_empty();
                if !has_cond || st.eval(a(0))?.truthy() {
                    return Ok(Flow::Next);
                }
                // the next ELSE, else the IFEND; the last ELSE of a chain has neither
                let next = ins.args.get(1).map_or(NO_JUMP, |a| a.jump);
                let end = ins.args.get(2).map_or(NO_JUMP, |a| a.jump);
                let to = if next != NO_JUMP {
                    next
                } else if end != NO_JUMP {
                    end
                } else {
                    self.prog.partner[pos.script][pos.ip]
                };
                if to != u32::MAX {
                    self.jump(to);
                }
            }
            "IFEND" => {}
            "IFBLEND" | "IFBREAK" => {
                let to = self.prog.partner[pos.script][pos.ip];
                if to != u32::MAX {
                    self.jump(to);
                }
            }
            "IFCONTINUE" => {}
            "LOOP" => {
                let count = match ins.args.first() {
                    // a negative count loops for ever, like no count
                    Some(arg) if !arg.value.is_empty() => Some(st.int(&arg.value)?).filter(|&c| c >= 0),
                    _ => None,
                };
                if count == Some(0) {
                    let end = ins.args.get(1).map_or(NO_JUMP, |a| a.jump);
                    self.jump(end + 1);
                } else {
                    self.frame_mut().loops.push(Loop { at: pos.ip, count, n: 1 });
                }
            }
            "LOOPEND" | "LOOPCONTINUE" => {
                let end = if name == "LOOPEND" { pos.ip } else { self.prog.partner[pos.script][pos.ip] as usize };
                let Some(l) = self.frame_mut().loops.last_mut() else { return Ok(Flow::Next) };
                l.n += 1;
                if l.count.is_none_or(|c| l.n <= c) {
                    let at = l.at;
                    self.jump(at as u32 + 1);
                } else {
                    self.frame_mut().loops.pop();
                    self.jump(end as u32 + 1);
                }
            }
            "LOOPBREAK" => {
                let levels = match ins.args.first() {
                    Some(arg) if !arg.value.is_empty() => st.int(&arg.value)?.max(1) as usize,
                    _ => 1,
                };
                let mut end = self.prog.partner[pos.script][pos.ip];
                for level in 0..levels {
                    self.frame_mut().loops.pop();
                    if level + 1 < levels {
                        // the LOOPEND of the enclosing loop: search forward from this one
                        let script = &self.prog.scripts[pos.script];
                        let mut depth = 0;
                        let mut i = end as usize + 1;
                        while i < script.code.len() {
                            let n = self.prog.name(&script.code[i]);
                            if n == "LOOP" {
                                depth += 1;
                            } else if n == "LOOPEND" {
                                if depth == 0 {
                                    break;
                                }
                                depth -= 1;
                            }
                            i += 1;
                        }
                        end = i as u32;
                    }
                }
                self.jump(end + 1);
            }
            "GO" => {
                let label = st.str(a(0))?;
                self.frame_mut().loops.clear();
                self.goto_label(&label)?;
            }
            "GOSUB" => self.gosub(ins)?,
            "RETURN" => {
                if !self.ret(ins)? {
                    self.task_mut().done = true;
                    return Ok(Flow::Yield);
                }
            }
            "RETURNBREAK" => {}
            "LET" => {
                let v = st.eval(a(1))?;
                let p = st.place(a(0))?;
                let op = Assign::from_type(ins.args[0].ty);
                let v = op.apply(expr::Vars::get(st, &p), v);
                st.set(&p, v);
            }
            "INT" | "FLT" | "STR" => {
                let (id, dims) = expr::declared(a(0), st)?;
                let kind = match name {
                    "INT" => Kind::Int,
                    "FLT" => Kind::Flt,
                    _ => Kind::Str,
                };
                let init = if ins.args.len() > 1 { st.eval(a(1))? } else { kind.zero() };
                st.declare(id, kind, dims, init);
            }
            // declared from ysv.ybn before the start
            "S_INT" | "S_FLT" | "S_STR" | "G_INT" | "G_FLT" | "G_STR" => {}
            "WAIT" => {
                let call = Self::call(&self.prog, ins, name, pos);
                if let Some(e) = call.arg("TIME") {
                    return Ok(Flow::Wait(self.state.int(e)? as f64));
                }
                if let Some(e) = call.arg("FRAME") {
                    return Ok(Flow::Frames(self.state.int(e)?.max(1) as u32));
                }
                return Ok(Flow::Yield);
            }
            "WORD" => return host.text(&mut self.state, a(0)),
            "_" => {
                let text = st.eval(a(0))?.bytes();
                return host.text(&mut self.state, &text);
            }
            "TASK" => self.task_cmd(ins, name, pos)?,
            "TASKEND" => {
                let call = Self::call(&self.prog, ins, name, pos);
                let id = match call.arg("ID") {
                    Some(e) => self.state.str(e)?,
                    None => self.state.task().id.clone(),
                };
                let me = self.state.task().id.eq_ignore_ascii_case(&id);
                for t in &mut self.state.tasks {
                    if t.id.eq_ignore_ascii_case(&id) {
                        t.done = true;
                    }
                }
                if me {
                    return Ok(Flow::Yield);
                }
            }
            "TASKINFO" => self.task_info(ins, name, pos)?,
            "LABELINFO" => {
                let call = Self::call(&self.prog, ins, name, pos);
                let label = call.arg("#").map(|e| self.state.str(e)).transpose()?.unwrap_or_default();
                let exists = self.prog.label(&label).is_some();
                if let Some(e) = call.arg("LET") {
                    let e = e.to_vec();
                    self.state.assign(&e, Value::Int(exists as i64))?;
                }
            }
            "VARACT" => self.var_act(ins, name, pos)?,
            "VARINFO" => self.var_info(ins, name, pos)?,
            "END" => return Ok(Flow::End),
            // a line of text ends: the text task (ERIS) shows it, then lets this task go on
            "RETURNCODE" => {
                let mut flag = st.get_sys(sys::TEXT_FLAG, true, &[]).bytes();
                flag.push(b'E');
                st.set_sys(sys::TEXT_FLAG, true, &[], Value::Str(flag));
                self.task_mut().text_shown = true;
                return Ok(Flow::Yield);
            }
            _ => {
                let call = Self::call(&self.prog, ins, name, pos);
                return host.command(&mut self.state, &call);
            }
        }
        Ok(Flow::Next)
    }

    fn call<'a>(prog: &'a Program, ins: &'a Instr, name: &'a str, pos: Pos) -> Call<'a> {
        let keywords = &prog.commands[ins.op as usize].keywords;
        let args = ins
            .args
            .iter()
            .map(|a| (keywords.get(a.keyword as usize).map_or("", String::as_str), Assign::from_type(a.ty), &a.value[..]))
            .collect();
        Call { name, args, pos }
    }

    fn gosub(&mut self, ins: &Instr) -> Result<(), String> {
        let keywords = &self.prog.commands[ins.op as usize].keywords;
        let mut label = None;
        let mut params: Vec<(u16, i64, Value)> = Vec::new();
        for a in &ins.args {
            let k = keywords.get(a.keyword as usize).map_or("", String::as_str);
            if k == "#" {
                label = Some(self.state.str(&a.value)?);
                continue;
            }
            let (var, num) = if let Some(n) = k.strip_prefix("PINT") {
                (sys::PINT, n)
            } else if let Some(n) = k.strip_prefix("PFLT") {
                (sys::PFLT, n)
            } else if let Some(n) = k.strip_prefix("PSTR") {
                (sys::PSTR, n)
            } else {
                continue;
            };
            let n = if num.is_empty() { 1 } else { num.parse().unwrap_or(1) };
            params.push((var, n, self.state.eval(&a.value)?));
        }
        let label = label.ok_or("GOSUB without a label")?;
        let to = self.prog.label(&label).ok_or_else(|| format!("no label {label:?}"))?;
        let saved: Vec<Var> = [sys::PINT, sys::PFLT, sys::PSTR, sys::PFLG]
            .iter()
            .map(|&id| self.state.var_mut(VarId { id, str: id == sys::PSTR }).clone())
            .collect();
        for (id, saved) in [sys::PINT, sys::PFLT, sys::PSTR, sys::PFLG].iter().zip(&saved) {
            let var = self.state.var_mut(VarId { id: *id, str: *id == sys::PSTR });
            *var = Var::new(saved.kind, saved.dims.clone(), saved.kind.zero());
        }
        for (var, n, v) in params {
            self.state.set_sys(var, var == sys::PSTR, &[n], v);
            self.state.set_sys(sys::PFLG, false, &[n], Value::Int(1));
        }
        let ret = self.state.task().pos;
        self.task_mut().frames.push(Frame { ret: Some(ret), loops: Vec::new(), params: saved });
        self.task_mut().pos = to;
        Ok(())
    }

    /// RETURN: false when there is nothing to return to (the task ends).
    fn ret(&mut self, ins: &Instr) -> Result<bool, String> {
        let keywords = &self.prog.commands[ins.op as usize].keywords;
        let mut out: Vec<(u16, i64, Value)> = Vec::new();
        for a in &ins.args {
            let k = keywords.get(a.keyword as usize).map_or("", String::as_str).to_ascii_uppercase();
            let (var, num) = if let Some(n) = k.strip_prefix("RINT") {
                (sys::RINT, n.to_string())
            } else if let Some(n) = k.strip_prefix("RFLT") {
                (sys::RFLT, n.to_string())
            } else if let Some(n) = k.strip_prefix("RSTR") {
                (sys::RSTR, n.to_string())
            } else {
                continue;
            };
            let n = if num.is_empty() { 1 } else { num.parse().unwrap_or(1) };
            out.push((var, n, self.state.eval(&a.value)?));
        }
        if self.state.task().frames.len() <= 1 {
            return Ok(false);
        }
        let frame = self.task_mut().frames.pop().unwrap();
        for (id, saved) in [sys::PINT, sys::PFLT, sys::PSTR, sys::PFLG].iter().zip(frame.params) {
            *self.state.var_mut(VarId { id: *id, str: *id == sys::PSTR }) = saved;
        }
        for id in [sys::RINT, sys::RFLT, sys::RSTR, sys::RFLG] {
            let var = self.state.var_mut(VarId { id, str: id == sys::RSTR });
            let kind = var.kind;
            var.vals.iter_mut().for_each(|x| *x = kind.zero());
        }
        for (var, n, v) in out {
            self.state.set_sys(var, var == sys::RSTR, &[n], v);
            self.state.set_sys(sys::RFLG, false, &[n], Value::Int(1));
        }
        if let Some(ret) = frame.ret {
            self.task_mut().pos = ret;
        }
        Ok(true)
    }

    fn task_cmd(&mut self, ins: &Instr, name: &str, pos: Pos) -> Result<(), String> {
        let call = Self::call(&self.prog, ins, name, pos);
        let st = &self.state;
        let id = match call.arg("ID") {
            Some(e) => st.str(e)?,
            None => st.task().id.clone(),
        };
        let label = call.arg("#").map(|e| st.str(e)).transpose()?;
        let enabled = call.arg("E").map(|e| st.int(e)).transpose()?;
        let z = call.arg("Z").map(|e| st.int(e)).transpose()?;
        let scriptpos = call.arg("SCRIPTPOS").map(|e| st.str(e)).transpose()?;
        if call.arg("RESTART").is_some_and(|e| st.int(e).unwrap_or(0) != 0) {
            self.state.restart = true;
            return Ok(());
        }
        let existing = self.state.tasks.iter().position(|t| t.id.eq_ignore_ascii_case(&id) && !t.done);
        if let Some(label) = label {
            let to = self.prog.label(&label).ok_or_else(|| format!("no label {label:?}"))?;
            if let Some(i) = existing {
                self.state.tasks[i].done = true;
            }
            let mut t = Task::new(&id, to);
            t.z = z.unwrap_or(1);
            if let Some(e) = enabled {
                t.enabled = e != 0;
            }
            self.state.tasks.push(t);
            return Ok(());
        }
        let Some(i) = existing else { return Ok(()) };
        let t = &mut self.state.tasks[i];
        if let Some(e) = enabled {
            t.enabled = e != 0;
        }
        if let Some(z) = z {
            t.z = z;
        }
        if let Some(sp) = scriptpos {
            if let Some(p) = parse_scriptpos(&sp) {
                t.pos = p;
                t.frames.truncate(1);
                t.frames[0].loops.clear();
            }
        }
        Ok(())
    }

    fn task_info(&mut self, ins: &Instr, name: &str, pos: Pos) -> Result<(), String> {
        let call = Self::call(&self.prog, ins, name, pos);
        let st = &self.state;
        let id = match call.arg("ID") {
            Some(e) => st.str(e)?,
            None => st.task().id.clone(),
        };
        let let_ = call.arg("LET").ok_or("TASKINFO without LET")?.to_vec();
        let on = |k: &str| call.arg(k).map(|e| st.int(e)).transpose().map(|v| v.unwrap_or(0) != 0);
        let t = st.tasks.iter().find(|t| t.id.eq_ignore_ascii_case(&id) && !t.done);
        let v = if on("EXIST")? {
            Value::Int(t.is_some() as i64)
        } else if on("E")? {
            Value::Int(t.is_some_and(|t| t.enabled) as i64)
        } else if on("Z")? {
            Value::Int(t.map_or(0, |t| t.z))
        } else if on("GOSUBNEST")? {
            Value::Int(t.map_or(0, |t| t.frames.len() as i64 - 1))
        } else if on("LOOPNEST")? {
            Value::Int(t.and_then(|t| t.frames.last()).map_or(0, |f| f.loops.len() as i64))
        } else if on("SCRIPTPOS")? {
            Value::text(&t.map_or(String::new(), |t| format!("{}:{}", t.pos.script, t.pos.ip)))
        } else if on("FILE")? {
            Value::text(&t.map_or(String::new(), |t| self.prog.script_names[t.pos.script].clone()))
        } else if on("LINE")? {
            let line = t.and_then(|t| self.prog.scripts[t.pos.script].code.get(t.pos.ip)).map_or(0, |i| i.line);
            Value::Int(line as i64)
        } else {
            Value::Int(0)
        };
        self.state.assign(&let_, v)
    }
}

impl Vm {
    fn var_act(&mut self, ins: &Instr, name: &str, pos: Pos) -> Result<(), String> {
        let call = Self::call(&self.prog, ins, name, pos);
        let st = &mut self.state;
        if call.arg("INIT").is_some_and(|e| st.int(e).is_ok_and(|v| v != 0)) {
            // 482: VARACT[INIT=1 G_INT3=1]: the G_INT3 variables get their declared values again
            for (ty, kw) in [(VarType::Int, "G_INT"), (VarType::Flt, "G_FLT"), (VarType::Str, "G_STR")] {
                for group in 0..4u8 {
                    let k = if group == 0 { kw.to_string() } else { format!("{kw}{}", group + 1) };
                    if !call.arg(&k).is_some_and(|e| st.int(e).is_ok_and(|v| v != 0)) {
                        continue;
                    }
                    for d in self.prog.decls.iter().filter(|d| d.scope == 1 && d.group == group && d.ty == ty) {
                        if (541..10000).contains(&d.id) {
                            st.declare_from(d)?;
                        }
                    }
                }
            }
            return Ok(());
        }
        let set = st.place(call.arg("SET").ok_or("VARACT without SET")?)?;
        let num = |k: &str| call.arg(k).map(|e| st.int(e)).transpose();
        let on = |k: &str| num(k).map(|v| v.unwrap_or(0) != 0);
        if let Some((op, e)) = call.arg_op("DIMSIZE") {
            let n = st.int(e)?;
            let var = st.var_mut(set.var);
            let old = var.dims.first().copied().unwrap_or(var.vals.len()) as i64;
            let size = op.apply(Value::Int(old), Value::Int(n)).int().max(0) as usize;
            let zero = var.kind.zero();
            var.vals.resize(size.max(1), zero);
            var.dims = vec![size];
            return Ok(());
        }
        if let Some(at) = num("PUSH")? {
            let var = st.var_mut(set.var);
            let at = (at.max(0) as usize).min(var.vals.len());
            let zero = var.kind.zero();
            var.vals.insert(at, zero);
            var.dims = vec![var.vals.len()];
            return Ok(());
        }
        if let Some(at) = num("POP")? {
            let var = st.var_mut(set.var);
            if (at.max(0) as usize) < var.vals.len() {
                var.vals.remove(at as usize);
                let zero = var.kind.zero();
                var.vals.push(zero);
            }
            return Ok(());
        }
        let from = expr::Vars::get(st, &set);
        let out = if on("TYPE")? {
            match from {
                Value::Str(_) => Value::Int(from.int()),
                v => Value::Str(v.bytes()),
            }
        } else if on("COPY")? || on("CUT")? {
            let bytes = from.bytes();
            let chars = sjis::chars(&bytes);
            let start = (num("POS")?.unwrap_or(1).max(1) - 1) as usize;
            let len = num("LENGTH")?.map_or(chars.len(), |l| l.max(0) as usize);
            let end = (start + len).min(chars.len());
            let start = start.min(end);
            if on("CUT")? {
                Value::Str([&chars[..start], &chars[end..]].concat().concat())
            } else {
                Value::Str(chars[start..end].concat())
            }
        } else if on("UPPER")? || on("UPPER2")? {
            Value::text(&from.str().to_uppercase())
        } else if on("LOWER")? || on("LOWER2")? {
            Value::text(&from.str().to_lowercase())
        } else if on("HANTOZEN")? {
            Value::text(&from.str().chars().map(sjis::to_zen).collect::<String>())
        } else if on("ZENTOHAN")? {
            Value::text(&from.str().chars().map(sjis::to_han).collect::<String>())
        } else {
            return Ok(());
        };
        match call.arg("LET") {
            Some(e) => {
                let e = e.to_vec();
                self.state.assign(&e, out)
            }
            None => {
                self.state.set(&set, out);
                Ok(())
            }
        }
    }

    fn var_info(&mut self, ins: &Instr, name: &str, pos: Pos) -> Result<(), String> {
        let call = Self::call(&self.prog, ins, name, pos);
        let st = &self.state;
        let set = st.place(call.arg("SET").ok_or("VARINFO without SET")?)?;
        let on = |k: &str| call.arg(k).map(|e| st.int(e)).transpose().map(|v| v.unwrap_or(0) != 0);
        let var = st.vars.get(set.var.id as usize).and_then(Option::as_ref);
        let v = if on("TYPE")? {
            var.map_or(0, |v| match v.kind {
                Kind::Int => 1,
                Kind::Flt => 2,
                Kind::Str => 3,
            })
        } else if on("LENGTH")? {
            sjis::chars(&expr::Vars::get(st, &set).bytes()).len() as i64
        } else if on("DIMNUM")? {
            var.map_or(0, |v| v.dims.len() as i64)
        } else if on("SEARCH")? {
            let want = ["INT", "FLT", "STR"].iter().find_map(|k| call.arg(k)).map(|e| st.eval(e)).transpose()?;
            let from = call.arg("NO").map(|e| st.int(e)).transpose()?.unwrap_or(0).max(0) as usize;
            match (var, want) {
                (Some(v), Some(want)) => {
                    let want = v.kind.coerce(want);
                    v.vals.iter().skip(from).position(|x| *x == want).map_or(-1, |i| (i + from) as i64)
                }
                _ => -1,
            }
        } else if let Some(k) = (1..=8).find(|k| on(&if *k == 1 { "DIMSIZE".into() } else { format!("DIMSIZE{k}") }).unwrap_or(false)) {
            var.and_then(|v| v.dims.get(k - 1)).map_or(0, |&d| d as i64)
        } else if on("SJISCODE")? {
            // the code of the first character: both bytes of a double-byte one
            let b = expr::Vars::get(st, &set).bytes();
            sjis::chars(&b).first().map_or(0, |c| c.iter().fold(0, |n, &x| n << 8 | x as i64))
        } else if on("STRTYPE")? {
            // 1 when every character is a digit
            let b = expr::Vars::get(st, &set).bytes();
            (!b.is_empty() && b.iter().all(u8::is_ascii_digit)) as i64
        } else {
            0
        };
        let e = call.arg("LET").ok_or("VARINFO without LET")?.to_vec();
        self.state.assign(&e, Value::Int(v))
    }
}

fn parse_scriptpos(s: &str) -> Option<Pos> {
    let (a, b) = s.split_once(':')?;
    Some(Pos { script: a.parse().ok()?, ip: b.parse().ok()? })
}

impl Task {
    fn new(id: &str, pos: Pos) -> Task {
        Task {
            id: id.to_string(),
            enabled: true,
            z: 1,
            pos,
            frames: vec![Frame { ret: None, loops: Vec::new(), params: Vec::new() }],
            wake: 0.0,
            frames_left: 0,
            done: false,
            text_shown: false,
        }
    }
}
