//! Runs compiled ACPX C: one flat 32-bit memory (a stack region, then the heap that holds
//! globals, strings and what the game allocates), an operand stack of 32-bit values and a call
//! stack. Built-in functions go to a [`Host`]; the machine can stop after any of them (the game
//! waits for the next frame) and carry on where it stopped.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::c::code::{FN_BASE, Function, NATIVE_BASE, Op};
use crate::c::compile::Target;

/// Frames of called functions live here.
pub const STACK_BASE: u32 = 0x1_0000;
pub const STACK_END: u32 = STACK_BASE + 8 * 1024 * 1024;

/// What a built-in function did.
pub enum Native {
    /// returned a value
    Value(u32),
    /// returned a value, and the machine stops until the next frame
    Yield(u32),
    /// calls compiled code (a program's start) whose value is the built-in's value
    Call(u32),
    /// the game ended
    Exit,
}

pub trait Host {
    fn native(&mut self, vm: &mut Vm, id: u16, args: &[u32]) -> Result<Native, String>;
}

/// Why [`Vm::run`] returned.
#[derive(Debug, PartialEq)]
pub enum Stop {
    /// a built-in asked to wait for the next frame
    Yield,
    /// ran its budget of instructions
    Budget,
    /// the outermost function returned, or the game exited
    Done,
}

struct Frame {
    func: u32,
    pc: u32,
    fp: u32,
    /// the stack region's top before this frame
    sp: u32,
}

#[derive(Clone, Copy)]
struct Jump {
    frames: usize,
    stack: usize,
    sp: u32,
    pc: u32,
}

/// A first-fit allocator over the memory above the stack region.
#[derive(Default)]
struct Heap {
    top: u32,
    /// free blocks: address -> size
    free: BTreeMap<u32, u32>,
    /// allocated blocks: address -> size
    used: HashMap<u32, u32>,
}

pub struct Vm {
    pub mem: Vec<u8>,
    pub funcs: Vec<Function>,
    codes: Vec<Rc<[Op]>>,
    pub natives: Vec<String>,
    native_ids: HashMap<String, u16>,
    stack: Vec<u32>,
    frames: Vec<Frame>,
    sp: u32,
    heap: Heap,
    jumps: Vec<Jump>,
    setjmp: Option<u16>,
    longjmp: Option<u16>,
    /// instructions run, for budgets
    pub steps: u64,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

fn align(n: u32, a: u32) -> u32 {
    n.div_ceil(a) * a
}

impl Vm {
    pub fn new() -> Vm {
        Vm {
            mem: vec![0; STACK_END as usize],
            funcs: Vec::new(),
            codes: Vec::new(),
            natives: Vec::new(),
            native_ids: HashMap::new(),
            stack: Vec::new(),
            frames: Vec::new(),
            sp: STACK_BASE,
            heap: Heap {
                top: STACK_END,
                ..Default::default()
            },
            jumps: Vec::new(),
            setjmp: None,
            longjmp: None,
            steps: 0,
        }
    }

    // ---- memory

    fn check(&self, addr: u32, n: u32) -> Result<usize, String> {
        let end = addr as u64 + n as u64;
        if end > self.mem.len() as u64 {
            return Err(format!("address 0x{addr:x} (+{n}) is outside memory"));
        }
        Ok(addr as usize)
    }

    pub fn bytes(&self, addr: u32, n: u32) -> Result<&[u8], String> {
        let a = self.check(addr, n)?;
        Ok(&self.mem[a..a + n as usize])
    }

    pub fn bytes_mut(&mut self, addr: u32, n: u32) -> Result<&mut [u8], String> {
        let a = self.check(addr, n)?;
        Ok(&mut self.mem[a..a + n as usize])
    }

    pub fn r8(&self, addr: u32) -> Result<u8, String> {
        let a = self.check(addr, 1)?;
        Ok(self.mem[a])
    }

    pub fn r16(&self, addr: u32) -> Result<u16, String> {
        let a = self.check(addr, 2)?;
        Ok(u16::from_le_bytes([self.mem[a], self.mem[a + 1]]))
    }

