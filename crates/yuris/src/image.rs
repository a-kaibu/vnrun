//! Layer images (RGBA, not premultiplied) and what CGACT does to them.

#[derive(Debug, Clone, Default)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

/// A rectangle of pixels, `x1`/`y1` excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

impl Rect {
    pub fn new(x: i64, y: i64, w: i64, h: i64) -> Rect {
        Rect { x0: x, y0: y, x1: x + w, y1: y + h }
    }
    pub fn w(&self) -> i64 {
        (self.x1 - self.x0).max(0)
    }
    pub fn h(&self) -> i64 {
        (self.y1 - self.y0).max(0)
    }
    pub fn and(&self, o: &Rect) -> Rect {
        Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }
    pub fn is_empty(&self) -> bool {
        self.w() == 0 || self.h() == 0
    }
}

/// How a layer is drawn over what is below it (CG MODE).
pub fn blend(mode: i64, d: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    let f = |d: f32, s: f32| -> f32 {
        match mode {
            1 => (d + s).min(1.0),
            2 => (d - s).max(0.0),
            3 => d * s,
            4 => if s <= 0.0 { 1.0 } else { (d / s).min(1.0) },
            5 => 1.0 - (1.0 - d) * (1.0 - s),
            6 => d.max(s),
            7 => d.min(s),
            8 => (d - s).abs(),
            9 => d + s - 2.0 * d * s,
            10 => if d < 0.5 { 2.0 * d * s } else { 1.0 - 2.0 * (1.0 - d) * (1.0 - s) },
            11 => if s >= 1.0 { 1.0 } else { (d / (1.0 - s)).min(1.0) },
            12 => if s <= 0.0 { 0.0 } else { 1.0 - ((1.0 - d) / s).min(1.0) },
            13 => (d + s - 1.0).max(0.0),
            14 => if s < 0.5 { 2.0 * d * s } else { 1.0 - 2.0 * (1.0 - d) * (1.0 - s) },
            15 => if s < 0.5 { d - (1.0 - 2.0 * s) * d * (1.0 - d) } else { d + (2.0 * s - 1.0) * (d.sqrt() - d) },
            16 => {
                if s < 0.5 {
                    if s <= 0.0 { 0.0 } else { 1.0 - ((1.0 - d) / (2.0 * s)).min(1.0) }
                } else if s >= 1.0 {
                    1.0
                } else {
                    (d / (2.0 * (1.0 - s))).min(1.0)
                }
            }
            17 => (d + 2.0 * s - 1.0).clamp(0.0, 1.0),
            18 => if s < 0.5 { d.min(2.0 * s) } else { d.max(2.0 * s - 1.0) },
            _ => s,
        }
    };
    [f(d[0], s[0]), f(d[1], s[1]), f(d[2], s[2])]
}

impl Image {
    pub fn new(w: u32, h: u32) -> Image {
        Image { w, h, px: vec![0; w as usize * h as usize * 4] }
    }

    /// One black pixel: what `CG[FILE=""]` makes.
    pub fn black() -> Image {
        Image { w: 1, h: 1, px: vec![0, 0, 0, 255] }
    }

    pub fn rect(&self) -> Rect {
        Rect::new(0, 0, self.w as i64, self.h as i64)
    }

    pub fn decode(bytes: &[u8]) -> Result<Image, String> {
        let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.into_rgba8();
        let (w, h) = img.dimensions();
        let mut out = Image { w, h, px: img.into_raw() };
        // 32-bit BMPs often leave the alpha byte at 0 when they mean opaque.
        if bytes.starts_with(b"BM") && out.px.chunks_exact(4).all(|p| p[3] == 0) {
            out.px.chunks_exact_mut(4).for_each(|p| p[3] = 255);
        }
        Ok(out)
    }

