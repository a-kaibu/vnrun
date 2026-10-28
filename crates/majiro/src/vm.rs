//! The bytecode interpreter: cooperative threads of call frames over one value stack each.
//!
//! Calling convention (seen in the compiled scripts): the caller pushes the arguments last
//! first, then `call` pushes their count; locals live above that, so in a frame `o = -1` is the
//! count, `-2` the first argument, `-3` the second, and `0..` the locals made by `alloca`.

use std::collections::HashMap;
use std::rc::Rc;

use crate::mjo::{Op, Script, decode};
use crate::value::Value;

/// What happens to a function's result when it returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cont {
    Push,
    Drop,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Skip {
    pub target: usize,
    pub height: usize,
}

pub struct Frame {
    pub script: Rc<Script>,
    pub pc: usize,
    /// the stack index of local 0
    pub base: usize,
    pub argc: usize,
    pub case: Value,
    /// click handlers of a skip block: left, right, either
    pub skip: [Option<Skip>; 3],
    /// the destructor block, run when the function returns
    pub dtor: Option<usize>,
    pub ret: Option<Value>,
    pub cont: Cont,
    pub func: u32,
    /// the wait an event handler interrupted, back when it returns
    pub saved_wait: Option<(Wait, Cont)>,
}

/// Why a thread is not running.
#[derive(Debug, Clone)]
pub enum Wait {
    /// until this time (ms)
    Until(f64),
    /// a click or key (0: any, else this button)
    Click,
    /// `$mk_wait`-like: a click or key, the result pushed
    Key,
    /// the text of the console is still appearing, then a click
    Page,
    /// until the sprite stops moving
    Sprite(i32),
    /// until the sound channel has ended
    Sound(usize),
    /// until the movie ends
    Movie,
    /// until no other thread runs the named function... or a frame
    Frame,
    /// a turn for the other threads (`$wait(-2)`)
    Tick,
    /// until the thread ends
    Thread(usize),
    /// until a menu choice (the console's select)
    Forever,
}

pub struct Thread {
    pub id: usize,
    pub stack: Vec<Value>,
    pub frames: Vec<Frame>,
    pub vars: HashMap<u32, Value>,
    pub wait: Option<(Wait, Cont)>,
    pub line: i32,
    /// event handlers to call (from other threads' `$do_event`)
    pub pending: Vec<(u32, Vec<Value>)>,
    /// ending: each frame's destructor (`bsel.5`) runs, then the frame goes
    pub unwinding: bool,
}

impl Thread {
    /// Ends the thread the way the engine does: the destructors of its frames still run.
    pub fn unwind(&mut self) {
        self.unwinding = true;
        self.wait = None;
        self.pending.clear();
        self.next_dtor();
    }

    /// Drops frames down to the next one with a destructor and goes there.
    fn next_dtor(&mut self) {
        while let Some(f) = self.frames.last_mut() {
            if let Some(d) = f.dtor.take() {
                f.pc = d;
                // the locals stay for the destructor
                f.ret = Some(Value::Int(0));
                return;
            }
            let f = self.frames.pop().unwrap();
            self.stack.truncate(f.base.saturating_sub(1 + f.argc));
        }
    }

    pub fn pop(&mut self) -> Value {
        self.stack.pop().unwrap_or_default()
    }
}

/// What a syscall does.
pub enum Ret {
    Val(Value),
    Wait(Wait),
    /// calls a script function (by hash) with these arguments; its result is the syscall's
    Call(u32, Vec<Value>),
    /// calls these functions, the last first, results dropped (event handlers)
    Calls(Vec<(u32, Vec<Value>)>),
    /// replaces the thread's whole call stack with this function (`$exec`-like jumps), running
    /// these calls first
    Jump(u32, Vec<(u32, Vec<Value>)>),
    /// the thread ends
    End,
}

