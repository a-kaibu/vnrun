//! PB3B pictures, after GARbro's ImagePB3.cs (MIT, (C) morkt). Types 1 (per-channel LZSS in
//! 16x16 blocks), 2 and 3 (JBP1, a JPEG-like codec, with RLE alpha), 5 (LZSS of deltas) and 6
//! (8x8 blocks drawn over a base picture named in the file).
//!
//! The header (after [`crate::cpz::decrypt_pb3`]): "PB3B" input_size:u32 ... sub_type:u32 at
//! 0x18, type:u16 width:u16 height:u16 bpp:u16 at 0x1C.

use nexas::gfx::Image;

fn u16_(b: &[u8], at: usize) -> usize {
    b.get(at..at + 2)
        .map_or(0, |s| u16::from_le_bytes(s.try_into().unwrap()) as usize)
}

fn u32_(b: &[u8], at: usize) -> usize {
    b.get(at..at + 4)
        .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()) as usize)
}

pub struct Header {
    pub kind: usize,
    pub w: usize,
    pub h: usize,
    pub bpp: usize,
}

pub fn header(b: &[u8]) -> Option<Header> {
    if !b.starts_with(b"PB3B") || b.len() < 0x34 {
        return None;
    }
    Some(Header {
        kind: u16_(b, 0x1c),
        w: u16_(b, 0x1e),
        h: u16_(b, 0x20),
        bpp: u16_(b, 0x22),
    })
}

/// The name (without ".pb3") of a type 6 picture's base.
pub fn base_name(b: &[u8]) -> Option<String> {
    const KEY: [u8; 16] = [
        0xA6, 0x75, 0xF3, 0x9C, 0xC5, 0x69, 0x78, 0xA3, 0x3E, 0xA5, 0x4F, 0x79, 0x59, 0xFE, 0x3A,
        0xC7,
    ];
    let mut name = Vec::new();
    for i in 0..0x20 {
        let c = b.get(0x34 + i)? ^ KEY[i & 15];
        if c == 0 {
            break;
        }
        name.push(c);
    }
    Some(nexas::sjis::decode(&name))
}

struct Lz {
    frame: [u8; 0x800],
}

impl Lz {
    fn new() -> Lz {
        Lz { frame: [0; 0x800] }
    }
    /// Flags MSB first at `bits`, data at `data`.
    fn unpack(&mut self, input: &[u8], mut bits: usize, mut data: usize, out: &mut [u8]) {
        let mut mask = 0x80u8;
        let mut fo = 0x7DEusize;
        let mut dst = 0;
        let at = |i: usize| input.get(i).copied().unwrap_or(0);
        while dst < out.len() {
            if mask == 0 {
                mask = 0x80;
                bits += 1;
            }
            if at(bits) & mask != 0 {
                let v = at(data) as usize | (at(data + 1) as usize) << 8;
                data += 2;
                let (count, off) = ((v & 0x1f) + 3, v >> 5);
                for i in 0..count {
                    let b = self.frame[(i + off) & 0x7ff];
                    if dst < out.len() {
                        out[dst] = b;
                    }
                    dst += 1;
                    self.frame[fo] = b;
                    fo = (fo + 1) & 0x7ff;
                }
            } else {
                let b = at(data);
                data += 1;
                out[dst] = b;
                dst += 1;
                self.frame[fo] = b;
                fo = (fo + 1) & 0x7ff;
            }
            mask >>= 1;
        }
    }
}