    pub fn at(&self, x: i64, y: i64) -> Option<[u8; 4]> {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return None;
        }
        let i = (y as usize * self.w as usize + x as usize) * 4;
        Some(self.px[i..i + 4].try_into().unwrap())
    }

    fn idx(&self, x: i64, y: i64) -> usize {
        (y as usize * self.w as usize + x as usize) * 4
    }

    /// The pixels of a rectangle (cut to the image) as a new image.
    pub fn crop(&self, r: Rect) -> Image {
        let r = r.and(&self.rect());
        let mut out = Image::new(r.w() as u32, r.h() as u32);
        for y in 0..r.h() {
            let s = self.idx(r.x0, r.y0 + y);
            let d = out.idx(0, y);
            let n = r.w() as usize * 4;
            out.px[d..d + n].copy_from_slice(&self.px[s..s + n]);
        }
        out
    }

    /// Calls `f` on every pixel of a rectangle (cut to the image).
    pub fn each(&mut self, r: Rect, mut f: impl FnMut(&mut [u8])) {
        let r = r.and(&self.rect());
        for y in r.y0..r.y1 {
            let i = self.idx(r.x0, y);
            for p in self.px[i..i + r.w() as usize * 4].chunks_exact_mut(4) {
                f(p);
            }
        }
    }

    /// Draws `src` with its top left at (x, y), alpha `a` (0..=256), in a blend mode.
    pub fn draw(&mut self, src: &Image, x: i64, y: i64, a: i64, mode: i64, clip: Rect) {
        let r = Rect::new(x, y, src.w as i64, src.h as i64).and(&self.rect()).and(&clip);
        let a = a.clamp(0, 256) as f32 / 256.0;
        if r.is_empty() || a <= 0.0 {
            return;
        }
        for yy in r.y0..r.y1 {
            for xx in r.x0..r.x1 {
                let s = src.idx(xx - x, yy - y);
                let d = self.idx(xx, yy);
                let sa = src.px[s + 3] as f32 / 255.0 * a;
                if sa <= 0.0 {
                    continue;
                }
                let sc = [src.px[s] as f32 / 255.0, src.px[s + 1] as f32 / 255.0, src.px[s + 2] as f32 / 255.0];
                let dc = [self.px[d] as f32 / 255.0, self.px[d + 1] as f32 / 255.0, self.px[d + 2] as f32 / 255.0];
                let da = self.px[d + 3] as f32 / 255.0;
                let b = blend(mode, dc, sc);
                // Where the destination is see-through, the blend has nothing to work on.
                let c = [0, 1, 2].map(|k| b[k] * da + sc[k] * (1.0 - da));
                let oa = sa + da * (1.0 - sa);
                for k in 0..3 {
                    let v = (c[k] * sa + dc[k] * da * (1.0 - sa)) / oa.max(1e-6);
                    self.px[d + k] = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
                self.px[d + 3] = (oa * 255.0 + 0.5) as u8;
            }
        }
    }

    /// Copies pixels as they are (alpha too) with the top left of `src` at (x, y).
    pub fn put(&mut self, src: &Image, x: i64, y: i64) {
        let r = Rect::new(x, y, src.w as i64, src.h as i64).and(&self.rect());
        for yy in r.y0..r.y1 {
            let s = src.idx(r.x0 - x, yy - y);
            let d = self.idx(r.x0, yy);
            let n = r.w() as usize * 4;
            self.px[d..d + n].copy_from_slice(&src.px[s..s + n]);
        }
    }

    /// Scales to a new size: nearest neighbour or bilinear.
    pub fn scale(&self, w: u32, h: u32, smooth: bool) -> Image {
        let mut out = Image::new(w, h);
        if self.w == 0 || self.h == 0 {
            return out;
        }
        for y in 0..h {
            for x in 0..w {
                let d = out.idx(x as i64, y as i64);
                if smooth {
                    let fx = ((x as f32 + 0.5) * self.w as f32 / w as f32 - 0.5).max(0.0);
                    let fy = ((y as f32 + 0.5) * self.h as f32 / h as f32 - 0.5).max(0.0);
                    let (x0, y0) = (fx as i64, fy as i64);
                    let (x1, y1) = ((x0 + 1).min(self.w as i64 - 1), (y0 + 1).min(self.h as i64 - 1));
                    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
                    for k in 0..4 {
                        let p = |xx: i64, yy: i64| self.px[self.idx(xx, yy) + k] as f32;
                        let top = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                        let bottom = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                        out.px[d + k] = (top * (1.0 - ty) + bottom * ty + 0.5) as u8;
                    }
                } else {
                    let sx = (x as u64 * self.w as u64 / w as u64) as i64;
                    let sy = (y as u64 * self.h as u64 / h as u64) as i64;
                    let s = self.idx(sx, sy);
                    out.px[d..d + 4].copy_from_slice(&self.px[s..s + 4]);
                }
            }
        }
        out
    }

    /// RESIZE: grows or cuts each edge, keeping the picture where it is.
    pub fn resize_edges(&self, left: i64, top: i64, right: i64, bottom: i64) -> Image {
        let w = (self.w as i64 + left + right).max(1) as u32;
        let h = (self.h as i64 + top + bottom).max(1) as u32;
        let mut out = Image::new(w, h);
        out.put(self, left, top);
        out
    }

    /// TRIM: cuts away fully transparent rows and columns on the sides in `sides`
    /// (8 top, 4 bottom, 2 left, 1 right). Returns the image and how far its top left moved.
    pub fn trim(&self, sides: i64) -> (Image, i64, i64) {
        let opaque = |x: i64, y: i64| self.px[self.idx(x, y) + 3] != 0;
        let (w, h) = (self.w as i64, self.h as i64);
        let row = |y: i64| (0..w).any(|x| opaque(x, y));
        let col = |x: i64| (0..h).any(|y| opaque(x, y));
        let mut r = self.rect();
        if sides & 8 != 0 {
            while r.y0 < r.y1 && !row(r.y0) {
                r.y0 += 1;
            }
        }
        if sides & 4 != 0 {
            while r.y1 > r.y0 && !row(r.y1 - 1) {
                r.y1 -= 1;
            }
        }
        if sides & 2 != 0 {
            while r.x0 < r.x1 && !col(r.x0) {
                r.x0 += 1;
            }
        }
        if sides & 1 != 0 {
            while r.x1 > r.x0 && !col(r.x1 - 1) {
                r.x1 -= 1;
            }
        }
        if r.is_empty() {
            return (Image::new(1, 1), 0, 0);
        }
        (self.crop(r), r.x0, r.y0)
    }

    pub fn flip(&mut self, vertical: bool) {
        let (w, h) = (self.w as usize, self.h as usize);
        let src = self.px.clone();
        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = if vertical { (x, h - 1 - y) } else { (w - 1 - x, y) };
                let d = (y * w + x) * 4;
                let s = (sy * w + sx) * 4;
                self.px[d..d + 4].copy_from_slice(&src[s..s + 4]);
            }
        }
    }
}