/// Engine services the interpreter needs.
pub trait Host {
    fn function(&mut self, hash: u32) -> Option<(Rc<Script>, usize)>;
    fn global(&mut self, scope: u8, hash: u32) -> Option<&mut Value>;
    fn set_global(&mut self, scope: u8, hash: u32, v: Value);
    fn syscall(&mut self, th: &mut Thread, hash: u32, args: Vec<Value>) -> Result<Ret, String>;
    fn text(&mut self, th: &mut Thread, s: &[u8]) -> Result<(), String>;
    fn proc_text(&mut self, th: &mut Thread);
    fn ctrl(&mut self, th: &mut Thread, s: &[u8]) -> Result<Option<Wait>, String>;
}

pub enum Stop {
    /// waiting (the thread's `wait` says on what)
    Wait,
    /// the thread has no frames left
    Ended,
    /// ran out of the step budget (a busy loop)
    Budget,
}

fn scope_of(flags: u16) -> u8 {
    ((flags >> 5) & 7) as u8
}
fn type_of(flags: u16) -> u8 {
    ((flags >> 8) & 7) as u8
}
fn dims_of(flags: u16) -> usize {
    ((flags >> 11) & 3) as usize
}

impl Thread {
    pub fn new(id: usize) -> Thread {
        Thread {
            id,
            stack: Vec::new(),
            frames: Vec::new(),
            vars: HashMap::new(),
            wait: None,
            line: 0,
            pending: Vec::new(),
            unwinding: false,
        }
    }

    /// Starts function `hash` with `args` on top of the current frames.
    pub fn call(
        &mut self,
        host: &mut dyn Host,
        hash: u32,
        args: Vec<Value>,
        cont: Cont,
    ) -> Result<(), String> {
        let (script, at) = host
            .function(hash)
            .ok_or_else(|| format!("no function ${hash:08x}"))?;
        let argc = args.len();
        for a in args.into_iter().rev() {
            self.stack.push(a);
        }
        self.stack.push(Value::Int(argc as i32));
        self.frames.push(Frame {
            script,
            pc: at,
            base: self.stack.len(),
            argc,
            case: Value::Int(0),
            skip: [None; 3],
            dtor: None,
            ret: None,
            cont,
            func: hash,
            saved_wait: None,
        });
        Ok(())
    }

    /// Where the thread is, for messages.
    pub fn whereabouts(&self) -> String {
        match self.frames.last() {
            Some(f) => format!("{}:{} ({:#x})", f.script.name, self.line, f.pc),
            None => "ended".into(),
        }
    }

    /// Unwinds to the frame of the innermost skip handler for `button` (0 left, 1 right) and
    /// jumps to it. False when no frame has one.
    pub fn fire_skip(&mut self, button: usize) -> bool {
        let Some(fi) = self
            .frames
            .iter()
            .rposition(|f| f.skip[button].is_some() || f.skip[2].is_some())
        else {
            return false;
        };
        let f = &self.frames[fi];
        let s = f.skip[button].or(f.skip[2]).unwrap();
        self.frames.truncate(fi + 1);
        self.stack.truncate(s.height);
        let f = self.frames.last_mut().unwrap();
        f.pc = s.target;
        f.skip = [None; 3];
        self.wait = None;
        true
    }

    pub fn has_skip(&self) -> bool {
        self.frames
            .iter()
            .any(|f| f.skip.iter().any(Option::is_some))
    }

    /// Delivers the result of a finished wait or syscall.
    pub fn resume(&mut self, v: Value, cont: Cont) {
        if cont == Cont::Push {
            self.stack.push(v);
        }
    }

    fn local(&mut self, o: i16) -> &mut Value {
        let base = self.frames.last().unwrap().base as isize;
        let i = (base + o as isize).max(0) as usize;
        if i >= self.stack.len() {
            self.stack.resize(i + 1, Value::Int(0));
        }
        &mut self.stack[i]
    }

