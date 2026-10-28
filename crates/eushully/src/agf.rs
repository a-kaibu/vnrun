//! AGF pictures ("ACGF"): a 0x18-byte header (type 1: 24-bit, 2: with an alpha plane; the
//! unpacked and packed size of the next part), the bitmap headers (LZSS-packed when the sizes
//! differ), then the pixel rows bottom-up (LZSS-packed the same way) and for type 2 an "ACIF"
//! part with one alpha byte per pixel, top-down. 8- and 4-bit pictures carry a palette after
//! the bitmap headers.

use nexas::gfx::Image;

use crate::alf::lzss;

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn section(b: &[u8], at: usize, unpacked: usize, packed: usize) -> Option<Vec<u8>> {
    let src = b.get(at..at.checked_add(packed)?)?;
    if unpacked == packed {
        Some(src.to_vec())
    } else {
        Some(lzss(src, unpacked))
    }
}

pub fn decode(b: &[u8]) -> Result<Image, String> {
    if !(b.starts_with(b"ACGF") || b.starts_with(&[0, 0, 0, 0])) {
        return Err("AGF: no signature".into());
    }
    let short = || "AGF: short".to_string();
    let kind = u32_at(b, 4).ok_or_else(short)?;
    if kind != 1 && kind != 2 {
        return Err(format!("AGF: type {kind}"));
    }
    let unpacked = u32_at(b, 0xc).ok_or_else(short)? as usize;
    let packed = u32_at(b, 0x14).ok_or_else(short)? as usize;
    let head = section(b, 0x18, unpacked, packed).ok_or_else(short)?;
    let w = u32_at(&head, 0x14).ok_or_else(short)? as usize;
    let h = u32_at(&head, 0x18).ok_or_else(short)? as usize;
    let bpp = u16::from_le_bytes(head.get(0x1e..0x20).ok_or_else(short)?.try_into().unwrap());
    if w == 0 || h == 0 || w > 8192 || h > 8192 || bpp == 0 {
        return Err(format!("AGF: {w}x{h} {bpp} bits"));
    }
    let palette: Vec<[u8; 3]> = if bpp <= 8 {
        let p = head.get(0x38..).unwrap_or(&[]);
        p.chunks_exact(4).map(|c| [c[2], c[1], c[0]]).collect()
    } else {
        Vec::new()
    };
    let mut at = 0x18 + packed;
    let data_size = u32_at(b, at + 4).ok_or_else(short)? as usize;
    let data_packed = u32_at(b, at + 8).ok_or_else(short)? as usize;
    at += 12;
    let bmp = section(b, at, data_size, data_packed).ok_or_else(short)?;
    at += data_packed;
    let alpha = if kind == 2 && b.get(at..at + 4) == Some(b"ACIF") {
        let un = u32_at(b, at + 0x1c).unwrap_or(0) as usize;
        let pk = u32_at(b, at + 0x20).unwrap_or(0) as usize;
        if un == w * h {
            section(b, at + 0x24, un, pk)
        } else {
            None
        }
    } else {
        None
    };
    let stride = (w * bpp as usize / 8 + 3) & !3;
    let mut img = Image::new(w as u32, h as u32);
    for y in 0..h {
        let row = (h - 1 - y) * stride;
        for x in 0..w {
            let rgb = match bpp {
                4 => {
                    let v = bmp.get(row + x / 2).copied().unwrap_or(0);
                    let i = if x % 2 == 0 { v >> 4 } else { v & 15 };
                    palette.get(i as usize).copied().unwrap_or([0; 3])
                }
                8 => {
                    let i = bmp.get(row + x).copied().unwrap_or(0);
                    palette.get(i as usize).copied().unwrap_or([0; 3])
                }
                _ => {
                    let n = bpp as usize / 8;
                    let s = row + x * n;
                    match bmp.get(s..s + 3) {
                        Some(p) => [p[2], p[1], p[0]],
                        None => [0; 3],
                    }
                }
            };
            let d = (y * w + x) * 4;
            img.px[d..d + 3].copy_from_slice(&rgb);
            img.px[d + 3] = match &alpha {
                Some(a) => a.get(y * w + x).copied().unwrap_or(255),
                None => 255,
            };
        }
    }
    Ok(img)
}
