//! The "*Pola*" compression of the SCD scripts, as Adv.exe unpacks it (hand-written assembly
//! at 0x40fbc0; not the Tactics LZ that GARbro reads for the same signature).
//!
//! ```text
//! "*Pola*  " size:u32 ?:u32 ?:u32, then the stream from 0x14
//! ```
//!
//! The stream mixes 16-bit words of flags (read from the low bit up) with whole bytes. A flag 1
//! is a byte as it is; 0 1.. a copy from up to 0x8ff back of 2 bytes; 0 0.. a longer copy
//! from up to 0x2fff back. A short copy from 1 back ends the stream (or, with a flag 1 after
//! it, does nothing).

struct Bits<'a> {
    src: &'a [u8],
    p: usize,
    word: u16,
    left: u32,
}

impl Bits<'_> {
    fn word_at(&self, p: usize) -> u16 {
        match (self.src.get(p), self.src.get(p + 1)) {
            (Some(&a), Some(&b)) => u16::from_le_bytes([a, b]),
            _ => 0,
        }
    }

    fn bit(&mut self) -> bool {
        let b = self.word & 1 != 0;
        self.word >>= 1;
        self.left -= 1;
        if self.left == 0 {
            self.word = self.word_at(self.p);
            self.p += 2;
            self.left = 16;
        }
        b
    }

    fn byte(&mut self) -> Option<u8> {
        let v = *self.src.get(self.p)?;
        self.p += 1;
        Some(v)
    }
}

pub fn is_pola(b: &[u8]) -> bool {
    b.len() >= 0x16 && &b[..6] == b"*Pola*"
}

/// Unpacks a "*Pola*" stream; None when it is not one or is broken.
pub fn unpack(src: &[u8]) -> Option<Vec<u8>> {
    if !is_pola(src) {
        return None;
    }
    let size = u32::from_le_bytes(src[8..12].try_into().unwrap()) as usize;
    if size > 64 << 20 {
        return None;
    }
    let mut out = vec![0u8; size + 64];
    let mut o = 0usize;
    let mut bits = Bits {
        src,
        p: 0x16,
        word: 0,
        left: 16,
    };
    bits.word = bits.word_at(0x14);
    // a copy as the game does it: 1 byte, then 2, then 4 at a time, each read before written
    let copy = |out: &mut Vec<u8>, o: &mut usize, off: i64, n: usize| -> Option<()> {
        let mut s = usize::try_from(*o as i64 + off).ok()?;
        if *o + n > out.len() {
            return None;
        }
        if n & 1 != 0 {
            out[*o] = out[s];
            *o += 1;
            s += 1;
        }
        if n & 2 != 0 {
            let w = [out[s], out[s + 1]];
            out[*o..*o + 2].copy_from_slice(&w);
            *o += 2;
            s += 2;
        }
        for _ in 0..n >> 2 {
            let w = [out[s], out[s + 1], out[s + 2], out[s + 3]];
            out[*o..*o + 4].copy_from_slice(&w);
            *o += 4;
            s += 4;
        }
        Some(())
    };
    loop {
        if bits.bit() {
            if o >= out.len() {
                return None;
            }
            out[o] = bits.byte()?;
            o += 1;
            continue;
        }
        if !bits.bit() {
            let mut off = bits.byte()? as i64 - 256;
            if !bits.bit() {
                if off == -1 {
                    if bits.bit() {
                        continue;
                    }
                    break;
                }
                copy(&mut out, &mut o, off, 2)?;
                continue;
            }
            off -= 0x100;
            if !bits.bit() {
                off -= 0x400;
            }
            if !bits.bit() {
                off -= 0x200;
            }
            if !bits.bit() {
                off -= 0x100;
            }
            copy(&mut out, &mut o, off, 2)?;
            continue;
        }
        let mut off = bits.byte()? as i64 - 256;
        let mut ch = 0i64;
        if !bits.bit() {
            ch += 1;
        }
        if !bits.bit() {
            off -= 0x200;
            if !bits.bit() {
                ch <<= 1;
                if !bits.bit() {
                    ch += 1;
                }
                off -= 0x200;
                if !bits.bit() {
                    ch <<= 1;
                    if !bits.bit() {
                        ch += 1;
                    }
                    off -= 0x400;
                    if !bits.bit() {
                        off -= 0x800;
                        ch <<= 1;
                        if !bits.bit() {
                            ch += 1;
                        }
                    }
                }
            }
        }
        off -= ch * 256;
        let n = if bits.bit() {
            3
        } else if bits.bit() {
            4
        } else if bits.bit() {
            5
        } else if bits.bit() {
            6
        } else if bits.bit() {
            if bits.bit() { 8 } else { 7 }
        } else if bits.bit() {
            bits.byte()? as usize + 0x11
        } else {
            let mut n = 9;
            if bits.bit() {
                n += 4;
            }
            if bits.bit() {
                n += 2;
            }
            if bits.bit() {
                n += 1;
            }
            n
        };
        copy(&mut out, &mut o, off, n)?;
    }
    out.truncate(o.min(size));
    Some(out)
}
