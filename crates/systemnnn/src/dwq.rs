//! DWQ pictures: a 0x40-byte header ("PACKTYPE=n" at 0x30, an "A" after n when a mask follows)
//! and the picture: 0 BMP, 5 JPEG, 8 PNG, 2 BMP and mask, 3 packed BMP and mask, 7 JPEG and
//! mask. The packed size is at 0x20 when a mask follows it. The BMPs keep their rows top down
//! and (24 and 32 bits) their colours as R G B. A packed BMP stores a 0 byte as 0 and a count
//! of zeros, and each row is XORed with the one above. The mask is a packed 8-bit BMP whose
//! palette grey is the opacity.

use nexas::gfx::Image;

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn i32_at(b: &[u8], o: usize) -> Option<i32> {
    u32_at(b, o).map(|v| v as i32)
}

/// The pack type and whether a mask follows.
pub fn pack_type(b: &[u8]) -> Option<(u32, bool)> {
    let h = std::str::from_utf8(b.get(0x30..0x40)?).ok()?;
    let rest = h.strip_prefix("PACKTYPE=")?.trim_end();
    let (num, a) = match rest.strip_suffix('A') {
        Some(n) => (n, true),
        None => (rest, false),
    };
    Some((num.parse().ok()?, a))
}

pub fn decode(b: &[u8]) -> Result<Image, String> {
    let (kind, a) = pack_type(b).ok_or("DWQ: no PACKTYPE")?;
    let raw_bmp = b.starts_with(b"IF PACKTYPE==");
    let masked = !raw_bmp && matches!(kind, 2 | 3 | 7);
    let packed = if masked {
        u32_at(b, 0x20).ok_or("DWQ: short")? as usize
    } else {
        b.len().saturating_sub(0x40)
    };
    let body = b
        .get(0x40..(0x40 + packed).min(b.len()))
        .ok_or("DWQ: short")?;
    let mut img = match kind {
        5 | 7 | 8 if !raw_bmp => {
            let i = image::load_from_memory(body).map_err(|e| format!("DWQ: {e}"))?;
            let i = i.to_rgba8();
            let (w, h) = i.dimensions();
            let mut out = Image::new(w, h);
            out.px = i.into_raw();
            out
        }
        1 | 3 if !raw_bmp => packed_bmp(body)?.0,
        _ => plain_bmp(body)?,
    };
    if (a || masked)
        && !raw_bmp
        && let Some(m) = b.get(0x40 + packed..)
        && !m.is_empty()
        && let Ok((mask, pal)) = packed_bmp(m)
        && mask.w == img.w
        && mask.h == img.h
        && let Some(pal) = pal
    {
        for (p, q) in img.px.chunks_exact_mut(4).zip(mask.px.chunks_exact(4)) {
            // the mask image keeps the palette index in red
            let c = pal.get(q[0] as usize).copied().unwrap_or([255; 3]);
            p[3] = ((c[0] as u32 + c[1] as u32 + c[2] as u32) / 3) as u8;
        }
    }
    img.renew();
    Ok(img)
}

type Palette = Vec<[u8; 3]>;

/// A BMP header: width, height, bits, palette (R G B), where the pixels start.
struct Bmp {
    w: u32,
    h: u32,
    bpp: u16,
    pal: Vec<[u8; 3]>,
    data: usize,
}

fn bmp_header(b: &[u8]) -> Result<Bmp, String> {
    if b.len() < 0x36 {
        return Err("DWQ: short BMP".into());
    }
    let w = i32_at(b, 0x12).unwrap_or(0);
    let h = i32_at(b, 0x16).unwrap_or(0).unsigned_abs();
    let bpp = u16::from_le_bytes([b[0x1c], b[0x1d]]);
    if w <= 0 || h == 0 || w > 16384 || h > 16384 || !matches!(bpp, 8 | 16 | 24 | 32) {
        return Err(format!("DWQ: BMP {w}x{h} {bpp}"));
    }
    let mut pal = Vec::new();
    let mut data = 0x36;
    if bpp == 8 {
        let mut n = u32_at(b, 0x2e).unwrap_or(0).min(256) as usize;
        if n == 0 {
            n = 256;
        }
        for i in 0..n {
            let o = 0x36 + i * 4;
            let c = b.get(o..o + 3).unwrap_or(&[0, 0, 0]);
            pal.push([c[2], c[1], c[0]]);
        }
        data = 0x36 + n * 4;
    }
    Ok(Bmp {
        w: w as u32,
        h,
        bpp,
        pal,
        data,
    })
}

