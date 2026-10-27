//! The script machine of NeXAS releases from about 2010 on.
//!
//! Two registers, A and B (`regs[0]`, `regs[1]`), and a stack of typed arguments. Integers are
//! 32-bit. A string is passed around as a handle: a literal (index into the running block's
//! strings), a string variable (0x80000000 | index, plus 0x40000000 for a global one) or one of
//! three temporaries (-1, -2, -3) that results land in.
//!
//! | op | does |
//! |----|------|
//! | 0x00 | A = arg |
//! | 0x01 / 0x02 / 0x03 | early scripts: jump / jump if A == 0 / branch label |
//! | 0x04 / 0x06 | push reg[arg] / pop into reg[arg] |
//! | 0x05 | push A (arg 0: as an integer, else the string A refers to) |
//! | 0x07 | call arg & 0xFFFF with arg >> 16 arguments: a builtin, or (0x8000) a function of the file |
//! | 0x08 | reg[arg] = variable reg[arg] |
//! | 0x09..0x1A | A = B op A (0x09 with arg: string concatenation, 0x19/0x1A with arg: string compare) |
//! | 0x0E | variable arg = A (arg -1: variable A = B); string variables take a copy |
//! | 0x1B | label arg (where the engine can start a file) |
//! | 0x1C | end of this frame's work (yield) |
//! | 0x22 / 0x23 | variable arg (-1: A) += 1 / -= 1 |
//! | 0x2F..0x3A | shifts, compound assignment (arg -1: variable A op= B, else variable arg op= A) |
//! | 0x3F / 0x44 | reg[arg] to a string / string to an integer |
//! | 0x40 | return (arg: A is a string) |
//! | 0x41 / 0x42 / 0x43 | jump / if A == 0 / if A != 0: continue after instruction arg |
//!
//! Variables: bit 0x40000000 picks the globals, else the running block's locals (each function
//! call has its own).

use std::rc::Rc;

use crate::script::{Block, Ins, Script};
use crate::sjis;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i32),
    Str(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoCalls;
    impl Host for NoCalls {
        fn builtin(
            &mut self,
            _: &mut Thread,
            _: &mut Globals,
            _: u32,
            _: &[Value],
        ) -> Result<(Value, Flow), String> {
            panic!("unexpected builtin")
        }
    }

    #[test]
    fn early_branches_skip_to_next_label_in_the_same_group() {
        let code = vec![
            Ins { op: 0, arg: 0 },
            Ins {
                op: 2,
                arg: 0x10001,
            },
            Ins { op: 0, arg: 10 },
            Ins {
                op: 3,
                arg: 0x20001,
            },
            Ins { op: 0, arg: 20 },
            Ins {
                op: 3,
                arg: 0x10002,
            },
            Ins { op: 0, arg: 30 },
            Ins {
                op: 1,
                arg: 0x1ffff,
            },
            Ins { op: 0, arg: 40 },
            Ins {
                op: 3,
                arg: 0x1ffff,
            },
            Ins { op: 0, arg: 50 },
        ];
        let script = Rc::new(Script {
            main: Block {
                code,
                ..Block::default()
            },
            ..Script::default()
        });
        let mut thread = Thread::new("test", script);
        let mut globals = Globals::default();
        let mut host = NoCalls;
        thread.step(&mut globals, &mut host).unwrap();
        thread.step(&mut globals, &mut host).unwrap();
        assert_eq!(thread.frames[0].ip, 6);
        thread.step(&mut globals, &mut host).unwrap();
        assert_eq!(thread.regs[0], 30);
        thread.step(&mut globals, &mut host).unwrap();
        assert_eq!(thread.frames[0].ip, 10);
        thread.step(&mut globals, &mut host).unwrap();
        assert_eq!(thread.regs[0], 50);
    }
}

impl Value {
    pub fn int(&self) -> i32 {
        match self {
            Value::Int(i) => *i,
            Value::Str(s) => atoi(s),
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Value::Int(i) => i.to_string().into_bytes(),
            Value::Str(s) => s.clone(),
        }
    }
    pub fn text(&self) -> String {
        sjis::decode(&self.bytes())
    }
}

