//! Compiled Cotopha scripts (script.csx, "Cotopha Image file", version 1): the code image, the
//! named functions in it, the prologue (global initialization code), the global and data
//! variables and the constant string table; and a decoder for the instructions.

use std::collections::HashMap;

pub type S = std::rc::Rc<str>;

#[derive(Clone, Debug)]
pub enum Init {
    Object(String),
    Reference,
    Array(Vec<Init>),
    Hash,
    Integer(i64),
    Real(f64),
    String(String),
    /// a structure: named members
    Members(Vec<(String, Init)>),
}

pub struct Image {
    pub code: Vec<u8>,
    pub int64: bool,
    /// addresses of the code run once at start, in order
    pub prologue: Vec<u32>,
    pub epilogue: Vec<u32>,
    pub functions: Vec<(u32, String)>,
    pub by_name: HashMap<String, u32>,
    pub globals: Vec<(String, Init)>,
    pub data: Vec<(String, Init)>,
    pub strings: Vec<String>,
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}

impl R<'_> {
    fn u32(&mut self) -> Result<u32, String> {
        let v = self
            .b
            .get(self.p..self.p + 4)
            .ok_or("csx: truncated")?
            .try_into()
            .unwrap();
        self.p += 4;
        Ok(u32::from_le_bytes(v))
    }
    fn u64(&mut self) -> Result<u64, String> {
        let v = self
            .b
            .get(self.p..self.p + 8)
            .ok_or("csx: truncated")?
            .try_into()
            .unwrap();
        self.p += 8;
        Ok(u64::from_le_bytes(v))
    }
    fn wstr(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        let b = self.b.get(self.p..self.p + n * 2).ok_or("csx: truncated")?;
        self.p += n * 2;
        let u: Vec<u16> = b
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Ok(String::from_utf16_lossy(&u))
    }
    fn dwords(&mut self) -> Result<Vec<u32>, String> {
        let n = self.u32()? as usize;
        (0..n).map(|_| self.u32()).collect()
    }
    fn object(&mut self, int64: bool) -> Result<Init, String> {
        Ok(match self.u32()? {
            0 => Init::Object(self.wstr()?),
            1 => Init::Reference,
            2 => {
                let n = self.u32()? as usize;
                Init::Array(
                    (0..n)
                        .map(|_| self.object(int64))
                        .collect::<Result<_, _>>()?,
                )
            }
            3 => Init::Hash,
            4 => Init::Integer(if int64 {
                self.u64()? as i64
            } else {
                self.u32()? as i32 as i64
            }),
            5 => Init::Real(f64::from_bits(self.u64()?)),
            6 => Init::String(self.wstr()?),
            t => return Err(format!("csx: object type {t}")),
        })
    }
}

impl Image {
    pub fn parse(b: &[u8]) -> Result<Image, String> {
        if !b.starts_with(b"Entis\x1a\0\0") {
            return Err("csx: not a Cotopha image".into());
        }
        let mut r = R { b, p: 0x40 };
        let mut img = Image {
            code: Vec::new(),
            int64: false,
            prologue: Vec::new(),
            epilogue: Vec::new(),
            functions: Vec::new(),
            by_name: HashMap::new(),
            globals: Vec::new(),
            data: Vec::new(),
            strings: Vec::new(),
        };
        while r.p + 16 <= b.len() {
            let id = &b[r.p..r.p + 8];
            if id == [0; 8] {
                break;
            }
            let id: [u8; 8] = id.try_into().unwrap();
            r.p += 8;
            let size = r.u64()? as usize;
            let end = r.p + size;
            match &id {
                b"header  " => {
                    if size >= 8 {
                        let mut h = R { b, p: r.p + 4 };
                        img.int64 = h.u32()? == 64;
                    }
                }
                b"image   " => img.code = b.get(r.p..end).ok_or("csx: truncated")?.to_vec(),
                b"function" => {
                    img.prologue = r.dwords()?;
                    img.epilogue = r.dwords()?;
                    let n = r.u32()?;
                    for _ in 0..n {
                        let a = r.u32()?;
                        let name = r.wstr()?;
                        img.by_name.insert(name.clone(), a);
                        img.functions.push((a, name));
                    }
                }
                b"global  " => {
                    let n = r.u32()?;
                    for _ in 0..n {
                        let name = r.wstr()?;
                        let o = r.object(img.int64)?;
                        img.globals.push((name, o));
                    }
                }
                b"data    " => {
                    let n = r.u32()?;
                    for _ in 0..n {
                        let name = r.wstr()?;
                        let len = r.u32()? as i32;
                        let o = if len >= 0 {
                            let mut m = Vec::new();
                            for _ in 0..len {
                                let k = r.wstr()?;
                                m.push((k, r.object(img.int64)?));
                            }
                            Init::Members(m)
                        } else {
                            r.object(img.int64)?
                        };
                        img.data.push((name, o));
                    }
                }
                b"conststr" => {
                    let n = r.u32()?;
                    for _ in 0..n {
                        img.strings.push(r.wstr()?);
                        r.dwords()?;
                    }
                }
                _ => {}
            }
            r.p = end;
        }
        if img.code.is_empty() {
            return Err("csx: no code image".into());
        }
        Ok(img)
    }
}