    pub fn r32(&self, addr: u32) -> Result<u32, String> {
        let a = self.check(addr, 4)?;
        Ok(u32::from_le_bytes(self.mem[a..a + 4].try_into().unwrap()))
    }

    pub fn ri32(&self, addr: u32) -> Result<i32, String> {
        Ok(self.r32(addr)? as i32)
    }

    pub fn w8(&mut self, addr: u32, v: u8) -> Result<(), String> {
        let a = self.check(addr, 1)?;
        self.mem[a] = v;
        Ok(())
    }

    pub fn w16(&mut self, addr: u32, v: u16) -> Result<(), String> {
        let a = self.check(addr, 2)?;
        self.mem[a..a + 2].copy_from_slice(&v.to_le_bytes());
        Ok(())
    }

    pub fn w32(&mut self, addr: u32, v: u32) -> Result<(), String> {
        let a = self.check(addr, 4)?;
        self.mem[a..a + 4].copy_from_slice(&v.to_le_bytes());
        Ok(())
    }

    /// A NUL-terminated string's bytes.
    pub fn cstr(&self, addr: u32) -> Result<Vec<u8>, String> {
        let a = self.check(addr, 0)?;
        let rest = &self.mem[a..];
        let n = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or("string not terminated")?;
        Ok(rest[..n].to_vec())
    }

    /// Writes a string and its NUL.
    pub fn write_cstr(&mut self, addr: u32, s: &[u8]) -> Result<(), String> {
        let d = self.bytes_mut(addr, s.len() as u32 + 1)?;
        d[..s.len()].copy_from_slice(s);
        d[s.len()] = 0;
        Ok(())
    }

    // ---- the heap

    /// Zeroed memory of at least `size` bytes, 8-aligned.
    pub fn malloc(&mut self, size: u32) -> u32 {
        let size = align(size.max(1), 8);
        let found = self
            .heap
            .free
            .iter()
            .find(|(_, s)| **s >= size)
            .map(|(a, s)| (*a, *s));
        let addr = match found {
            Some((a, s)) => {
                self.heap.free.remove(&a);
                if s > size {
                    self.heap.free.insert(a + size, s - size);
                }
                a
            }
            None => {
                let a = self.heap.top;
                self.heap.top += size;
                if self.mem.len() < self.heap.top as usize {
                    let want = (self.heap.top as usize).max(self.mem.len() + self.mem.len() / 4);
                    self.mem.resize(want, 0);
                }
                a
            }
        };
        self.heap.used.insert(addr, size);
        self.mem[addr as usize..(addr + size) as usize].fill(0);
        addr
    }

    pub fn free(&mut self, addr: u32) -> bool {
        let Some(mut size) = self.heap.used.remove(&addr) else {
            return false;
        };
        let mut addr = addr;
        // join the free neighbours
        if let Some((&a, &s)) = self.heap.free.range(..addr).next_back()
            && a + s == addr
        {
            self.heap.free.remove(&a);
            addr = a;
            size += s;
        }
        if let Some(&s) = self.heap.free.get(&(addr + size)) {
            self.heap.free.remove(&(addr + size));
            size += s;
        }
        if addr + size == self.heap.top {
            self.heap.top = addr;
        } else {
            self.heap.free.insert(addr, size);
        }
        true
    }

    /// The size of an allocated block.
    pub fn block_size(&self, addr: u32) -> Option<u32> {
        self.heap.used.get(&addr).copied()
    }

    // ---- functions

    pub fn native_id(&self, name: &str) -> Option<u16> {
        self.native_ids.get(name).copied()
    }

    pub fn find_function(&self, name: &str) -> Option<u32> {
        self.funcs
            .iter()
            .position(|f| f.name == name && f.defined)
            .map(|i| i as u32)
    }

    /// Where the machine is: function and source line of each frame, innermost first.
    pub fn backtrace(&self) -> String {
        let mut out = Vec::new();
        for fr in self.frames.iter().rev().take(8) {
            let f = &self.funcs[fr.func as usize];
            let line = f
                .lines
                .get(fr.pc.saturating_sub(1) as usize)
                .copied()
                .unwrap_or(0);
            out.push(format!("{} ({}:{line})", f.name, f.file));
        }
        out.join(" < ")
    }

