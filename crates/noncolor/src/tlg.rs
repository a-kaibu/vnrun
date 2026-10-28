//! KiriKiri's TLG pictures: TLG5 (per-channel LZSS of the differences) and TLG6 (Golomb codes
//! of the residues of a predictor per 8x8 block), optionally inside a TLG0 "sds" wrapper whose
//! `tags` chunk carries extra values (the place of a part). Written from the format as
//! KiriKiri's LoadTLG.cpp reads it.

use std::collections::HashMap;

use nexas::gfx::Image;

pub struct Tlg {
    pub img: Image,
    /// the `tags` of a TLG0 wrapper (`left`, `top`, ...)
    pub tags: HashMap<Vec<u8>, Vec<u8>>,
}

fn u32_at(b: &[u8], i: usize) -> Result<u32, String> {
    b.get(i..i + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| "TLG: short".to_string())
}

pub fn decode(b: &[u8]) -> Result<Tlg, String> {
    if b.starts_with(b"TLG0.0\0sds\x1a") {
        let len = u32_at(b, 11)? as usize;
        let inner = b.get(15..15 + len).ok_or("TLG0: short")?;
        let mut t = decode(inner)?;
        let mut p = 15 + len;
        while p + 8 <= b.len() {
            let id = &b[p..p + 4];
            let n = u32_at(b, p + 4)? as usize;
            let d = b.get(p + 8..p + 8 + n).ok_or("TLG0: short chunk")?;
            if id == b"tags" {
                t.tags = tags(d);
            }
            p += 8 + n;
        }
        return Ok(t);
    }
    let img = if b.starts_with(b"TLG5.0\0raw\x1a") {
        tlg5(b)?
    } else if b.starts_with(b"TLG6.0\0raw\x1a") {
        tlg6(b)?
    } else {
        return Err("not a TLG picture".into());
    };
    Ok(Tlg {
        img,
        tags: HashMap::new(),
    })
}

/// `len:name=len:value,` pairs.
fn tags(d: &[u8]) -> HashMap<Vec<u8>, Vec<u8>> {
    let mut out = HashMap::new();
    let mut p = 0;
    let field = |p: &mut usize| -> Option<Vec<u8>> {
        let colon = d[*p..].iter().position(|&c| c == b':')? + *p;
        let n: usize = std::str::from_utf8(&d[*p..colon])
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let s = d.get(colon + 1..colon + 1 + n)?;
        *p = colon + 1 + n + 1;
        Some(s.to_vec())
    };
    while p < d.len() {
        let (Some(k), Some(v)) = (field(&mut p), field(&mut p)) else {
            break;
        };
        out.insert(k, v);
    }
    out
}

/// LZSS with a 4096-byte window; returns the window position.
fn slide(out: &mut Vec<u8>, src: &[u8], text: &mut [u8; 4096], mut r: usize) -> usize {
    let mut i = 0;
    let mut flags = 0u32;
    while i < src.len() {
        flags >>= 1;
        if flags & 0x100 == 0 {
            flags = src[i] as u32 | 0xff00;
            i += 1;
            if i >= src.len() {
                break;
            }
        }
        if flags & 1 != 0 {
            if i + 1 >= src.len() {
                break;
            }
            let mut pos = src[i] as usize | ((src[i + 1] as usize & 0xf) << 8);
            let mut len = (src[i + 1] as usize >> 4) + 3;
            i += 2;
            if len == 18 {
                len += *src.get(i).unwrap_or(&0) as usize;
                i += 1;
            }
            for _ in 0..len {
                let c = text[pos];
                out.push(c);
                text[r] = c;
                r = (r + 1) & 4095;
                pos = (pos + 1) & 4095;
            }
        } else {
            let c = src[i];
            i += 1;
            out.push(c);
            text[r] = c;
            r = (r + 1) & 4095;
        }
    }
    r
}

