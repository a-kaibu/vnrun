//! Pictures: plain BMP (24 or 8 bits) and ABM, a BMP header whose bit count field says how the
//! pixels are packed (after GARbro's `ImageABM.cs` / `ArcABM.cs`):
//! 32 / 24: run-length coded, 32 with an opacity per pixel; 1 / 2: several frames (the first
//! is used); 8 / -8: palette pictures, -8 with opacity. Plain BMP rows are stored bottom
//! first, the packed ABM ones top first.

use nexas::gfx::Image;

fn u16at(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2)
        .map_or(0, |s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4)
        .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.at)?;
        self.at += 1;
        Some(v)
    }
}

/// 0 n: skip n bytes; 0xff n: n bytes follow; else one byte follows.
fn unpack24(r: &mut Reader, out: &mut [u8]) {
    let mut dst = 0;
    while dst < out.len() {
        let Some(v) = r.u8() else { return };
        match v {
            0 => {
                let Some(n) = r.u8() else { return };
                dst += n as usize;
            }
            0xff => {
                let Some(n) = r.u8() else { return };
                for _ in 0..n {
                    if dst >= out.len() {
                        break;
                    }
                    let Some(c) = r.u8() else { return };
                    out[dst] = c;
                    dst += 1;
                }
            }
            _ => {
                let Some(c) = r.u8() else { return };
                out[dst] = c;
                dst += 1;
            }
        }
    }
}

/// As [`unpack24`] over the colour bytes of BGRA pixels; skipped pixels stay clear, runs are
/// opaque and a single byte's code is its pixel's opacity.
fn unpack32(r: &mut Reader, out: &mut [u8]) {
    let mut dst = 0;
    let mut comp = 0;
    while dst < out.len() {
        let Some(v) = r.u8() else { return };
        match v {
            0 => {
                let Some(n) = r.u8() else { return };
                for _ in 0..n {
                    dst += 1;
                    comp += 1;
                    if comp == 3 {
                        dst += 1;
                        comp = 0;
                    }
                }
            }
            0xff => {
                let Some(n) = r.u8() else { return };
                for _ in 0..n {
                    if dst >= out.len() {
                        break;
                    }
                    let Some(c) = r.u8() else { return };
                    out[dst] = c;
                    dst += 1;
                    comp += 1;
                    if comp == 3 {
                        if dst < out.len() {
                            out[dst] = 0xff;
                        }
                        dst += 1;
                        comp = 0;
                    }
                }
            }
            _ => {
                let Some(c) = r.u8() else { return };
                if dst < out.len() {
                    out[dst] = c;
                }
                dst += 1;
                comp += 1;
                if comp == 3 {
                    if dst < out.len() {
                        out[dst] = v;
                    }
                    dst += 1;
                    comp = 0;
                }
            }
        }
    }
}

/// Rows of `bpp`-byte pixels (BGR or BGRA) to an RGBA picture; BMP rows are padded to 4
/// bytes and stored bottom first.
fn to_image(w: usize, h: usize, bpp: usize, px: &[u8], opaque: bool, bmp: bool) -> Image {
    let mut img = Image::new(w as u32, h as u32);
    let stride = if bmp && bpp == 3 {
        (w * 3).div_ceil(4) * 4
    } else {
        w * bpp
    };
    for y in 0..h {
        let src = if bmp {
            (h - 1 - y) * stride
        } else {
            y * stride
        };
        for x in 0..w {
            let s = src + x * bpp;
            let d = (y * w + x) * 4;
            let Some(p) = px.get(s..s + bpp) else {
                continue;
            };
            img.px[d] = p[2];
            img.px[d + 1] = p[1];
            img.px[d + 2] = p[0];
            img.px[d + 3] = if bpp == 4 && !opaque { p[3] } else { 255 };
        }
    }
    img
}