/// Decodes to RGBA; `base` loads a type 6 picture's base (by name, without ".pb3").
pub fn decode(b: &[u8], base: &mut dyn FnMut(&str) -> Option<Image>) -> Result<Image, String> {
    let h = header(b).ok_or("not PB3B")?;
    if h.w == 0 || h.h == 0 || h.w > 8192 || h.h > 8192 {
        return Err("bad PB3 size".into());
    }
    let stride = h.w * 4;
    let bgra = match h.kind {
        1 => v1(b, &h),
        5 => v5(b, &h),
        2 | 3 => jbp_alpha(b, &h, 0x34, u32_(b, 0x2c))?,
        6 | 8 => {
            let name = base_name(b).ok_or("PB3 base name")?;
            let img = base(&name).ok_or_else(|| format!("PB3 base {name} not found"))?;
            if img.w as usize != h.w || img.h as usize != h.h {
                return Err(format!("PB3 base {name} differs in size"));
            }
            // back to BGRA, blend, then to RGBA below
            let mut out = img.px.clone();
            for p in out.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            v6(b, &h, &mut out);
            out
        }
        k => return Err(format!("PB3 type {k} is not supported")),
    };
    let mut img = Image::new(h.w as u32, h.h as u32);
    for (d, s) in img.px.chunks_exact_mut(4).zip(bgra.chunks_exact(4)) {
        d.copy_from_slice(&[s[2], s[1], s[0], if h.bpp == 32 { s[3] } else { 255 }]);
    }
    let _ = stride;
    Ok(img)
}

fn v1(input: &[u8], h: &Header) -> Vec<u8> {
    let (w, ht) = (h.w, h.h);
    let stride = w * 4;
    let channels = h.bpp / 8;
    let mut out = vec![0u8; stride * ht];
    let xb = w.div_ceil(16);
    let yb = ht.div_ceil(16);
    let mut plane = vec![0u8; w * ht];
    let (d1, d2) = (u32_(input, 0x2c), u32_(input, 0x30));
    let at = |i: usize| input.get(i).copied().unwrap_or(0);
    for ch in 0..channels {
        let mut off = 4 * channels;
        for i in 0..ch {
            off += u32_(input, d1 + 4 * i);
        }
        let v21 = d1 + off;
        let bit_src = v21 + 12 + u32_(input, v21) + u32_(input, v21 + 4);
        let size = u32_(input, v21 + 8).min(plane.len());
        let mut off = 4 * channels;
        for i in 0..ch {
            off += u32_(input, d2 + 4 * i);
        }
        Lz::new().unpack(input, bit_src, d2 + off, &mut plane[..size]);
        let mut ps = 0usize;
        let mut bits = v21 + 12;
        let mut mask = 0x80u8;
        let mut data = bits + u32_(input, v21);
        for y in 0..yb {
            let row = 16 * y;
            for x in 0..xb {
                let bw = 16.min(w - 16 * x);
                let bh = 16.min(ht - row);
                if mask == 0 {
                    bits += 1;
                    mask = 0x80;
                }
                let dst0 = stride * row + x * 64 + ch;
                if at(bits) & mask != 0 {
                    let v = at(data);
                    data += 1;
                    for j in 0..bh {
                        for i in 0..bw {
                            out[dst0 + j * stride + i * 4] = v;
                        }
                    }
                } else {
                    for j in 0..bh {
                        for i in 0..bw {
                            out[dst0 + j * stride + i * 4] = plane.get(ps).copied().unwrap_or(0);
                            ps += 1;
                        }
                    }
                }
                mask >>= 1;
            }
        }
    }
    out
}

fn v5(input: &[u8], h: &Header) -> Vec<u8> {
    let mut out = vec![0u8; h.w * 4 * h.h];
    let at = |i: usize| input.get(i).copied().unwrap_or(0);
    for ch in 0..4 {
        let mut bits = 0x54 + u32_(input, 8 * ch + 0x34);
        let mut data = 0x54 + u32_(input, 8 * ch + 0x38);
        let mut frame = [0u8; 0x800];
        let mut fo = 0x7DEusize;
        let mut acc = 0u8;
        let mut mask = 0x80u8;
        let mut dst = ch;
        while dst < out.len() {
            if mask == 0 {
                bits += 1;
                mask = 0x80;
            }
            if at(bits) & mask != 0 {
                let v = at(data) as usize | (at(data + 1) as usize) << 8;
                data += 2;
                for k in 0..(v & 0x1f) + 3 {
                    let b = frame[(k + (v >> 5)) & 0x7ff];
                    frame[fo] = b;
                    fo = (fo + 1) & 0x7ff;
                    acc = acc.wrapping_add(b);
                    if dst < out.len() {
                        out[dst] = acc;
                    }
                    dst += 4;
                }
            } else {
                let b = at(data);
                data += 1;
                frame[fo] = b;
                fo = (fo + 1) & 0x7ff;
                acc = acc.wrapping_add(b);
                out[dst] = acc;
                dst += 4;
            }
            mask >>= 1;
        }
    }
    out
}