fn tlg5(b: &[u8]) -> Result<Image, String> {
    let colors = *b.get(11).ok_or("TLG5: short")? as usize;
    let w = u32_at(b, 12)? as usize;
    let h = u32_at(b, 16)? as usize;
    let bh = u32_at(b, 20)? as usize;
    if !(3..=4).contains(&colors) || bh == 0 {
        return Err(format!("TLG5: {colors} colours"));
    }
    let blocks = h.div_ceil(bh);
    let mut p = 24 + blocks * 4;
    let mut text = [0u8; 4096];
    let mut r = 0;
    let mut img = Image::new(w as u32, h as u32);
    let mut y0 = 0;
    while y0 < h {
        let mut bufs = Vec::new();
        for _ in 0..colors {
            let mark = *b.get(p).ok_or("TLG5: short")?;
            let size = u32_at(b, p + 1)? as usize;
            let d = b.get(p + 5..p + 5 + size).ok_or("TLG5: short")?;
            p += 5 + size;
            let mut out = Vec::with_capacity(bh * w);
            if mark == 0 {
                r = slide(&mut out, d, &mut text, r);
            } else {
                out.extend_from_slice(d);
            }
            out.resize(bh * w, 0);
            bufs.push(out);
        }
        for y in y0..(y0 + bh).min(h) {
            let row = (y - y0) * w;
            let mut pc = [0u8; 4];
            for x in 0..w {
                let mut c = [0u8; 4];
                for (k, v) in c.iter_mut().enumerate().take(colors) {
                    *v = bufs[k][row + x];
                }
                c[0] = c[0].wrapping_add(c[1]);
                c[2] = c[2].wrapping_add(c[1]);
                for k in 0..colors {
                    pc[k] = pc[k].wrapping_add(c[k]);
                }
                let o = (y * w + x) * 4;
                let up = |k: usize| if y > 0 { img.px[o - w * 4 + k] } else { 0 };
                // stored B G R (A): the picture is RGBA
                let bb = pc[0].wrapping_add(if y > 0 { up_b(&img.px, o, w) } else { 0 });
                let g = pc[1].wrapping_add(up(1));
                let rr = pc[2].wrapping_add(if y > 0 { up_r(&img.px, o, w) } else { 0 });
                let a = if colors == 4 {
                    pc[3].wrapping_add(up(3))
                } else {
                    255
                };
                img.px[o..o + 4].copy_from_slice(&[rr, g, bb, a]);
            }
        }
        y0 += bh;
    }
    Ok(img)
}

/// The row above's blue and red (the picture keeps RGBA).
fn up_b(px: &[u8], o: usize, w: usize) -> u8 {
    px[o - w * 4 + 2]
}

fn up_r(px: &[u8], o: usize, w: usize) -> u8 {
    px[o - w * 4]
}

const W_BLOCK: usize = 8;
const H_BLOCK: usize = 8;
const GOLOMB_N: usize = 4;

fn golomb_table() -> Vec<[u8; GOLOMB_N]> {
    const C: [[u16; 9]; GOLOMB_N] = [
        [3, 7, 15, 27, 63, 108, 223, 448, 130],
        [3, 5, 13, 24, 51, 95, 192, 384, 257],
        [2, 5, 12, 21, 39, 86, 155, 320, 384],
        [2, 3, 9, 18, 33, 61, 129, 258, 511],
    ];
    let mut t = vec![[0u8; GOLOMB_N]; GOLOMB_N * 2 * 128];
    for (n, row) in C.iter().enumerate() {
        let mut a = 0;
        for (i, &c) in row.iter().enumerate() {
            for _ in 0..c {
                t[a][n] = i as u8;
                a += 1;
            }
        }
    }
    t
}

/// A bit reader over a zero-padded buffer, from the low bits of each byte.
struct Bits<'a> {
    d: &'a [u8],
    p: usize,
    bit: u32,
}

impl Bits<'_> {
    fn peek(&self) -> u32 {
        let mut w = [0u8; 4];
        for (k, v) in w.iter_mut().enumerate() {
            *v = *self.d.get(self.p + k).unwrap_or(&0);
        }
        u32::from_le_bytes(w) >> self.bit
    }
    fn skip(&mut self, n: u32) {
        let t = self.bit + n;
        self.p += (t >> 3) as usize;
        self.bit = t & 7;
    }
    /// The count of bits up to and including the next 1.
    fn gamma_len(&mut self) -> Option<u32> {
        let mut n = 0;
        loop {
            let t = self.peek();
            if t != 0 {
                let z = t.trailing_zeros();
                self.skip(z + 1);
                return Some(n + z + 1);
            }
            if self.p >= self.d.len() {
                return None;
            }
            // at least 25 bits were zero
            self.skip(24);
            n += 24;
        }
    }
}

