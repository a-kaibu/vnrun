//! `.OCB` scripts (`fAGS`): sections `cTEX` (the texts, 0-terminated Shift_JIS), `cFNM` (file
//! and script names) and `cCOD` (after a 28-byte header, instructions of a u16 op, a u16 length
//! in bytes and u32 operands). Operands name strings by their offset in a section; a jump's
//! target is a code offset, or a script's name as `0xff000000 | cFNM offset`.

pub struct Ocb {
    tex: Vec<u8>,
    fnm: Vec<u8>,
    code: Vec<u8>,
}

pub struct Ins {
    pub op: u16,
    pub args: Vec<u32>,
    /// the offset of the next instruction
    pub next: usize,
}

/// Where a jump goes.
pub enum Target {
    Offset(usize),
    Script(String),
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

fn cstr(b: &[u8], off: usize) -> String {
    let s = b.get(off..).unwrap_or(&[]);
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    nexas::sjis::decode(&s[..end])
}

impl Ocb {
    pub fn parse(b: &[u8]) -> Result<Ocb, String> {
        if b.get(..4) != Some(b"fAGS") {
            return Err("not an OCB script".into());
        }
        let mut o = Ocb {
            tex: Vec::new(),
            fnm: Vec::new(),
            code: Vec::new(),
        };
        let mut p = 8;
        while p + 12 <= b.len() {
            let size = u32_at(b, p + 4).unwrap_or(0) as usize;
            let head = u32_at(b, p + 8).unwrap_or(0) as usize;
            let body = b
                .get(p + head..(p + size).min(b.len()))
                .unwrap_or(&[])
                .to_vec();
            match &b[p..p + 4] {
                b"cTEX" => o.tex = body,
                b"cFNM" => o.fnm = body,
                b"cCOD" => o.code = body,
                _ => {}
            }
            if size == 0 {
                break;
            }
            p += size;
        }
        if o.code.is_empty() {
            return Err("OCB: no code".into());
        }
        Ok(o)
    }

    pub fn ins(&self, pc: usize) -> Option<Ins> {
        let h = self.code.get(pc..pc + 4)?;
        let op = u16::from_le_bytes([h[0], h[1]]);
        let n = u16::from_le_bytes([h[2], h[3]]) as usize;
        let args = (pc + 4..pc + n.max(4))
            .step_by(4)
            .filter_map(|i| u32_at(&self.code, i))
            .collect();
        Some(Ins {
            op,
            args,
            next: pc + n.max(4),
        })
    }

    pub fn tex(&self, off: u32) -> String {
        cstr(&self.tex, off as usize)
    }

    /// A name from `cFNM` (offset), without the `pack:` some names start with.
    pub fn name(&self, off: u32) -> Option<String> {
        if off == u32::MAX {
            return None;
        }
        let s = cstr(&self.fnm, (off & 0xffffff) as usize);
        let s = s
            .rsplit_once(':')
            .map_or(s.as_str(), |(_, n)| n)
            .to_string();
        (!s.is_empty()).then_some(s)
    }

    pub fn target(&self, w: u32) -> Target {
        if w >> 24 == 0xff {
            Target::Script(self.name(w).unwrap_or_default())
        } else {
            Target::Offset(w as usize)
        }
    }
}
