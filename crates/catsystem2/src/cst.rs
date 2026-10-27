//! Scenes (.cst): the story text and the commands between it.
//!
//! ```text
//! "CatScene" packed:u32 size:u32 zlib(
//!   len:u32 inputs:u32 offsets_at:u32 strings_at:u32       (offsets from byte 16)
//!   inputs x { lines:u32 first:u32 }                        a block of lines ending at a click
//!   lines x { offset:u32 }                                   from strings_at
//!   lines x { 0x01 kind:u8 text\0 }
//! )
//! ```
//!
//! Kinds: 0x02 wait for a click, 0x03 new page, 0x20 message, 0x21 speaker, 0x30 command,
//! 0xF0 file name, 0xF1 line number.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Input,
    Page,
    Message,
    Name,
    Command,
    Other(u8),
}

#[derive(Debug, Clone)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
}

pub struct Scene {
    pub lines: Vec<Line>,
}

pub fn unpack(b: &[u8], magic: &[u8], head: usize) -> Result<Vec<u8>, String> {
    if !b.starts_with(magic) || b.len() < head {
        return Err("bad magic".into());
    }
    let size = u32::from_le_bytes(b[magic.len() + 4..magic.len() + 8].try_into().unwrap());
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&b[head..], size as usize)
        .map_err(|e| format!("zlib: {e:?}"))
}

impl Scene {
    pub fn parse(b: &[u8]) -> Result<Scene, String> {
        let d = unpack(b, b"CatScene", 16)?;
        let u32_ = |at: usize| -> Result<usize, String> {
            Ok(u32::from_le_bytes(
                d.get(at..at + 4)
                    .ok_or("scene is cut short")?
                    .try_into()
                    .unwrap(),
            ) as usize)
        };
        let (offsets, strings) = (16 + u32_(8)?, 16 + u32_(12)?);
        let n = strings.checked_sub(offsets).ok_or("bad tables")? / 4;
        let mut lines = Vec::with_capacity(n);
        for i in 0..n {
            let at = strings + u32_(offsets + i * 4)?;
            let s = d.get(at..).ok_or("bad line offset")?;
            if s.len() < 2 || s[0] != 1 {
                return Err(format!("line {i}: bad head"));
            }
            let end = s[2..].iter().position(|&c| c == 0).unwrap_or(s.len() - 2);
            let kind = match s[1] {
                0x02 => Kind::Input,
                0x03 => Kind::Page,
                0x20 => Kind::Message,
                0x21 => Kind::Name,
                0x30 => Kind::Command,
                k => Kind::Other(k),
            };
            lines.push(Line {
                kind,
                text: crate::sjis(&s[2..2 + end]),
            });
        }
        Ok(Scene { lines })
    }
}
