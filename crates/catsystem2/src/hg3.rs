//! Pictures (.hg3), after GARbro's ImageHG3.cs (MIT).
//!
//! ```text
//! "HG-3" 0x0C:u32 ?:u32  frames: { next:u32 id:u32 tags }      next: from the frame start, 0 last
//! tag = name[8] next:u32 len:u32 data                           next: from the tag start, 0 last
//! stdinfo  = w h bpp x y canvas_w canvas_h ? base_x base_y      (u32 / i32 each)
//! img0000  = top:u32 rows:u32 packed:u32 size:u32 ctl_packed:u32 ctl_size:u32 zlib zlib
//! img_jpg  = JPEG, with img_al = packed:u32 size:u32 zlib(alpha) beside it
//! ```
//!
//! img0000 pixels are four bit planes interleaved by a run-length control stream, stored bottom
//! up as deltas from the left / upper neighbour.

use std::rc::Rc;

use nexas::gfx::Image;

/// One frame: its picture and where it goes on the canvas.
#[derive(Debug, Clone)]
pub struct Frame {
    pub id: u32,
    pub image: Rc<Image>,
    pub x: i32,
    pub y: i32,
    pub canvas_w: u32,
    pub canvas_h: u32,
    pub base_x: i32,
    pub base_y: i32,
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        b.get(at..at + 4)
            .ok_or("HG-3 is cut short")?
            .try_into()
            .unwrap(),
    ))
}

fn zlib(b: &[u8], size: usize) -> Result<Vec<u8>, String> {
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(b, size)
        .map_err(|e| format!("zlib: {e:?}"))
}

/// The frames of a picture, in order.
pub fn decode(b: &[u8]) -> Result<Vec<Frame>, String> {
    if !b.starts_with(b"HG-3") {
        return Err("not an HG-3 picture".into());
    }
    let mut frames = Vec::new();
    let mut at = u32_at(b, 4)? as usize;
    loop {
        let next = u32_at(b, at)? as usize;
        let id = u32_at(b, at + 4)?;
        frames.push(frame(b, at + 8, id)?);
        if next == 0 || frames.len() > 10000 {
            break;
        }
        at += next;
    }
    Ok(frames)
}

/// Only the frame with this id.
pub fn decode_id(b: &[u8], want: u32) -> Result<Option<Frame>, String> {
    if !b.starts_with(b"HG-3") {
        return Err("not an HG-3 picture".into());
    }
    let mut at = u32_at(b, 4)? as usize;
    for _ in 0..10000 {
        let next = u32_at(b, at)? as usize;
        if u32_at(b, at + 4)? == want {
            return frame(b, at + 8, want).map(Some);
        }
        if next == 0 {
            break;
        }
        at += next;
    }
    Ok(None)
}

fn frame(b: &[u8], first: usize, id: u32) -> Result<Frame, String> {
    let mut tags: Vec<(&[u8], usize, usize)> = Vec::new();
    let mut at = first;
    loop {
        let name = b.get(at..at + 8).ok_or("HG-3 tag is cut short")?;
        let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(8)];
        let next = u32_at(b, at + 8)? as usize;
        let len = u32_at(b, at + 12)? as usize;
        tags.push((name, at + 16, len));
        if next == 0 || tags.len() > 64 {
            break;
        }
        at += next;
    }
    let tag = |n: &[u8]| tags.iter().find(|t| t.0 == n).map(|t| (t.1, t.2));
    let (si, _) = tag(b"stdinfo").ok_or("HG-3 without stdinfo")?;
    let i = |k: usize| u32_at(b, si + k * 4);
    let (w, h, bpp) = (i(0)?, i(1)?, i(2)?);
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return Err(format!("HG-3 size {w}x{h}"));
    }
    let mut img = Image::new(w, h);
    if let Some((jpg, len)) = tag(b"img_jpg") {
        let j = image::load_from_memory_with_format(
            b.get(jpg..jpg + len).ok_or("HG-3 jpeg cut short")?,
            image::ImageFormat::Jpeg,
        )
        .map_err(|e| e.to_string())?
        .to_rgba8();
        let n = (w * h) as usize;
        let alpha = match tag(b"img_al") {
            Some((al, _)) => {
                let packed = u32_at(b, al)? as usize;
                let size = u32_at(b, al + 4)? as usize;
                zlib(
                    b.get(al + 8..al + 8 + packed)
                        .ok_or("HG-3 alpha cut short")?,
                    size,
                )?
            }
            None => vec![255; n],
        };
        let src = j.as_raw();
        for k in 0..n.min(src.len() / 4) {
            img.px[k * 4..k * 4 + 3].copy_from_slice(&src[k * 4..k * 4 + 3]);
            img.px[k * 4 + 3] = alpha.get(k).copied().unwrap_or(255);
        }
    } else {
        for &(name, d, _) in &tags {
            if name.len() != 7
                || !name.starts_with(b"img")
                || !name[3..].iter().all(u8::is_ascii_digit)
            {
                continue;
            }
            let top = u32_at(b, d)?;
            let rows = u32_at(b, d + 4)?;
            let (packed, size) = (u32_at(b, d + 8)? as usize, u32_at(b, d + 12)? as usize);
            let (cpacked, csize) = (u32_at(b, d + 16)? as usize, u32_at(b, d + 20)? as usize);
            let data_at = d + 24;
            let data = zlib(
                b.get(data_at..data_at + packed)
                    .ok_or("HG-3 data cut short")?,
                size,
            )?;
            let ctl = zlib(
                b.get(data_at + packed..data_at + packed + cpacked)
                    .ok_or("HG-3 control cut short")?,
                csize,
            )?;
            let px = unpack(&data, &ctl, w as usize, rows as usize, (bpp / 8) as usize)?;
            let ps = (bpp / 8) as usize;
            // bottom up, BGR(A)
            for y in 0..rows as usize {
                let dy = top as usize + rows as usize - 1 - y;
                if dy >= h as usize {
                    continue;
                }
                for x in 0..w as usize {
                    let s = (y * w as usize + x) * ps;
                    let o = (dy * w as usize + x) * 4;
                    img.px[o] = px[s + 2];
                    img.px[o + 1] = px[s + 1];
                    img.px[o + 2] = px[s];
                    img.px[o + 3] = if ps == 4 { px[s + 3] } else { 255 };
                }
            }
        }
    }
    Ok(Frame {
        id,
        image: Rc::new(img),
        x: i(3)? as i32,
        y: i(4)? as i32,
        canvas_w: i(5)?,
        canvas_h: i(6)?,
        base_x: i(8).unwrap_or(0) as i32,
        base_y: i(9).unwrap_or(0) as i32,
    })
}

