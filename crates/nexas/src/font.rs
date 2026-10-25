//! Bitmap fonts (.fnt): every Shift_JIS character pre-drawn with its outline.
//!
//! ```text
//! "FNT\0" w:u32 h:u32 index_len:u32 packed_len:u32 zlib(index) glyph data
//! index = 9042 x { dx:i8 ?:i8 dy:i16 w:u16 h:u16 offset:u32 }
//! glyph = w*h x { brightness:u8 alpha:u8 }
//! ```
//!
//! Glyphs are numbered: 0x20..0x7F, then 0xA1..0xDF, then the double-byte characters with lead
//! bytes 0x81..0x9F, 0xE0..0xEF and 189 trail bytes (0x40..0xFC) each.

use crate::gfx::Image;
use crate::sjis;

pub struct Font {
    /// the cell: how far a full-width character moves the pen
    pub w: i32,
    pub h: i32,
    index: Vec<[i32; 5]>,
    data: Vec<u8>,
}

impl Font {
    pub fn parse(b: &[u8]) -> Result<Font, String> {
        if !b.starts_with(b"FNT\0") || b.len() < 20 {
            return Err("not a FNT font".into());
        }
        let u32_ = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        let (w, h, len, packed) = (u32_(4), u32_(8), u32_(12) as usize, u32_(16) as usize);
        let end = 20usize
            .checked_add(packed)
            .filter(|&e| e <= b.len())
            .ok_or("FNT is cut short")?;
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&b[20..end], len)
            .map_err(|e| format!("FNT index: {e:?}"))?;
        let index = raw
            .chunks_exact(12)
            .map(|e| {
                [
                    e[0] as i8 as i32,
                    i16::from_le_bytes([e[2], e[3]]) as i32,
                    u16::from_le_bytes([e[4], e[5]]) as i32,
                    u16::from_le_bytes([e[6], e[7]]) as i32,
                    u32::from_le_bytes(e[8..12].try_into().unwrap()) as i32,
                ]
            })
            .collect();
        Ok(Font {
            w: w as i32,
            h: h as i32,
            index,
            data: b[end..].to_vec(),
        })
    }

    fn glyph_index(c: &[u8]) -> Option<usize> {
        match *c {
            [b @ 0x20..=0x7f] => Some(b as usize - 0x20),
            [b @ 0xa1..=0xdf] => Some(96 + b as usize - 0xa1),
            [l, t @ 0x40..=0xfc] => {
                let lead = match l {
                    0x81..=0x9f => l as usize - 0x81,
                    0xe0..=0xef => l as usize - 0xe0 + 31,
                    _ => return None,
                };
                Some(159 + lead * 189 + t as usize - 0x40)
            }
            _ => None,
        }
    }

    /// Draws one line of Shift_JIS text in `rgb`; the result's (0, 0) is `pad` pixels up and
    /// left of the first cell.
    pub fn render(&self, text: &[u8], rgb: [u8; 3], pad: i32) -> Image {
        let chars = sjis::chars(text);
        let advance = |c: &[u8]| if c.len() == 2 { self.w } else { self.w / 2 };
        let width: i32 = chars.iter().map(|c| advance(c)).sum();
        let mut out = Image::new((width + pad * 2).max(1) as u32, (self.h + pad * 2) as u32);
        let mut pen = pad;
        for c in chars {
            if let Some(&[dx, dy, w, h, off]) = Self::glyph_index(c).and_then(|i| self.index.get(i))
            {
                let px = self
                    .data
                    .get(off as usize..(off + w * h * 2) as usize)
                    .unwrap_or(&[]);
                for (k, p) in px.chunks_exact(2).enumerate() {
                    let (x, y) = (pen + dx + k as i32 % w, pad + dy + k as i32 / w);
                    if p[1] == 0 || x < 0 || y < 0 || x >= out.w as i32 || y >= out.h as i32 {
                        continue;
                    }
                    let i = ((y as u32 * out.w + x as u32) * 4) as usize;
                    for (ch, &v) in rgb.iter().enumerate() {
                        out.px[i + ch] = (v as u32 * p[0] as u32 / 255) as u8;
                    }
                    out.px[i + 3] = out.px[i + 3].max(p[1]);
                }
            }
            pen += advance(c);
        }
        out
    }
}
