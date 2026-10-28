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
        let count = ((r[2] - r[0]) * (r[3] - r[1])).max(0) as usize;
        let Some(p) = self.map.get(&n) else {
            return vec![[0, 0, 0, 255]; count];
        };
        let alpha = self.alpha_of(p);
        let mut out = Vec::with_capacity(count);
        for y in r[1]..r[3] {
            read_row(&vm.mem, p, alpha, y, r[0], r[2], &mut out);
        }
        out
    }

    /// The 8-bit plane holding a plane's alpha.
    pub fn alpha_of(&self, p: &Plane) -> Option<&Plane> {
        p.alpha
            .and_then(|a| self.map.get(&a))
            .filter(|a| a.depth == 8)
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
        let w = (r[2] - r[0]).max(0) as usize;
        if w == 0 {
            return;
        }
        for (k, y) in (r[1]..r[3]).enumerate() {
            write_row(&mut vm.mem, p, y, r[0], &px[k * w..(k + 1) * w], alpha);
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
        // a full blend is a copy (but keeps a 32-bit destination's alpha)
        let plain = m.mode == 0 || (m.mode == 3 && m.level == 255 && d.depth != 32);
        // the same kind of pixels: rows of bytes
        if plain && s.depth == d.depth && (s.depth != 8 || s.pal == d.pal) {
            let bpp = s.bpp();
            let n = (sr[2] - sr[0]) as usize * bpp;
            let mut tmp = vec![0u8; n];
            for k in 0..(sr[3] - sr[1]) {
                let from = s.row(sr[1] + k) + sr[0] as usize * bpp;
                tmp.copy_from_slice(&vm.mem[from..from + n]);
                let to = d.row(dr[1] + k) + dr[0] as usize * bpp;
                vm.mem[to..to + n].copy_from_slice(&tmp);
            }
            return;
        }
        // 32-bit pictures laid over by their alpha
        if m.mode == 7
            && s.depth == 32
            && self.alpha_of(s).is_none()
            && (d.depth == 24 || d.depth == 32)
        {
            let dbpp = d.bpp();
            let level = m.level as u32;
            let w = (sr[2] - sr[0]) as usize;
            let mut tmp = vec![0u8; w * 4];
            for k in 0..(sr[3] - sr[1]) {
                let from = s.row(sr[1] + k) + sr[0] as usize * 4;
                tmp.copy_from_slice(&vm.mem[from..from + w * 4]);
                let to = d.row(dr[1] + k) + dr[0] as usize * dbpp;
                let drow = &mut vm.mem[to..to + w * dbpp];
                for (sp, dp) in tmp.chunks_exact(4).zip(drow.chunks_exact_mut(dbpp)) {
                    let a = sp[3] as u32 * level / 255;
                    if a == 0 {
                        continue;
                    }
                    if a == 255 {
                        dp[..3].copy_from_slice(&sp[..3]);
                        if dbpp == 4 {
                            dp[3] = 255;
                        }
                        continue;
                    }
                    if dbpp == 4 && dp[3] == 0 {
                        dp.copy_from_slice(&[sp[0], sp[1], sp[2], a as u8]);
                        continue;
                    }
                    dp[0] = mix(dp[0], sp[0], a);
                    dp[1] = mix(dp[1], sp[1], a);
                    dp[2] = mix(dp[2], sp[2], a);
                    if dbpp == 4 {
                        dp[3] = (a + dp[3] as u32 * (255 - a) / 255).min(255) as u8;
                    }
                }
            }
            return;
        }
        let s_px = self.read(vm, src, sr);
        let s_val = if needs_value(m.mode) {
            self.read_value(vm, src, sr)
        } else {
            Vec::new()
        };
        let s_depth = s.depth;
        self.compose(vm, dst, dr, &s_px, &s_val, s_depth, m, None);
    }

    /// Lays source pixels (the size of `dr`) over `dst` in `dr`.
    #[allow(clippy::too_many_arguments)]
    pub fn compose(
        &self,
        vm: &mut Vm,
        dst: i32,
        dr: Rect,
        s_px: &[Px],
        s_val: &[u8],
        s_depth: u32,
        m: &Mode,
        mask: Option<&[bool]>,
    ) {
        let Some(d) = self.map.get(&dst) else { return };
        let d32 = d.depth == 32;
        if m.mode == 0 && mask.is_none() {
            self.write(vm, dst, dr, s_px, true);
            return;
        }
        let d_alpha = self.alpha_of(d);
        let level = m.level as u32;
        let col = bgr(m.color);
        let trans = bgr(m.trans);
        let w = (dr[2] - dr[0]).max(0) as usize;
        let mut row: Vec<Px> = Vec::with_capacity(w);
        for (k, y) in (dr[1]..dr[3]).enumerate() {
            row.clear();
            read_row(&vm.mem, d, d_alpha, y, dr[0], dr[2], &mut row);
            for (j, dp) in row.iter_mut().enumerate() {
                let i = k * w + j;
                if let Some(mk) = mask
                    && !mk[i]
                {
                    continue;
                }
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
                    // pattern wipes: the source's value (its alpha plane, or an 8-bit source)
                    // is the pattern; where it is under the level the source shows, with a
                    // soft edge `range` wide
                    11 | 12 => {
                        let range = m.range as i32;
                        let a = if range == 0 {
                            if (v as u32) < level || level == 255 {
                                255
                            } else {
                                0
                            }
                        } else {
                            let t = level as i32 * (255 + range) / 255;
                            ((t - v as i32) * 255 / range).clamp(0, 255)
                        } as u32;
                        if a == 0 {
                            None
                        } else {
                            Some([
                                mix(d[0], s[0], a),
                                mix(d[1], s[1], a),
                                mix(d[2], s[2], a),
                                d[3],
                            ])
                        }
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
                        let g =
                            ((s[0] as u32 * 29 + s[1] as u32 * 150 + s[2] as u32 * 77) >> 8) as u8;
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
                if let Some(mut o) = out {
                    // on a picture with alpha the drawn part also becomes visible
                    if d32 && matches!(m.mode, 13..=21 | 25..=35) {
                        let a = s[3] as u32 * level / 255;
                        o[3] = (a + d[3] as u32 * (255 - a) / 255).min(255) as u8;
                    }
                    *dp = o;
                }
            }
            write_row(&mut vm.mem, d, y, dr[0], &row, d32);
        }
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
        self.compose(vm, dst, dr, &out, &vals, s.depth, m, Some(&inside));
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
    if b.starts_with(b"\x89PNG") {
        return read_png(b);
    }
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

/// A PNG (悠刻のファムファタル's gfx.bin) as a 32-bit BMP would read: BGRA rows, bottom up.
fn read_png(b: &[u8]) -> Option<(i32, i32, u32, Vec<u8>, Option<Vec<u32>>)> {
    let img = image::load_from_memory_with_format(b, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let (w, h) = img.dimensions();
    let raw = img.into_raw();
    let stride = w as usize * 4;
    let mut px = Vec::with_capacity(raw.len());
    for row in raw.chunks_exact(stride).rev() {
        for c in row.chunks_exact(4) {
            px.extend_from_slice(&[c[2], c[1], c[0], c[3]]);
        }
    }
    Some((w as i32, h as i32, 32, px, None))
}

/// Appends the pixels x0..x1 of row y.
pub fn read_row(
    mem: &[u8],
    p: &Plane,
    alpha: Option<&Plane>,
    y: i32,
    x0: i32,
    x1: i32,
    out: &mut Vec<Px>,
) {
    let row = p.row(y);
    let at = out.len();
    let (a, b) = (x0 as usize, x1 as usize);
    match p.depth {
        32 => out.extend(
            mem[row + a * 4..row + b * 4]
                .chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]]),
        ),
        24 => out.extend(
            mem[row + a * 3..row + b * 3]
                .chunks_exact(3)
                .map(|c| [c[0], c[1], c[2], 255]),
        ),
        16 => out.extend(mem[row + a * 2..row + b * 2].chunks_exact(2).map(|c| {
            let v = u16::from_le_bytes([c[0], c[1]]);
            let f = |s: u16| (((v >> s) & 31) as u32 * 255 / 31) as u8;
            [f(0), f(5), f(10), 255]
        })),
        _ => match &p.pal {
            Some(pal) => out.extend(mem[row + a..row + b].iter().map(|&v| {
                let [b, g, r] = bgr(pal[v as usize]);
                [b, g, r, 255]
            })),
            None => out.extend(mem[row + a..row + b].iter().map(|&v| [v, v, v, 255])),
        },
    }
    if let Some(ap) = alpha
        && y < ap.h
    {
        let ar = ap.row(y);
        let end = b.min(ap.w as usize);
        for x in a..end {
            out[at + x - a][3] = mem[ar + x];
        }
    }
}

/// Writes pixels at x0.. of row y.
pub fn write_row(mem: &mut [u8], p: &Plane, y: i32, x0: i32, px: &[Px], alpha: bool) {
    let row = p.row(y);
    let a = x0 as usize;
    let n = px.len();
    match p.depth {
        32 => {
            for (d, c) in mem[row + a * 4..row + (a + n) * 4]
                .chunks_exact_mut(4)
                .zip(px)
            {
                d[..3].copy_from_slice(&c[..3]);
                if alpha {
                    d[3] = c[3];
                }
            }
        }
        24 => {
            for (d, c) in mem[row + a * 3..row + (a + n) * 3]
                .chunks_exact_mut(3)
                .zip(px)
            {
                d.copy_from_slice(&c[..3]);
            }
        }
        16 => {
            for (d, c) in mem[row + a * 2..row + (a + n) * 2]
                .chunks_exact_mut(2)
                .zip(px)
            {
                let v = (c[0] as u16 >> 3) | ((c[1] as u16 >> 3) << 5) | ((c[2] as u16 >> 3) << 10);
                d.copy_from_slice(&v.to_le_bytes());
            }
        }
        _ => {
            for (d, c) in mem[row + a..row + a + n].iter_mut().zip(px) {
                *d = c[2];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pattern wipe shows the source where the pattern is under the level.
    #[test]
    fn pattern_wipe() {
        let mut vm = Vm::new();
        let mut planes = Planes::default();
        planes.alloc(&mut vm, 1, 4, 1, 24);
        planes.alloc(&mut vm, 2, 4, 1, 24);
        planes.alloc(&mut vm, 3, 4, 1, 8);
        planes.fill(&mut vm, 1, None, 0x000000);
        planes.fill(&mut vm, 2, None, 0xffffff);
        let p = planes.map[&3].clone();
        let row = p.row(0);
        vm.mem[row..row + 4].copy_from_slice(&[0, 100, 200, 255]);
        planes.map.get_mut(&2).unwrap().alpha = Some(3);
        let wipe = |mode, level, range| Mode {
            mode,
            level,
            range,
            ..Mode::NORMAL
        };
        planes.copy(&mut vm, 1, 0, 0, 2, None, &wipe(11, 150, 0));
        let got: Vec<u8> = planes
            .read(&vm, 1, [0, 0, 4, 1])
            .iter()
            .map(|c| c[0])
            .collect();
        assert_eq!(got, [255, 255, 0, 0]);
        planes.fill(&mut vm, 1, None, 0x000000);
        planes.copy(&mut vm, 1, 0, 0, 2, None, &wipe(12, 128, 32));
        let got: Vec<u8> = planes
            .read(&vm, 1, [0, 0, 4, 1])
            .iter()
            .map(|c| c[0])
            .collect();
        // t = 128 * 287 / 255 = 144: 0 and 100 are past the edge, 200 and 255 not reached
        assert_eq!(got, [255, 255, 0, 0]);
        planes.copy(&mut vm, 1, 0, 0, 2, None, &wipe(12, 255, 32));
        let got: Vec<u8> = planes
            .read(&vm, 1, [0, 0, 4, 1])
            .iter()
            .map(|c| c[0])
            .collect();
        assert_eq!(got, [255, 255, 255, 255]);
    }
}