/// Decodes a picture file (unpacked).
pub fn decode(b: &[u8]) -> Result<Image, String> {
    if b.len() < 0x36 || &b[..2] != b"BM" {
        return Err("not a BMP".into());
    }
    let w = u32at(b, 0x12) as usize;
    let hraw = u32at(b, 0x16) as i32;
    let h = hraw.unsigned_abs() as usize;
    let kind = u16at(b, 0x1c) as i16;
    let data = u32at(b, 0xa) as usize;
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(format!("bad size {w}x{h}"));
    }
    let size_field = u32at(b, 2) as usize;
    let plain = size_field == b.len() || size_field == 0;
    let mut img = match kind {
        8 | -8 => {
            let colours = u32at(b, 0x2e) as usize;
            let colours = if colours == 0 { 256 } else { colours.min(256) };
            let pal: Vec<[u8; 3]> = (0..colours)
                .map(|i| {
                    let p = 0x36 + i * 4;
                    [
                        *b.get(p + 2).unwrap_or(&0),
                        *b.get(p + 1).unwrap_or(&0),
                        *b.get(p).unwrap_or(&0),
                    ]
                })
                .collect();
            let mut img = Image::new(w as u32, h as u32);
            if kind == 8 {
                let stride = w.div_ceil(4) * 4;
                for y in 0..h {
                    for x in 0..w {
                        let i = *b.get(data + (h - 1 - y) * stride + x).unwrap_or(&0) as usize;
                        let c = pal.get(i).copied().unwrap_or([0; 3]);
                        let d = (y * w + x) * 4;
                        img.px[d..d + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                    }
                }
            } else {
                // a byte per pixel with an opacity code before it (0 n: n clear, 0xff n: n opaque)
                let mut idx = vec![0u8; w * h];
                let mut alpha = vec![0u8; w * h];
                let mut r = Reader { b, at: data };
                let mut dst = 0;
                while dst < idx.len() {
                    let Some(v) = r.u8() else { break };
                    match v {
                        0 => dst += r.u8().unwrap_or(0) as usize,
                        0xff => {
                            let n = r.u8().unwrap_or(0) as usize;
                            for _ in 0..n {
                                if dst < idx.len() {
                                    idx[dst] = r.u8().unwrap_or(0);
                                    alpha[dst] = 255;
                                }
                                dst += 1;
                            }
                        }
                        a => {
                            if dst < idx.len() {
                                idx[dst] = r.u8().unwrap_or(0);
                                alpha[dst] = a;
                            }
                            dst += 1;
                        }
                    }
                }
                for y in 0..h {
                    for x in 0..w {
                        let s = y * w + x;
                        let c = pal.get(idx[s] as usize).copied().unwrap_or([0; 3]);
                        let d = (y * w + x) * 4;
                        img.px[d..d + 4].copy_from_slice(&[c[0], c[1], c[2], alpha[s]]);
                    }
                }
            }
            img
        }
        24 if plain => to_image(w, h, 3, &b[data.min(b.len())..], true, true),
        32 if plain => to_image(w, h, 4, &b[data.min(b.len())..], false, true),
        24 => {
            let mut px = vec![0u8; w * h * 3];
            unpack24(&mut Reader { b, at: data }, &mut px);
            to_image(w, h, 3, &px, true, false)
        }
        32 => {
            let mut px = vec![0u8; w * h * 4];
            unpack32(&mut Reader { b, at: data }, &mut px);
            to_image(w, h, 4, &px, false, false)
        }
        1 | 2 => {
            // frames: the first one, from the offset at 0x42
            let at = u32at(b, 0x42) as usize;
            if kind == 1 {
                let n = w * h * 3;
                let px = b.get(at..at + n).ok_or("short frame")?;
                to_image(w, h, 3, px, true, false)
            } else {
                let mut r = Reader { b, at };
                let fx = u32at(b, at) as usize;
                let fy = u32at(b, at + 4) as usize;
                let fw = u32at(b, at + 8) as usize;
                let fh = u32at(b, at + 12) as usize;
                r.at += 17;
                if fw == 0 || fh == 0 || fw > 8192 || fh > 8192 {
                    return Err("bad frame".into());
                }
                let mut frame = vec![0u8; fw * fh * 4];
                unpack32(&mut r, &mut frame);
                // the frame is laid into a clear picture, rows top first in its own place
                let mut px = vec![0u8; w * h * 4];
                for y in 0..fh {
                    let ty = fy + y;
                    if ty >= h {
                        break;
                    }
                    for x in 0..fw.min(w.saturating_sub(fx)) {
                        let s = (y * fw + x) * 4;
                        let d = (ty * w + fx + x) * 4;
                        px[d..d + 4].copy_from_slice(&frame[s..s + 4]);
                    }
                }
                to_image(w, h, 4, &px, false, false)
            }
        }
        _ => return Err(format!("picture kind {kind}")),
    };
    if hraw < 0 {
        // top-first rows: turn back
        let row = w * 4;
        let mut flipped = vec![0u8; img.px.len()];
        for y in 0..h {
            flipped[y * row..(y + 1) * row]
                .copy_from_slice(&img.px[(h - 1 - y) * row..(h - y) * row]);
        }
        img.px = flipped;
    }
    Ok(img)
}

/// One of a button picture's states, stacked top to bottom (`n` of them).
pub fn state(img: &Image, k: usize, n: usize) -> Image {
    let n = n.max(1);
    let h = img.h as usize / n;
    let k = k.min(n - 1);
    let mut out = Image::new(img.w, h as u32);
    let row = img.w as usize * 4;
    out.px
        .copy_from_slice(&img.px[k * h * row..(k + 1) * h * row]);
    out
}
