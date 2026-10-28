//! Pictures: Windows BMP (24 or 32 bits, or 8 bits with a palette) and, beside a `name.bmp`, a
//! `name.alp` holding one opacity byte per pixel from the top row down (the BMP's own rows go
//! from the bottom up).

use nexas::gfx::Image;

/// Decodes a BMP and its opacity (if any) into RGBA.
pub fn decode(bmp: &[u8], alp: Option<&[u8]>) -> Result<Image, String> {
    if bmp.len() < 54 || &bmp[..2] != b"BM" {
        return Err("not a BMP".into());
    }
    let u32_at = |at: usize| u32::from_le_bytes(bmp[at..at + 4].try_into().unwrap());
    let data = u32_at(10) as usize;
    let dib = u32_at(14) as usize;
    let w = u32_at(18) as i32;
    let h = u32_at(22) as i32;
    let bpp = u16::from_le_bytes([bmp[28], bmp[29]]) as usize;
    if w <= 0 || h == 0 || w > 16384 || h.abs() > 16384 {
        return Err(format!("BMP size {w}x{h}"));
    }
    let (w, hh) = (w as usize, h.unsigned_abs() as usize);
    let stride = (w * bpp).div_ceil(32) * 4;
    let palette: Vec<[u8; 3]> = if bpp <= 8 {
        let n = match u32_at(46) {
            0 => 1 << bpp,
            n => n as usize,
        };
        (0..n)
            .filter_map(|i| {
                let at = 14 + dib + i * 4;
                bmp.get(at..at + 3).map(|p| [p[2], p[1], p[0]])
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut img = Image::new(w as u32, hh as u32);
    for y in 0..hh {
        let row = if h > 0 { hh - 1 - y } else { y };
        let Some(src) = bmp.get(data + row * stride..data + row * stride + stride) else {
            break;
        };
        for x in 0..w {
            let o = (y * w + x) * 4;
            let px = match bpp {
                32 => [src[x * 4 + 2], src[x * 4 + 1], src[x * 4], src[x * 4 + 3]],
                24 => [src[x * 3 + 2], src[x * 3 + 1], src[x * 3], 255],
                8 => {
                    let c = palette.get(src[x] as usize).copied().unwrap_or([0; 3]);
                    [c[0], c[1], c[2], 255]
                }
                _ => return Err(format!("BMP of {bpp} bits")),
            };
            img.px[o..o + 4].copy_from_slice(&px);
        }
    }
    if let Some(a) = alp
        && a.len() >= w * hh
    {
        for (p, &v) in img.px.chunks_exact_mut(4).zip(a) {
            p[3] = v;
        }
    }
    Ok(img)
}