    pub fn running(&self) -> bool {
        !self.frames.is_empty()
    }

    /// Starts calling a function with arguments (the machine must be idle, or this call nests
    /// above what is running).
    pub fn call(&mut self, func: u32, args: &[u32]) -> Result<(), String> {
        self.stack.extend_from_slice(args);
        self.enter(func, args.len())
    }

    fn enter(&mut self, func: u32, argc: usize) -> Result<(), String> {
        let Some(f) = self.funcs.get(func as usize) else {
            return Err(format!("no function {func}"));
        };
        if !f.defined {
            return Err(format!("function {} is not defined", f.name));
        }
        let fp = align(self.sp, 8);
        let top = fp + align(f.frame, 4);
        if top > STACK_END {
            return Err("stack overflow".into());
        }
        self.mem[fp as usize..top as usize].fill(0);
        let base = self.stack.len() - argc;
        for (k, &(off, size)) in f.params.iter().enumerate() {
            let Some(&v) = self.stack.get(base + k) else {
                break;
            };
            if size > 0 {
                let src = self.check(v, size)?;
                self.mem
                    .copy_within(src..src + size as usize, (fp + off) as usize);
            } else {
                self.mem[(fp + off) as usize..(fp + off + 4) as usize]
                    .copy_from_slice(&v.to_le_bytes());
            }
        }
        self.stack.truncate(base);
        self.frames.push(Frame {
            func,
            pc: 0,
            fp,
            sp: self.sp,
        });
        self.sp = top;
        Ok(())
    }

    fn pop(&mut self) -> u32 {
        self.stack.pop().unwrap_or(0)
    }

    /// Runs until a built-in yields, the budget runs out or the outermost function returns.
    pub fn run(&mut self, host: &mut dyn Host, budget: u64) -> Result<Stop, String> {
        let r = self.run_inner(host, budget);
        r.map_err(|e| format!("{e} in {}", self.backtrace()))
    }

