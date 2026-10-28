//! Pictures: WCG and the LWG sets of them (after GARbro, MIT, by morkt: `Liar/ImageWCG.cs`,
//! `Liar/ArcLWG.cs`).
//!
//! ```text
//! WCG: "WG" flags:u16 0x20 0 ?:u16 width:u32 height:u32, then two packed planes of 16-bit
//!      words: the red and alpha bytes of each pixel, then the blue and green ones. A plane:
//!      words:u32 (2 * pixels) data_size:u32 index_size:u16 ?:u16, index ([u16; index_size]),
//!      data (MSB-first bits: a run length, then an index into the index)
//!      alpha is stored inverted
//! LWG: "LG" 1 0 height:u32 width:u32 count:u32 ?:u32 dir_size:u32, then from 24 per picture:
//!      x:i32 y:i32 bpp:u8 offset:u32 size:u32 name_len:u8 name, and after the directory
//!      data_size:u32 and the WCG pictures (offsets count from there)
//! ```

use nexas::gfx::Image;

struct Bits<'a> {
    b: &'a [u8],
    at: usize,
    cur: u32,
    left: u32,
}

impl Bits<'_> {
    fn bit(&mut self) -> Option<u32> {
        if self.left == 0 {
            self.cur = *self.b.get(self.at)? as u32;
            self.at += 1;
            self.left = 8;
        }
        self.left -= 1;
        Some((self.cur >> self.left) & 1)
    }

    fn bits(&mut self, n: u32) -> Option<u32> {
        let mut v = 0;
        for _ in 0..n {
            v = v << 1 | self.bit()?;
        }
        Some(v)
    }
}

fn u16_(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// One plane at `at`: writes its words to byte `lane` of each pixel (BGRA order) and returns
/// where the next plane starts.
fn plane(b: &[u8], at: usize, out: &mut [u8], lane: usize) -> Option<usize> {
    let pixels = out.len() / 4;
    let words = u32_(b, at)? as usize;
    let data_size = u32_(b, at + 4)? as usize;
    let index_size = u16_(b, at + 8)? as usize;
    if words != pixels * 2 || index_size == 0 {
        return None;
    }
    let index: Vec<u16> = (0..index_size)
        .map(|i| u16_(b, at + 12 + i * 2))
        .collect::<Option<_>>()?;
    let data_at = at + 12 + index_size * 2;
    let data = b.get(data_at..data_at + data_size)?;
    let mut bits = Bits {
        b: data,
        at: 0,
        cur: 0,
        left: 0,
    };
    let small = index_size < 0x1002;
    let limit = if small { 6 } else { 14 };
    let len_bits = if small { 3 } else { 4 };
    let mut px = 0;
    while px < pixels {
        let mut count = 1;
        let mut n = bits.bits(len_bits)?;
        if n == 0 {
            count = bits.bits(4)? as usize + 2;
            n = bits.bits(len_bits)?;
        }
        if n == 0 {
            return None;
        }
        let mut n = n - 1;
        let i = if n == 0 {
            bits.bit()?
        } else if n < limit {
            1 << n | bits.bits(n)?
        } else {
            while bits.bit()? != 0 {
                if n >= 0x10 {
                    return None;
                }
                n += 1;
            }
            1 << n | bits.bits(n)?
        } as usize;
        let w = *index.get(i)?;
        if px + count > pixels {
            return None;
        }
        for _ in 0..count {
            out[px * 4 + lane] = w as u8;
            out[px * 4 + lane + 1] = (w >> 8) as u8;
            px += 1;
        }
    }
    Some(data_at + data_size)
}

pub fn is_wcg(b: &[u8]) -> bool {
    b.len() >= 16 && &b[..2] == b"WG" && b[2] & 0x0f == 1 && b[4] == 0x20
}

pub fn decode(b: &[u8]) -> Result<Image, String> {
    if !is_wcg(b) {
        return Err("not a WCG picture".into());
    }
    let (w, h) = (u32_(b, 8).unwrap_or(0), u32_(b, 12).unwrap_or(0));
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(format!("WCG: bad size {w}x{h}"));
    }
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    let next = plane(b, 16, &mut bgra, 2).ok_or("WCG: bad first plane")?;
    // a picture without its second plane keeps black there
    let _ = plane(b, next, &mut bgra, 0);
    let mut img = Image::new(w, h);
    for (o, i) in img.px.chunks_exact_mut(4).zip(bgra.chunks_exact(4)) {
        o[0] = i[2];
        o[1] = i[1];
        o[2] = i[0];
        o[3] = !i[3];
    }
    Ok(img)
}

/// A picture of an LWG set.
pub struct Part {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub offset: usize,
    pub size: usize,
}

pub struct Lwg {
    pub width: u32,
    pub height: u32,
    pub parts: Vec<Part>,
}

impl Lwg {
    pub fn parse(b: &[u8]) -> Option<Lwg> {
        if b.len() < 24 || &b[..4] != b"LG\x01\x00" {
            return None;
        }
        let (height, width) = (u32_(b, 4)?, u32_(b, 8)?);
        let count = u32_(b, 12)? as usize;
        let dir_size = u32_(b, 20)? as usize;
        let data = 24 + dir_size + 4;
        let mut at = 24;
        let mut parts = Vec::new();
        for _ in 0..count.min(4096) {
            let x = u32_(b, at)? as i32;
            let y = u32_(b, at + 4)? as i32;
            let offset = data + u32_(b, at + 9)? as usize;
            let size = u32_(b, at + 13)? as usize;
            let n = *b.get(at + 17)? as usize;
            let name = encoding_rs::SHIFT_JIS
                .decode(b.get(at + 18..at + 18 + n)?)
                .0
                .to_lowercase();
            at += 18 + n;
            if at > 24 + dir_size {
                return None;
            }
            if size > 0 && offset + size <= b.len() {
                parts.push(Part {
                    name,
                    x,
                    y,
                    offset,
                    size,
                });
            }
        }
        Some(Lwg {
            width,
            height,
            parts,
        })
    }

    pub fn part<'a>(&self, b: &'a [u8], name: &str) -> Option<(&Part, &'a [u8])> {
        let p = self.parts.iter().find(|p| p.name == name)?;
        Some((p, b.get(p.offset..p.offset + p.size)?))
    }
}
