//! GR2 pictures, as Adv.exe loads them (0x407990), mostly wrapped in LL5.dll's run lengths.
//!
//! ```text
//! "LL5\0" then runs: a signed byte c; c >= 0: the next byte c times; c < 0: -c bytes as they are
//! "GR2_" width:u16 height:u16 key:i32 (-1: none) depth:u16 (3: 24-bit BGR, 2: 15-bit) 0:u16
//! then the rows top down, each padded to 4 bytes
//! ```
//!
//! A sprite's shape is a picture of its own (the entry after it, `..M.GR2`): white is solid.

use nexas::gfx::Image;

/// LL5.dll's `CLL5Lib::Decode`.
pub fn ll5(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len() * 4);
    let mut i = 4;
    while i < b.len() {
        let c = b[i] as i8;
        if c >= 0 {
            let Some(&v) = b.get(i + 1) else { break };
            out.extend(std::iter::repeat_n(v, c as usize));
            i += 2;
        } else {
            let n = -(c as i32) as usize;
            let end = (i + 1 + n).min(b.len());
            out.extend_from_slice(&b[i + 1..end]);
            i += 1 + n;
        }
    }
    out
}

/// The picture of a GR2 entry (LL5-wrapped or not), opaque.
pub fn decode(b: &[u8]) -> Option<Image> {
    let raw;
    let g = if b.len() >= 4 && &b[..4] == b"LL5\0" {
        raw = ll5(b);
        &raw[..]
    } else {
        b
    };
    if g.len() < 16 || &g[..4] != b"GR2_" {
        return None;
    }
    let w = u16::from_le_bytes([g[4], g[5]]) as usize;
    let h = u16::from_le_bytes([g[6], g[7]]) as usize;
    let depth = u16::from_le_bytes([g[12], g[13]]);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return None;
    }
    let bpp = if depth == 2 { 2 } else { 3 };
    let stride = (w * bpp + 3) & !3;
    let px = &g[16..];
    let mut img = Image::new(w as u32, h as u32);
    for y in 0..h {
        let Some(row) = px.get(y * stride..y * stride + w * bpp) else {
            break;
        };
        for x in 0..w {
            let o = (y * w + x) * 4;
            let rgb = if bpp == 3 {
                let s = &row[x * 3..x * 3 + 3];
                [s[2], s[1], s[0]]
            } else {
                let v = u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]);
                let c = |s: u16| ((v >> s & 31) * 255 / 31) as u8;
                [c(10), c(5), c(0)]
            };
            img.px[o..o + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    Some(img)
}

/// `img` with the brightness of `mask` (same place) as its opacity.
pub fn apply_mask(img: &mut Image, mask: &Image) {
    let (w, h) = (img.w.min(mask.w) as usize, img.h.min(mask.h) as usize);
    for y in 0..h {
        for x in 0..w {
            let m = mask.px[(y * mask.w as usize + x) * 4];
            img.px[(y * img.w as usize + x) * 4 + 3] = m;
        }
    }
    img.renew();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs() {
        assert_eq!(ll5(b"LL5\0\x03\x07\xfe\x01\x02"), vec![7, 7, 7, 1, 2]);
    }

    #[test]
    fn picture() {
        let mut g = b"GR2_".to_vec();
        g.extend([1, 0, 2, 0, 0xff, 0xff, 0xff, 0xff, 3, 0, 0, 0]);
        g.extend([1, 2, 3, 0, 4, 5, 6, 0]);
        let i = decode(&g).unwrap();
        assert_eq!((i.w, i.h), (1, 2));
        assert_eq!(&i.px[..8], &[3, 2, 1, 255, 6, 5, 4, 255]);
    }
}
