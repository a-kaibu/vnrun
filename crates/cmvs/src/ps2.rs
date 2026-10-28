//! PS2 scripts and the machine that runs them (read from cmvs32.exe, fetch loop at 0x44d480).
//!
//! ```text
//! "PS2A" 0x30-byte header: label_count at 0x10, code_size 0x14, data_size 0x18,
//! string_size 0x1C, entry 0x20 (where the script's own code starts; before it is the
//! shared library), work_size 0x24
//! labels: u32 x label_count (code offsets), code, data, strings (NUL-terminated)
//! ```
//!
//! Code is little-endian u16 words. 0x200 / 0x201 / 0x202 start a postfix expression (int,
//! string, float) ending at 0x20F; its value goes to `reg`. 0x400.. are jumps, calls and
//! pushes; 0x2000 | n runs command n with the arguments pushed before it.

pub struct Script {
    pub name: String,
    pub labels: Vec<u32>,
    pub code: Vec<u8>,
    pub data: Vec<u8>,
    pub strs: Vec<u8>,
    pub entry: u32,
    pub work: usize,
    /// bytes of arguments each label's function drops when it returns (0x411's operand)
    pub arg_bytes: Vec<u16>,
}

fn u32_(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4)
        .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
}

fn u16_(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2)
        .map_or(0, |s| u16::from_le_bytes(s.try_into().unwrap()))
}

impl Script {
    pub fn parse(name: &str, b: &[u8]) -> Result<Script, String> {
        if !b.starts_with(b"PS2A") || b.len() < 0x30 {
            return Err(format!("{name}: not a PS2 script"));
        }
        let (nl, cs, ds, ss) = (
            u32_(b, 0x10) as usize,
            u32_(b, 0x14) as usize,
            u32_(b, 0x18) as usize,
            u32_(b, 0x1c) as usize,
        );
        let co = 0x30 + nl * 4;
        if co + cs + ds + ss > b.len() {
            return Err(format!("{name}: cut short"));
        }
        let labels: Vec<u32> = (0..nl).map(|i| u32_(b, 0x30 + i * 4)).collect();
        let code = b[co..co + cs].to_vec();
        let mut s = Script {
            name: name.to_string(),
            labels,
            data: b[co + cs..co + cs + ds].to_vec(),
            strs: b[co + cs + ds..co + cs + ds + ss].to_vec(),
            entry: u32_(b, 0x20),
            work: (u32_(b, 0x24) as usize).min(1 << 24),
            code,
            arg_bytes: Vec::new(),
        };
        s.arg_bytes = s.find_arg_bytes();
        Ok(s)
    }

    /// The size of the instruction at `pc`.
    pub fn size(&self, pc: usize) -> usize {
        let op = u16_(&self.code, pc);
        match op {
            0x200..=0x202 => self.expr_end(pc + 2, op & 3) - pc,
            0x400..=0x402 => 6,
            0x403 | 0x407 => 10,
            0x405 | 0x410..=0x412 => 4,
            0x440 | 0x442 => 8,
            _ => 2,
        }
    }

    /// Where the expression starting at `p` (after its 0x20x word) ends.
    fn expr_end(&self, mut p: usize, mode: u16) -> usize {
        let c = &self.code;
        loop {
            if p + 2 > c.len() {
                return c.len();
            }
            let t = u16_(c, p);
            p = match t {
                0x20f => return p + 2,
                0x200 => self.expr_end(p + 2, 0),
                0x101 | 0x102 | 0x12b => self.expr_end(p + 4, 0),
                0x121 if mode == 1 => self.expr_end(p + 4, 0),
                0x105 | 0x109 | 0x110 | 0x112 | 0x12d | 0x12f => self.expr_end(p + 8, 0),
                0x134 | 0x136 if mode == 2 => self.expr_end(p + 8, 0),
                0x107 | 0x10b => self.expr_end(p + 10, 0),
                0x131 | 0x133 if mode == 2 => self.expr_end(p + 10, 0),
                0x111 | 0x113 => {
                    let q = self.expr_end(p + 10, 0);
                    self.expr_end(q + 2, 0)
                }
                0x135 | 0x137 if mode == 2 => {
                    let q = self.expr_end(p + 10, 0);
                    self.expr_end(q + 2, 0)
                }
                0x160..=0x17e => p + 2,
                _ => p + 6,
            };
        }
    }

