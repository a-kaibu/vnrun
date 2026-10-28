//! EPA pictures: "EP", byte 3 the mode (2: an x, y offset at 16), byte 4 the colours (0: a
//! palette, 1: BGR, 2: BGRA, 4: a palette and alpha), the size at 8. Each channel (planar: B, G,
//! R, A) is packed with copies from 16 fixed distances.

use nexas::gfx::Image;

pub struct Epa {
    pub img: Image,
    /// where it is drawn from its position (mode 2)
    pub ox: i32,
    pub oy: i32,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4)
        .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
}

pub fn decode(d: &[u8]) -> Result<Epa, String> {
    if d.len() < 16 || !d.starts_with(b"EP") {
        return Err("not an EPA picture".into());
    }
    let (mode, colours) = (d[3], d[4]);
    let (w, h) = (u32_at(d, 8) as usize, u32_at(d, 12) as usize);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(format!("EPA: a bad size {w}x{h}"));
    }
    let (mut ox, mut oy, mut pos) = (0, 0, 16);
    if mode == 2 {
        ox = u32_at(d, 16) as i32;
        oy = u32_at(d, 20) as i32;
        pos = 24;
    }
    let bpp = match colours {
        0 | 4 => 1,
        1 => 3,
        2 => 4,
        c => return Err(format!("EPA: colour type {c} is not supported")),
    };
    let mut pal = &[][..];
    if bpp == 1 {
        pal = d
            .get(pos..pos + 768)
            .ok_or("EPA: the palette is cut short")?;
        pos += 768;
    }
    let off = [
        0,
        1,
        w,
        w + 1,
        2,
        w - 1,
        w * 2,
        3,
        (w + 1) * 2,
        w + 2,
        w * 2 + 1,
        w * 2 - 1,
        (w - 1) * 2,
        w.saturating_sub(2),
        w * 3,
        4,
    ];
    let mut channel = |size: usize| {
        let mut out = vec![0u8; size];
        let mut dst = 0;
        while dst < size && pos < d.len() {
            let flag = d[pos];
            pos += 1;
            if flag & 0xf0 == 0 {
                let n = (flag as usize).min(size - dst).min(d.len() - pos);
                out[dst..dst + n].copy_from_slice(&d[pos..pos + n]);
                pos += n;
                dst += n;
                if n == 0 {
                    break;
                }
            } else {
                let n = if flag & 8 != 0 {
                    let b = d.get(pos).copied().unwrap_or(0) as usize;
                    pos += 1;
                    b + (((flag & 7) as usize) << 8)
                } else {
                    (flag & 7) as usize
                };
                let back = off[(flag >> 4) as usize];
                if dst + n > size || back > dst || back == 0 {
                    break;
                }
                for i in dst..dst + n {
                    out[i] = out[i - back];
                }
                dst += n;
            }
        }
        out
    };
    let n = w * h;
    let px = channel(n * bpp);
    let mut img = Image::new(w as u32, h as u32);
    if bpp == 1 {
        let alpha = if colours == 4 { Some(channel(n)) } else { None };
        for i in 0..n {
            let c = px[i] as usize * 3;
            img.px[i * 4..i * 4 + 4].copy_from_slice(&[
                pal[c + 2],
                pal[c + 1],
                pal[c],
                alpha.as_ref().map_or(255, |a| a[i]),
            ]);
        }
    } else {
        for (p, target) in [2usize, 1, 0, 3].into_iter().take(bpp).enumerate() {
            let plane = &px[p * n..(p + 1) * n];
            for (i, &v) in plane.iter().enumerate() {
                img.px[i * 4 + target] = v;
            }
        }
        if bpp == 3 {
            for i in 0..n {
                img.px[i * 4 + 3] = 255;
            }
        }
    }
    Ok(Epa { img, ox, oy })
}