    fn load(&mut self, host: &mut dyn Host, op: &Op) -> Value {
        let t = type_of(op.flags);
        match scope_of(op.flags) {
            3 => self.local(op.var).clone(),
            2 => self
                .vars
                .entry(op.hash)
                .or_insert_with(|| Value::default_of(t))
                .clone(),
            s => match host.global(s, op.hash) {
                Some(v) => v.clone(),
                None => {
                    let v = Value::default_of(t);
                    host.set_global(s, op.hash, v.clone());
                    v
                }
            },
        }
    }

    fn store(&mut self, host: &mut dyn Host, op: &Op, v: Value) {
        match scope_of(op.flags) {
            3 => *self.local(op.var) = v,
            2 => {
                self.vars.insert(op.hash, v);
            }
            s => host.set_global(s, op.hash, v),
        }
    }

    /// The array a variable holds, made (empty) when it holds none yet.
    fn array(
        &mut self,
        host: &mut dyn Host,
        op: &Op,
    ) -> std::rc::Rc<std::cell::RefCell<crate::value::Array>> {
        if let Value::Arr(a) = self.load(host, op) {
            return a;
        }
        let a = std::rc::Rc::new(std::cell::RefCell::new(crate::value::Array::new(
            type_of(op.flags).min(2),
            vec![0],
        )));
        self.store(host, op, Value::Arr(a.clone()));
        a
    }

    fn indices(&mut self, n: usize) -> Vec<i32> {
        let mut idx = vec![0; n];
        for i in (0..n).rev() {
            idx[i] = self.pop().int();
        }
        idx
    }

    /// Runs until the thread waits or ends, at most `budget` instructions.
    pub fn run(&mut self, host: &mut dyn Host, mut budget: usize) -> Result<Stop, String> {
        loop {
            if self.wait.is_some() {
                return Ok(Stop::Wait);
            }
            let Some(frame) = self.frames.last() else {
                return Ok(Stop::Ended);
            };
            if budget == 0 {
                return Ok(Stop::Budget);
            }
            budget -= 1;
            let script = frame.script.clone();
            let op = decode(&script.code, frame.pc).map_err(|e| format!("{}: {e}", script.name))?;
            self.frames.last_mut().unwrap().pc = op.next;
            self.exec(host, &op)
                .map_err(|e| format!("{}:{} ({:#x}): {e}", script.name, self.line, op.at))?;
        }
    }

    fn jump(&mut self, op: &Op) {
        self.frames.last_mut().unwrap().pc = (op.next as i64 + op.jump as i64) as usize;
    }