    fn find_arg_bytes(&self) -> Vec<u16> {
        let mut out = vec![0u16; self.labels.len()];
        let mut starts: Vec<(u32, usize)> = self
            .labels
            .iter()
            .enumerate()
            .map(|(i, &a)| (a, i))
            .collect();
        starts.sort();
        for (k, &(a, i)) in starts.iter().enumerate() {
            let end = starts
                .get(k + 1)
                .map_or(self.code.len(), |s| s.0 as usize)
                .min(self.code.len());
            let mut pc = a as usize;
            while pc + 2 <= end {
                if u16_(&self.code, pc) == 0x411 {
                    out[i] = u16_(&self.code, pc + 2);
                }
                pc += self.size(pc).max(2);
            }
        }
        out
    }

    pub fn str_at(&self, off: usize) -> &[u8] {
        let s = self.strs.get(off..).unwrap_or(&[]);
        &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
    }
}

/// What a command did; see [`Vm::step`].
pub enum Done {
    /// go on, dropping this many bytes of arguments
    Next(u32),
    /// go on next frame, dropping this many bytes
    Yield(u32),
    /// run the same command again next frame (nothing dropped)
    Again,
    /// the command moved pc itself
    Jumped,
}

/// The host side: commands, and library functions taken over.
pub trait Host {
    fn command(&mut self, vm: &mut Vm, op: u16) -> Done;
    /// A call of label `label` of the running script: Some(done) when the host runs it
    /// itself (`Done::Next(n)` drops the call's arguments, `Again` waits).
    fn call(&mut self, vm: &mut Vm, label: usize) -> Option<Done>;
}

#[derive(Clone, Default)]
pub struct Frame {
    /// the code offset to go back to
    pub ret: u32,
    /// the stack offset of the arguments' end (the frame base)
    pub base: usize,
}

/// The machine of one script with the globals it shares with every script.
pub struct Vm {
    pub script: std::rc::Rc<Script>,
    pub pc: usize,
    pub stack: Vec<u8>,
    pub sp: usize,
    pub frames: Vec<usize>,
    pub reg: i32,
    pub flag: bool,
    /// the system values of type 0x10F (0 is commands' result)
    pub sys: [i32; 16],
    pub work: Vec<u8>,
    pub g: Vec<i32>,
    pub bits: Vec<u8>,
    pub gf: Vec<f32>,
    pub gs: Vec<Vec<u8>>,
    /// the string an expression built (the engine's 0x13CB0)
    pub temp: Vec<u8>,
    pub ended: bool,
    pub log: bool,
    /// set by a command that jumps to another script: its name
    pub goto: Option<String>,
}

pub const G_COUNT: usize = 0x1000;
pub const BIT_COUNT: usize = 0x8000 * 8;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tok {
    V(u16, i32),
}

impl Vm {
    pub fn new(script: std::rc::Rc<Script>) -> Vm {
        let mut work = vec![0u8; script.work.max(script.data.len())];
        work[..script.data.len()].copy_from_slice(&script.data);
        Vm {
            pc: script.entry as usize,
            script,
            stack: vec![0u8; 0x40000],
            sp: 0,
            frames: vec![0],
            reg: 0,
            flag: false,
            sys: [0; 16],
            work,
            g: vec![0; G_COUNT],
            bits: vec![0; BIT_COUNT / 8],
            gf: vec![0.0; 0x800],
            gs: vec![Vec::new(); 128],
            temp: Vec::new(),
            ended: false,
            log: false,
            goto: None,
        }
    }