fn atoi(s: &[u8]) -> i32 {
    let s = std::str::from_utf8(s).unwrap_or("").trim();
    let end = s
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))))
        .map_or(s.len(), |(i, _)| i);
    s[..end].parse().unwrap_or(0)
}

const GLOBAL: u32 = 0x4000_0000;
const STRING: u32 = 0x8000_0000;

/// One running block: a file's main code or a called function.
#[derive(Debug, Clone)]
pub struct Frame {
    /// the function, or None for the main code
    pub func: Option<u32>,
    /// running the main code's init block (run_init)
    pub init: bool,
    pub ip: usize,
    pub ints: Vec<i32>,
    pub strs: Vec<Vec<u8>>,
}

/// What a builtin does to the thread that called it.
#[derive(Debug, Clone, PartialEq)]
pub enum Flow {
    Next,
    /// stop running this thread for this frame (the call is done; the thread goes on after it)
    Yield,
    /// stop this thread for good
    End,
    /// load a file and start at a label (the thread's frames and stack are dropped)
    Jump {
        file: String,
        label: u32,
    },
}

pub trait Host {
    /// Runs builtin `id`. Its result goes to A.
    fn builtin(
        &mut self,
        vm: &mut Thread,
        globals: &mut Globals,
        id: u32,
        args: &[Value],
    ) -> Result<(Value, Flow), String>;
}

#[derive(Debug, Default)]
pub struct Globals {
    pub ints: Vec<i32>,
    pub strs: Vec<Vec<u8>>,
}

impl Globals {
    fn int(&mut self, i: usize) -> &mut i32 {
        if self.ints.len() <= i {
            self.ints.resize(i + 1, 0);
        }
        &mut self.ints[i]
    }
    fn str(&mut self, i: usize) -> &mut Vec<u8> {
        if self.strs.len() <= i {
            self.strs.resize(i + 1, Vec::new());
        }
        &mut self.strs[i]
    }
}

/// A script thread: the engine runs the main one and a few system ones.
#[derive(Clone)]
pub struct Thread {
    pub name: String,
    pub script: Rc<Script>,
    pub frames: Vec<Frame>,
    pub regs: [i32; 8],
    pub stack: Vec<Value>,
    temps: [Vec<u8>; 3],
    next_temp: usize,
    pub done: bool,
    pub trace: bool,
}

pub enum Step {
    Next,
    Yield,
    End,
    Jump { file: String, label: u32 },
}

impl Thread {
    pub fn new(name: &str, script: Rc<Script>) -> Thread {
        let main = Frame {
            func: None,
            init: false,
            ip: 0,
            ints: vec![0; script.main.int_names.len()],
            strs: vec![Vec::new(); script.main.str_names.len()],
        };
        Thread {
            name: name.to_string(),
            script,
            frames: vec![main],
            regs: [0; 8],
            stack: Vec::new(),
            temps: Default::default(),
            next_temp: 0,
            done: false,
            trace: false,
        }
    }

    /// Runs the init block of the main code, which sets the file's variables; the thread then
    /// goes on where it was.
    pub fn run_init(&mut self, g: &mut Globals, host: &mut dyn Host) -> Result<(), String> {
        let ip = self.frames[0].ip;
        self.frames[0].init = true;
        self.frames[0].ip = 0;
        while !self.done {
            if let Step::Jump { .. } = self.step(g, host)? {
                break;
            }
        }
        self.frames.truncate(1);
        self.frames[0].init = false;
        self.frames[0].ip = ip;
        self.stack.clear();
        self.done = false;
        Ok(())
    }

    /// Starts the main code at label `label` (op 0x1B); false if there is none.
    pub fn goto_label(&mut self, label: u32) -> bool {
        let Some(at) = self
            .script
            .main
            .code
            .iter()
            .position(|i| i.op == 0x1b && i.arg == label)
        else {
            return false;
        };
        self.frames.truncate(1);
        self.frames[0].ip = at;
        self.stack.clear();
        self.done = false;
        true
    }

