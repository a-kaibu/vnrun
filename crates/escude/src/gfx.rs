//! Planes (the engine's pictures) and drawing between them. A plane's pixels are in the
//! machine's memory, as a Windows DIB: rows bottom up, 4-byte aligned, BGR(A) for 24 and 32
//! bits, 5-5-5 for 16 bits and one byte (a palette index, grey or alpha) for 8 bits. A 24-bit
//! plane can have an 8-bit plane attached as its alpha.

use std::collections::HashMap;

use crate::vm::Vm;

#[derive(Debug, Clone)]
pub struct Plane {
    pub w: i32,
    pub h: i32,
    pub depth: u32,
    /// the pixels (the bottom row first)
    pub addr: u32,
    pub stride: u32,
    /// the plane that holds this one's alpha
    pub alpha: Option<i32>,
    /// 256 colours (0xRRGGBB) for an 8-bit plane
    pub pal: Option<Vec<u32>>,
    pub clip: Option<[i32; 4]>,
}

impl Plane {
    /// The address of row y (0 at the top).
    pub fn row(&self, y: i32) -> usize {
        (self.addr + (self.h - 1 - y) as u32 * self.stride) as usize
    }

    pub fn bpp(&self) -> usize {
        (self.depth as usize).div_ceil(8).max(1)
    }

    /// The part of the plane drawing may change.
    pub fn bounds(&self) -> [i32; 4] {
        let b = [0, 0, self.w, self.h];
        match self.clip {
            Some(c) => intersect(b, c),
            None => b,
        }
    }
}

/// [left, top, right, bottom]
pub type Rect = [i32; 4];