    /// Starts `script` at its entry, keeping the globals.
    pub fn load(&mut self, script: std::rc::Rc<Script>) {
        let mut work = vec![0u8; script.work.max(script.data.len())];
        work[..script.data.len()].copy_from_slice(&script.data);
        self.work = work;
        self.pc = script.entry as usize;
        self.script = script;
        self.sp = 0;
        self.frames = vec![0];
    }

    fn base(&self) -> usize {
        *self.frames.last().unwrap_or(&0)
    }

    pub fn read32(&self, at: usize) -> i32 {
        self.stack
            .get(at..at + 4)
            .map_or(0, |s| i32::from_le_bytes(s.try_into().unwrap()))
    }

    fn write32(&mut self, at: usize, v: i32) {
        if let Some(s) = self.stack.get_mut(at..at + 4) {
            s.copy_from_slice(&v.to_le_bytes());
        }
    }

    pub fn push(&mut self, v: i32) {
        self.write32(self.sp, v);
        self.sp += 4;
    }

    pub fn pop(&mut self) -> i32 {
        self.sp = self.sp.saturating_sub(4);
        self.read32(self.sp)
    }

    /// The `n` words most recently pushed, first pushed first.
    pub fn args(&self, n: usize) -> Vec<i32> {
        (0..n)
            .map(|i| self.read32(self.sp.wrapping_sub(4 * (n - i))))
            .collect()
    }

    /// The `i`th (from 0) of `n` arguments of the running function (type 0x103 at
    /// offset 4 * (n - i) + 4).
    pub fn arg(&self, n: usize, i: usize) -> i32 {
        self.read32(self.base().wrapping_sub(4 * (n - i) + 4))
    }

    pub fn get_bit(&self, i: i32) -> bool {
        let i = i as usize;
        self.bits
            .get(i / 8)
            .is_some_and(|b| b & (1 << (i % 8)) != 0)
    }

    pub fn set_bit(&mut self, i: i32, v: bool) {
        let i = i as usize;
        if let Some(b) = self.bits.get_mut(i / 8) {
            if v {
                *b |= 1 << (i % 8);
            } else {
                *b &= !(1 << (i % 8));
            }
        }
    }

    fn work32(&self, at: usize) -> i32 {
        self.work
            .get(at..at + 4)
            .map_or(0, |s| i32::from_le_bytes(s.try_into().unwrap()))
    }

    fn set_work32(&mut self, at: usize, v: i32) {
        if let Some(s) = self.work.get_mut(at..at + 4) {
            s.copy_from_slice(&v.to_le_bytes());
        }
    }