/// Rows of pixels (top down) into RGBA; `rgb` when 24/32-bit pixels are R G B, not B G R.
fn to_image(hd: &Bmp, rows: &[u8], stride: usize, rgb: bool) -> Image {
    let mut img = Image::new(hd.w, hd.h);
    let bytes = (hd.bpp / 8) as usize;
    for y in 0..hd.h as usize {
        let Some(row) = rows.get(y * stride..y * stride + hd.w as usize * bytes) else {
            break;
        };
        for x in 0..hd.w as usize {
            let o = (y * hd.w as usize + x) * 4;
            let p = &row[x * bytes..x * bytes + bytes];
            let c = match hd.bpp {
                8 => {
                    let c = hd.pal.get(p[0] as usize).copied().unwrap_or([0; 3]);
                    [c[0], c[1], c[2]]
                }
                16 => {
                    let v = u16::from_le_bytes([p[0], p[1]]);
                    let r = ((v >> 11) & 31) as u8;
                    let g = ((v >> 5) & 63) as u8;
                    let b = (v & 31) as u8;
                    [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2]
                }
                _ if rgb => [p[0], p[1], p[2]],
                _ => [p[2], p[1], p[0]],
            };
            img.px[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    img
}

/// The plain BMP of types 0 and 2: rows top down, padded to 4 bytes, colours R G B.
fn plain_bmp(b: &[u8]) -> Result<Image, String> {
    let hd = bmp_header(b)?;
    let stride = (hd.w as usize * (hd.bpp / 8) as usize + 3) & !3;
    let rows = b.get(hd.data..).unwrap_or(&[]);
    Ok(to_image(&hd, rows, stride, true))
}

/// A packed BMP (types 1 and 3, and the masks): its picture, and the palette when 8-bit (the
/// picture then has the index in red).
fn packed_bmp(b: &[u8]) -> Result<(Image, Option<Palette>), String> {
    let hd = bmp_header(b)?;
    let stride = hd.w as usize * (hd.bpp / 8) as usize;
    let start = u32_at(b, 0x0a).unwrap_or(0) as usize;
    let mut src = b.get(start..).unwrap_or(&[]).iter().copied();
    let mut rows = vec![0u8; stride * hd.h as usize];
    let mut prev = vec![0u8; stride];
    for y in 0..hd.h as usize {
        let row = &mut rows[y * stride..(y + 1) * stride];
        let mut x = 0;
        while x < stride {
            let v = src.next().unwrap_or(0);
            if v != 0 {
                row[x] = v;
                x += 1;
            } else {
                let n = src.next().unwrap_or(1).max(1) as usize;
                x += n;
            }
        }
        for (c, p) in row.iter_mut().zip(prev.iter_mut()) {
            *c ^= *p;
            *p = *c;
        }
    }
    if hd.bpp == 8 {
        let mut img = Image::new(hd.w, hd.h);
        for (i, &v) in rows.iter().enumerate() {
            img.px[i * 4] = v;
            img.px[i * 4 + 3] = 255;
        }
        return Ok((img, Some(hd.pal)));
    }
    Ok((to_image(&hd, &rows, stride, false), None))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bmp8(w: u32, h: u32, pal: &[[u8; 3]], body: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 0x36];
        b[0] = b'B';
        b[1] = b'M';
        let data = 0x36 + pal.len() * 4;
        b[0x0a..0x0e].copy_from_slice(&(data as u32).to_le_bytes());
        b[0x12..0x16].copy_from_slice(&w.to_le_bytes());
        b[0x16..0x1a].copy_from_slice(&h.to_le_bytes());
        b[0x1c] = 8;
        b[0x2e..0x32].copy_from_slice(&(pal.len() as u32).to_le_bytes());
        for c in pal {
            b.extend([c[2], c[1], c[0], 0]);
        }
        b.extend(body);
        b
    }

    #[test]
    fn packed_rows_xor_and_zero_runs() {
        // 3x2: row 0 = 1 0 0 (1, then a run of two zeros), row 1 = row 0 ^ (0 2 0)
        let b = bmp8(3, 2, &[[0; 3], [9; 3], [7; 3]], &[1, 0, 2, 0, 1, 2, 0, 1]);
        let (img, pal) = packed_bmp(&b).unwrap();
        let idx: Vec<u8> = img.px.chunks(4).map(|p| p[0]).collect();
        assert_eq!(idx, vec![1, 0, 0, 1, 2, 0]);
        assert_eq!(pal.unwrap()[1], [9, 9, 9]);
    }

    #[test]
    fn header_type() {
        let mut b = vec![b' '; 0x40];
        b[0x30..0x3b].copy_from_slice(b"PACKTYPE=7A");
        assert_eq!(pack_type(&b), Some((7, true)));
        b[0x30..0x3b].copy_from_slice(b"PACKTYPE=5 ");
        assert_eq!(pack_type(&b), Some((5, false)));
    }
}
