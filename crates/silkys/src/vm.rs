//! The stack machine of the `.mes` scripts. A statement pushes its operands and ends in a store,
//! a jump, a library call or a system call; system calls pop their number, then a sub-function
//! number where they have one, then their arguments, and push a result. `LIBLARY.LIB` registers
//! the library routines (argument count, number, body) that scripts call by number.

use std::rc::Rc;

use crate::mes::{Mes, Op};

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Int(i32),
    Str(Vec<u8>),
}

impl Val {
    pub fn int(&self) -> i32 {
        match self {
            Val::Int(i) => *i,
            Val::Str(_) => 0,
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Val::Int(i) => i.to_string().into_bytes(),
            Val::Str(s) => s.clone(),
        }
    }
}

impl Default for Val {
    fn default() -> Val {
        Val::Int(0)
    }
}

/// A place the code runs: a script or the library, and where in it.
#[derive(Clone)]
pub struct Frame {
    pub code: Rc<Mes>,
    pub pc: usize,
    pub name: String,
    /// the arguments of a library routine
    pub args: Vec<Val>,
    /// the stack depth when the routine was called
    pub base: usize,
}

/// What a run of the machine stopped at.
pub enum Stop {
    /// a system call: its number; the arguments are on the stack
    Syscall(i32),
    /// text to show
    Text(Vec<u8>),
    /// the end of the script (or a return from it)
    End,
}

#[derive(Default)]
pub struct Vm {
    pub stack: Vec<Val>,
    pub frames: Vec<Frame>,
    /// library routines by number: argument count and entry
    pub lib: Vec<Option<(usize, usize)>>,
    pub lib_code: Option<Rc<Mes>>,
    /// 0x02 / 0x0c: bytes (flags)
    pub g1: Vec<u8>,
    /// 0x03 / 0x0d: 16-bit
    pub g2: Vec<i16>,
    /// 0x04 / 0x0e: values (system variables)
    pub g3: Vec<Val>,
    /// 0x05 / 0x0f: values
    pub g4: Vec<Val>,
    /// 0x07..=0x09 / 0x11..=0x13: a byte memory read as 8, 16 or 32 bits
    pub g5: Vec<u8>,
    pub trace: bool,
    /// the last message mark passed in the bottom script (for saves)
    pub last_msg: i32,
    /// the sound effect channel chosen by the last Effect call
    pub g5_effect: i32,
    /// the stack depth where the statement being run began
    pub mark: usize,
}

fn grow<T: Clone + Default>(v: &mut Vec<T>, i: usize) {
    if i >= v.len() && i < 1 << 20 {
        v.resize(i + 1, T::default());
    }
}

impl Vm {
    pub fn new() -> Vm {
        Vm {
            g1: vec![0; 8192],
            g2: vec![0; 4096],
            g3: vec![Val::Int(0); 1024],
            g4: vec![Val::Int(0); 1024],
            g5: vec![0; 65536],
            ..Default::default()
        }
    }

    pub fn pop(&mut self) -> Val {
        let base = self.frames.last().map_or(0, |f| f.base);
        if self.stack.len() > base {
            self.stack.pop().unwrap()
        } else {
            Val::Int(0)
        }
    }

    pub fn pop_int(&mut self) -> i32 {
        self.pop().int()
    }

    pub fn push(&mut self, v: Val) {
        self.stack.push(v);
    }

    /// Registers the library's routines: it is run once from its start, each LibReg taking
    /// the argument count and number off the stack and skipping the body.
    pub fn load_lib(&mut self, lib: Mes) -> Result<(), String> {
        let mut pc = 0;
        let mut st: Vec<i32> = Vec::new();
        while pc < lib.code.len() {
            let (op, next) = lib.op(pc)?;
            match op {
                Op::Int(i) => st.push(i),
                Op::LibReg(end) => {
                    let id = st.pop().unwrap_or(0) as usize;
                    let n = st.pop().unwrap_or(0) as usize;
                    grow(&mut self.lib, id);
                    if id < self.lib.len() {
                        self.lib[id] = Some((n, next));
                    }
                    st.clear();
                    pc = end as usize;
                    continue;
                }
                _ => st.clear(),
            }
            pc = next;
        }
        self.lib_code = Some(Rc::new(lib));
        Ok(())
    }

