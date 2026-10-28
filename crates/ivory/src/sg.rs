//! `fSG` pictures: a `cRGB` section (raw, per-channel runs, 8-bit palette runs or pixel runs;
//! 24 or 32 bits) or a `cJPG` section (a JPEG scrambled like the archives' indexes). Each has a
//! place (x, y) and a size. Follows GARbro's `Ivory/ImageSG.cs` (MIT, by morkt).

use nexas::gfx::Image;

pub struct Sg {
    pub x: i32,
    pub y: i32,
    pub img: Image,
}

fn u16_at(b: &[u8], i: usize) -> Result<u16, String> {
    b.get(i..i + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| "SG: short".to_string())
}

fn u32_at(b: &[u8], i: usize) -> Result<u32, String> {
    b.get(i..i + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| "SG: short".to_string())
}

pub fn decode(b: &[u8]) -> Result<Sg, String> {
    if b.get(..4) != Some(b"fSG ") {
        return Err("not an SG picture".into());
    }
    let h = b.get(8..8 + 0x24).ok_or("SG: short")?;
    let head_size = u32_at(h, 8)? as usize;
    let data_size = u32_at(h, 0xc)? as usize;
    let data_at = 8 + head_size;
    let data = b.get(data_at..).ok_or("SG: short")?;
    let data = &data[..data_size.min(data.len())];
    match &h[..4] {
        b"cRGB" => {
            let mode = u16_at(h, 0x10)?;
            let x = u16_at(h, 0x18)? as i16 as i32;
            let y = u16_at(h, 0x1a)? as i16 as i32;
            let w = u16_at(h, 0x1c)? as usize;
            let hh = u16_at(h, 0x1e)? as usize;
            let bpp = u16_at(h, 0x22)?;
            let ch = (bpp / 8) as usize;
            if ch != 3 && ch != 4 {
                return Err(format!("SG: {bpp} bits"));
            }
            let img = rgb(data, mode, w, hh, ch)?;
            Ok(Sg { x, y, img })
        }
        b"cJPG" => {
            let x = u16_at(h, 0x14)? as i16 as i32;
            let y = u16_at(h, 0x16)? as i16 as i32;
            let key = u32_at(h, 0x20)?;
            let mut d = data.to_vec();
            crate::pk::decrypt(&mut d, key);
            let j = image::load_from_memory_with_format(&d, image::ImageFormat::Jpeg)
                .map_err(|e| format!("SG JPEG: {e}"))?
                .to_rgba8();
            let mut img = Image::new(j.width(), j.height());
            img.px = j.into_raw();
            Ok(Sg { x, y, img })
        }
        _ => Err("SG: unknown section".into()),
    }
}

