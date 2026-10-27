//! PRS pictures of the Marble engine: LZ-packed BGR or BGRA rows.
//!
//! ```text
//! "YB" flags:u8 depth:u8 packed:u32 ?:u32 w:u16 h:u16 data
//! ```
//!
//! With flag 0x80 each byte is the difference from the byte one pixel before. The packing
//! follows GARbro's ArcFormats/Marble/ImagePRS.cs (MIT).

use nexas::gfx::Image;

pub fn decode(b: &[u8]) -> Result<Image, String> {
    if b.len() < 16 || &b[..2] != b"YB" || !(3..=4).contains(&b[3]) {
        return Err("prs: not a PRS picture".into());
    }
    let (flag, depth) = (b[2], b[3] as usize);
    let w = u16::from_le_bytes([b[12], b[13]]) as usize;
    let h = u16::from_le_bytes([b[14], b[15]]) as usize;
    let packed = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    let src = &b[16..(16 + packed).min(b.len())];
    let mut out = vec![0u8; w * h * depth];
    let (mut s, mut d) = (0usize, 0usize);
    let mut ctl = 0u8;
    let mut bit = 0u8;
    let next = |s: &mut usize| -> Option<u8> {
        let v = src.get(*s).copied();
        *s += 1;
        v
    };
    while d < out.len() {
        bit >>= 1;
        if bit == 0 {
            let Some(c) = next(&mut s) else { break };
            ctl = c;
            bit = 0x80;
        }
        let Some(v) = next(&mut s) else { break };
        if ctl & bit == 0 {
            out[d] = v;
            d += 1;
            continue;
        }
        let v = v as usize;
        let (mut len, mut shift);
        if v & 0x80 != 0 {
            let Some(lo) = next(&mut s) else { break };
            shift = lo as usize | (v & 0x3f) << 8;
            if v & 0x40 != 0 {
                let Some(n) = next(&mut s) else { break };
                len = match n {
                    0xfe => 0x400,
                    0xff => 0x1000,
                    n => n as usize + 3,
                };
            } else {
                len = (shift & 0xf) + 3;
                shift >>= 4;
            }
        } else {
            len = v >> 2;
            if v & 3 == 3 {
                len += 9;
                let n = len.min(src.len().saturating_sub(s)).min(out.len() - d);
                out[d..d + n].copy_from_slice(&src[s..s + n]);
                s += len;
                d += n;
                continue;
            }
            shift = len;
            len = (v & 3) + 2;
        }
        shift += 1;
        if d < shift {
            return Err("prs: bad offset".into());
        }
        for _ in 0..len.min(out.len() - d) {
            out[d] = out[d - shift];
            d += 1;
        }
    }
    if flag & 0x80 != 0 {
        for i in depth..out.len() {
            out[i] = out[i].wrapping_add(out[i - depth]);
        }
    }
    let mut img = Image::new(w as u32, h as u32);
    for (p, q) in img.px.chunks_exact_mut(4).zip(out.chunks_exact(depth)) {
        p[..3].copy_from_slice(&[q[2], q[1], q[0]]);
        p[3] = if depth == 4 { q[3] } else { 255 };
    }
    // a constant alpha channel other than opaque is not used
    if depth == 4 {
        let a = img.px[3];
        if a != 255 && img.px.chunks_exact(4).all(|p| p[3] == a) {
            img.px.chunks_exact_mut(4).for_each(|p| p[3] = 255);
        }
    }
    Ok(img)
}