    /// Starts the main code at bank `label` with its parameters (99999: the bank's default).
    pub fn enter_bank(&mut self, label: u32, params: &[i32]) -> bool {
        let bank = self
            .script
            .main
            .banks
            .iter()
            .find(|b| b.label == label as i32)
            .copied();
        if !self.goto_label(label) {
            return false;
        }
        if let Some(bank) = bank {
            for k in 0..8 {
                let Ok(var) = usize::try_from(bank.vars[k]) else {
                    continue;
                };
                let v = match params.get(k) {
                    Some(&p) if p != 99999 => p,
                    _ => bank.defaults[k],
                };
                let ints = &mut self.frames[0].ints;
                if ints.len() <= var {
                    ints.resize(var + 1, 0);
                }
                ints[var] = v;
            }
        }
        true
    }

    fn block_of(&self, f: &Frame) -> &Block {
        match f.func {
            Some(id) => self.script.funcs.get(&id).unwrap_or(&self.script.main),
            None => &self.script.main,
        }
    }

    fn code_of(&self, f: &Frame) -> &[Ins] {
        let b = self.block_of(f);
        if f.init { &b.init } else { &b.code }
    }

    fn frame(&mut self) -> &mut Frame {
        self.frames.last_mut().unwrap()
    }

    fn int_var<'a>(&'a mut self, g: &'a mut Globals, r: u32) -> &'a mut i32 {
        if r & GLOBAL != 0 {
            return g.int((r & !GLOBAL & !STRING) as usize);
        }
        let f = self.frames.last_mut().unwrap();
        let i = (r & !STRING) as usize;
        if f.ints.len() <= i {
            f.ints.resize(i + 1, 0);
        }
        &mut f.ints[i]
    }

    /// The string a handle refers to.
    pub fn str_of(&self, g: &Globals, h: i32) -> Vec<u8> {
        let r = h as u32;
        if (-3..=-1).contains(&h) {
            return self.temps[(-h - 1) as usize].clone();
        }
        if r & STRING != 0 {
            let i = (r & !GLOBAL & !STRING) as usize;
            return if r & GLOBAL != 0 {
                g.strs.get(i).cloned().unwrap_or_default()
            } else {
                self.frames
                    .last()
                    .unwrap()
                    .strs
                    .get(i)
                    .cloned()
                    .unwrap_or_default()
            };
        }
        let f = self.frames.last().unwrap();
        self.block_of(f)
            .strings
            .get(h as usize)
            .cloned()
            .unwrap_or_default()
    }

    fn str_var<'a>(&'a mut self, g: &'a mut Globals, r: u32) -> &'a mut Vec<u8> {
        let i = (r & !GLOBAL & !STRING) as usize;
        if r & GLOBAL != 0 {
            return g.str(i);
        }
        let f = self.frames.last_mut().unwrap();
        if f.strs.len() <= i {
            f.strs.resize(i + 1, Vec::new());
        }
        &mut f.strs[i]
    }

    /// Puts a string in the next temporary; returns its handle.
    pub fn temp(&mut self, s: Vec<u8>) -> i32 {
        let k = self.next_temp;
        self.next_temp = (k + 1) % 3;
        self.temps[k] = s;
        -(k as i32 + 1)
    }