    fn exec(&mut self, host: &mut dyn Host, op: &Op) -> Result<(), String> {
        let c = op.code;
        match c {
            0x100..=0x18f => {
                let b = self.pop();
                let a = self.pop();
                self.stack.push(binary(c, a, b)?);
            }
            0x190 | 0x191 => {
                let a = self.pop();
                self.stack.push(Value::Int((a.int() == 0) as i32));
            }
            0x198 => {
                let a = self.pop();
                self.stack.push(Value::Int(!a.int()));
            }
            0x1a0 => {
                let a = self.pop();
                self.stack.push(Value::Int(a.int().wrapping_neg()));
            }
            0x1a1 => {
                let a = self.pop();
                self.stack.push(Value::Float(-a.float()));
            }
            0x1a8 | 0x1a9 => {}
            0x1b0..=0x320 => {
                let (pop, elem, opi) = match c {
                    0x1b0..=0x20f => (false, false, (c - 0x1b0) / 8),
                    0x210..=0x26f => (true, false, (c - 0x210) / 8),
                    0x270..=0x2cf => (false, true, (c - 0x270) / 8),
                    _ => (true, true, (c - 0x2d0) / 8),
                };
                let t = type_of(op.flags);
                let new = if elem {
                    let idx = self.indices(dims_of(op.flags).max(1));
                    let v = self.pop();
                    let arr = self.array(host, op);
                    let mut a = arr.borrow_mut();
                    let kind = a.kind;
                    // a one dimension array grows to take the element
                    if a.dims.len() == 1
                        && idx.len() == 1
                        && idx[0] >= 0
                        && idx[0] as usize >= a.dims[0]
                    {
                        let n = idx[0] as usize + 1;
                        a.dims[0] = n;
                        let d = Value::default_of(kind);
                        a.data.resize(n, d);
                    }
                    let Some(i) = a.index(&idx) else {
                        if !pop {
                            self.stack.push(v);
                        }
                        return Ok(());
                    };
                    let new = assign(opi, a.data[i].clone(), v.to_kind(kind))?;
                    a.data[i] = new.clone();
                    new
                } else {
                    let v = self.pop();
                    let new = if opi == 0 {
                        v.to_kind(t)
                    } else {
                        assign(opi, self.load(host, op), v.to_kind(t))?
                    };
                    self.store(host, op, new.clone());
                    new
                };
                if !pop {
                    self.stack.push(new);
                }
            }
            0x800 => self.stack.push(Value::Int(op.int)),
            0x801 => self
                .stack
                .push(Value::string(encoding_rs::SHIFT_JIS.decode(&op.text).0)),
            0x803 => self.stack.push(Value::Float(op.float)),
            0x802 | 0x837 => {
                let v = if c == 0x837 {
                    let idx = self.indices(dims_of(op.flags).max(1));
                    match self.load(host, op) {
                        Value::Arr(a) => {
                            let a = a.borrow();
                            a.index(&idx)
                                .map(|i| a.data[i].clone())
                                .unwrap_or_else(|| Value::default_of(a.kind))
                        }
                        Value::Str(s) => {
                            // a string indexed: its character
                            let i = idx[0].max(0) as usize;
                            Value::string(s.chars().nth(i).map(String::from).unwrap_or_default())
                        }
                        _ => Value::Int(0),
                    }
                } else {
                    self.load(host, op)
                };
                let modifier = op.flags & 7;
                let v = if modifier != 0 && c == 0x802 {
                    let one = |v: &Value, d: i32| match v {
                        Value::Float(x) => Value::Float(x + d as f32),
                        _ => Value::Int(v.int().wrapping_add(d)),
                    };
                    let d = if modifier == 1 || modifier == 3 {
                        1
                    } else {
                        -1
                    };
                    let new = one(&v, d);
                    self.store(host, op, new.clone());
                    if modifier <= 2 { new } else { v }
                } else {
                    v
                };
                let v = match (op.flags >> 3) & 3 {
                    1 => match v {
                        Value::Float(x) => Value::Float(-x),
                        v => Value::Int(v.int().wrapping_neg()),
                    },
                    2 => Value::Int((v.int() == 0) as i32),
                    3 => Value::Int(!v.int()),
                    _ => v,
                };
                self.stack.push(v);
            }
            0x80f | 0x810 => {
                let n = op.argc as usize;
                let at = self.stack.len().saturating_sub(n);
                let mut args: Vec<Value> = self.stack.split_off(at);
                args.reverse();
                let cont = if c == 0x80f { Cont::Push } else { Cont::Drop };
                self.call(host, op.hash, args, cont)?;
            }
            0x834 | 0x835 => {
                let n = op.argc as usize;
                let at = self.stack.len().saturating_sub(n);
                let mut args: Vec<Value> = self.stack.split_off(at);
                args.reverse();
                let cont = if c == 0x834 { Cont::Push } else { Cont::Drop };
                match host.syscall(self, op.hash, args)? {
                    Ret::Val(v) => self.resume(v, cont),
                    Ret::Wait(w) => self.wait = Some((w, cont)),
                    Ret::Call(f, args) => self.call(host, f, args, cont)?,
                    Ret::Calls(calls) => {
                        self.resume(Value::Int(0), cont);
                        for (f, args) in calls {
                            self.call(host, f, args, Cont::Drop)?;
                        }
                    }
                    Ret::Jump(f, first) => {
                        self.frames.clear();
                        self.stack.clear();
                        self.call(host, f, Vec::new(), Cont::Drop)?;
                        for (f, args) in first.into_iter().rev() {
                            self.call(host, f, args, Cont::Drop)?;
                        }
                    }
                    Ret::End => self.unwind(),
                }
            }
            0x829 => {
                for &t in &op.types {
                    self.stack.push(Value::default_of(t));
                }
            }
            0x836 => {
                // argument types: convert ints handed to float parameters
                let base = self.frames.last().unwrap().base;
                for (k, &t) in op.types.iter().enumerate() {
                    let i = base as isize - 2 - k as isize;
                    if i >= 0 && t == 1 {
                        let v = self.stack[i as usize].clone();
                        self.stack[i as usize] = v.to_kind(1);
                    }
                }
            }
            0x82b => {
                let f = self.frames.last_mut().unwrap();
                let v = if self.stack.len() > f.base {
                    self.stack.pop().unwrap()
                } else {
                    Value::Int(0)
                };
                let f = self.frames.last_mut().unwrap();
                if let Some(d) = f.dtor.take() {
                    f.ret = Some(v);
                    f.pc = d;
                    return Ok(());
                }
                let v = f.ret.take().unwrap_or(v);
                let f = self.frames.pop().unwrap();
                self.stack.truncate(f.base.saturating_sub(1 + f.argc));
                if self.unwinding {
                    self.next_dtor();
                    return Ok(());
                }
                if f.cont == Cont::Push {
                    self.stack.push(v);
                }
                if f.saved_wait.is_some() {
                    self.wait = f.saved_wait;
                }
            }
            0x82c => self.jump(op),
            0x82d => {
                if self.pop().int() != 0 {
                    self.jump(op)
                }
            }
            0x82e => {
                if self.pop().int() == 0 {
                    self.jump(op)
                }
            }
            0x82f => {
                self.pop();
            }
            0x830 => {
                let v = self.pop();
                self.frames.last_mut().unwrap().case = v;
                self.jump(op);
            }
            0x831..=0x833 | 0x838 | 0x839 => {
                let v = self.pop();
                let r = self.frames.last().unwrap().case.clone();
                let ord = match (&r, &v) {
                    (Value::Str(a), Value::Str(b)) => a.cmp(b),
                    (Value::Float(_), _) | (_, Value::Float(_)) => r
                        .float()
                        .partial_cmp(&v.float())
                        .unwrap_or(std::cmp::Ordering::Equal),
                    _ => r.int().cmp(&v.int()),
                };
                use std::cmp::Ordering::*;
                let hit = match c {
                    0x831 => ord != Equal,
                    0x832 => ord != Less,
                    0x833 => ord != Greater,
                    0x838 => ord == Less,
                    _ => ord == Greater,
                };
                if hit {
                    self.jump(op);
                }
            }
            0x83a => self.line = op.int,
            0x83b | 0x83d | 0x843 => {
                let height = self.stack.len();
                let target = (op.next as i64 + op.jump as i64) as usize;
                let k = match c {
                    0x83b => 0,
                    0x83d => 1,
                    _ => 2,
                };
                self.frames.last_mut().unwrap().skip[k] = Some(Skip { target, height });
            }
            0x83c | 0x845 => {}
            0x844 => self.frames.last_mut().unwrap().skip = [None; 3],
            0x846 => {}
            0x847 => {
                let target = (op.next as i64 + op.jump as i64) as usize;
                self.frames.last_mut().unwrap().dtor = Some(target);
            }
            0x83e => {
                let v = self.pop();
                self.stack.push(Value::Int(v.int()));
            }
            0x83f => {
                let v = self.pop();
                self.stack.push(Value::Float(v.float()));
            }
            0x840 => host.text(self, &op.text)?,
            0x841 => host.proc_text(self),
            0x842 => {
                if let Some(w) = host.ctrl(self, &op.text)? {
                    self.wait = Some((w, Cont::Drop));
                }
            }
            0x850 => {
                let i = self.pop().int();
                if i >= 0 && (i as usize) < op.cases.len() {
                    let n = op.cases.len();
                    let end_of_entry = op.next - 4 * (n - 1 - i as usize);
                    self.frames.last_mut().unwrap().pc =
                        (end_of_entry as i64 + op.cases[i as usize] as i64) as usize;
                }
            }
            _ => return Err(format!("opcode {c:#x}")),
        }
        Ok(())
    }
}

