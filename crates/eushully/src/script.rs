//! AGE scripts ("SYS4421 "): a 0x3c-byte header (local variable counts, then three tables
//! whose smallest offset ends the code), then instructions of 32-bit words: the opcode and per
//! argument a type and a value. Offsets (jumps, strings, arrays) count words after the header.
//! A string argument (type 2) points at bytes XORed with 0xff and ended by 0xff; the arrays of
//! `copy-local-array` are a word count and the words. SYS5 scripts ("SYS5501 " in UTF-16, 2024)
//! have the same header after a 16-byte signature (0x44 bytes) and UTF-16 strings, each unit
//! XORed with 0xffff and ended by 0xffff; the instructions are the same.

use std::collections::HashMap;

/// (opcode, argument count), sorted, from the Eushully-Decompiler's table.
const ARGS: &[(u32, u8)] = &[
    (0x1, 0),
    (0x2, 0),
    (0x3, 1),
    (0x4, 2),
    (0x5, 0),
    (0x6, 2),
    (0x7, 1),
    (0x8, 1),
    (0x9, 0),
    (0xa, 2),
    (0xb, 11),
    (0xc, 0),
    (0xd, 4),
    (0xe, 12),
    (0xf, 1),
    (0x10, 4),
    (0x11, 9),
    (0x12, 1),
    (0x13, 4),
    (0x14, 0),
    (0x15, 5),
    (0x16, 2),
    (0x17, 2),
    (0x1e, 8),
    (0x1f, 12),
    (0x20, 6),
    (0x21, 2),
    (0x22, 2),
    (0x23, 2),
    (0x24, 2),
    (0x25, 3),
    (0x26, 4),
    (0x27, 4),
    (0x28, 4),
    (0x2a, 4),
    (0x2b, 5),
    (0x2c, 5),
    (0x2d, 12),
    (0x2e, 5),
    (0x2f, 4),
    (0x30, 5),
    (0x31, 4),
    (0x32, 10),
    (0x33, 6),
    (0x34, 12),
    (0x35, 11),
    (0x36, 3),
    (0x37, 11),
    (0x38, 12),
    (0x50, 3),
    (0x51, 3),
    (0x52, 3),
    (0x53, 3),
    (0x54, 3),
    (0x55, 2),
    (0x56, 3),
    (0x57, 3),
    (0x58, 3),
    (0x59, 3),
    (0x5a, 3),
    (0x5b, 3),
    (0x5c, 3),
    (0x5d, 3),
    (0x5e, 3),
    (0x5f, 3),
    (0x60, 2),
    (0x61, 3),
    (0x62, 3),
    (0x63, 2),
    (0x64, 2),
    (0x65, 2),
    (0x66, 3),
    (0x67, 3),
    (0x68, 3),
    (0x69, 3),
    (0x6a, 3),
    (0x6b, 3),
    (0x6c, 2),
    (0x6d, 0),
    (0x6e, 2),
    (0x6f, 1),
    (0x70, 5),
    (0x71, 1),
    (0x72, 1),
    (0x73, 10),
    (0x74, 1),
    (0x75, 1),
    (0x76, 1),
    (0x77, 1),
    (0x78, 1),
    (0x79, 3),
    (0x7a, 3),
    (0x7b, 2),
    (0x7c, 0),
    (0x7d, 2),
    (0x7e, 1),
    (0x7f, 1),
    (0x80, 1),
    (0x81, 1),
    (0x82, 5),
    (0x83, 3),
    (0x84, 1),
    (0x85, 0),
    (0x86, 1),
    (0x87, 0),
    (0x88, 1),
    (0x89, 4),
    (0x8a, 6),
    (0x8b, 1),
    (0x8c, 1),
    (0x8d, 2),
    (0x8e, 1),
    (0x8f, 1),
    (0x90, 7),
    (0x91, 1),
    (0x92, 2),
    (0x93, 0),
    (0x94, 0),
    (0x95, 2),
    (0x96, 0),
    (0x97, 5),
    (0xa0, 3),
    (0xa1, 0),
    (0xa2, 2),
    (0xa3, 2),
    (0xaa, 2),
    (0xab, 2),
    (0xac, 9),
    (0xad, 0),
    (0xae, 0),
    (0xaf, 0),
    (0xb0, 1),
    (0xb1, 1),
    (0xb2, 2),
    (0xb3, 0),
    (0xb4, 2),
    (0xb5, 1),
    (0xb6, 1),
    (0xb7, 1),
    (0xb8, 0),
    (0xb9, 1),
    (0xba, 1),
    (0xbb, 1),
    (0xbc, 1),
    (0xbd, 1),
    (0xbe, 1),
    (0xbf, 1),
    (0xc0, 1),
    (0xc1, 0),
    (0xc2, 2),
    (0xc3, 1),
    (0xc4, 1),
    (0xc5, 2),
    (0xc6, 2),
    (0xc7, 2),
    (0xc8, 1),
    (0xc9, 0),
    (0xca, 0),
    (0xcb, 1),
    (0xcc, 2),
    (0xcd, 0),
    (0xce, 3),
    (0xcf, 0),
    (0xd0, 1),
    (0xd1, 0),
    (0xd2, 1),
    (0xd3, 0),
    (0xd4, 4),
    (0xd5, 1),
    (0xd6, 6),
    (0xd7, 1),
    (0xd8, 2),
    (0xd9, 0),
    (0xda, 6),
    (0xfa, 0),
    (0xfb, 2),
    (0xfc, 0),
    (0xfd, 2),
    (0xfe, 1),
    (0xff, 0),
    (0x100, 0),
    (0x101, 0),
    (0x102, 3),
    (0x103, 1),
    (0x104, 0),
    (0x105, 1),
    (0x106, 1),
    (0x107, 2),
    (0x108, 1),
    (0x109, 2),
    (0x10a, 2),
    (0x10b, 2),
    (0x10c, 2),
    (0x10d, 1),
    (0x10e, 2),
    (0x10f, 1),
    (0x12c, 5),
    (0x12d, 7),
    (0x12e, 8),
    (0x12f, 4),
    (0x130, 1),
    (0x131, 1),
    (0x132, 1),
    (0x133, 2),
    (0x134, 3),
    (0x135, 2),
    (0x136, 2),
    (0x137, 1),
    (0x138, 2),
    (0x139, 3),
    (0x13a, 6),
    (0x13b, 7),
    (0x13c, 1),
    (0x13d, 3),
    (0x13e, 2),
    (0x13f, 3),
    (0x140, 4),
    (0x141, 1),
    (0x142, 1),
    (0x143, 0),
    (0x144, 2),
    (0x145, 1),
    (0x146, 1),
    (0x147, 6),
    (0x148, 1),
    (0x149, 1),
    (0x14a, 7),
    (0x14b, 1),
    (0x14c, 2),
    (0x14d, 6),
    (0x190, 2),
    (0x191, 2),
    (0x192, 2),
    (0x193, 3),
    (0x194, 3),
    (0x195, 3),
    (0x196, 3),
    (0x197, 1),
    (0x198, 3),
    (0x199, 0),
    (0x19a, 1),
    (0x19b, 0),
    (0x19c, 0),
    (0x19d, 2),
    (0x19e, 2),
    (0x19f, 2),
    (0x1a0, 9),
    (0x1a1, 2),
    (0x1a2, 1),
    (0x1a3, 1),
    (0x1a4, 2),
    (0x1a5, 1),
    (0x1a6, 2),
    (0x1a7, 1),
    (0x1a8, 0),
    (0x1a9, 1),
    (0x1aa, 1),
    (0x1ab, 2),
    (0x1ac, 3),
    (0x1ad, 0),
    (0x1ae, 3),
    (0x1af, 3),
    (0x1b0, 3),
    (0x1b1, 1),
    (0x1b2, 1),
    (0x1b3, 0),
    (0x1b4, 0),
    (0x1b5, 1),
    (0x1b6, 1),
    (0x1b7, 1),
    (0x1b8, 2),
    (0x1b9, 2),
    (0x1ba, 2),
    (0x1bb, 1),
    (0x1bc, 0),
    (0x1bd, 1),
    (0x1be, 2),
    (0x1bf, 0),
    (0x1c0, 1),
    (0x1c1, 3),
    (0x1c2, 2),
    (0x1c3, 2),
    (0x1c4, 1),
    (0x1c5, 4),
    (0x1c6, 2),
    (0x1c7, 1),
    (0x1c8, 2),
    (0x1c9, 3),
    (0x1ca, 1),
    (0x1cb, 1),
    (0x1cc, 1),
    (0x1cd, 2),
    (0x1ce, 1),
    (0x1cf, 1),
    (0x1d0, 3),
    (0x1d1, 5),
    (0x1d2, 2),
    (0x1d3, 5),
    (0x1d4, 4),
    (0x1d5, 0),
    (0x1d6, 2),
    (0x1d7, 2),
    (0x1d8, 3),
    (0x1d9, 2),
    (0x1f4, 0),
    (0x1f5, 0),
    (0x1f6, 0),
    (0x1f7, 2),
    (0x1f8, 4),
    (0x1f9, 3),
    (0x1fa, 1),
    (0x1fb, 8),
    (0x1fc, 1),
    (0x1fd, 4),
    (0x1fe, 5),
    (0x1ff, 4),
    (0x200, 1),
    (0x201, 1),
    (0x202, 5),
    (0x203, 4),
    (0x204, 4),
    (0x205, 6),
    (0x206, 7),
    (0x207, 8),
    (0x208, 3),
    (0x209, 5),
    (0x20a, 1),
    (0x20b, 7),
    (0x20c, 0),
    (0x20d, 1),
    (0x20e, 0),
    (0x20f, 3),
    (0x210, 1),
    (0x211, 1),
    (0x212, 2),
    (0x213, 3),
    (0x214, 2),
    (0x215, 2),
    (0x216, 2),
    (0x217, 4),
    (0x218, 4),
    (0x219, 4),
    (0x21a, 4),
    (0x21b, 1),
    (0x21c, 0),
    (0x21d, 2),
    (0x21e, 6),
    (0x21f, 7),
    (0x220, 6),
    (0x221, 4),
    (0x222, 2),
    (0x223, 8),
    (0x224, 0),
    (0x225, 2),
    (0x226, 5),
    (0x227, 6),
    (0x228, 5),
    (0x229, 5),
    (0x22a, 3),
    (0x22b, 4),
    (0x22c, 3),
    (0x22d, 5),
    (0x22e, 6),
    (0x22f, 5),
    (0x230, 1),
    (0x231, 4),
    (0x232, 4),
    (0x233, 5),
    (0x234, 5),
    (0x235, 5),
    (0x236, 4),
    (0x237, 2),
    (0x238, 1),
    (0x239, 6),
    (0x23a, 2),
    (0x23b, 7),
    (0x23c, 0),
    (0x23d, 0),
    (0x23e, 2),
    (0x23f, 2),
    (0x240, 4),
    (0x241, 5),
    (0x242, 2),
    (0x243, 0),
    (0x244, 0),
    (0x245, 2),
    (0x246, 2),
    (0x247, 1),
    (0x248, 1),
    (0x249, 3),
    (0x24a, 3),
    (0x24d, 12),
    (0x24e, 1),
    (0x24f, 10),
    (0x250, 10),
    (0x251, 12),
    (0x252, 1),
    (0x253, 2),
    (0x254, 5),
    (0x256, 5),
    (0x257, 5),
    (0x258, 2),
    (0x259, 0),
    (0x25a, 1),
    (0x25b, 1),
    (0x25c, 8),
    (0x25d, 3),
    (0x25e, 5),
    (0x25f, 4),
    (0x260, 4),
    (0x261, 1),
    (0x262, 1),
    (0x263, 1),
    (0x264, 5),
    (0x2bc, 11),
    (0x2bd, 1),
    (0x2be, 1),
    (0x2bf, 3),
    (0x2c0, 3),
    (0x2c1, 1),
    (0x2c2, 6),
    (0x2c3, 2),
    (0x2c4, 0),
    (0x2c5, 2),
    (0x2c6, 2),
    (0x2c7, 4),
    (0x2c8, 4),
    (0x2c9, 3),
    (0x2cc, 1),
    (0x2cd, 1),
    (0x2ce, 1),
    (0x2cf, 1),
    (0x2d0, 3),
    (0x2d1, 3),
    (0x2d2, 3),
    (0x2d3, 3),
    (0x2d5, 2),
    (0x2d7, 2),
    (0x2d8, 3),
    (0x2d9, 2),
    (0x2da, 8),
    (0x2db, 1),
    (0x2dc, 1),
    (0x2dd, 2),
    (0x2de, 2),
    (0x2df, 3),
    (0x2e0, 3),
    (0x2e1, 3),
    (0x2e2, 3),
    (0x2e3, 3),
    (0x2e4, 3),
    (0x2e5, 1),
    (0x2e6, 2),
    (0x2e7, 2),
    (0x2e8, 1),
    (0x2e9, 1),
    (0x2ea, 1),
    (0x2eb, 1),
    (0x2ec, 2),
    (0x2ee, 1),
    (0x2ef, 11),
    (0x2f0, 9),
    (0x2f1, 7),
    (0x2f2, 6),
    (0x2f3, 6),
    (0x2f4, 3),
    (0x2f5, 4),
    (0x2f6, 1),
    (0x2f7, 1),
    (0x2f8, 2),
    (0x2f9, 7),
    (0x2fa, 1),
    (0x2fb, 1),
    (0x2fc, 5),
    (0x2fd, 6),
    (0x2fe, 1),
    (0x2ff, 2),
    (0x300, 3),
    (0x301, 1),
    (0x302, 2),
    (0x303, 3),
    (0x304, 0),
    (0x305, 0),
    (0x306, 1),
    (0x307, 1),
    (0x308, 1),
    (0x30a, 2),
    (0x30c, 1),
    (0x320, 10),
    (0x321, 3),
    (0x322, 4),
    (0x323, 5),
    (0x324, 0),
    (0x325, 2),
    (0x326, 4),
    (0x327, 1),
    (0x328, 3),
    (0x329, 2),
    (0x32a, 1),
    (0x32b, 0),
    (0x32c, 6),
    (0x32d, 2),
    (0x32e, 11),
    (0x32f, 1),
    (0x330, 2),
    (0x332, 4),
    (0x334, 1),
    (0x335, 4),
    (0x337, 4),
    (0x33b, 4),
    (0x33d, 3),
    (0x33e, 5),
    (0x33f, 3),
    (0x340, 1),
    (0x341, 2),
    (0x342, 1),
    (0x344, 2),
    (0x345, 3),
    (0x349, 4),
    (0x34d, 6),
    (0x34e, 4),
    (0x352, 3),
    (0x353, 2),
    (0x354, 2),
    (0x358, 5),
    (0x35a, 5),
    (0x35b, 2),
    (0x35c, 2),
    (0x35d, 3),
    (0x35f, 3),
    (0x360, 3),
    (0x361, 2),
    (0x363, 3),
    (0x364, 3),
    (0x384, 3),
    (0x386, 11),
    (0x387, 8),
    (0x388, 3),
    (0x389, 6),
    (0x38f, 6),
    (0x390, 7),
    (0x391, 2),
    (0x392, 1),
    (0x393, 6),
    (0x396, 5),
    (0x398, 3),
    (0x399, 7),
    (0x39b, 5),
];