/// Where a Load/New/Call finds its object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Immediate,
    Stack,
    This,
    Global,
    Data,
    Auto,
}

#[derive(Clone, Debug)]
pub enum Imm {
    Object(S),
    Reference,
    Array,
    Hash,
    Integer(i64),
    Real(f64),
    String(S),
}

#[derive(Clone, Debug)]
pub enum Key {
    Index(i32),
    Name(S),
    /// the whole frame / object
    Whole,
}

#[derive(Clone, Debug)]
pub enum Op {
    /// a new variable: (where, type, class name for objects, name)
    New(Mode, u8, Option<S>, S),
    Free,
    LoadImm(Imm),
    Load(Mode, Key),
    /// assignment, with an operator for "+=" and the like (None: plain)
    Store(Option<u8>),
    /// function entry: name, parameters (type, class, name); None: a try block whose catch
    /// handler is at the address
    Enter(S, Result<Vec<(u8, Option<S>, S)>, u32>),
    Leave,
    Jump(u32),
    /// jump when the popped value is (non-)zero: (jump when true?, target)
    CJump(u8, u32),
    Call(Mode, i32, S),
    Return(u8),
    Element(Key),
    ElementIndirect,
    Operate(u8),
    UniOperate(u8),
    Compare(u8),
}

impl Image {
    fn rd_u8(&self, p: &mut usize) -> Result<u8, String> {
        let v = *self.code.get(*p).ok_or("csx: code ends")?;
        *p += 1;
        Ok(v)
    }
    fn rd_u32(&self, p: &mut usize) -> Result<u32, String> {
        let v = self.code.get(*p..*p + 4).ok_or("csx: code ends")?;
        *p += 4;
        Ok(u32::from_le_bytes(v.try_into().unwrap()))
    }
    fn rd_str(&self, p: &mut usize) -> Result<S, String> {
        let n = self.rd_u32(p)?;
        if n == 0x8000_0000 {
            let i = self.rd_u32(p)? as usize;
            return self
                .strings
                .get(i)
                .map(|s| S::from(s.as_str()))
                .ok_or_else(|| format!("csx: string {i}"));
        }
        let n = n as usize;
        let b = self.code.get(*p..*p + n * 2).ok_or("csx: code ends")?;
        *p += n * 2;
        let u: Vec<u16> = b
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Ok(String::from_utf16_lossy(&u).into())
    }
    fn rd_mode(&self, p: &mut usize) -> Result<Mode, String> {
        Ok(match self.rd_u8(p)? {
            0 => Mode::Immediate,
            1 => Mode::Stack,
            2 => Mode::This,
            3 => Mode::Global,
            4 => Mode::Data,
            5 => Mode::Auto,
            m => return Err(format!("csx: mode {m} at {p:#x}")),
        })
    }