    pub fn start(&mut self, name: &str, code: Rc<Mes>) {
        self.frames.clear();
        self.stack.clear();
        self.mark = 0;
        self.frames.push(Frame {
            code,
            pc: 0,
            name: name.to_string(),
            args: Vec::new(),
            base: 0,
        });
    }

    pub fn call_lib(&mut self, id: usize) -> Result<(), String> {
        let (n, entry) = self
            .lib
            .get(id)
            .copied()
            .flatten()
            .ok_or_else(|| format!("no library routine {id}"))?;
        let mut args = vec![Val::Int(0); n];
        // the first argument is the last one pushed
        for a in args.iter_mut() {
            *a = self.pop();
        }
        let code = self.lib_code.clone().ok_or("no library")?;
        self.frames.push(Frame {
            code,
            pc: entry,
            name: format!("lib{id}"),
            args,
            base: self.stack.len(),
        });
        Ok(())
    }

    fn load(&mut self, kind: u8, i: i32) -> Val {
        let u = i.max(0) as usize;
        match kind {
            0x02 => Val::Int(*self.g1.get(u).unwrap_or(&0) as i32),
            0x03 => Val::Int(*self.g2.get(u).unwrap_or(&0) as i32),
            0x04 => self.g3.get(u).cloned().unwrap_or_default(),
            0x05 => self.g4.get(u).cloned().unwrap_or_default(),
            0x06 => self
                .frames
                .last()
                .and_then(|f| f.args.get(u).cloned())
                .unwrap_or_default(),
            0x07 => Val::Int(*self.g5.get(u).unwrap_or(&0) as i8 as i32),
            0x08 => {
                let b = |k| *self.g5.get(u + k).unwrap_or(&0);
                Val::Int(i16::from_le_bytes([b(0), b(1)]) as i32)
            }
            _ => {
                let b = |k| *self.g5.get(u + k).unwrap_or(&0);
                Val::Int(i32::from_le_bytes([b(0), b(1), b(2), b(3)]))
            }
        }
    }

    pub fn store(&mut self, kind: u8, i: i32, v: Val) {
        let u = i.max(0) as usize;
        match kind {
            0x0c => {
                grow(&mut self.g1, u);
                if let Some(x) = self.g1.get_mut(u) {
                    *x = v.int() as u8;
                }
            }
            0x0d => {
                grow(&mut self.g2, u);
                if let Some(x) = self.g2.get_mut(u) {
                    *x = v.int() as i16;
                }
            }
            0x0e => {
                grow(&mut self.g3, u);
                if let Some(x) = self.g3.get_mut(u) {
                    *x = v;
                }
            }
            0x0f => {
                grow(&mut self.g4, u);
                if let Some(x) = self.g4.get_mut(u) {
                    *x = v;
                }
            }
            0x10 => {
                if let Some(x) = self.frames.last_mut().and_then(|f| f.args.get_mut(u)) {
                    *x = v;
                }
            }
            k => {
                let n = match k {
                    0x11 => 1,
                    0x12 => 2,
                    _ => 4,
                };
                grow(&mut self.g5, u + n);
                let b = v.int().to_le_bytes();
                if u + n <= self.g5.len() {
                    self.g5[u..u + n].copy_from_slice(&b[..n]);
                }
            }
        }
    }

    pub fn g3i(&self, i: usize) -> i32 {
        self.g3.get(i).map_or(0, |v| v.int())
    }

    /// Runs until a system call, text, or the end.
    pub fn run(&mut self) -> Result<Stop, String> {
        let mut steps = 0;
        loop {
            steps += 1;
            if steps > 5_000_000 {
                return Err("script loops".into());
            }
            let Some(f) = self.frames.last() else {
                return Ok(Stop::End);
            };
            let code = f.code.clone();
            let pc = f.pc;
            if pc >= code.code.len() {
                if self.frames.len() == 1 {
                    return Ok(Stop::End);
                }
                self.frames.pop();
                continue;
            }
            let (op, next) = code.op(pc).map_err(|e| format!("{}: {e}", f.name))?;
            if self.trace {
                eprintln!("{} {pc:06x} {op:?} {:?}", f.name, self.stack);
            }
            self.frames.last_mut().unwrap().pc = next;
            let ends = matches!(
                op,
                Op::Store(_)
                    | Op::Jz(_)
                    | Op::Jmp(_)
                    | Op::LibCall
                    | Op::Ret
                    | Op::Yield
                    | Op::Text(_)
                    | Op::MsgId(_)
                    | Op::MsgId2(_)
            );
            let stop = self.step(op)?;
            if ends {
                self.mark = self.stack.len();
            }
            if let Some(stop) = stop {
                return Ok(stop);
            }
        }
    }

