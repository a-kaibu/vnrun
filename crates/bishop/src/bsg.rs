//! BSS-Graphics pictures (*.bsg), optionally inside a "BSS-Composition" wrapper. The header
//! (0x40 bytes) holds the unpacked size (0x12), width and height (0x16, 0x18), the position
//! (0x20, 0x22), the colour mode (0x30: 0 BGRA, 1 BGR, 2 8-bit with palette), the packing
//! (0x31: 0 none, 1 RLE, 2 LZ), and the offsets of the data and palette (0x32 size at 0x36,
//! 0x3a). Packed pictures keep each channel apart, rows bottom up; LZ planes are deltas.

use nexas::gfx::Image;

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn i16_at(b: &[u8], o: usize) -> Option<i16> {
    u16_at(b, o).map(|v| v as i16)
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// A picture and where its top left corner goes.
pub struct Bsg {
    pub image: Image,
    pub x: i32,
    pub y: i32,
}

pub fn decode(b: &[u8]) -> Result<Bsg, String> {
    let base = if b.starts_with(b"BSS-Composition\0") {
        0x20
    } else {
        0
    };
    let h = b.get(base..).ok_or("BSG: short")?;
    if !h.starts_with(b"BSS-Graphics\0") {
        return Err("BSG: no signature".into());
    }
    let size = u32_at(h, 0x12).ok_or("BSG: short")? as usize;
    let w = u16_at(h, 0x16).ok_or("BSG: short")? as usize;
    let ht = u16_at(h, 0x18).ok_or("BSG: short")? as usize;
    let x = i16_at(h, 0x20).unwrap_or(0) as i32;
    let y = i16_at(h, 0x22).unwrap_or(0) as i32;
    let mode = *h.get(0x30).ok_or("BSG: short")?;
    let pack = *h.get(0x31).ok_or("BSG: short")?;
    let data = u32_at(h, 0x32).ok_or("BSG: short")? as usize;
    let dsize = u32_at(h, 0x36).ok_or("BSG: short")? as usize;
    let pal_at = u32_at(h, 0x3a).unwrap_or(0) as usize;
    if mode > 2 || pack > 2 || w == 0 || ht == 0 || w > 8192 || ht > 8192 {
        return Err(format!("BSG: mode {mode} pack {pack} {w}x{ht}"));
    }
    let bpp = if mode == 2 { 1 } else { 4 };
    let size = size.max(w * ht * bpp);
    let mut out = vec![0u8; size];
    let src = h.get(data..).unwrap_or(&[]);
    let src = &src[..dsize.min(src.len())];
    if pack == 0 {
        if mode == 1 {
            for (d, s) in out.chunks_exact_mut(4).zip(src.chunks_exact(3)) {
                d[..3].copy_from_slice(s);
            }
        } else {
            let n = src.len().min(out.len());
            out[..n].copy_from_slice(&src[..n]);
        }
    } else {
        let planes = match mode {
            0 => 4,
            1 => 3,
            _ => 1,
        };
        let mut pos = 0;
        for p in 0..planes {
            pos += if pack == 1 {
                rle(&src[pos.min(src.len())..], &mut out, p, bpp)
            } else {
                lz(&src[pos.min(src.len())..], &mut out, p, bpp)
            };
        }
    }
    let mut img = Image::new(w as u32, ht as u32);
    let pal: Vec<[u8; 4]> = if mode == 2 {
        let p = h.get(pal_at..).unwrap_or(&[]);
        (0..256)
            .map(|i| {
                let c = p.get(i * 4..i * 4 + 4).unwrap_or(&[0, 0, 0, 0]);
                [c[2], c[1], c[0], 255]
            })
            .collect()
    } else {
        Vec::new()
    };
    for yy in 0..ht {
        // rows are stored bottom up
        let row = &out[(ht - 1 - yy) * w * bpp..(ht - yy) * w * bpp];
        for xx in 0..w {
            let o = (yy * w + xx) * 4;
            let c = match mode {
                0 => {
                    let p = &row[xx * 4..xx * 4 + 4];
                    [p[2], p[1], p[0], p[3]]
                }
                1 => {
                    let p = &row[xx * 4..xx * 4 + 4];
                    [p[2], p[1], p[0], 255]
                }
                _ => pal[row[xx] as usize],
            };
            img.px[o..o + 4].copy_from_slice(&c);
        }
    }
    img.renew();
    Ok(Bsg { image: img, x, y })
}

/// One RLE plane: a byte count, then runs (n >= 0: n + 1 bytes; n < 0: 1 - n copies).
/// Returns the bytes used.
fn rle(src: &[u8], out: &mut [u8], plane: usize, step: usize) -> usize {
    let Some(total) = u32_at(src, 0) else {
        return src.len();
    };
    let mut s = 4;
    let end = (4 + total as usize).min(src.len());
    let mut d = plane;
    while s < end {
        let n = src[s] as i8;
        s += 1;
        if n >= 0 {
            for _ in 0..=n as usize {
                if let (Some(&v), Some(o)) = (src.get(s), out.get_mut(d)) {
                    *o = v;
                }
                s += 1;
                d += step;
            }
        } else {
            let v = src.get(s).copied().unwrap_or(0);
            s += 1;
            for _ in 0..(1 - n as i32) {
                if let Some(o) = out.get_mut(d) {
                    *o = v;
                }
                d += step;
            }
        }
    }
    end
}

/// One LZ plane: the control byte and the size (with these 5 bytes), then bytes; the control
/// byte starts a back reference (distance, count) unless doubled. The plane is then summed.
fn lz(src: &[u8], out: &mut [u8], plane: usize, step: usize) -> usize {
    let (Some(&ctl), Some(total)) = (src.first(), u32_at(src, 1)) else {
        return src.len();
    };
    let end = (total as usize).min(src.len());
    let mut s = 5;
    let mut d = plane;
    while s < end {
        let c = src[s];
        s += 1;
        if c == ctl {
            let mut off = src.get(s).copied().unwrap_or(0) as usize;
            s += 1;
            if off != ctl as usize {
                let n = src.get(s).copied().unwrap_or(0);
                s += 1;
                if off > ctl as usize {
                    off -= 1;
                }
                let off = off * step;
                for _ in 0..n {
                    if d >= off && d < out.len() {
                        out[d] = out[d - off];
                    }
                    d += step;
                }
                continue;
            }
        }
        if let Some(o) = out.get_mut(d) {
            *o = c;
        }
        d += step;
    }
    let mut i = plane + step;
    while i < out.len() {
        out[i] = out[i].wrapping_add(out[i - step]);
        i += step;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz_back_reference_and_delta() {
        // control 0xff; bytes 1, 2, then copy 2 bytes from distance 2, then a literal 0xff
        let src = [0xff, 12, 0, 0, 0, 1, 2, 0xff, 2, 2, 0xff, 0xff];
        let mut out = [0u8; 5];
        let used = lz(&src, &mut out, 0, 1);
        assert_eq!(used, 12);
        // before the sums: 1 2 1 2 ff
        assert_eq!(out, [1, 3, 4, 6, 5]);
    }

    #[test]
    fn rle_runs() {
        let src = [5, 0, 0, 0, 1, 7, 8, 0xfe, 9];
        let mut out = [0u8; 5];
        rle(&src, &mut out, 0, 1);
        assert_eq!(out, [7, 8, 9, 9, 9]);
    }
}