    fn run_inner(&mut self, host: &mut dyn Host, budget: u64) -> Result<Stop, String> {
        let end = self.steps + budget;
        'outer: loop {
            let Some(fr) = self.frames.last() else {
                return Ok(Stop::Done);
            };
            let code = self.codes[fr.func as usize].clone();
            let fp = fr.fp;
            let mut pc = fr.pc as usize;
            loop {
                if self.steps >= end {
                    self.frames.last_mut().unwrap().pc = pc as u32;
                    return Ok(Stop::Budget);
                }
                self.steps += 1;
                let Some(&op) = code.get(pc) else {
                    return Err("ran off the end of a function".into());
                };
                pc += 1;
                macro_rules! bin {
                    ($f:expr) => {{
                        let b = self.pop();
                        let a = self.pop();
                        #[allow(clippy::redundant_closure_call)]
                        self.stack.push($f(a, b));
                    }};
                }
                macro_rules! fbin {
                    ($f:expr) => {{
                        let b = f32::from_bits(self.pop());
                        let a = f32::from_bits(self.pop());
                        #[allow(clippy::redundant_closure_call)]
                        self.stack.push($f(a, b));
                    }};
                }
                macro_rules! un {
                    ($f:expr) => {{
                        let a = self.pop();
                        #[allow(clippy::redundant_closure_call)]
                        self.stack.push($f(a));
                    }};
                }
                match op {
                    Op::Push(v) => self.stack.push(v),
                    Op::Pop => {
                        self.stack.pop();
                    }
                    Op::Dup => {
                        let v = *self.stack.last().unwrap_or(&0);
                        self.stack.push(v);
                    }
                    Op::Swap => {
                        let n = self.stack.len();
                        if n >= 2 {
                            self.stack.swap(n - 1, n - 2);
                        }
                    }
                    Op::Fp(off) => self.stack.push(fp + off),
                    Op::Ld8s => {
                        let a = self.pop();
                        self.stack.push(self.r8(a)? as i8 as i32 as u32);
                    }
                    Op::Ld8u => {
                        let a = self.pop();
                        self.stack.push(self.r8(a)? as u32);
                    }
                    Op::Ld16s => {
                        let a = self.pop();
                        self.stack.push(self.r16(a)? as i16 as i32 as u32);
                    }
                    Op::Ld16u => {
                        let a = self.pop();
                        self.stack.push(self.r16(a)? as u32);
                    }
                    Op::Ld32 => {
                        let a = self.pop();
                        self.stack.push(self.r32(a)?);
                    }
                    Op::St8 => {
                        let v = self.pop();
                        let a = self.pop();
                        self.w8(a, v as u8)?;
                        self.stack.push(v);
                    }
                    Op::St16 => {
                        let v = self.pop();
                        let a = self.pop();
                        self.w16(a, v as u16)?;
                        self.stack.push(v);
                    }
                    Op::St32 => {
                        let v = self.pop();
                        let a = self.pop();
                        self.w32(a, v)?;
                        self.stack.push(v);
                    }
                    Op::Copy(n) => {
                        let src = self.pop();
                        let dst = *self.stack.last().unwrap_or(&0);
                        let s = self.check(src, n)?;
                        let d = self.check(dst, n)?;
                        self.mem.copy_within(s..s + n as usize, d);
                    }
                    Op::Zero(n) => {
                        let a = self.pop();
                        self.bytes_mut(a, n)?.fill(0);
                    }
                    Op::Add => bin!(|a: u32, b: u32| a.wrapping_add(b)),
                    Op::Sub => bin!(|a: u32, b: u32| a.wrapping_sub(b)),
                    Op::Mul => bin!(|a: u32, b: u32| a.wrapping_mul(b)),
                    Op::DivS => bin!(|a: u32, b: u32| if b == 0 {
                        0
                    } else {
                        (a as i32).wrapping_div(b as i32) as u32
                    }),
                    Op::DivU => bin!(|a: u32, b: u32| if b == 0 { 0 } else { a / b }),
                    Op::ModS => bin!(|a: u32, b: u32| if b == 0 {
                        0
                    } else {
                        (a as i32).wrapping_rem(b as i32) as u32
                    }),
                    Op::ModU => bin!(|a: u32, b: u32| if b == 0 { 0 } else { a % b }),
                    Op::And => bin!(|a: u32, b: u32| a & b),
                    Op::Or => bin!(|a: u32, b: u32| a | b),
                    Op::Xor => bin!(|a: u32, b: u32| a ^ b),
                    Op::Shl => bin!(|a: u32, b: u32| a.wrapping_shl(b)),
                    Op::ShrS => bin!(|a: u32, b: u32| (a as i32).wrapping_shr(b) as u32),
                    Op::ShrU => bin!(|a: u32, b: u32| a.wrapping_shr(b)),
                    Op::Neg => un!(|a: u32| a.wrapping_neg()),
                    Op::BitNot => un!(|a: u32| !a),
                    Op::LNot => un!(|a: u32| (a == 0) as u32),
                    Op::Eq => bin!(|a, b| (a == b) as u32),
                    Op::Ne => bin!(|a, b| (a != b) as u32),
                    Op::LtS => bin!(|a: u32, b: u32| ((a as i32) < (b as i32)) as u32),
                    Op::LtU => bin!(|a: u32, b: u32| (a < b) as u32),
                    Op::LeS => bin!(|a: u32, b: u32| ((a as i32) <= (b as i32)) as u32),
                    Op::LeU => bin!(|a: u32, b: u32| (a <= b) as u32),
                    Op::GtS => bin!(|a: u32, b: u32| ((a as i32) > (b as i32)) as u32),
                    Op::GtU => bin!(|a: u32, b: u32| (a > b) as u32),
                    Op::GeS => bin!(|a: u32, b: u32| ((a as i32) >= (b as i32)) as u32),
                    Op::GeU => bin!(|a: u32, b: u32| (a >= b) as u32),
                    Op::FAdd => fbin!(|a: f32, b: f32| (a + b).to_bits()),
                    Op::FSub => fbin!(|a: f32, b: f32| (a - b).to_bits()),
                    Op::FMul => fbin!(|a: f32, b: f32| (a * b).to_bits()),
                    Op::FDiv => fbin!(|a: f32, b: f32| (a / b).to_bits()),
                    Op::FMod => fbin!(|a: f32, b: f32| (a % b).to_bits()),
                    Op::FNeg => un!(|a: u32| (-f32::from_bits(a)).to_bits()),
                    Op::FEq => fbin!(|a: f32, b: f32| (a == b) as u32),
                    Op::FNe => fbin!(|a: f32, b: f32| (a != b) as u32),
                    Op::FLt => fbin!(|a: f32, b: f32| (a < b) as u32),
                    Op::FLe => fbin!(|a: f32, b: f32| (a <= b) as u32),
                    Op::FGt => fbin!(|a: f32, b: f32| (a > b) as u32),
                    Op::FGe => fbin!(|a: f32, b: f32| (a >= b) as u32),
                    Op::I2F => un!(|a: u32| (a as i32 as f32).to_bits()),
                    Op::U2F => un!(|a: u32| (a as f32).to_bits()),
                    Op::F2I => un!(|a: u32| f32::from_bits(a) as i32 as u32),
                    Op::F2U => un!(|a: u32| f32::from_bits(a) as i64 as u32),
                    Op::Sx8 => un!(|a: u32| a as i8 as i32 as u32),
                    Op::Zx8 => un!(|a: u32| a & 0xff),
                    Op::Sx16 => un!(|a: u32| a as i16 as i32 as u32),
                    Op::Zx16 => un!(|a: u32| a & 0xffff),
                    Op::FBool => un!(|a: u32| (f32::from_bits(a) != 0.0) as u32),
                    Op::Jmp(t) => pc = t as usize,
                    Op::Jz(t) => {
                        if self.pop() == 0 {
                            pc = t as usize;
                        }
                    }
                    Op::Jnz(t) => {
                        if self.pop() != 0 {
                            pc = t as usize;
                        }
                    }
                    Op::IncDec {
                        width,
                        signed,
                        delta,
                        post,
                    } => {
                        let a = self.pop();
                        let old = match (width, signed) {
                            (1, true) => self.r8(a)? as i8 as i32 as u32,
                            (1, false) => self.r8(a)? as u32,
                            (2, true) => self.r16(a)? as i16 as i32 as u32,
                            (2, false) => self.r16(a)? as u32,
                            _ => self.r32(a)?,
                        };
                        let new = old.wrapping_add(delta as u32);
                        match width {
                            1 => self.w8(a, new as u8)?,
                            2 => self.w16(a, new as u16)?,
                            _ => self.w32(a, new)?,
                        }
                        let new = match (width, signed) {
                            (1, true) => new as i8 as i32 as u32,
                            (1, false) => new & 0xff,
                            (2, true) => new as i16 as i32 as u32,
                            (2, false) => new & 0xffff,
                            _ => new,
                        };
                        self.stack.push(if post { old } else { new });
                    }
                    Op::FIncDec { delta, post } => {
                        let a = self.pop();
                        let old = f32::from_bits(self.r32(a)?);
                        let new = old + delta as f32;
                        self.w32(a, new.to_bits())?;
                        self.stack.push(if post { old } else { new }.to_bits());
                    }
                    Op::Call(i, argc) => {
                        self.frames.last_mut().unwrap().pc = pc as u32;
                        self.enter(i, argc as usize)?;
                        continue 'outer;
                    }
                    Op::CallInd(argc) => {
                        let p = self.pop();
                        self.frames.last_mut().unwrap().pc = pc as u32;
                        if p >= NATIVE_BASE {
                            match self.native(host, (p - NATIVE_BASE) as u16, argc)? {
                                Some(stop) => return Ok(stop),
                                None => continue 'outer,
                            }
                        } else if p >= FN_BASE {
                            self.enter(p - FN_BASE, argc as usize)?;
                            continue 'outer;
                        } else {
                            return Err(format!("call through 0x{p:x}, not a function"));
                        }
                    }
                    Op::Native(id, argc) => {
                        self.frames.last_mut().unwrap().pc = pc as u32;
                        match self.native(host, id, argc)? {
                            Some(stop) => return Ok(stop),
                            None => continue 'outer,
                        }
                    }
                    Op::Ret => {
                        let v = self.pop();
                        let fr = self.frames.pop().unwrap();
                        self.sp = fr.sp;
                        self.stack.push(v);
                        if self.frames.is_empty() {
                            return Ok(Stop::Done);
                        }
                        continue 'outer;
                    }
                }
            }
        }
    }