/// One channel of a block row: runs of zeros and of Golomb-coded values, alternating.
fn golomb(bits: &mut Bits, out: &mut [u8], stride: usize, offset: usize, table: &[[u8; 4]]) {
    let count_px = out.len() / stride;
    let mut zero = bits.d.first().is_some_and(|b| b & 1 == 0);
    bits.skip(1);
    let mut n = GOLOMB_N as i32 - 1;
    let mut a: i32 = 0;
    let mut i = 0;
    while i < count_px {
        let Some(len) = bits.gamma_len() else {
            return;
        };
        let bc = len - 1;
        let mut count = (1usize << bc) + (bits.peek() as usize & ((1usize << bc) - 1));
        bits.skip(bc);
        if zero {
            while count > 0 && i < count_px {
                out[i * stride + offset] = 0;
                i += 1;
                count -= 1;
            }
        } else {
            while count > 0 && i < count_px {
                let k = table[(a as usize).min(table.len() - 1)][n as usize] as u32;
                let q = if bits.peek() == 0 {
                    // a long value: its count of zeros in the fifth byte from here
                    bits.p += 5;
                    bits.bit = 0;
                    *bits.d.get(bits.p - 1).unwrap_or(&0) as u32
                } else {
                    let Some(q) = bits.gamma_len() else {
                        return;
                    };
                    q - 1
                };
                let v = (q << k) | (bits.peek() & ((1u32 << k) - 1));
                bits.skip(k);
                let sign = (v & 1) as i32 - 1;
                let v = (v >> 1) as i32;
                a += v;
                out[i * stride + offset] = ((v ^ sign) + sign + 1) as u8;
                i += 1;
                count -= 1;
                n -= 1;
                if n < 0 {
                    a >>= 1;
                    n = GOLOMB_N as i32 - 1;
                }
            }
        }
        zero = !zero;
    }
}

fn med(a: u8, b: u8, c: u8) -> u8 {
    let (lo, hi) = if a > b { (b, a) } else { (a, b) };
    if c >= hi {
        lo
    } else if c <= lo {
        hi
    } else {
        lo.wrapping_add(hi).wrapping_sub(c)
    }
}

fn tlg6(b: &[u8]) -> Result<Image, String> {
    let colors = *b.get(11).ok_or("TLG6: short")? as usize;
    if b.get(12..15) != Some(&[0, 0, 0]) || !(matches!(colors, 1 | 3 | 4)) {
        return Err("TLG6: unsupported".into());
    }
    let w = u32_at(b, 15)? as usize;
    let h = u32_at(b, 19)? as usize;
    let xb = w.div_ceil(W_BLOCK);
    let yb = h.div_ceil(H_BLOCK);
    let mut p = 27;
    let fsize = u32_at(b, p)? as usize;
    let fsrc = b.get(p + 4..p + 4 + fsize).ok_or("TLG6: short")?;
    p += 4 + fsize;
    let mut text = [0u8; 4096];
    let mut q = 0;
    for i in 0..32u8 {
        for j in 0..16u8 {
            text[q..q + 4].fill(i);
            text[q + 4..q + 8].fill(j);
            q += 8;
        }
    }
    let mut ftypes = Vec::new();
    slide(&mut ftypes, fsrc, &mut text, 0);
    ftypes.resize(xb * yb, 0);
    let table = golomb_table();
    // the picture as BGRA words
    let mut px = vec![0u32; w * h];
    let init: u32 = if colors == 3 { 0xff00_0000 } else { 0 };
    let zero = vec![init; w];
    let main = w / W_BLOCK;
    let frac = w - main * W_BLOCK;
    let mut y = 0;
    while y < h {
        let ylim = (y + H_BLOCK).min(h);
        let count = (ylim - y) * w;
        let mut buf = vec![0u8; count * 4];
        for c in 0..colors {
            let bl = u32_at(b, p)?;
            p += 4;
            let method = bl >> 30;
            let bl = (bl & 0x3fff_ffff) as usize;
            let bytes = bl.div_ceil(8);
            let d = b.get(p..p + bytes).ok_or("TLG6: short")?;
            p += bytes;
            if method != 0 {
                return Err(format!("TLG6: coding {method}"));
            }
            let mut bits = Bits { d, p: 0, bit: 0 };
            golomb(&mut bits, &mut buf, 4, c, &table);
            if colors == 1 {
                // grey: the same value in every channel
                for k in 0..count {
                    let v = buf[k * 4];
                    buf[k * 4 + 1] = v;
                    buf[k * 4 + 2] = v;
                }
            }
        }
        let words: Vec<u32> = buf
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let ft = &ftypes[(y / H_BLOCK) * xb..];
        let skip = (ylim - y) * W_BLOCK;
        for yy in y..ylim {
            let dir = (yy & 1) ^ 1;
            let oddskip = (ylim - yy - 1) as isize - (yy - y) as isize;
            let prev: Vec<u32> = if yy == 0 {
                zero.clone()
            } else {
                px[(yy - 1) * w..yy * w].to_vec()
            };
            let cur = &mut px[yy * w..(yy + 1) * w];
            if main > 0 {
                let start = w.min(W_BLOCK) * (yy - y);
                line(
                    &prev, cur, w, 0, main, ft, skip, &words, start, init, oddskip, dir,
                );
            }
            if main != xb {
                let ww = frac.min(W_BLOCK);
                let start = ww * (yy - y);
                line(
                    &prev, cur, w, main, xb, ft, skip, &words, start, init, oddskip, dir,
                );
            }
        }
        y = ylim;
    }
    let mut img = Image::new(w as u32, h as u32);
    for (i, v) in px.iter().enumerate() {
        let [bb, g, r, a] = v.to_le_bytes();
        img.px[i * 4..i * 4 + 4].copy_from_slice(&[r, g, bb, if colors == 4 { a } else { 255 }]);
    }
    Ok(img)
}

