//! AKB pictures (after GARbro's `ImageAKB.cs`): "AKB " or "AKB+", u16 width and height, flags
//! (bit 30: 24-bit, bit 31: alpha), a background colour, the inner rectangle (x0 y0 x1 y1) that
//! holds pixels, ("AKB+": the 32-byte name of the base picture), then LZSS rows of the inner
//! rectangle, bottom row first, each byte the sum of the one before (left, then above).

use crate::arc::lzss;
use nexas::gfx::Image;

pub struct Akb {
    pub w: u32,
    pub h: u32,
    /// the inner rectangle
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    pub base: Option<String>,
}

pub fn header(b: &[u8]) -> Result<(Akb, usize, u32, [u8; 4]), String> {
    if b.len() < 0x20 || &b[..3] != b"AKB" {
        return Err("not an AKB picture".into());
    }
    let u16_ = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as u32;
    let u32_ = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let flags = u32_(8);
    let bg = [b[12], b[13], b[14], b[15]];
    let mut a = Akb {
        w: u16_(4),
        h: u16_(6),
        x0: u32_(16) as i32,
        y0: u32_(20) as i32,
        x1: u32_(24) as i32,
        y1: u32_(28) as i32,
        base: None,
    };
    let mut p = 0x20;
    if b[3] == b'+' {
        let n = b.get(p..p + 0x20).ok_or("short AKB+")?;
        let end = n.iter().position(|&c| c == 0).unwrap_or(0x20);
        a.base = Some(nexas::sjis::decode(&n[..end]));
        p += 0x20;
    }
    Ok((a, p, flags, bg))
}

/// The picture as RGBA. `base` gives the pixels of an "AKB+" picture's base.
pub fn decode(b: &[u8], base: Option<&Image>) -> Result<Image, String> {
    let (a, p, flags, bg) = header(b)?;
    let px = if flags & 0x4000_0000 != 0 { 3 } else { 4 };
    let alpha = px == 4 && flags & 0x8000_0000 != 0;
    let (iw, ih) = ((a.x1 - a.x0).max(0) as usize, (a.y1 - a.y0).max(0) as usize);
    if a.w > 8192 || a.h > 8192 || iw > a.w as usize || ih > a.h as usize {
        return Err("bad AKB size".into());
    }
    let mut img = match base {
        Some(b) if b.w == a.w && b.h == a.h => {
            let mut c = Image::new(a.w, a.h);
            c.px.copy_from_slice(&b.px);
            c
        }
        _ => {
            let mut c = Image::new(a.w, a.h);
            if u32::from_le_bytes(bg) != 0 {
                let o = if alpha { bg[3] } else { 255 };
                c.fill([bg[2], bg[1], bg[0], o]);
            }
            c
        }
    };
    if iw == 0 || ih == 0 {
        return Ok(img);
    }
    let stride = iw * px;
    let raw = lzss(&b[p..], stride * ih);
    if raw.len() < stride * ih {
        return Err("short AKB data".into());
    }
    // rows are stored bottom first
    let mut d = vec![0u8; stride * ih];
    for y in 0..ih {
        d[(ih - 1 - y) * stride..(ih - y) * stride]
            .copy_from_slice(&raw[y * stride..(y + 1) * stride]);
    }
    for i in px..stride {
        d[i] = d[i].wrapping_add(d[i - px]);
    }
    for i in stride..d.len() {
        d[i] = d[i].wrapping_add(d[i - stride]);
    }
    let incremental = a.base.is_some() && base.is_some();
    for y in 0..ih {
        let ty = a.y0 as usize + y;
        for x in 0..iw {
            let s = &d[y * stride + x * px..y * stride + x * px + px];
            if incremental && s[0] == 0 && s[1] == 0xff && s[2] == 0 {
                continue;
            }
            let t = (ty * a.w as usize + a.x0 as usize + x) * 4;
            img.px[t] = s[2];
            img.px[t + 1] = s[1];
            img.px[t + 2] = s[0];
            img.px[t + 3] = if alpha { s[3] } else { 255 };
        }
    }
    Ok(img)
}