    /// The text a string reference names (two top bits: 0 string table, 1 work area,
    /// 2 global string, 3 the frame).
    pub fn text(&self, v: i32) -> Vec<u8> {
        let v = v as u32;
        let off = (v & 0x3fffffff) as usize;
        let cut = |s: &[u8]| s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())].to_vec();
        match v >> 30 {
            0 => self.script.str_at(off).to_vec(),
            1 => cut(self.work.get(off..).unwrap_or(&[])),
            2 => self.gs.get(off).cloned().unwrap_or_default(),
            _ => cut(self.stack.get(self.base() + off..).unwrap_or(&[])),
        }
    }

    fn set_text(&mut self, v: i32, t: &[u8]) {
        let v = v as u32;
        let off = (v & 0x3fffffff) as usize;
        match v >> 30 {
            1 => {
                if off + t.len() < self.work.len() {
                    self.work[off..off + t.len()].copy_from_slice(t);
                    self.work[off + t.len()] = 0;
                }
            }
            2 => {
                if let Some(s) = self.gs.get_mut(off) {
                    *s = t.to_vec();
                }
            }
            3 => {
                let at = self.base() + off;
                if at + t.len() < self.stack.len() {
                    self.stack[at..at + t.len()].copy_from_slice(t);
                    self.stack[at + t.len()] = 0;
                }
            }
            _ => {}
        }
    }

    fn load_int(&self, t: Tok) -> i32 {
        let Tok::V(k, v) = t;
        let b = self.base();
        match k {
            0x100 | 0x12a => v,
            0x101 => self.g.get(v as usize).copied().unwrap_or(0),
            0x102 => self.get_bit(v) as i32,
            0x103 => self.read32(b.wrapping_sub(v as usize)),
            0x104 | 0x106 | 0x107 => self.work32(v as usize),
            0x108 | 0x10a | 0x10b => self.read32(b.wrapping_add(v as usize)),
            0x10e => self.script.labels.get(v as usize).copied().unwrap_or(0) as i32,
            0x10f => self.sys.get(v as usize).copied().unwrap_or(0),
            0x129 => f32::from_bits(self.read32(b.wrapping_sub(v as usize)) as u32) as i32,
            0x12b => self.gf.get(v as usize).copied().unwrap_or(0.0) as i32,
            0x12c => f32::from_bits(self.read32(b.wrapping_add(v as usize)) as u32) as i32,
            0x12e => f32::from_bits(self.work32(v as usize) as u32) as i32,
            _ => v,
        }
    }

    fn load_f(&self, t: Tok) -> f32 {
        let Tok::V(k, v) = t;
        let b = self.base();
        match k {
            0x12a | 0x100f => f32::from_bits(v as u32),
            0x129 => f32::from_bits(self.read32(b.wrapping_sub(v as usize)) as u32),
            0x12b => self.gf.get(v as usize).copied().unwrap_or(0.0),
            0x12c | 0x130 | 0x131 => f32::from_bits(self.read32(b.wrapping_add(v as usize)) as u32),
            0x12e | 0x132 | 0x133 => f32::from_bits(self.work32(v as usize) as u32),
            _ => self.load_int(t) as f32,
        }
    }

    fn store_int(&mut self, t: Tok, x: i32) {
        let Tok::V(k, v) = t;
        let b = self.base();
        match k {
            0x101 => {
                if let Some(g) = self.g.get_mut(v as usize) {
                    *g = x;
                }
            }
            0x102 => self.set_bit(v, x != 0),
            0x103 => self.write32(b.wrapping_sub(v as usize), x),
            0x104 | 0x106 | 0x107 => self.set_work32(v as usize, x),
            0x108 | 0x10a | 0x10b => self.write32(b.wrapping_add(v as usize), x),
            0x10f => {
                if let Some(s) = self.sys.get_mut(v as usize) {
                    *s = x;
                }
            }
            0x12b => {
                if let Some(g) = self.gf.get_mut(v as usize) {
                    *g = x as f32;
                }
            }
            _ => {}
        }
    }

    fn store_f(&mut self, t: Tok, x: f32) {
        let Tok::V(k, v) = t;
        let b = self.base();
        match k {
            0x12b => {
                if let Some(g) = self.gf.get_mut(v as usize) {
                    *g = x;
                }
            }
            0x129 => self.write32(b.wrapping_sub(v as usize), x.to_bits() as i32),
            0x12c | 0x130 | 0x131 => self.write32(b.wrapping_add(v as usize), x.to_bits() as i32),
            0x12e | 0x132 | 0x133 => self.set_work32(v as usize, x.to_bits() as i32),
            _ => self.store_int(t, x as i32),
        }
    }

    /// Evaluates the int expression at `p` (after its 0x200 word); returns where it ends.
    fn eval_int(&mut self, mut p: usize) -> usize {
        let s = self.script.clone();
        let c = &s.code;
        let mut st: Vec<Tok> = Vec::new();
        loop {
            if p + 2 > c.len() {
                break;
            }
            let t = u16_(c, p);
            let v = u32_(c, p + 2) as i32;
            match t {
                0x20f => {
                    p += 2;
                    break;
                }
                0x200 => {
                    p = self.eval_int(p + 2);
                    st.push(Tok::V(0x100, self.reg));
                }
                0x101 | 0x102 | 0x12b => {
                    p = self.eval_int(p + 4);
                    st.push(Tok::V(t, self.reg));
                }
                0x105 | 0x109 | 0x110 | 0x112 | 0x12d | 0x12f => {
                    p = self.eval_int(p + 8);
                    let k = match t {
                        0x105 => 0x104,
                        0x109 => 0x108,
                        0x110 => 0x106,
                        0x112 => 0x10a,
                        0x12d => 0x12c,
                        _ => 0x12e,
                    };
                    st.push(Tok::V(k, v.wrapping_add(self.reg.wrapping_mul(4))));
                }
                0x107 | 0x10b => {
                    let stride = u16_(c, p + 6) as i32;
                    p = self.eval_int(p + 10);
                    st.push(Tok::V(t, v.wrapping_add(self.reg.wrapping_mul(stride))));
                }
                0x111 | 0x113 => {
                    let stride = u16_(c, p + 6) as i32;
                    p = self.eval_int(p + 10);
                    let a = self.reg.wrapping_mul(stride);
                    p = self.eval_int(p + 2);
                    let k = if t == 0x111 { 0x107 } else { 0x10b };
                    st.push(Tok::V(
                        k,
                        v.wrapping_add(a).wrapping_add(self.reg.wrapping_mul(4)),
                    ));
                }
                0x12a => {
                    st.push(Tok::V(0x100, f32::from_bits(v as u32) as i32));
                    p += 6;
                }
                0x160..=0x172 if t != 0x170 => {
                    let b = st.pop().map_or(0, |x| self.load_int(x));
                    let a = st.pop().map_or(0, |x| self.load_int(x));
                    let r = match t {
                        0x160 => a.wrapping_mul(b),
                        0x161 => a.checked_div(b).unwrap_or(0),
                        0x162 => a.checked_rem(b).unwrap_or(0),
                        0x163 => a.wrapping_add(b),
                        0x164 => a.wrapping_sub(b),
                        0x165 => a & b,
                        0x166 => a | b,
                        0x167 => a ^ b,
                        0x168 => a.wrapping_shl(b as u32),
                        0x169 => a.wrapping_shr(b as u32),
                        0x16a => (a > b) as i32,
                        0x16b => (a >= b) as i32,
                        0x16c => (a < b) as i32,
                        0x16d => (a <= b) as i32,
                        0x16e => (a != 0 && b != 0) as i32,
                        0x16f => (a != 0 || b != 0) as i32,
                        0x171 => (a == b) as i32,
                        _ => (a != b) as i32,
                    };
                    st.push(Tok::V(0x100, r));
                    p += 2;
                }
                0x170 | 0x175..=0x17e => {
                    let b = st.pop().map_or(0, |x| self.load_int(x));
                    let dst = st.pop().unwrap_or(Tok::V(0x100, 0));
                    let a = self.load_int(dst);
                    let r = match t {
                        0x170 => b,
                        0x175 => a.wrapping_add(b),
                        0x176 => a.wrapping_sub(b),
                        0x177 => a.wrapping_mul(b),
                        0x178 => a.checked_div(b).unwrap_or(0),
                        0x179 => a & b,
                        0x17a => a | b,
                        0x17b => a.checked_rem(b).unwrap_or(0),
                        0x17c => a ^ b,
                        0x17d => a.wrapping_shl(b as u32),
                        _ => a.wrapping_shr(b as u32),
                    };
                    self.store_int(dst, r);
                    st.push(Tok::V(0x100, r));
                    p += 2;
                }
                0x173 | 0x174 => {
                    let dst = st.pop().unwrap_or(Tok::V(0x100, 0));
                    let a = self.load_int(dst);
                    let r = if t == 0x173 {
                        a.wrapping_add(1)
                    } else {
                        a.wrapping_sub(1)
                    };
                    self.store_int(dst, r);
                    st.push(Tok::V(0x100, r));
                    p += 2;
                }
                _ => {
                    st.push(Tok::V(t, v));
                    p += 6;
                }
            }
        }
        self.reg = st.last().map_or(0, |&x| self.load_int(x));
        self.flag = self.reg != 0;
        p
    }

    fn eval_float(&mut self, mut p: usize) -> usize {
        let s = self.script.clone();
        let c = &s.code;
        let mut st: Vec<(Tok, Option<f32>)> = Vec::new();
        let val = |vm: &Vm, x: &(Tok, Option<f32>)| x.1.unwrap_or_else(|| vm.load_f(x.0));
        loop {
            if p + 2 > c.len() {
                break;
            }
            let t = u16_(c, p);
            let v = u32_(c, p + 2) as i32;
            match t {
                0x20f => {
                    p += 2;
                    break;
                }
                0x200 => {
                    p = self.eval_int(p + 2);
                    st.push((Tok::V(0x100, 0), Some(self.reg as f32)));
                }
                0x101 | 0x102 | 0x12b => {
                    p = self.eval_int(p + 4);
                    st.push((Tok::V(t, self.reg), None));
                }
                0x105 | 0x109 | 0x110 | 0x112 | 0x12d | 0x12f | 0x134 | 0x136 => {
                    p = self.eval_int(p + 8);
                    let k = match t {
                        0x105 => 0x104,
                        0x109 => 0x108,
                        0x110 => 0x106,
                        0x112 => 0x10a,
                        0x12d | 0x134 => 0x12c,
                        _ => 0x12e,
                    };
                    st.push((Tok::V(k, v.wrapping_add(self.reg.wrapping_mul(4))), None));
                }
                0x107 | 0x10b | 0x131 | 0x133 => {
                    let stride = u16_(c, p + 6) as i32;
                    p = self.eval_int(p + 10);
                    let k = match t {
                        0x131 => 0x12c,
                        0x133 => 0x12e,
                        k => k,
                    };
                    st.push((
                        Tok::V(k, v.wrapping_add(self.reg.wrapping_mul(stride))),
                        None,
                    ));
                }
                0x111 | 0x113 | 0x135 | 0x137 => {
                    let stride = u16_(c, p + 6) as i32;
                    p = self.eval_int(p + 10);
                    let a = self.reg.wrapping_mul(stride);
                    p = self.eval_int(p + 2);
                    let k = match t {
                        0x111 => 0x107,
                        0x113 => 0x10b,
                        0x135 => 0x12c,
                        _ => 0x12e,
                    };
                    st.push((
                        Tok::V(k, v.wrapping_add(a).wrapping_add(self.reg.wrapping_mul(4))),
                        None,
                    ));
                }
                0x12a => {
                    st.push((Tok::V(0x100, 0), Some(f32::from_bits(v as u32))));
                    p += 6;
                }
                0x160..=0x172 if t != 0x170 => {
                    let b = st.pop().map_or(0.0, |x| val(self, &x));
                    let a = st.pop().map_or(0.0, |x| val(self, &x));
                    let r = match t {
                        0x160 => a * b,
                        0x161 => {
                            if b == 0.0 {
                                0.0
                            } else {
                                a / b
                            }
                        }
                        0x162 => a % b,
                        0x163 => a + b,
                        0x164 => a - b,
                        0x16a => (a > b) as i32 as f32,
                        0x16b => (a >= b) as i32 as f32,
                        0x16c => (a < b) as i32 as f32,
                        0x16d => (a <= b) as i32 as f32,
                        0x16e => (a != 0.0 && b != 0.0) as i32 as f32,
                        0x16f => (a != 0.0 || b != 0.0) as i32 as f32,
                        0x171 => (a == b) as i32 as f32,
                        0x172 => (a != b) as i32 as f32,
                        _ => ((a as i32) & (b as i32)) as f32,
                    };
                    st.push((Tok::V(0x100, 0), Some(r)));
                    p += 2;
                }
                0x170 | 0x175..=0x17e => {
                    let b = st.pop().map_or(0.0, |x| val(self, &x));
                    let dst = st.pop().unwrap_or((Tok::V(0x100, 0), Some(0.0)));
                    let a = val(self, &dst);
                    let r = match t {
                        0x170 => b,
                        0x175 => a + b,
                        0x176 => a - b,
                        0x177 => a * b,
                        0x178 => {
                            if b == 0.0 {
                                0.0
                            } else {
                                a / b
                            }
                        }
                        _ => b,
                    };
                    if dst.1.is_none() {
                        self.store_f(dst.0, r);
                    }
                    st.push((Tok::V(0x100, 0), Some(r)));
                    p += 2;
                }
                0x173 | 0x174 => {
                    let dst = st.pop().unwrap_or((Tok::V(0x100, 0), Some(0.0)));
                    let a = val(self, &dst);
                    let r = if t == 0x173 { a + 1.0 } else { a - 1.0 };
                    if dst.1.is_none() {
                        self.store_f(dst.0, r);
                    }
                    st.push((Tok::V(0x100, 0), Some(r)));
                    p += 2;
                }
                _ => {
                    st.push((Tok::V(t, v), None));
                    p += 6;
                }
            }
        }
        let r = st.last().map_or(0.0, |x| val(self, x));
        self.reg = r.to_bits() as i32;
        self.flag = r != 0.0;
        p
    }

    fn eval_str(&mut self, mut p: usize) -> usize {
        let s = self.script.clone();
        let c = &s.code;
        self.temp.clear();
        let mut st: Vec<Tok> = Vec::new();
        let mut last: Option<i32> = None;
        loop {
            if p + 2 > c.len() {
                break;
            }
            let t = u16_(c, p);
            let v = u32_(c, p + 2) as i32;
            match t {
                0x20f => {
                    p += 2;
                    break;
                }
                0x121 => {
                    p = self.eval_int(p + 4);
                    st.push(Tok::V(0x121, self.reg));
                }
                0x160..=0x172 if t != 0x170 => {
                    // concatenation: both sides go to the temporary string
                    let b = st.pop();
                    let a = st.pop();
                    // the temporary string itself (V(0, 0)) is already there
                    for x in [a, b].into_iter().flatten().filter(|&x| x != Tok::V(0, 0)) {
                        let r = self.str_ref(x);
                        let txt = self.text(r);
                        self.temp.extend_from_slice(&txt);
                    }
                    st.push(Tok::V(0, 0));
                    last = None;
                    p += 2;
                }
                0x170 => {
                    let b = st.pop();
                    let dst = st.pop();
                    if let Some(b) = b
                        && b != Tok::V(0, 0)
                    {
                        let r = self.str_ref(b);
                        let txt = self.text(r);
                        self.temp.extend_from_slice(&txt);
                    }
                    if let Some(d) = dst {
                        let r = self.str_ref(d);
                        let t = self.temp.clone();
                        self.set_text(r, &t);
                    }
                    st.push(Tok::V(0, 0));
                    last = None;
                    p += 2;
                }
                _ => {
                    st.push(Tok::V(t, v));
                    p += 6;
                }
            }
            if let Some(&x) = st.last()
                && x != Tok::V(0, 0)
            {
                last = Some(self.str_ref(x));
            }
        }
        if let Some(r) = last
            && st.len() == 1
        {
            self.reg = r;
        } else if !self.temp.is_empty() {
            // a built string: keep it in global string 127
            let t = self.temp.clone();
            self.gs[127] = t;
            self.reg = (0x80000000u32 | 127) as i32;
        }
        p
    }

    /// The string reference (see [`Vm::text`]) of a string operand.
    fn str_ref(&self, t: Tok) -> i32 {
        let Tok::V(k, v) = t;
        match k {
            0x120 => v & 0x3fffffff,
            0x121 => (0x80000000u32 | (v as u32 & 0x3fffffff)) as i32,
            0x122 => self.read32(self.base().wrapping_sub(v as usize)),
            0x125 => (0x40000000u32 | (v as u32 & 0x3fffffff)) as i32,
            0x127 => (0xc0000000u32 | (v as u32 & 0x3fffffff)) as i32,
            _ => v,
        }
    }

    /// Runs until a command yields (true) or the script stops (false).
    pub fn run(&mut self, host: &mut dyn Host, budget: usize) -> bool {
        for _ in 0..budget {
            if self.ended || self.goto.is_some() {
                return false;
            }
            match self.step(host) {
                Some(true) => return true,
                Some(false) => {}
                None => {
                    self.ended = true;
                    return false;
                }
            }
        }
        true
    }

    /// One instruction: Some(true) when the frame is over.
    pub fn step(&mut self, host: &mut dyn Host) -> Option<bool> {
        let s = self.script.clone();
        let c = &s.code;
        let pc = self.pc;
        if pc + 2 > c.len() {
            return None;
        }
        let op = u16_(c, pc);
        let a32 = u32_(c, pc + 2);
        let a16 = u16_(c, pc + 2);
        match op {
            0x200 => self.pc = self.eval_int(pc + 2),
            0x201 => self.pc = self.eval_str(pc + 2),
            0x202 => self.pc = self.eval_float(pc + 2),
            0x400 => self.pc = a32 as usize,
            0x401 => self.pc = if self.flag { pc + 6 } else { a32 as usize },
            0x402 => self.pc = if self.flag { a32 as usize } else { pc + 6 },
            0x403 => {
                self.pc = if self.reg == a32 as i32 {
                    u32_(c, pc + 6) as usize
                } else {
                    pc + 10
                }
            }
            0x405 => self.pc = s.labels.get(a16 as usize).copied().unwrap_or(0) as usize,
            0x407 => self.pc = u32_(c, pc + 6) as usize,
            0x410 => {
                let label = a16 as usize;
                if let Some(d) = host.call(self, label) {
                    match d {
                        Done::Next(n) => {
                            self.sp = self.sp.saturating_sub(n as usize);
                            self.pc = pc + 4;
                        }
                        Done::Yield(n) => {
                            self.sp = self.sp.saturating_sub(n as usize);
                            self.pc = pc + 4;
                            return Some(true);
                        }
                        Done::Again => return Some(true),
                        Done::Jumped => {}
                    }
                    return Some(false);
                }
                self.push((pc + 4) as i32);
                self.frames.push(self.sp);
                self.pc = s.labels.get(label).copied().unwrap_or(0) as usize;
            }
            0x411 => {
                self.sp = self.base().saturating_sub(4);
                let ret = self.read32(self.sp);
                if self.frames.len() > 1 {
                    self.frames.pop();
                } else {
                    return None;
                }
                self.sp = self.sp.saturating_sub(a16 as usize);
                self.pc = ret as usize;
            }
            0x412 => {
                self.sp = self.sp.saturating_sub(a16 as usize);
                self.pc = pc + 4;
            }
            0x413 => {
                self.sp = self.base().saturating_sub(4);
                let ret = self.read32(self.sp);
                if self.frames.len() > 1 {
                    self.frames.pop();
                } else {
                    return None;
                }
                self.sp = self.sp.saturating_sub(self.reg as usize * 4);
                self.pc = ret as usize;
            }
            0x414 => return None,
            0x416 => {
                self.push((pc + 2) as i32);
                self.frames.push(self.sp);
                self.pc = self.reg as usize;
            }
            0x430 => {
                self.push(self.reg);
                self.pc = pc + 2;
            }
            0x440 | 0x442 => {
                self.push(u32_(c, pc + 4) as i32);
                self.pc = pc + 8;
            }
            0x2000..=0x27ff => {
                let n = op & 0x7ff;
                if self.log {
                    eprintln!("{} {:06x} cmd {:x} {:?}", s.name, pc, n, self.args(4));
                }
                match host.command(self, n) {
                    Done::Next(b) => {
                        self.sp = self.sp.saturating_sub(b as usize);
                        self.pc = pc + 2;
                    }
                    Done::Yield(b) => {
                        self.sp = self.sp.saturating_sub(b as usize);
                        self.pc = pc + 2;
                        return Some(true);
                    }
                    Done::Again => return Some(true),
                    Done::Jumped => {}
                }
            }
            _ => self.pc = pc + 2,
        }
        Some(false)
    }
}