fn v6(input: &[u8], h: &Header, out: &mut [u8]) {
    let (w, ht) = (h.w, h.h);
    let stride = w * 4;
    let bits = 0x20 + u32_(input, 0xc);
    let data = bits + u32_(input, 0x2c);
    let size = u32_(input, 0x18).min(1 << 26);
    let mut ov = vec![0u8; size];
    Lz::new().unpack(input, bits, data, &mut ov);
    let mut bits = 8usize;
    let mut data = 8 + u32_(&ov, 0);
    let mut mask = 0x80u8;
    let xb = w.div_ceil(8);
    let yb = ht.div_ceil(8);
    for by in 0..yb {
        for x in 0..xb {
            if mask == 0 {
                bits += 1;
                mask = 0x80;
            }
            if ov.get(bits).copied().unwrap_or(0xff) & mask == 0 {
                let xc = 8.min(w - 8 * x);
                let yc = 8.min(ht - 8 * by);
                let mut dst = by * 8 * stride + x * 32;
                for _ in 0..yc {
                    let n = 4 * xc;
                    if data + n <= ov.len() && dst + n <= out.len() {
                        out[dst..dst + n].copy_from_slice(&ov[data..data + n]);
                    }
                    data += n;
                    dst += stride;
                }
            }
            mask >>= 1;
        }
    }
}

fn jbp_alpha(input: &[u8], h: &Header, pos: usize, alpha: usize) -> Result<Vec<u8>, String> {
    let (px, jstride) = jbp(input, pos)?;
    let stride = h.w * 4;
    let mut out = vec![0u8; stride * h.h];
    for y in 0..h.h {
        let s = y * jstride;
        if s + stride <= px.len() {
            out[y * stride..(y + 1) * stride].copy_from_slice(&px[s..s + stride]);
        }
    }
    if h.bpp == 32 && alpha > 0 {
        let mut a = alpha;
        let mut dst = 3;
        while dst < out.len() && a < input.len() {
            let v = input[a];
            a += 1;
            if v != 0 && v != 0xff {
                out[dst] = v;
                dst += 4;
            } else {
                let n = input.get(a).copied().unwrap_or(0);
                a += 1;
                for _ in 0..n {
                    if dst < out.len() {
                        out[dst] = v;
                    }
                    dst += 4;
                }
            }
        }
    }
    Ok(out)
}

struct Bits<'a> {
    b: &'a [u8],
    pos: usize,
    end: usize,
    acc: u32,
    n: u32,
}

impl Bits<'_> {
    fn get(&mut self, count: u32) -> Result<u32, String> {
        while self.n < count {
            if self.pos >= self.end {
                return Err("JBP bits run out".into());
            }
            self.acc = (self.acc << 8) | self.b[self.pos].reverse_bits() as u32;
            self.pos += 1;
            self.n += 8;
        }
        self.n -= count;
        Ok((self.acc >> self.n) & ((1u32 << count) - 1))
    }
}

struct Tree {
    base: [u8; 16],
    nodes: Vec<usize>,
    root: usize,
}

impl Tree {
    fn new(base: [u8; 16], mut freq: [u32; 32]) -> Tree {
        const MAX: u32 = 2_100_000_000;
        let mut nodes = vec![0usize; 0x400];
        let mut depth = 16;
        loop {
            let pick = |freq: &[u32; 32], depth: usize, skip: Option<usize>| {
                let (mut best, mut min) = (None, MAX - 1);
                for (i, &f) in freq.iter().enumerate().take(depth) {
                    if Some(i) != skip && f < min {
                        min = f;
                        best = Some(i);
                    }
                }
                best
            };
            let Some(l) = pick(&freq, depth, None) else {
                break;
            };
            let Some(r) = pick(&freq, depth, Some(l)) else {
                break;
            };
            if depth >= 32 {
                break;
            }
            nodes[depth] = l;
            nodes[depth + 0x200] = r;
            freq[depth] = freq[l].saturating_add(freq[r]);
            depth += 1;
            freq[l] = MAX;
            freq[r] = MAX;
        }
        Tree {
            base,
            nodes,
            root: depth - 1,
        }
    }
    fn read(&self, bits: &mut Bits) -> Result<usize, String> {
        let mut v = self.root;
        while v >= 16 {
            v = self.nodes[v + ((bits.get(1)? as usize) << 9)];
        }
        Ok(v)
    }
}