    /// Decodes the instruction at `at`; returns it and the address after it.
    pub fn decode(&self, at: u32) -> Result<(Op, u32), String> {
        let mut p = at as usize;
        let code = self.rd_u8(&mut p)?;
        let op = match code {
            0 => {
                let m = self.rd_mode(&mut p)?;
                let t = self.rd_u8(&mut p)?;
                let class = if t == 0 {
                    Some(self.rd_str(&mut p)?)
                } else {
                    None
                };
                Op::New(m, t, class, self.rd_str(&mut p)?)
            }
            1 => Op::Free,
            2 => {
                let m = self.rd_mode(&mut p)?;
                let t = self.rd_u8(&mut p)?;
                if m == Mode::Immediate {
                    Op::LoadImm(match t {
                        0 => Imm::Object(self.rd_str(&mut p)?),
                        1 => Imm::Reference,
                        2 => Imm::Array,
                        3 => Imm::Hash,
                        4 => Imm::Integer(self.rd_u32(&mut p)? as i32 as i64),
                        5 => {
                            let lo = self.rd_u32(&mut p)? as u64;
                            let hi = self.rd_u32(&mut p)? as u64;
                            Imm::Real(f64::from_bits(lo | hi << 32))
                        }
                        6 => Imm::String(self.rd_str(&mut p)?),
                        7 => {
                            let lo = self.rd_u32(&mut p)? as u64;
                            let hi = self.rd_u32(&mut p)? as u64;
                            Imm::Integer((lo | hi << 32) as i64)
                        }
                        8 => Imm::Integer(self.rd_u32(&mut p)? as i64),
                        t => return Err(format!("csx: load type {t} at {at:#x}")),
                    })
                } else {
                    let k = match t {
                        1 => Key::Whole,
                        4 => Key::Index(self.rd_u32(&mut p)? as i32),
                        6 => Key::Name(self.rd_str(&mut p)?),
                        t => return Err(format!("csx: load key type {t} at {at:#x}")),
                    };
                    Op::Load(m, k)
                }
            }
            3 => {
                let o = self.rd_u8(&mut p)?;
                Op::Store((o != 0xff).then_some(o))
            }
            4 => {
                let name = self.rd_str(&mut p)?;
                let n = self.rd_u32(&mut p)? as i32;
                if n == -1 {
                    let _flag = self.rd_u8(&mut p)?;
                    let rel = self.rd_u32(&mut p)? as i32;
                    Op::Enter(name, Err((p as i64 + rel as i64) as u32))
                } else {
                    let mut args = Vec::new();
                    for _ in 0..n {
                        let t = self.rd_u8(&mut p)?;
                        let class = if t == 0 {
                            Some(self.rd_str(&mut p)?)
                        } else {
                            None
                        };
                        args.push((t, class, self.rd_str(&mut p)?));
                    }
                    Op::Enter(name, Ok(args))
                }
            }
            5 => Op::Leave,
            6 => {
                let rel = self.rd_u32(&mut p)? as i32;
                Op::Jump((p as i64 + rel as i64) as u32)
            }
            7 => {
                let c = self.rd_u8(&mut p)?;
                let rel = self.rd_u32(&mut p)? as i32;
                Op::CJump(c, (p as i64 + rel as i64) as u32)
            }
            8 => {
                let m = self.rd_mode(&mut p)?;
                let n = self.rd_u32(&mut p)? as i32;
                Op::Call(m, n, self.rd_str(&mut p)?)
            }
            9 => Op::Return(self.rd_u8(&mut p)?),
            10 => {
                let t = self.rd_u8(&mut p)?;
                Op::Element(match t {
                    4 => Key::Index(self.rd_u32(&mut p)? as i32),
                    6 => Key::Name(self.rd_str(&mut p)?),
                    t => return Err(format!("csx: element type {t} at {at:#x}")),
                })
            }
            11 => Op::ElementIndirect,
            12 => Op::Operate(self.rd_u8(&mut p)?),
            13 => Op::UniOperate(self.rd_u8(&mut p)?),
            14 => Op::Compare(self.rd_u8(&mut p)?),
            c => return Err(format!("csx: instruction {c} at {at:#x}")),
        };
        Ok((op, p as u32))
    }
}