pub fn intersect(a: Rect, b: Rect) -> Rect {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

pub fn empty(r: Rect) -> bool {
    r[2] <= r[0] || r[3] <= r[1]
}

/// How pixels are laid down (BLTMODE).
#[derive(Debug, Clone, Copy, Default)]
pub struct Mode {
    pub mode: u8,
    pub mask: u8,
    pub level: u8,
    pub range: u8,
    pub trans: u32,
    pub color: u32,
}

impl Mode {
    pub const NORMAL: Mode = Mode {
        mode: 0,
        mask: 7,
        level: 255,
        range: 0,
        trans: 0,
        color: 0,
    };
}

/// A pixel: blue, green, red, alpha.
pub type Px = [u8; 4];

fn bgr(c: u32) -> [u8; 3] {
    [c as u8, (c >> 8) as u8, (c >> 16) as u8]
}

fn mix(d: u8, s: u8, a: u32) -> u8 {
    ((d as u32 * (255 - a) + s as u32 * a + 127) / 255) as u8
}

#[derive(Default)]
pub struct Planes {
    pub map: HashMap<i32, Plane>,
}

impl Planes {
    pub fn get(&self, n: i32) -> Option<&Plane> {
        self.map.get(&n)
    }

    /// Makes plane n anew.
    pub fn alloc(&mut self, vm: &mut Vm, n: i32, w: i32, h: i32, depth: u32) {
        self.free(vm, n);
        let (w, h) = (w.max(1), h.max(1));
        let depth = match depth {
            1..=8 => 8,
            9..=16 => 16,
            17..=24 => 24,
            _ => 32,
        };
        let stride = (w as u32 * depth / 8).div_ceil(4) * 4;
        let addr = vm.malloc(stride * h as u32);
        let pal = (depth == 8).then(|| (0..256).map(|i| i * 0x010101).collect());
        self.map.insert(
            n,
            Plane {
                w,
                h,
                depth,
                addr,
                stride,
                alpha: None,
                pal,
                clip: None,
            },
        );
    }

    pub fn free(&mut self, vm: &mut Vm, n: i32) -> bool {
        match self.map.remove(&n) {
            Some(p) => {
                vm.free(p.addr);
                true
            }
            None => false,
        }
    }

    /// The pixels of a rectangle (inside the plane), with alpha from a 32-bit plane's
    /// fourth byte or an attached plane (255 otherwise).
    pub fn read(&self, vm: &Vm, n: i32, r: Rect) -> Vec<Px> {
        let Some(p) = self.map.get(&n) else {
            return vec![[0, 0, 0, 255]; ((r[2] - r[0]) * (r[3] - r[1])).max(0) as usize];
        };
        let mut out = Vec::with_capacity(((r[2] - r[0]) * (r[3] - r[1])).max(0) as usize);
        let alpha = p
            .alpha
            .and_then(|a| self.map.get(&a))
            .filter(|a| a.depth == 8);
        for y in r[1]..r[3] {
            let row = p.row(y);
            let arow = alpha.map(|a| if y < a.h { a.row(y) } else { usize::MAX });
            for x in r[0]..r[2] {
                let mut px = match p.depth {
                    32 => {
                        let i = row + x as usize * 4;
                        [vm.mem[i], vm.mem[i + 1], vm.mem[i + 2], vm.mem[i + 3]]
                    }
                    24 => {
                        let i = row + x as usize * 3;
                        [vm.mem[i], vm.mem[i + 1], vm.mem[i + 2], 255]
                    }
                    16 => {
                        let i = row + x as usize * 2;
                        let v = u16::from_le_bytes([vm.mem[i], vm.mem[i + 1]]);
                        let c = |s: u16| (((v >> s) & 31) as u32 * 255 / 31) as u8;
                        [c(0), c(5), c(10), 255]
                    }
                    _ => {
                        let v = vm.mem[row + x as usize];
                        match &p.pal {
                            Some(pal) => {
                                let [b, g, r] = bgr(pal[v as usize]);
                                [b, g, r, 255]
                            }
                            None => [v, v, v, 255],
                        }
                    }
                };
                if let (Some(a), Some(ar)) = (alpha, arow)
                    && ar != usize::MAX
                    && x < a.w
                {
                    px[3] = vm.mem[ar + x as usize];
                }
                out.push(px);
            }
        }
        out
    }

    /// The 8-bit values of a rectangle of an 8-bit plane (or the alpha of another).
    fn read_value(&self, vm: &Vm, n: i32, r: Rect) -> Vec<u8> {
        match self.map.get(&n) {
            Some(p) if p.depth == 8 => {
                let mut out = Vec::new();
                for y in r[1]..r[3] {
                    let row = p.row(y);
                    out.extend_from_slice(&vm.mem[row + r[0] as usize..row + r[2] as usize]);
                }
                out
            }
            _ => self.read(vm, n, r).iter().map(|p| p[3]).collect(),
        }
    }

    /// Writes pixels into a rectangle (inside the plane). `alpha`: also write the alpha of a
    /// 32-bit plane.
    fn write(&self, vm: &mut Vm, n: i32, r: Rect, px: &[Px], alpha: bool) {
        let Some(p) = self.map.get(&n) else { return };
        let mut k = 0;
        for y in r[1]..r[3] {
            let row = p.row(y);
            for x in r[0]..r[2] {
                let c = px[k];
                k += 1;
                match p.depth {
                    32 => {
                        let i = row + x as usize * 4;
                        vm.mem[i..i + 3].copy_from_slice(&c[..3]);
                        if alpha {
                            vm.mem[i + 3] = c[3];
                        }
                    }
                    24 => {
                        let i = row + x as usize * 3;
                        vm.mem[i..i + 3].copy_from_slice(&c[..3]);
                    }
                    16 => {
                        let i = row + x as usize * 2;
                        let v = (c[0] as u16 >> 3)
                            | ((c[1] as u16 >> 3) << 5)
                            | ((c[2] as u16 >> 3) << 10);
                        vm.mem[i..i + 2].copy_from_slice(&v.to_le_bytes());
                    }
                    _ => vm.mem[row + x as usize] = c[2],
                }
            }
        }
    }

    /// Fills a rectangle with a colour (0xAARRGGBB; the value itself for 8 bits).
    pub fn fill(&self, vm: &mut Vm, n: i32, r: Option<Rect>, color: u32) {
        let Some(p) = self.map.get(&n) else { return };
        let r = intersect(r.unwrap_or([0, 0, p.w, p.h]), p.bounds());
        if empty(r) {
            return;
        }
        let px: Px = if p.depth == 8 {
            let v = color as u8;
            [v, v, v, v]
        } else {
            let [b, g, rr] = bgr(color);
            [b, g, rr, (color >> 24) as u8]
        };
        let n_px = ((r[2] - r[0]) * (r[3] - r[1])) as usize;
        self.write(vm, n, r, &vec![px; n_px], true);
    }

    /// Copies a rectangle of `src` to (x, y) of `dst`.
    pub fn copy(
        &self,
        vm: &mut Vm,
        dst: i32,
        x: i32,
        y: i32,
        src: i32,
        sr: Option<Rect>,
        m: &Mode,
    ) {
        let (Some(d), Some(s)) = (self.map.get(&dst), self.map.get(&src)) else {
            return;
        };
        let mut sr = intersect(sr.unwrap_or([0, 0, s.w, s.h]), [0, 0, s.w, s.h]);
        // where it lands, clipped
        let mut dr = [x, y, x + sr[2] - sr[0], y + sr[3] - sr[1]];
        let c = intersect(dr, d.bounds());
        sr[0] += c[0] - dr[0];
        sr[1] += c[1] - dr[1];
        sr[2] -= dr[2] - c[2];
        sr[3] -= dr[3] - c[3];
        dr = c;
        if empty(dr) || empty(sr) {
            return;
        }
        let s_px = self.read(vm, src, sr);
        let s_val = if needs_value(m.mode) {
            self.read_value(vm, src, sr)
        } else {
            Vec::new()
        };
        let s_depth = s.depth;
        self.compose(vm, dst, dr, &s_px, &s_val, s_depth, m);
    }

    /// Lays source pixels (the size of `dr`) over `dst` in `dr`.
    pub fn compose(
        &self,
        vm: &mut Vm,
        dst: i32,
        dr: Rect,
        s_px: &[Px],
        s_val: &[u8],
        s_depth: u32,
        m: &Mode,
    ) {
        let Some(d) = self.map.get(&dst) else { return };
        let d32 = d.depth == 32;
        if m.mode == 0 {
            self.write(vm, dst, dr, s_px, true);
            return;
        }
        let mut px = self.read(vm, dst, dr);
        let level = m.level as u32;
        let col = bgr(m.color);
        let trans = bgr(m.trans);
        for (i, dp) in px.iter_mut().enumerate() {
            let s = s_px[i];
            let v = s_val.get(i).copied().unwrap_or(s[3]);
            let d = *dp;
            let out: Option<Px> = match m.mode {
                1 => (s[..3] != trans).then_some([s[0], s[1], s[2], 255]),
                2 => (d[..3] == trans).then_some([s[0], s[1], s[2], 255]),
                3 | 4 => {
                    if m.mode == 4 && s[..3] == trans {
                        None
                    } else {
                        Some([
                            mix(d[0], s[0], level),
                            mix(d[1], s[1], level),
                            mix(d[2], s[2], level),
                            d[3],
                        ])
                    }
                }
                5 | 6 => {
                    if m.mode == 6 && s[..3] == trans {
                        None
                    } else {
                        Some([
                            mix(s[0], col[0], level),
                            mix(s[1], col[1], level),
                            mix(s[2], col[2], level),
                            255,
                        ])
                    }
                }
                // source alpha
                7 | 9 | 10 => {
                    let a = s[3] as u32 * level / 255;
                    let s = if m.mode == 10 {
                        [
                            (s[0] as u32 * col[0] as u32 / 255) as u8,
                            (s[1] as u32 * col[1] as u32 / 255) as u8,
                            (s[2] as u32 * col[2] as u32 / 255) as u8,
                        ]
                    } else {
                        [s[0], s[1], s[2]]
                    };
                    if a == 0 {
                        None
                    } else {
                        let da = if d32 {
                            (a + d[3] as u32 * (255 - a) / 255).min(255) as u8
                        } else {
                            d[3]
                        };
                        if d32 && d[3] == 0 {
                            Some([s[0], s[1], s[2], a as u8])
                        } else {
                            Some([
                                mix(d[0], s[0], a),
                                mix(d[1], s[1], a),
                                mix(d[2], s[2], a),
                                da,
                            ])
                        }
                    }
                }
                // destination alpha
                8 => {
                    let a = (255 - d[3] as u32) * level / 255;
                    Some([
                        mix(d[0], s[0], a),
                        mix(d[1], s[1], a),
                        mix(d[2], s[2], a),
                        d[3],
                    ])
                }
                // pattern wipes: the source is the pattern, the colour comes from ...
                11 | 12 => {
                    let t = level as i32 * (256 + m.range as i32) / 255 - m.range as i32;
                    let a = if m.range == 0 {
                        if (v as i32) < t { 255 } else { 0 }
                    } else {
                        ((t - v as i32) * 255 / m.range as i32).clamp(0, 255)
                    };
                    Some([d[0], d[1], d[2], a as u8])
                }
                13 => Some(op3(s, d, level, |s, d| (d + s).min(255))),
                14 => Some(op3(s, d, level, |s, d| d.saturating_sub(s))),
                15 => Some(op3(s, d, level, |s, d| d * s / 255)),
                16 => Some(op3(s, d, level, |s, d| d & s)),
                17 => Some(op3(s, d, level, |s, d| d | s)),
                18 => Some(op3(s, d, level, |s, d| d ^ s)),
                19 => Some(op3(s, d, level, |s, d| 255 - (255 - d) * (255 - s) / 255)),
                20 => Some(op3(s, d, level, |s, d| d.max(s))),
                21 => Some(op3(s, d, level, |s, d| d.min(s))),
                22 => {
                    let mut o = d;
                    for (c, bit) in [(0, 1), (1, 2), (2, 4)] {
                        if m.mask & bit != 0 {
                            o[c] = s[c];
                        }
                    }
                    Some(o)
                }
                // the source's value into the alpha
                23 => Some([d[0], d[1], d[2], if s_depth == 8 { v } else { s[3] }]),
                // a colour through the source's value as alpha
                24 => {
                    let a = (if s_depth == 8 { v } else { s[3] }) as u32 * level / 255;
                    if a == 0 {
                        None
                    } else {
                        let da = if d32 {
                            (a + d[3] as u32 * (255 - a) / 255).min(255) as u8
                        } else {
                            d[3]
                        };
                        Some([
                            mix(d[0], col[0], a),
                            mix(d[1], col[1], a),
                            mix(d[2], col[2], a),
                            da,
                        ])
                    }
                }
                // monochrome
                25 => {
                    let g = ((s[0] as u32 * 29 + s[1] as u32 * 150 + s[2] as u32 * 77) >> 8) as u8;
                    let a = s[3] as u32 * level / 255;
                    Some([mix(d[0], g, a), mix(d[1], g, a), mix(d[2], g, a), d[3]])
                }
                26 | 27 => Some(op3(s, d, level, |s, d| {
                    if d < 128 {
                        2 * s * d / 255
                    } else {
                        255 - 2 * (255 - s) * (255 - d) / 255
                    }
                })),
                28..=31 => Some(op3(s, d, level, |s, d| {
                    if s < 128 {
                        2 * s * d / 255
                    } else {
                        255 - 2 * (255 - s) * (255 - d) / 255
                    }
                })),
                32 | 33 => Some(op3(s, d, level, |s, d| {
                    if s >= 255 {
                        255
                    } else {
                        (d * 255 / (255 - s)).min(255)
                    }
                })),
                34 | 35 => Some(op3(s, d, level, |s, d| {
                    if s == 0 {
                        0
                    } else {
                        255 - ((255 - d) * 255 / s).min(255)
                    }
                })),
                _ => Some(s),
            };
            if let Some(o) = out {
                *dp = o;
            }
        }
        // the pattern wipes blend the second source in by the computed alpha
        if m.mode == 11 || m.mode == 12 {
            return;
        }
        self.write(vm, dst, dr, &px, d32);
    }

    /// Draws a rectangle of `src` mapped by an affine transform: `inv` takes a destination
    /// pixel centre to source coordinates; `area` is where it may land.
    #[allow(clippy::too_many_arguments)]
    pub fn affine(
        &self,
        vm: &mut Vm,
        dst: i32,
        area: Rect,
        src: i32,
        sr: Rect,
        inv: [f32; 6],
        m: &Mode,
        smooth: bool,
    ) {
        let (Some(d), Some(s)) = (self.map.get(&dst), self.map.get(&src)) else {
            return;
        };
        let dr = intersect(area, d.bounds());
        let sr = intersect(sr, [0, 0, s.w, s.h]);
        if empty(dr) || empty(sr) {
            return;
        }
        let s_px = self.read(vm, src, sr);
        let s_val = if needs_value(m.mode) {
            self.read_value(vm, src, sr)
        } else {
            Vec::new()
        };
        let sw = (sr[2] - sr[0]) as usize;
        let sh = (sr[3] - sr[1]) as usize;
        let n = ((dr[2] - dr[0]) * (dr[3] - dr[1])) as usize;
        // pixels outside the source stay as they are: lay them over with alpha 0
        let mut out = Vec::with_capacity(n);
        let mut vals = Vec::with_capacity(n);
        let mut inside = Vec::with_capacity(n);
        for y in dr[1]..dr[3] {
            for x in dr[0]..dr[2] {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let u = inv[0] * fx + inv[1] * fy + inv[2] - sr[0] as f32;
                let v = inv[3] * fx + inv[4] * fy + inv[5] - sr[1] as f32;
                if u < 0.0 || v < 0.0 || u >= sw as f32 || v >= sh as f32 {
                    out.push([0, 0, 0, 0]);
                    vals.push(0);
                    inside.push(false);
                    continue;
                }
                if smooth {
                    let (u0, v0) = ((u - 0.5).max(0.0), (v - 0.5).max(0.0));
                    let (x0, y0) = (u0 as usize, v0 as usize);
                    let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
                    let (tx, ty) = (u0 - x0 as f32, v0 - y0 as f32);
                    let p = |xx: usize, yy: usize| s_px[yy * sw + xx];
                    let mut c = [0u8; 4];
                    for (k, ck) in c.iter_mut().enumerate() {
                        let a = p(x0, y0)[k] as f32 * (1.0 - tx) + p(x1, y0)[k] as f32 * tx;
                        let b = p(x0, y1)[k] as f32 * (1.0 - tx) + p(x1, y1)[k] as f32 * tx;
                        *ck = (a * (1.0 - ty) + b * ty + 0.5) as u8;
                    }
                    out.push(c);
                } else {
                    out.push(s_px[v as usize * sw + u as usize]);
                }
                vals.push(
                    s_val
                        .get(v as usize * sw + u as usize)
                        .copied()
                        .unwrap_or(0),
                );
                inside.push(true);
            }
        }
        // only the pixels the source covers change
        let before = self.read(vm, dst, dr);
        let s_depth = s.depth;
        self.compose(vm, dst, dr, &out, &vals, s_depth, m);
        let after = self.read(vm, dst, dr);
        let merged: Vec<Px> = (0..n)
            .map(|i| if inside[i] { after[i] } else { before[i] })
            .collect();
        let d32 = d.depth == 32;
        self.write(vm, dst, dr, &merged, d32);
    }
}

fn needs_value(mode: u8) -> bool {
    matches!(mode, 11 | 12 | 23 | 24)
}

fn op3(s: Px, d: Px, level: u32, f: impl Fn(u32, u32) -> u32) -> Px {
    let a = s[3] as u32 * level / 255;
    let mut o = d;
    for c in 0..3 {
        let r = f(s[c] as u32, d[c] as u32).min(255) as u8;
        o[c] = mix(d[c], r, a);
    }
    o
}

/// Reads a BMP: (width, height, depth, rows bottom up with 4-byte stride, palette).
pub fn read_bmp(b: &[u8]) -> Option<(i32, i32, u32, Vec<u8>, Option<Vec<u32>>)> {
    if b.len() < 54 || &b[..2] != b"BM" {
        return None;
    }
    let u32at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let off = u32at(10) as usize;
    let hsize = u32at(14) as usize;
    let w = u32at(18) as i32;
    let hraw = u32at(22) as i32;
    let bits = u16::from_le_bytes([b[28], b[29]]) as u32;
    let compression = u32at(30);
    if compression != 0 && compression != 3 {
        return None;
    }
    let (h, top_down) = if hraw < 0 {
        (-hraw, true)
    } else {
        (hraw, false)
    };
    let stride = (w as usize * bits as usize / 8).div_ceil(4) * 4;
    let need = stride * h as usize;
    if off + need > b.len() {
        return None;
    }
    let mut px = b[off..off + need].to_vec();
    if top_down {
        let mut flipped = Vec::with_capacity(need);
        for y in (0..h as usize).rev() {
            flipped.extend_from_slice(&px[y * stride..(y + 1) * stride]);
        }
        px = flipped;
    }
    let pal = (bits <= 8).then(|| {
        let n = match u32at(46) {
            0 => 1 << bits,
            n => n as usize,
        };
        let p = 14 + hsize;
        (0..256)
            .map(|i| {
                if i < n && p + i * 4 + 4 <= b.len() {
                    u32::from_le_bytes(b[p + i * 4..p + i * 4 + 4].try_into().unwrap()) & 0xffffff
                } else {
                    0
                }
            })
            .collect()
    });
    Some((w, h, bits, px, pal))
}