const ZIGZAG: [usize; 64] = [
    1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63, 0,
];

fn signed(v: u32, n: u32) -> i32 {
    if n == 0 {
        return 0;
    }
    if v < (1 << (n - 1)) {
        v.wrapping_sub((1 << n) - 1) as i32
    } else {
        v as i32
    }
}

/// JBP1 at `pos`: BGRA pixels and their stride (the width rounded up to the block size).
fn jbp(input: &[u8], pos: usize) -> Result<(Vec<u8>, usize), String> {
    let data = u32_(input, pos + 4) + pos;
    let format = u32_(input, pos + 8) as u32;
    let (w, h) = (u16_(input, pos + 0x10), u16_(input, pos + 0x12));
    let (dc_bits, ac_bits) = (u32_(input, pos + 0x1c), u32_(input, pos + 0x20));
    let (aw, ah) = match (format >> 28) & 3 {
        0 => ((w + 7) & !7, (h + 7) & !7),
        1 => ((w + 15) & !15, (h + 15) & !15),
        2 => ((w + 31) & !31, (h + 15) & !15),
        _ => return Err("JBP format".into()),
    };
    let (bx, by) = (aw >> 4, ah >> 4);
    let stride = aw * 4;
    let mut out = vec![0u8; stride * ah];
    let tree_pos = data + 0x80;
    let mut base = [0u8; 16];
    for (i, v) in base.iter_mut().enumerate() {
        *v = input
            .get(tree_pos + i)
            .copied()
            .unwrap_or(0)
            .wrapping_add(1);
    }
    let freq = |at: usize| {
        let mut f = [0u32; 32];
        for (i, v) in f.iter_mut().enumerate() {
            *v = u32_(input, at + i * 4) as u32;
        }
        f
    };
    let tree_dc = Tree::new(base, freq(data));
    let tree_ac = Tree::new(base, freq(data + 0x40));
    let quant_pos = tree_pos + 0x10;
    let (mut qy, mut qc) = ([0i32; 64], [0i32; 64]);
    if format & 0x8000000 != 0 {
        for i in 0..64 {
            qy[i] = input.get(quant_pos + i).copied().unwrap_or(0) as i32;
            qc[i] = input.get(quant_pos + 0x40 + i).copied().unwrap_or(0) as i32;
        }
    }
    let bits_at = quant_pos + 0x80;
    let mut dc = Bits {
        b: input,
        pos: bits_at,
        end: (bits_at + dc_bits).min(input.len()),
        acc: 0,
        n: 0,
    };
    let mut ac = Bits {
        b: input,
        pos: bits_at + dc_bits,
        end: (bits_at + dc_bits + ac_bits).min(input.len()),
        acc: 0,
        n: 0,
    };
    let total = bx * by;
    let mut blocks = vec![[0i32; 6]; total];
    let mut prev = 0i32;
    for b in blocks.iter_mut() {
        for v in b.iter_mut() {
            let n = tree_dc.read(&mut dc).map_err(|e| format!("{e} (dc)"))? as u32;
            let x = signed(dc.get(n)?, n);
            prev = prev.wrapping_add(x);
            *v = prev as i16 as i32;
        }
    }
    let mut t = [[0i32; 64]; 6];
    for y in 0..by {
        for x in 0..bx {
            for (n, tb) in t.iter_mut().enumerate() {
                *tb = [0; 64];
                tb[0] = blocks[y * bx + x][n];
                let mut i = 0;
                while i < 63 {
                    let n = tree_ac
                        .read(&mut ac)
                        .map_err(|e| format!("{e} (ac, block {x} {y} of {bx} {by})"))?
                        as u32;
                    if n == 15 {
                        break;
                    }
                    if n == 0 {
                        let mut node = 0;
                        while ac.get(1)? != 0 {
                            node += 1;
                        }
                        i += tree_ac.base.get(node).copied().unwrap_or(1) as usize;
                    } else {
                        tb[ZIGZAG[i]] = signed(ac.get(n)?, n) as i16 as i32;
                        i += 1;
                    }
                }
            }
            for (n, tb) in t.iter_mut().enumerate() {
                idct(tb, if n < 4 { &qy } else { &qc });
            }
            let d1 = y * stride * 16 + x * 64;
            let d2 = d1 + stride * 9;
            ycc(&mut out, stride, d1, d1 + stride, &t[0], &t[4], &t[5], 0);
            ycc(
                &mut out,
                stride,
                d1 + 32,
                d1 + stride + 32,
                &t[1],
                &t[4],
                &t[5],
                4,
            );
            ycc(&mut out, stride, d2 - stride, d2, &t[2], &t[4], &t[5], 32);
            ycc(
                &mut out,
                stride,
                d2 - stride + 32,
                d2 + 32,
                &t[3],
                &t[4],
                &t[5],
                36,
            );
        }
    }
    Ok((out, stride))
}

