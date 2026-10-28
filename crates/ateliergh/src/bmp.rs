//! BMP pictures: 24-bit, and 32-bit whose fourth byte is the alpha (the format calls it
//! unused, so other readers drop it; the sprites, faces and window parts need it). 8-bit
//! paletted ones (the selection masks) too.

use nexas::gfx::Image;

pub fn decode(b: &[u8]) -> Result<Image, String> {
    if b.len() < 54 || &b[..2] != b"BM" {
        return Err("not a BMP".into());
    }
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let off = u32_at(10) as usize;
    let hsize = u32_at(14) as usize;
    let w = u32_at(18) as i32;
    let h = u32_at(22) as i32;
    let bpp = u16::from_le_bytes([b[28], b[29]]) as usize;
    let comp = u32_at(30);
    if w <= 0 || h == 0 || w > 8192 || h.abs() > 8192 || (comp != 0 && comp != 3) {
        return Err(format!("BMP {w}x{h} compression {comp}"));
    }
    let (w, top_down, h) = (w as usize, h < 0, h.unsigned_abs() as usize);
    let stride = (w * bpp).div_ceil(32) * 4;
    let mut palette = Vec::new();
    if bpp <= 8 {
        let n = match u32_at(46) {
            0 => 1 << bpp,
            n => n as usize,
        };
        let p = 14 + hsize;
        for i in 0..n {
            let q = p + i * 4;
            if q + 3 <= b.len() {
                palette.push([b[q + 2], b[q + 1], b[q], 255]);
            }
        }
    }
    let mut img = Image::new(w as u32, h as u32);
    let mut any_alpha = false;
    for y in 0..h {
        let row = off + if top_down { y } else { h - 1 - y } * stride;
        let Some(src) = b.get(row..row + stride) else {
            break;
        };
        for x in 0..w {
            let px = match bpp {
                32 => {
                    let p = &src[x * 4..x * 4 + 4];
                    any_alpha |= p[3] != 0;
                    [p[2], p[1], p[0], p[3]]
                }
                24 => {
                    let p = &src[x * 3..x * 3 + 3];
                    [p[2], p[1], p[0], 255]
                }
                8 => palette
                    .get(src[x] as usize)
                    .copied()
                    .unwrap_or([0, 0, 0, 255]),
                4 => {
                    let shift = if x % 2 == 0 { 4 } else { 0 };
                    let v = (src[x / 2] >> shift) & 15;
                    palette.get(v as usize).copied().unwrap_or([0, 0, 0, 255])
                }
                _ => return Err(format!("BMP {bpp} bits")),
            };
            let o = (y * w + x) * 4;
            img.px[o..o + 4].copy_from_slice(&px);
        }
    }
    if bpp == 32 && !any_alpha {
        for p in img.px.chunks_exact_mut(4) {
            p[3] = 255;
        }
    }
    Ok(img)
}