struct Bits<'a> {
    b: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<u32, String> {
        let byte = *self.b.get(self.at / 8).ok_or("HG-3 control ran out")?;
        let v = (byte >> (self.at % 8)) & 1;
        self.at += 1;
        Ok(v as u32)
    }

    /// Elias gamma.
    fn count(&mut self) -> Result<usize, String> {
        let mut n = 0;
        while self.bit()? == 0 {
            n += 1;
            if n >= 32 {
                return Err("HG-3 control overflow".into());
            }
        }
        let mut v = 1usize;
        for _ in 0..n {
            v = (v << 1) | self.bit()? as usize;
        }
        Ok(v)
    }
}

fn unpack(data: &[u8], ctl: &[u8], w: usize, h: usize, ps: usize) -> Result<Vec<u8>, String> {
    let mut bits = Bits { b: ctl, at: 0 };
    let mut copy = bits.bit()? != 0;
    let size = bits.count()?;
    if size != w * h * ps {
        return Err(format!("HG-3 size {size} for {w}x{h}x{ps}"));
    }
    let mut out = vec![0u8; size];
    let (mut src, mut dst) = (0, 0);
    while dst < size {
        let n = bits.count()?.min(size - dst);
        if copy {
            out[dst..dst + n].copy_from_slice(data.get(src..src + n).ok_or("HG-3 data ran out")?);
            src += n;
        }
        dst += n;
        copy = !copy;
    }
    // planes -> bytes
    let mut table = [[0u32; 256]; 4];
    for i in 0..256u32 {
        let mut v = i & 0xc0;
        v <<= 6;
        v |= i & 0x30;
        v <<= 6;
        v |= i & 0x0c;
        v <<= 6;
        v |= i & 0x03;
        table[0][i as usize] = v << 6;
        table[1][i as usize] = v << 4;
        table[2][i as usize] = v << 2;
        table[3][i as usize] = v;
    }
    let plane = size / 4;
    let mut px = vec![0u8; size];
    let conv = |v: u8| if v & 1 != 0 { (v >> 1) ^ 0xff } else { v >> 1 };
    for k in 0..plane {
        let v = table[0][out[k] as usize]
            | table[1][out[plane + k] as usize]
            | table[2][out[2 * plane + k] as usize]
            | table[3][out[3 * plane + k] as usize];
        px[k * 4] = conv(v as u8);
        px[k * 4 + 1] = conv((v >> 8) as u8);
        px[k * 4 + 2] = conv((v >> 16) as u8);
        px[k * 4 + 3] = conv((v >> 24) as u8);
    }
    let stride = w * ps;
    for x in ps..stride {
        px[x] = px[x].wrapping_add(px[x - ps]);
    }
    for y in 1..h {
        for x in 0..stride {
            px[y * stride + x] = px[y * stride + x].wrapping_add(px[(y - 1) * stride + x]);
        }
    }
    Ok(px)
}