fn assign(opi: u16, old: Value, v: Value) -> Result<Value, String> {
    // st, st.mul, st.div, st.rem, st.add, st.sub, st.shl, st.shr, st.and, st.xor, st.or
    let bin = match opi {
        0 => return Ok(v),
        1 => 0x100,
        2 => 0x108,
        3 => 0x110,
        4 => 0x118,
        5 => 0x120,
        6 => 0x130,
        7 => 0x128,
        8 => 0x180,
        9 => 0x168,
        _ => 0x188,
    };
    let t = match old {
        Value::Float(_) => 1,
        Value::Str(_) => 2,
        _ => 0,
    };
    binary(bin + t, old, v)
}

pub fn binary(c: u16, a: Value, b: Value) -> Result<Value, String> {
    use Value::*;
    let kind = (c & 7) as u8;
    let base = c & !7;
    let cmp = |o: std::cmp::Ordering| -> Value {
        use std::cmp::Ordering::*;
        Int(match base {
            0x138 => o != Greater,
            0x140 => o == Less,
            0x148 => o != Less,
            0x150 => o == Greater,
            0x158 => o == Equal,
            _ => o != Equal,
        } as i32)
    };
    if (0x138..=0x167).contains(&base) {
        let o = match kind {
            1 => a
                .float()
                .partial_cmp(&b.float())
                .unwrap_or(std::cmp::Ordering::Less),
            2 => a.str().cmp(&b.str()),
            3..=5 => {
                let same = match (&a, &b) {
                    (Arr(x), Arr(y)) => std::rc::Rc::ptr_eq(x, y),
                    _ => false,
                };
                if same {
                    std::cmp::Ordering::Equal
                } else {
                    std::cmp::Ordering::Less
                }
            }
            _ => a.int().cmp(&b.int()),
        };
        return Ok(cmp(o));
    }
    if kind == 1 {
        let (x, y) = (a.float(), b.float());
        return Ok(Float(match base {
            0x100 => x * y,
            0x108 => {
                if y == 0.0 {
                    0.0
                } else {
                    x / y
                }
            }
            0x118 => x + y,
            0x120 => x - y,
            _ => return Err(format!("float operator {c:#x}")),
        }));
    }
    if kind == 2 {
        if base == 0x118 {
            return Ok(Value::string(format!("{}{}", a.str(), b.str())));
        }
        return Err(format!("string operator {c:#x}"));
    }
    let (x, y) = (a.int(), b.int());
    Ok(Int(match base {
        0x100 => x.wrapping_mul(y),
        0x108 => {
            if y == 0 {
                0
            } else {
                x.wrapping_div(y)
            }
        }
        0x110 => {
            if y == 0 {
                0
            } else {
                x.wrapping_rem(y)
            }
        }
        0x118 => x.wrapping_add(y),
        0x120 => x.wrapping_sub(y),
        0x128 => x.wrapping_shr(y as u32),
        0x130 => x.wrapping_shl(y as u32),
        0x168 => x ^ y,
        0x170 => (x != 0 && y != 0) as i32,
        0x178 => (x != 0 || y != 0) as i32,
        0x180 => x & y,
        0x188 => x | y,
        _ => return Err(format!("operator {c:#x}")),
    }))
}
