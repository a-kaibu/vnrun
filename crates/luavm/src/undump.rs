//! Loads precompiled chunks (luac output) of Lua 5.1 and 5.3 for 32- and 64-bit builds.

use std::rc::Rc;

use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V51,
    V53,
}

#[derive(Debug)]
pub struct Proto {
    pub version: Version,
    pub source: String,
    pub line: i32,
    pub params: u8,
    pub vararg: bool,
    pub max_stack: u8,
    pub code: Vec<u32>,
    pub consts: Vec<Value>,
    /// 5.3: (in the enclosing function's registers, index) per upvalue
    pub upvals: Vec<(bool, u8)>,
    /// 5.1: the number of upvalues (their sources follow CLOSURE as pseudo-instructions)
    pub nups: u8,
    pub protos: Vec<Rc<Proto>>,
    pub lines: Vec<i32>,
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
    size_t: usize,
    version: Version,
}

type R<T> = Result<T, String>;

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> R<&[u8]> {
        let v = self
            .b
            .get(self.at..self.at.checked_add(n).ok_or("bad size")?)
            .ok_or("chunk is cut short")?;
        self.at += n;
        Ok(v)
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn int(&mut self) -> R<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn count(&mut self) -> R<usize> {
        let n = self.int()?;
        if n < 0 || n as usize > self.b.len() {
            return Err(format!("bad count {n}"));
        }
        Ok(n as usize)
    }
    fn size(&mut self) -> R<usize> {
        let b = self.bytes(self.size_t)?;
        let mut v = [0u8; 8];
        v[..b.len()].copy_from_slice(b);
        Ok(u64::from_le_bytes(v) as usize)
    }
    /// A string; None for a NULL one.
    fn string(&mut self) -> R<Option<Vec<u8>>> {
        let n = match self.version {
            Version::V51 => self.size()?,
            Version::V53 => match self.u8()? {
                0xff => self.size()?,
                n => n as usize,
            },
        };
        if n == 0 {
            return Ok(None);
        }
        // the stored length counts a terminating NUL, which 5.1 stores and 5.3 does not
        Ok(Some(match self.version {
            Version::V51 => self.bytes(n)?[..n - 1].to_vec(),
            Version::V53 => self.bytes(n - 1)?.to_vec(),
        }))
    }
    fn f64(&mut self) -> R<f64> {
        Ok(f64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> R<i64> {
        Ok(i64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    fn function(&mut self, parent_source: &str) -> R<Proto> {
        let source = match self.string()? {
            Some(s) => String::from_utf8_lossy(&s).into_owned(),
            None => parent_source.to_string(),
        };
        let line = self.int()?;
        let _last = self.int()?;
        let mut nups = 0;
        if self.version == Version::V51 {
            nups = self.u8()?;
        }
        let params = self.u8()?;
        let vararg = self.u8()? != 0;
        let max_stack = self.u8()?;
        let n = self.count()?;
        let code = self
            .bytes(n * 4)?
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let n = self.count()?;
        let mut consts = Vec::with_capacity(n);
        for _ in 0..n {
            let t = self.u8()?;
            consts.push(match (self.version, t) {
                (_, 0) => Value::Nil,
                (_, 1) => Value::Bool(self.u8()? != 0),
                (_, 3) => Value::Num(self.f64()?),
                (Version::V53, 0x13) => Value::Int(self.i64()?),
                (_, 4) | (Version::V53, 0x14) => Value::bytes(&self.string()?.unwrap_or_default()),
                (_, t) => return Err(format!("constant type {t}")),
            });
        }
        let mut upvals = Vec::new();
        let mut protos = Vec::new();
        match self.version {
            Version::V51 => {
                let n = self.count()?;
                for _ in 0..n {
                    protos.push(Rc::new(self.function(&source)?));
                }
            }
            Version::V53 => {
                let n = self.count()?;
                for _ in 0..n {
                    upvals.push((self.u8()? != 0, self.u8()?));
                }
                nups = n as u8;
                let n = self.count()?;
                for _ in 0..n {
                    protos.push(Rc::new(self.function(&source)?));
                }
            }
        }
        // debug information
        let n = self.count()?;
        let lines = (0..n).map(|_| self.int()).collect::<R<Vec<_>>>()?;
        let n = self.count()?;
        for _ in 0..n {
            self.string()?;
            self.int()?;
            self.int()?;
        }
        let n = self.count()?;
        for _ in 0..n {
            self.string()?;
        }
        Ok(Proto {
            version: self.version,
            source,
            line,
            params,
            vararg,
            max_stack,
            code,
            consts,
            upvals,
            nups,
            protos,
            lines,
        })
    }
}

pub fn is_chunk(b: &[u8]) -> bool {
    b.starts_with(b"\x1bLua")
}

pub fn load(b: &[u8], name: &str) -> R<Proto> {
    if !is_chunk(b) || b.len() < 12 {
        return Err(format!("{name}: not a precompiled Lua chunk"));
    }
    let mut r = Reader {
        b,
        at: 5,
        size_t: 4,
        version: Version::V51,
    };
    match b[4] {
        0x51 => {
            // format, endianness, int, size_t, Instruction, lua_Number, integral
            let h = r.bytes(7)?;
            if h[1] != 1 || h[2] != 4 || h[4] != 4 || h[5] != 8 || h[6] != 0 {
                return Err(format!("{name}: unsupported 5.1 chunk header {h:?}"));
            }
            r.size_t = h[3] as usize;
        }
        0x53 => {
            r.version = Version::V53;
            let _format = r.u8()?;
            if r.bytes(6)? != b"\x19\x93\r\n\x1a\n" {
                return Err(format!("{name}: corrupted 5.3 chunk"));
            }
            let h = r.bytes(5)?.to_vec();
            if h[0] != 4 || h[2] != 4 || h[3] != 8 || h[4] != 8 {
                return Err(format!("{name}: unsupported 5.3 chunk header {h:?}"));
            }
            r.size_t = h[1] as usize;
            if r.i64()? != 0x5678 || r.f64()? != 370.5 {
                return Err(format!("{name}: 5.3 chunk of another byte order"));
            }
            let _nups = r.u8()?;
        }
        v => return Err(format!("{name}: Lua version {v:x} is not supported")),
    }
    let mut p = r.function(&format!("={name}"))?;
    if p.source.is_empty() {
        p.source = name.to_string();
    }
    Ok(p)
}