    fn step(&mut self, op: Op) -> Result<Option<Stop>, String> {
        match op {
            Op::Int(i) => self.push(Val::Int(i)),
            Op::Str(s) => self.push(Val::Str(s)),
            Op::Load(k) => {
                let i = self.pop_int();
                let v = self.load(k, i);
                self.push(v);
            }
            Op::Store(k) => {
                let i = self.pop_int();
                let v = self.pop();
                self.store(k, i, v);
            }
            Op::Bin(0x39) => {
                // rand: a number below the operand
                let n = self.pop_int();
                let r = if n > 0 { (rand() % n as u32) as i32 } else { 0 };
                self.push(Val::Int(r));
            }
            Op::Bin(b) => {
                let y = self.pop();
                let x = self.pop();
                let (a, c) = (x.int(), y.int());
                let r = match b {
                    0x34 => {
                        if let (Val::Str(s), Val::Str(t)) = (&x, &y) {
                            let mut s = s.clone();
                            s.extend(t);
                            self.push(Val::Str(s));
                            return Ok(None);
                        }
                        a.wrapping_add(c)
                    }
                    0x35 => a.wrapping_sub(c),
                    0x36 => a.wrapping_mul(c),
                    0x37 => a.checked_div(c).unwrap_or(0),
                    0x38 => a.checked_rem(c).unwrap_or(0),
                    0x3a => (a != 0 && c != 0) as i32,
                    0x3b => (a != 0 || c != 0) as i32,
                    0x3c => a & c,
                    0x3d => a | c,
                    0x3e => (a < c) as i32,
                    0x3f => (a > c) as i32,
                    0x40 => (a <= c) as i32,
                    0x41 => (a >= c) as i32,
                    0x42 => match (&x, &y) {
                        (Val::Str(s), Val::Str(t)) => s.eq_ignore_ascii_case(t) as i32,
                        _ => (a == c) as i32,
                    },
                    _ => match (&x, &y) {
                        (Val::Str(s), Val::Str(t)) => !s.eq_ignore_ascii_case(t) as i32,
                        _ => (a != c) as i32,
                    },
                };
                self.push(Val::Int(r));
            }
            Op::Jz(a) => {
                if self.pop_int() == 0 {
                    self.frames.last_mut().unwrap().pc = a as usize;
                }
            }
            Op::Jmp(a) => self.frames.last_mut().unwrap().pc = a as usize,
            Op::LibCall => {
                let id = self.pop_int();
                self.call_lib(id as usize)?;
            }
            Op::Syscall => {
                let id = self.pop_int();
                return Ok(Some(Stop::Syscall(id)));
            }
            Op::Ret => {
                let f = self.frames.pop().unwrap();
                self.stack.truncate(f.base);
                if self.frames.is_empty() {
                    return Ok(Some(Stop::End));
                }
            }
            Op::Yield => {
                // a return with a value (the value is dropped)
                let _ = self.pop();
                let f = self.frames.pop().unwrap();
                self.stack.truncate(f.base);
                if self.frames.is_empty() {
                    return Ok(Some(Stop::End));
                }
            }
            Op::Text(s) => return Ok(Some(Stop::Text(s))),
            Op::Escape(e) => {
                if e == 0 {
                    return Ok(Some(Stop::Text(b"\n".to_vec())));
                }
            }
            Op::MsgId(n) => {
                if self.frames.len() == 1 {
                    self.last_msg = n;
                }
            }
            Op::MsgId2(_) | Op::Mark(_) | Op::Choice(_) | Op::LibReg(_) => {}
        }
        Ok(None)
    }
}

/// A small random number source (xorshift), the same on every run.
pub fn rand() -> u32 {
    use std::cell::Cell;
    thread_local!(static S: Cell<u32> = const { Cell::new(0x1234_5678) });
    S.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        s.set(x);
        x
    })
}