fn s16(v: i32) -> i32 {
    v as i16 as i32
}

fn idct(t: &mut [i32; 64], q: &[i32; 64]) {
    for i in 0..8 {
        let p = i;
        if (1..8).all(|k| t[p + 8 * k] == 0) {
            let v = s16(t[p] * q[p]);
            for k in 0..8 {
                t[p + 8 * k] = v;
            }
            continue;
        }
        let mut c = q[p + 0x10] * t[p + 0x10];
        let mut d = q[p + 0x30] * t[p + 0x30];
        let mut x = ((c + d) * 35467) >> 16;
        c = ((c * 50159) >> 16) + x;
        d = ((d * -121094) >> 16) + x;
        let mut a = t[p] * q[p];
        let mut b = t[p + 0x20] * q[p + 0x20];
        let w = a + b + c;
        x = a + b - c;
        let y = a - b + d;
        let z = a - b - d;
        c = t[p + 0x38] * q[p + 0x38];
        d = t[p + 0x28] * q[p + 0x28];
        a = t[p + 0x18] * q[p + 0x18];
        b = t[p + 0x08] * q[p + 0x08];
        let n = ((a + b + c + d) * 77062) >> 16;
        let u = n + ((c * 19571) >> 16) + (((c + a) * -128553) >> 16) + (((c + b) * -58980) >> 16);
        let v = n + ((d * 134553) >> 16) + (((d + b) * -25570) >> 16) + (((d + a) * -167963) >> 16);
        let tt = n + ((b * 98390) >> 16) + (((d + b) * -25570) >> 16) + (((c + b) * -58980) >> 16);
        let s =
            n + ((a * 201373) >> 16) + (((c + a) * -128553) >> 16) + (((d + a) * -167963) >> 16);
        t[p] = s16(w + tt);
        t[p + 0x38] = s16(w - tt);
        t[p + 0x08] = s16(y + s);
        t[p + 0x30] = s16(y - s);
        t[p + 0x10] = s16(z + v);
        t[p + 0x28] = s16(z - v);
        t[p + 0x18] = s16(x + u);
        t[p + 0x20] = s16(x - u);
    }
    for i in 0..8 {
        let p = i * 8;
        let a = t[p];
        let mut c = t[p + 2];
        let b = t[p + 4];
        let mut d = t[p + 6];
        let mut x = ((c + d) * 35467) >> 16;
        c = ((c * 50159) >> 16) + x;
        d = ((d * -121094) >> 16) + x;
        let w = a + b + c;
        x = a + b - c;
        let y = a - b + d;
        let z = a - b - d;
        let d = t[p + 5];
        let b = t[p + 1];
        let c = t[p + 7];
        let a = t[p + 3];
        let n = ((a + b + c + d) * 77062) >> 16;
        let s =
            n + ((a * 201373) >> 16) + (((a + c) * -128553) >> 16) + (((a + d) * -167963) >> 16);
        let tt = n + ((b * 98390) >> 16) + (((b + c) * -58980) >> 16) + (((b + d) * -25570) >> 16);
        let u = n + ((c * 19571) >> 16) + (((b + c) * -58980) >> 16) + (((a + c) * -128553) >> 16);
        let v = n + ((d * 134553) >> 16) + (((b + d) * -25570) >> 16) + (((a + d) * -167963) >> 16);
        t[p] = s16((w + tt) >> 3);
        t[p + 7] = s16((w - tt) >> 3);
        t[p + 1] = s16((y + s) >> 3);
        t[p + 6] = s16((y - s) >> 3);
        t[p + 2] = s16((z + v) >> 3);
        t[p + 5] = s16((z - v) >> 3);
        t[p + 3] = s16((x + u) >> 3);
        t[p + 4] = s16((x - u) >> 3);
    }
}

