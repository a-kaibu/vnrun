//! What the engine takes from AdvHD.exe: the argument layout of each script command.
//!
//! The exe keeps 256 pointers, one per command, to a list of argument types ending with a byte
//! of 0x80 or more (see [`crate::ws2::Arg`] for the types). The table moves between builds, so
//! it is found by its shape: an array of 256 entries, each 0 or the address of such a list.

pub struct Formats {
    /// the argument types of each command, None for commands the build doesn't have
    pub types: Vec<Option<Vec<u8>>>,
    /// Script strings are UTF-16LE (engine 2.1, the 64-bit exes) instead of Shift_JIS.
    pub utf16: bool,
}

struct Pe<'a> {
    b: &'a [u8],
    base: u64,
    /// 8 in PE32+ (64-bit) exes
    wide: bool,
    /// (virtual address, virtual size, file offset, file size)
    secs: Vec<(u32, u32, usize, usize)>,
}

impl<'a> Pe<'a> {
    fn parse(b: &'a [u8]) -> Option<Pe<'a>> {
        let u16_at = |i: usize| Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?));
        let u32_at = |i: usize| Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?));
        if b.get(..2)? != b"MZ" {
            return None;
        }
        let o = u32_at(0x3c)? as usize;
        if b.get(o..o + 4)? != b"PE\0\0" {
            return None;
        }
        let n = u16_at(o + 6)? as usize;
        let opt = u16_at(o + 20)? as usize;
        let wide = u16_at(o + 24)? == 0x20b;
        let base = if wide {
            u64::from_le_bytes(b.get(o + 48..o + 56)?.try_into().ok()?)
        } else {
            u32_at(o + 52)? as u64
        };
        let mut secs = Vec::new();
        for i in 0..n {
            let h = o + 24 + opt + 40 * i;
            secs.push((u32_at(h + 12)?, u32_at(h + 8)?, u32_at(h + 20)? as usize, u32_at(h + 16)? as usize));
        }
        Some(Pe { b, base, wide, secs })
    }

    /// The file bytes at a virtual address.
    fn at(&self, va: u64) -> Option<&'a [u8]> {
        let r = u32::try_from(va.checked_sub(self.base)?).ok()?;
        for &(v, vs, raw, rs) in &self.secs {
            if r >= v && r < v + vs.max(rs as u32) {
                let off = (r - v) as usize;
                return if off < rs { self.b.get(raw + off..raw + rs) } else { None };
            }
        }
        None
    }
}

/// The type list a pointer points to, if it looks like one.
fn types_at(pe: &Pe, va: u64) -> Option<Vec<u8>> {
    let s = pe.at(va)?;
    let end = s.iter().take(48).position(|&c| c >= 0x80)?;
    let t = &s[..end];
    t.iter().all(|&c| c <= 10).then(|| t.to_vec())
}

/// The command argument table of an AdvHD.exe. Early builds (1.4) have none in the exe; they
/// take the 2018 table, where only command 0x11 differs.
pub fn formats(exe: &[u8]) -> Formats {
    let pe = Pe::parse(exe);
    let utf16 = pe.as_ref().is_some_and(|pe| pe.wide);
    let types = pe.and_then(|pe| find(&pe)).unwrap_or_else(|| {
        let mut t: Vec<_> = crate::builtin::TABLE.iter().map(|t| (t != &[0xff]).then(|| t.to_vec())).collect();
        t[0x11] = Some(vec![6, 8, 5]);
        t
    });
    Formats { types, utf16 }
}

fn find(pe: &Pe) -> Option<Vec<Option<Vec<u8>>>> {
    for &(_, _, raw, rs) in &pe.secs {
        let sec = pe.b.get(raw..raw + rs)?;
        let mut off = 0;
        let w = if pe.wide { 8 } else { 4 };
        while off + 256 * w <= sec.len() {
            let ptr = |k: usize| {
                let e = &sec[off + w * k..off + w * k + w];
                if pe.wide { u64::from_le_bytes(e.try_into().unwrap()) } else { u32::from_le_bytes(e.try_into().unwrap()) as u64 }
            };
            // quick reject: commands 0x00-0x02 and 0x04-0x07 (flow control) and 0x14 (message)
            // exist in every build
            if [0, 1, 2, 4, 5, 6, 7, 0x14].iter().all(|&k| ptr(k) != 0 && types_at(pe, ptr(k)).is_some()) {
                let mut out = Vec::with_capacity(256);
                let mut ok = true;
                let mut n = 0;
                for k in 0..256 {
                    let p = ptr(k);
                    if p == 0 {
                        out.push(None);
                        continue;
                    }
                    match types_at(pe, p) {
                        Some(t) => {
                            n += 1;
                            out.push(Some(t));
                        }
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok && n > 128 {
                    return Some(out);
                }
            }
            off += w;
        }
    }
    None
}