/// The colour adjustments of CGACT on one pixel (alpha untouched).
pub mod adjust {
    /// SAIDO: saturation, 256 = as it is, 0 = grey.
    pub fn saturation(p: &mut [u8], v: i64) {
        let grey = (p[0] as i64 * 77 + p[1] as i64 * 150 + p[2] as i64 * 29) >> 8;
        for k in 0..3 {
            p[k] = (grey + (p[k] as i64 - grey) * v / 256).clamp(0, 255) as u8;
        }
    }
    /// MEIDO: per channel, 0xRRGGBB with 0x80 = as it is.
    pub fn channels(p: &mut [u8], rgb: i64) {
        for k in 0..3 {
            let f = (rgb >> (16 - 8 * k)) & 0xff;
            p[k] = (p[k] as i64 * f / 128).clamp(0, 255) as u8;
        }
    }
    /// AKARUSA: brightness, 256 = as it is, 0 = black, 512 = white.
    pub fn brightness(p: &mut [u8], v: i64) {
        for k in 0..3 {
            let c = p[k] as i64;
            p[k] = if v <= 256 { c * v / 256 } else { c + (255 - c) * (v - 256) / 256 }.clamp(0, 255) as u8;
        }
    }
    /// MONO: black or white by a threshold.
    pub fn mono(p: &mut [u8], threshold: i64) {
        let grey = (p[0] as i64 * 77 + p[1] as i64 * 150 + p[2] as i64 * 29) >> 8;
        let v = if grey >= threshold { 255 } else { 0 };
        p[..3].fill(v);
    }
    pub fn invert(p: &mut [u8]) {
        for k in 0..3 {
            p[k] = 255 - p[k];
        }
    }
}