/// The argument count of an opcode.
pub fn arg_count(op: u32) -> Option<usize> {
    ARGS.binary_search_by_key(&op, |e| e.0)
        .ok()
        .map(|i| ARGS[i].1 as usize)
}

/// An argument: its type (0 a number, 2 a string, 3 a global, 9 a local, 0xc a local pointer,
/// ...) and value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arg {
    pub t: u32,
    pub v: u32,
}

#[derive(Clone, Debug)]
pub struct Insn {
    pub op: u32,
    /// where it starts, in words after the header
    pub at: u32,
    pub args: Vec<Arg>,
}

pub struct Script {
    pub name: String,
    pub insns: Vec<Insn>,
    /// instruction index by word offset
    pub index: HashMap<u32, usize>,
    pub strings: HashMap<u32, String>,
    pub arrays: HashMap<u32, Vec<u32>>,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

impl Script {
    pub fn parse(name: &str, b: &[u8]) -> Result<Script, String> {
        let v5 = b.starts_with(b"S\0Y\0S\x005\0");
        if !v5 && !b.starts_with(b"SYS4") {
            return Err(format!("{name}: not a SYS4 or SYS5 script"));
        }
        // the header fields after the signature (8 bytes, or 16 in UTF-16)
        let (sig, head) = if v5 { (16, 0x44) } else { (8, 0x3c) };
        let word = |o: usize| u32_at(b, o).ok_or_else(|| format!("{name}: short"));
        let t1 = word(sig + 0x20)? as usize;
        let t2 = word(sig + 0x28)? as usize;
        let t3 = word(sig + 0x30)? as usize;
        let mut end = head + t1.min(t2).min(t3) * 4;
        end = end.min(b.len());
        let mut insns = Vec::new();
        let mut index = HashMap::new();
        let mut strings = HashMap::new();
        let mut arrays = HashMap::new();
        let mut p = head;
        while p + 4 <= end {
            let op = word(p)?;
            let Some(n) = arg_count(op) else {
                return Err(format!("{name}: opcode {op:#x} at {:#x}", (p - head) / 4));
            };
            let at = ((p - head) / 4) as u32;
            let mut args = Vec::with_capacity(n);
            let mut q = p + 4;
            for i in 0..n {
                let a = Arg {
                    t: word(q)?,
                    v: word(q + 4)?,
                };
                q += 8;
                let o = head + a.v as usize * 4;
                if a.t == 2 {
                    end = end.min(o);
                    if let std::collections::hash_map::Entry::Vacant(e) = strings.entry(a.v) {
                        let rest = b.get(o..).unwrap_or(&[]);
                        e.insert(if v5 {
                            let units: Vec<u16> = rest
                                .chunks_exact(2)
                                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                                .take_while(|&c| c != 0xffff)
                                .map(|c| c ^ 0xffff)
                                .collect();
                            String::from_utf16_lossy(&units)
                        } else {
                            let s: Vec<u8> = rest
                                .iter()
                                .take_while(|&&c| c != 0xff)
                                .map(|c| c ^ 0xff)
                                .collect();
                            nexas::sjis::decode(&s)
                        });
                    }
                } else if op == 0x64 && i == 1 {
                    end = end.min(o);
                    let len = u32_at(b, o).unwrap_or(0) as usize;
                    let words = (0..len.min(1 << 20))
                        .map_while(|k| u32_at(b, o + 4 + k * 4))
                        .collect();
                    arrays.insert(a.v, words);
                }
                args.push(a);
            }
            index.insert(at, insns.len());
            insns.push(Insn { op, at, args });
            p = q;
        }
        Ok(Script {
            name: name.to_string(),
            insns,
            index,
            strings,
            arrays,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A script with one instruction, show-text (0x6e: a string and a number), its string
    /// after the code, and the three tables pointing past the end.
    fn script(v5: bool) -> Vec<u8> {
        let mut b = Vec::new();
        if v5 {
            for c in "SYS5501 ".encode_utf16() {
                b.extend(c.to_le_bytes());
            }
        } else {
            b.extend(b"SYS4421 ");
        }
        let words = |b: &mut Vec<u8>, w: &[u32]| w.iter().for_each(|x| b.extend(x.to_le_bytes()));
        // counts, then the sub-header length and the tables (length, offset)
        words(&mut b, &[0, 0, 0, 0, 0, 0, 0x1c, 0, 9, 0, 9, 0, 9]);
        words(&mut b, &[0x6e, 2, 5, 0, 7]);
        if v5 {
            for c in "あa".encode_utf16() {
                b.extend((c ^ 0xffff).to_le_bytes());
            }
            b.extend([0xff, 0xff]);
        } else {
            b.extend([0x82 ^ 0xff, 0xa0 ^ 0xff, b'a' ^ 0xff, 0xff]);
        }
        b
    }

    #[test]
    fn sys4_and_sys5_scripts() {
        for v5 in [false, true] {
            let s = Script::parse("t", &script(v5)).unwrap();
            assert_eq!(s.insns.len(), 1, "v5={v5}");
            assert_eq!(s.insns[0].op, 0x6e);
            assert_eq!(s.insns[0].args[1], Arg { t: 0, v: 7 });
            assert_eq!(s.strings[&5], "あa", "v5={v5}");
        }
    }
}