/// Pixels (BGR(A) in the file) into RGBA.
fn rgb(d: &[u8], mode: u16, w: usize, h: usize, ch: usize) -> Result<Image, String> {
    let short = || "SG: short data".to_string();
    let mut img = Image::new(w as u32, h as u32);
    let mut put = |x: usize, y: usize, bgra: [u8; 4]| {
        let o = (y * w + x) * 4;
        img.px[o..o + 4].copy_from_slice(&[bgra[2], bgra[1], bgra[0], bgra[3]]);
    };
    let alpha = |s: &[u8]| if ch == 4 { s[3] } else { 255 };
    match mode {
        0 => {
            for y in 0..h {
                for x in 0..w {
                    let o = (y * w + x) * ch;
                    let s = d.get(o..o + ch).ok_or_else(short)?;
                    put(x, y, [s[0], s[1], s[2], alpha(s)]);
                }
            }
        }
        1 => {
            // each line: each channel in runs (0x80 repeat, 0x40 a second count byte)
            let mut p = 0;
            let mut line = vec![0u8; w * ch];
            for y in 0..h {
                let mut lp = 0;
                for _ in 0..ch {
                    let mut x = 0;
                    while x < w {
                        let c = *d.get(p).ok_or_else(short)?;
                        p += 1;
                        let mut n = (c & 0x3f) as usize;
                        if c & 0x40 != 0 {
                            n = (n << 8) | *d.get(p).ok_or_else(short)? as usize;
                            p += 1;
                        }
                        if lp + n > line.len() {
                            return Err(short());
                        }
                        if c & 0x80 != 0 {
                            let v = *d.get(p).ok_or_else(short)?;
                            p += 1;
                            line[lp..lp + n].fill(v);
                        } else {
                            line[lp..lp + n].copy_from_slice(d.get(p..p + n).ok_or_else(short)?);
                            p += n;
                        }
                        lp += n;
                        x += n;
                    }
                }
                for x in 0..w {
                    let a = if ch == 4 { line[x + 3 * w] } else { 255 };
                    put(x, y, [line[x], line[x + w], line[x + 2 * w], a]);
                }
            }
        }
        2 => {
            // a palette, then per line offsets
            let pal: Vec<[u8; 4]> = (0..256)
                .map(|i| {
                    let s = d.get(i * 4..i * 4 + 4).unwrap_or(&[0, 0, 0, 0]);
                    [s[0], s[1], s[2], 255]
                })
                .collect();
            let idx_at = 256 * 4;
            let data_at = idx_at + 4 * h;
            for y in 0..h {
                let off = u32_at(d, idx_at + 4 * y)? as usize;
                let mut p = data_at + off;
                if ch == 3 {
                    let mut x = 0;
                    while x < w {
                        let c = *d.get(p).ok_or_else(short)? as usize;
                        p += 1;
                        let mut n = c >> 2;
                        if c & 2 != 0 {
                            n |= (*d.get(p).ok_or_else(short)? as usize) << 6;
                            p += 1;
                        }
                        n = n.min(w - x);
                        if c & 1 != 0 {
                            let v = pal[*d.get(p).ok_or_else(short)? as usize];
                            p += 1;
                            for i in 0..n {
                                put(x + i, y, v);
                            }
                        } else {
                            for i in 0..n {
                                put(x + i, y, pal[*d.get(p + i).ok_or_else(short)? as usize]);
                            }
                            p += n;
                        }
                        x += n;
                        if n == 0 {
                            break;
                        }
                    }
                } else {
                    // bits from the low end: 2 control, count (2 or 10), 4 alpha + 8 index
                    let mut bits = Bits { d, p, acc: 0, n: 0 };
                    let mut x = 0;
                    while x < w {
                        let c = bits.get(2)?;
                        let mut n = if c & 2 != 0 {
                            bits.get(10)?
                        } else {
                            bits.get(2)?
                        } as usize;
                        n = n.min(w - x);
                        let a4 = |a: u32| if a != 0 { ((a << 4) | 0xf) as u8 } else { 0 };
                        if c & 1 != 0 {
                            let a = a4(bits.get(4)?);
                            let mut v = pal[bits.get(8)? as usize];
                            v[3] = a;
                            for i in 0..n {
                                put(x + i, y, v);
                            }
                        } else {
                            for i in 0..n {
                                let a = a4(bits.get(4)?);
                                let mut v = pal[bits.get(8)? as usize];
                                v[3] = a;
                                put(x + i, y, v);
                            }
                        }
                        x += n;
                    }
                }
            }
        }
        3 => {
            // per line offsets, then pixel runs (0x80 one pixel repeated)
            let data_at = 4 * h;
            for y in 0..h {
                let off = u32_at(d, 4 * y)? as usize;
                let mut p = data_at + off;
                let mut x = 0;
                while x < w {
                    let c = *d.get(p).ok_or_else(short)?;
                    p += 1;
                    let mut n = (c & 0x3f) as usize;
                    if c & 0x40 != 0 {
                        n = (n << 8) | *d.get(p).ok_or_else(short)? as usize;
                        p += 1;
                    }
                    let n = n.min(w - x);
                    if c & 0x80 != 0 {
                        let s = d.get(p..p + ch).ok_or_else(short)?;
                        p += ch;
                        let v = [s[0], s[1], s[2], alpha(s)];
                        for i in 0..n {
                            put(x + i, y, v);
                        }
                    } else {
                        for i in 0..n {
                            let s = d.get(p..p + ch).ok_or_else(short)?;
                            p += ch;
                            put(x + i, y, [s[0], s[1], s[2], alpha(s)]);
                        }
                    }
                    x += n;
                    if n == 0 {
                        break;
                    }
                }
            }
        }
        m => return Err(format!("SG: RGB mode {m}")),
    }
    Ok(img)
}

struct Bits<'a> {
    d: &'a [u8],
    p: usize,
    acc: u32,
    n: u32,
}

impl Bits<'_> {
    fn get(&mut self, k: u32) -> Result<u32, String> {
        while self.n < k {
            let b = *self.d.get(self.p).ok_or("SG: short bits")? as u32;
            self.p += 1;
            self.acc |= b << self.n;
            self.n += 8;
        }
        let v = self.acc & ((1 << k) - 1);
        self.acc >>= k;
        self.n -= k;
        Ok(v)
    }
}
