//! BMP pictures (24 or 32 bits, 8 with a palette).

use nexas::gfx::Image;

/// A BMP (24 or 32 bits, 8 with a palette) to RGBA.
pub fn decode(b: &[u8]) -> Option<Image> {
    if b.get(..2)? != b"BM" {
        return None;
    }
    let off = le32(b, 10)? as usize;
    let w = le32(b, 18)? as i32;
    let h = le32(b, 22)? as i32;
    let bpp = u16::from_le_bytes([*b.get(28)?, *b.get(29)?]) as usize;
    if w <= 0 || h == 0 || w > 8192 || h.abs() > 8192 {
        return None;
    }
    let (w, ah) = (w as usize, h.unsigned_abs() as usize);
    let stride = (w * bpp / 8 + 3) & !3;
    let hdr = le32(b, 14)? as usize;
    let pal = 14 + hdr;
    let mut img = Image::new(w as u32, ah as u32);
    for y in 0..ah {
        let sy = if h > 0 { ah - 1 - y } else { y };
        let Some(row) = b.get(off + sy * stride..off + sy * stride + w * bpp / 8) else {
            continue;
        };
        for x in 0..w {
            let px = match bpp {
                32 => [row[x * 4 + 2], row[x * 4 + 1], row[x * 4], row[x * 4 + 3]],
                24 => [row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255],
                8 => {
                    let i = pal + row[x] as usize * 4;
                    match b.get(i..i + 3) {
                        Some(c) => [c[2], c[1], c[0], 255],
                        None => [0, 0, 0, 255],
                    }
                }
                _ => return None,
            };
            let d = (y * w + x) * 4;
            img.px[d..d + 4].copy_from_slice(&px);
        }
    }
    img.renew();
    Some(img)
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