#[allow(clippy::too_many_arguments)]
fn line(
    prev: &[u32],
    cur: &mut [u32],
    width: usize,
    start_block: usize,
    block_limit: usize,
    ft: &[u8],
    skip: usize,
    input: &[u32],
    start: usize,
    init: u32,
    oddskip: isize,
    dir: usize,
) {
    let (mut p, mut up) = if start_block > 0 {
        let x = start_block * W_BLOCK;
        (cur[x - 1], prev[x - 1])
    } else {
        (init, init)
    };
    let mut x = start_block * W_BLOCK;
    let mut i_in = (start + skip * start_block) as isize;
    let step: isize = if dir & 1 != 0 { 1 } else { -1 };
    #[allow(clippy::needless_range_loop)]
    for i in start_block..block_limit {
        let w = (width - i * W_BLOCK).min(W_BLOCK);
        let ww = w as isize;
        if step == -1 {
            i_in += ww - 1;
        }
        if i & 1 != 0 {
            i_in += oddskip * ww;
        }
        let f = ft[i];
        let (kind, avg) = (f >> 1, f & 1 != 0);
        for _ in 0..w {
            let v = input.get(i_in as usize).copied().unwrap_or(0);
            let [ib, ig, ir, ia] = v.to_le_bytes();
            let (b, g, r) = chroma(kind, ib, ig, ir);
            let u = prev[x];
            let pb = p.to_le_bytes();
            let ub = u.to_le_bytes();
            let upb = up.to_le_bytes();
            let d = [b, g, r, ia];
            let mut o = [0u8; 4];
            for k in 0..4 {
                let pred = if avg {
                    ((pb[k] as u16 + ub[k] as u16 + 1) >> 1) as u8
                } else {
                    med(pb[k], ub[k], upb[k])
                };
                o[k] = pred.wrapping_add(d[k]);
            }
            p = u32::from_le_bytes(o);
            up = u;
            cur[x] = p;
            x += 1;
            i_in += step;
        }
        if step == 1 {
            i_in += skip as isize - ww;
        } else {
            i_in += skip as isize + 1;
        }
        if i & 1 != 0 {
            i_in -= oddskip * ww;
        }
    }
}

/// The colour transforms of TLG6 (blue, green, red from the coded values).
fn chroma(kind: u8, ib: u8, ig: u8, ir: u8) -> (u8, u8, u8) {
    let a = |x: u8, y: u8| x.wrapping_add(y);
    match kind {
        0 => (ib, ig, ir),
        1 => (a(ib, ig), ig, a(ir, ig)),
        2 => (ib, a(ig, ib), a(a(ir, ib), ig)),
        3 => (a(a(ib, ir), ig), a(ig, ir), ir),
        4 => (a(ib, ir), a(a(ig, ib), ir), a(a(a(ir, ib), ir), ig)),
        5 => (a(ib, ir), a(a(ig, ib), ir), ir),
        6 => (a(ib, ig), ig, ir),
        7 => (ib, a(ig, ib), ir),
        8 => (ib, ig, a(ir, ig)),
        9 => (a(a(a(ib, ig), ir), ib), a(a(ig, ir), ib), a(ir, ib)),
        10 => (a(ib, ir), a(ig, ir), ir),
        11 => (ib, a(ig, ib), a(ir, ib)),
        12 => (ib, a(a(ig, ir), ib), a(ir, ib)),
        13 => (a(ib, ig), a(a(a(ig, ir), ib), ig), a(a(ir, ib), ig)),
        14 => (a(a(ib, ig), ir), a(ig, ir), a(a(a(ir, ib), ig), ir)),
        _ => (ib, a(ig, ib << 1), a(ir, ib << 1)),
    }
}

impl Tlg {
    /// The place of a part (tag 2: u16 x, u16 y) on the picture it belongs to.
    pub fn offset(&self) -> Option<(i32, i32)> {
        let v = self.tags.get(&b"\x02"[..])?;
        if v.len() < 4 {
            return None;
        }
        Some((
            u16::from_le_bytes([v[0], v[1]]) as i32,
            u16::from_le_bytes([v[2], v[3]]) as i32,
        ))
    }
}