    /// Runs a built-in. The current frame's pc is saved; returns Some to stop the machine.
    fn native(&mut self, host: &mut dyn Host, id: u16, argc: u8) -> Result<Option<Stop>, String> {
        let base = self.stack.len().saturating_sub(argc as usize);
        let args: Vec<u32> = self.stack.split_off(base);
        if Some(id) == self.setjmp {
            let env = args.first().copied().unwrap_or(0);
            let j = Jump {
                frames: self.frames.len(),
                stack: self.stack.len(),
                sp: self.sp,
                pc: self.frames.last().unwrap().pc,
            };
            let old = self.r32(env)?;
            let tag = self.r32(env + 4)?;
            let n = if tag == 0x4A4D_5042 && (old as usize) < self.jumps.len() {
                self.jumps[old as usize] = j;
                old
            } else {
                self.jumps.push(j);
                self.jumps.len() as u32 - 1
            };
            self.w32(env, n)?;
            self.w32(env + 4, 0x4A4D_5042)?;
            self.stack.push(0);
            return Ok(None);
        }
        if Some(id) == self.longjmp {
            let env = args.first().copied().unwrap_or(0);
            let v = args.get(1).copied().unwrap_or(1);
            let n = self.r32(env)? as usize;
            let j = *self
                .jumps
                .get(n)
                .ok_or("longjmp to a buffer setjmp never filled")?;
            if j.frames > self.frames.len() {
                return Err("longjmp to a function that returned".into());
            }
            self.frames.truncate(j.frames);
            self.frames.last_mut().unwrap().pc = j.pc;
            self.stack.truncate(j.stack);
            self.sp = j.sp;
            self.stack.push(if v == 0 { 1 } else { v });
            return Ok(None);
        }
        match host.native(self, id, &args)? {
            Native::Value(v) => {
                self.stack.push(v);
                Ok(None)
            }
            Native::Yield(v) => {
                self.stack.push(v);
                Ok(Some(Stop::Yield))
            }
            Native::Call(f) => {
                self.enter(f, 0)?;
                Ok(None)
            }
            Native::Exit => {
                self.frames.clear();
                self.stack.clear();
                self.sp = STACK_BASE;
                Ok(Some(Stop::Done))
            }
        }
    }
}

impl Target for Vm {
    fn alloc(&mut self, size: u32, _align: u32) -> u32 {
        self.malloc(size)
    }

    fn write(&mut self, addr: u32, bytes: &[u8]) {
        self.mem[addr as usize..addr as usize + bytes.len()].copy_from_slice(bytes);
    }

    fn new_function(&mut self, name: &str) -> u32 {
        self.funcs.push(Function {
            name: name.to_string(),
            ..Default::default()
        });
        self.codes.push(Rc::from(Vec::new()));
        self.funcs.len() as u32 - 1
    }

    fn set_function(&mut self, index: u32, f: Function) {
        self.codes[index as usize] = Rc::from(f.code.clone());
        self.funcs[index as usize] = f;
    }

    fn native(&mut self, name: &str) -> u16 {
        if let Some(&id) = self.native_ids.get(name) {
            return id;
        }
        let id = self.natives.len() as u16;
        self.natives.push(name.to_string());
        self.native_ids.insert(name.to_string(), id);
        match name {
            "setjmp" => self.setjmp = Some(id),
            "longjmp" => self.longjmp = Some(id),
            _ => {}
        }
        id
    }
}