fn clamp(c: i32) -> u8 {
    // GARbro's table: 0..0x100 -> 0, 0x100..0x200 -> c - 0x100, above -> 0xFF
    (c - 0x100).clamp(0, 255) as u8
}

#[allow(clippy::too_many_arguments)]
fn ycc(
    out: &mut [u8],
    stride: usize,
    mut dc: usize,
    mut ac: usize,
    y: &[i32; 64],
    cb: &[i32; 64],
    cr: &[i32; 64],
    mut cs: usize,
) {
    let mut ys = 0;
    let mut put = |i: usize, v: u8| {
        if let Some(p) = out.get_mut(i) {
            *p = v;
        }
    };
    for _ in 0..4 {
        for _ in 0..4 {
            let (b_, r_) = (cb[cs], cr[cs]);
            let r = (r_ * 0x166F0) >> 16;
            let g = ((b_ * 0x5810) >> 16) + ((r_ * 0xB6C0) >> 16);
            let b = (b_ * 0x1C590) >> 16;
            let c0 = y[ys] + 0x180;
            let c1 = y[ys + 1] + 0x180;
            let c8 = y[ys + 8] + 0x180;
            let c9 = y[ys + 9] + 0x180;
            put(dc, clamp(c0 + b));
            put(ac + 1 - stride, clamp(c0 - g));
            put(ac + 2 - stride, clamp(c0 + r));
            put(ac + 4 - stride, clamp(c1 + b));
            put(ac + 5 - stride, clamp(c1 - g));
            put(ac + 6 - stride, clamp(c1 + r));
            put(ac, clamp(c8 + b));
            put(ac + 1, clamp(c8 - g));
            put(ac + 2, clamp(c8 + r));
            put(ac + 4, clamp(c9 + b));
            put(ac + 5, clamp(c9 - g));
            put(ac + 6, clamp(c9 + r));
            ys += 2;
            dc += 8;
            ac += 8;
            cs += 1;
        }
        dc += stride * 2 - 32;
        ac += stride * 2 - 32;
        ys += 8;
        cs += 4;
    }
}

/// The first frame of a CMV9 movie (the moving backgrounds of data/imgvideo): the header has
/// the frames' offset at 4, the width and height at 0x1C and 0x20, and each frame is a JBP
/// picture ("JBPD").
pub fn cmv_first_frame(b: &[u8]) -> Result<Image, String> {
    if !b.starts_with(b"CMV") || b.len() < 0x2c {
        return Err("not a CMV movie".into());
    }
    let (at, w, h) = (u32_(b, 4), u32_(b, 0x1c), u32_(b, 0x20));
    if w == 0 || h == 0 || w > 8192 || h > 8192 || at + 0x24 > b.len() {
        return Err("bad CMV header".into());
    }
    let (px, stride) = jbp(b, at)?;
    let mut img = Image::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let s = y * stride + x * 4;
            let d = (y * w + x) * 4;
            if let Some(p) = px.get(s..s + 3) {
                img.px[d..d + 4].copy_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
    }
    Ok(img)
}