    /// Runs one instruction.
    pub fn step(&mut self, g: &mut Globals, host: &mut dyn Host) -> Result<Step, String> {
        // a function that runs off its end returns
        loop {
            let f = self.frames.last().unwrap();
            let len = self.code_of(f).len();
            if f.ip < len {
                break;
            }
            if self.frames.len() == 1 {
                self.done = true;
                return Ok(Step::End);
            }
            self.frames.pop();
            self.frame().ip += 1;
        }
        let f = self.frames.last().unwrap();
        let Ins { op, arg } = self.code_of(f)[f.ip];
        let depth = self.frames.len();
        if self.trace {
            eprintln!(
                "[{}] {}{:?}:{} {op:#x} {}",
                self.name,
                " ".repeat(depth - 1),
                f.func,
                f.ip,
                arg as i32
            );
        }
        let mut out = Step::Next;
        let (a, b) = (self.regs[0], self.regs[1]);
        let reg = (arg as usize) & 7;
        let set_a = |t: &mut Thread, v: i32| t.regs[0] = v;
        match op {
            0x00 => set_a(self, arg as i32),
            // Early scripts use named control-flow labels instead of instruction offsets.
            0x01 | 0x02 if op == 0x01 || a == 0 => {
                let at = self
                    .code_of(self.frames.last().unwrap())
                    .iter()
                    .position(|ins| {
                        ins.op == 0x03
                            && ins.arg >> 16 == arg >> 16
                            && (ins.arg & 0xffff) >= (arg & 0xffff)
                    })
                    .ok_or_else(|| format!("{}: no branch label {arg:#x}", self.name))?;
                self.frame().ip = at;
            }
            0x02 | 0x03 => {}
            0x04 => self.stack.push(Value::Int(self.regs[reg])),
            0x05 => {
                let v = if arg == 0 {
                    Value::Int(a)
                } else {
                    Value::Str(self.str_of(g, a))
                };
                self.stack.push(v);
            }
            0x06 => {
                if let Some(v) = self.stack.pop() {
                    self.regs[reg] = match v {
                        Value::Int(i) => i,
                        Value::Str(s) => self.temp(s),
                    };
                }
            }
            0x07 => {
                let id = arg & 0xffff;
                let n = (arg >> 16) as usize;
                let args = self.stack.split_off(self.stack.len().saturating_sub(n));
                if id & 0x8000 != 0 {
                    let fid = id & 0x7fff;
                    let Some(func) = self.script.funcs.get(&fid) else {
                        return Err(format!("{}: no function {fid}", self.name));
                    };
                    let mut frame = Frame {
                        func: Some(fid),
                        init: false,
                        ip: 0,
                        ints: vec![0; func.int_names.len()],
                        strs: vec![Vec::new(); func.str_names.len()],
                    };
                    let (mut ni, mut ns) = (0, 0);
                    for v in args {
                        match v {
                            Value::Int(i) => {
                                if ni < frame.ints.len() {
                                    frame.ints[ni] = i;
                                }
                                ni += 1;
                            }
                            Value::Str(s) => {
                                if ns < frame.strs.len() {
                                    frame.strs[ns] = s;
                                }
                                ns += 1;
                            }
                        }
                    }
                    self.frames.push(frame);
                    // the new frame starts at its first instruction
                    return Ok(Step::Next);
                }
                let (v, flow) = host.builtin(self, g, id, &args)?;
                self.regs[0] = match v {
                    Value::Int(i) => i,
                    Value::Str(s) => self.temp(s),
                };
                out = match flow {
                    Flow::Next => Step::Next,
                    Flow::Yield => Step::Yield,
                    Flow::End => Step::End,
                    Flow::Jump { file, label } => return Ok(Step::Jump { file, label }),
                };
            }
            0x08 => {
                let r = self.regs[reg] as u32;
                let v = *self.int_var(g, r);
                self.regs[reg] = v;
            }
            0x09 if arg != 0 => {
                let s = [self.str_of(g, b), self.str_of(g, a)].concat();
                let h = self.temp(s);
                set_a(self, h);
            }
            0x19 | 0x1a if arg != 0 => {
                let eq = self.str_of(g, b) == self.str_of(g, a);
                set_a(self, (eq == (op == 0x19)) as i32);
            }
            0x09..=0x1a => {
                let v = match op {
                    0x09 => b.wrapping_add(a),
                    0x0a => b.wrapping_sub(a),
                    0x0b => b.wrapping_mul(a),
                    0x0c => {
                        if a != 0 {
                            b.wrapping_div(a)
                        } else {
                            a
                        }
                    }
                    0x0d => {
                        if a != 0 {
                            b.wrapping_rem(a)
                        } else {
                            a
                        }
                    }
                    0x0e => {
                        let (dest, v) = if arg == u32::MAX {
                            (a as u32, b)
                        } else {
                            (arg, a)
                        };
                        if dest & STRING != 0 {
                            let s = self.str_of(g, v);
                            *self.str_var(g, dest) = s;
                        } else {
                            *self.int_var(g, dest) = v;
                        }
                        a
                    }
                    0x0f => (a != 0 || b != 0) as i32,
                    0x10 => (a != 0 && b != 0) as i32,
                    0x11 => a | b,
                    0x12 => a & b,
                    0x13 => a ^ b,
                    0x14 => !a,
                    0x15 => (a <= b) as i32,
                    0x16 => (b <= a) as i32,
                    0x17 => (a < b) as i32,
                    0x18 => (b < a) as i32,
                    0x19 => (a == b) as i32,
                    _ => (a != b) as i32,
                };
                set_a(self, v);
            }
            0x1b | 0x1d | 0x2c => {}
            0x1c => out = Step::Yield,
            0x22 | 0x23 => {
                let dest = if arg == u32::MAX { a as u32 } else { arg };
                let v = self.int_var(g, dest);
                *v = if op == 0x22 {
                    v.wrapping_add(1)
                } else {
                    v.wrapping_sub(1)
                };
            }
            0x2b => set_a(self, (a == 0) as i32),
            0x2d => set_a(self, b >> (a & 31)),
            0x2e => set_a(self, b.wrapping_shl(a as u32 & 31)),
            0x2f => set_a(self, ((b as u32) >> (a & 31)) as i32),
            0x30..=0x3a => {
                let (dest, v) = if arg == u32::MAX {
                    (a as u32, b)
                } else {
                    (arg, a)
                };
                if op == 0x30 && dest & STRING != 0 {
                    let s = self.str_of(g, v);
                    self.str_var(g, dest).extend_from_slice(&s);
                } else {
                    let x = self.int_var(g, dest);
                    *x = match op {
                        0x30 => x.wrapping_add(v),
                        0x31 => x.wrapping_sub(v),
                        0x32 => x.wrapping_mul(v),
                        0x33 => {
                            if v != 0 {
                                x.wrapping_div(v)
                            } else {
                                *x
                            }
                        }
                        0x34 => {
                            if v != 0 {
                                x.wrapping_rem(v)
                            } else {
                                *x
                            }
                        }
                        0x35 => *x | v,
                        0x36 => *x & v,
                        0x37 => *x ^ v,
                        0x38 => *x >> (v & 31),
                        0x39 => x.wrapping_shl(v as u32 & 31),
                        _ => ((*x as u32) >> (v & 31)) as i32,
                    };
                }
            }
            0x3f => {
                let s = self.regs[reg].to_string().into_bytes();
                let h = self.temp(s);
                self.regs[reg] = h;
            }
            0x44 => {
                let h = self.regs[reg];
                let s = self.str_of(g, h);
                self.regs[reg] = atoi(&s);
            }
            0x40 => {
                if arg != 0 {
                    let s = self.str_of(g, a);
                    // the temporary outlives the frame
                    let h = self.temp(s);
                    set_a(self, h);
                }
                if self.frames.len() > 1 {
                    self.frames.pop();
                } else {
                    self.done = true;
                    return Ok(Step::End);
                }
            }
            0x41 => {
                self.frame().ip = arg as usize;
            }
            0x42 => {
                if a == 0 {
                    self.frame().ip = arg as usize;
                }
            }
            0x43 => {
                if a != 0 {
                    self.frame().ip = arg as usize;
                }
            }
            _ => {
                return Err(format!(
                    "{}: unknown op {op:#x} at {}",
                    self.name,
                    self.frames.last().unwrap().ip
                ));
            }
        }
        self.frame().ip += 1;
        Ok(out)
    }
}
