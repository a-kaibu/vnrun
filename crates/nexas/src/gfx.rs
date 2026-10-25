//! Pictures: PNG images, SPM sprite maps (named rectangles of a PNG) and drawing them with
//! position, scale, rotation and opacity.

/// RGBA, not premultiplied.
#[derive(Debug, Clone, Default)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Image {
    pub fn new(w: u32, h: u32) -> Image {
        Image {
            w,
            h,
            px: vec![0; (w * h * 4) as usize],
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Image, String> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        Ok(Image {
            w: img.width(),
            h: img.height(),
            px: img.into_raw(),
        })
    }

    pub fn fill(&mut self, rgba: [u8; 4]) {
        for p in self.px.chunks_exact_mut(4) {
            p.copy_from_slice(&rgba);
        }
    }

    fn at(&self, x: i32, y: i32) -> [u8; 4] {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return [0; 4];
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        self.px[i..i + 4].try_into().unwrap()
    }
}

/// One rectangle of an SPM frame: where it goes (relative to the object's position) and where
/// it comes from in the image.
#[derive(Debug, Clone, Copy)]
pub struct Part {
    pub image: usize,
    pub dst: [i32; 4],
    pub src: [i32; 4],
}

#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub w: i32,
    pub h: i32,
    /// the frame's extent relative to the object's position
    pub bounds: [i32; 4],
    pub parts: Vec<Part>,
}

/// A sprite map:
///
/// ```text
/// "SPM VER-2.00\0" n:u32 n x frame  images:u32 images x NUL-terminated name  (named groups ...)
/// frame = parts:u32 w:i32 h:i32 bounds:[i32; 4] [i32; 3]  parts x part  0:i32
/// part  = ?:i32 image:i32 dst:[i32; 4] w:i32 h:i32 src:[i32; 4] ?:i32 256:i32
/// ```
#[derive(Debug, Clone, Default)]
pub struct Spm {
    pub frames: Vec<Frame>,
    pub images: Vec<String>,
}

impl Spm {
    pub fn parse(b: &[u8]) -> Result<Spm, String> {
        if !b.starts_with(b"SPM VER-") {
            return Err("not an SPM file".into());
        }
        let mut at = b.iter().position(|&c| c == 0).ok_or("SPM header")? + 1;
        let mut i32_ = || -> Result<i32, String> {
            let v = b.get(at..at + 4).ok_or("SPM is cut short")?;
            at += 4;
            Ok(i32::from_le_bytes(v.try_into().unwrap()))
        };
        let n = i32_()?.max(0) as usize;
        let mut frames = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            let np = i32_()?.max(0) as usize;
            let w = i32_()?;
            let h = i32_()?;
            let bounds = [i32_()?, i32_()?, i32_()?, i32_()?];
            for _ in 0..3 {
                i32_()?;
            }
            let mut parts = Vec::with_capacity(np.min(256));
            for _ in 0..np {
                i32_()?;
                let image = i32_()?.max(0) as usize;
                let dst = [i32_()?, i32_()?, i32_()?, i32_()?];
                i32_()?;
                i32_()?;
                let src = [i32_()?, i32_()?, i32_()?, i32_()?];
                for _ in 0..2 {
                    i32_()?;
                }
                parts.push(Part { image, dst, src });
            }
            i32_()?;
            frames.push(Frame {
                w,
                h,
                bounds,
                parts,
            });
        }
        let ni = i32_()?.max(0) as usize;
        let mut images = Vec::with_capacity(ni.min(64));
        for _ in 0..ni {
            let len = b[at.min(b.len())..]
                .iter()
                .position(|&c| c == 0)
                .ok_or("SPM image name")?;
            images.push(crate::sjis::decode(&b[at..at + len]));
            at += len + 1;
        }
        Ok(Spm { frames, images })
    }
}

/// How an object is drawn: where, how large, turned how far, and how opaque.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    pub x: f32,
    pub y: f32,
    pub sx: f32,
    pub sy: f32,
    /// the point that scaling and turning keep in place (screen coordinates)
    pub cx: f32,
    pub cy: f32,
    /// radians, clockwise
    pub rot: f32,
    /// 0..=1
    pub alpha: f32,
    pub add: bool,
    /// mirrored left to right / top to bottom
    pub flip: (bool, bool),
    /// only this part of the screen is drawn: left, top, right, bottom
    pub clip: [f32; 4],
}

/// Draws `src[sr]` so that it covers `dst` (before scaling and turning) on `out`.
pub fn draw(out: &mut Image, src: &Image, sr: [i32; 4], dst: [f32; 4], p: &Place) {
    if p.alpha <= 0.0 || p.sx == 0.0 || p.sy == 0.0 {
        return;
    }
    let (sw, sh) = ((sr[2] - sr[0]) as f32, (sr[3] - sr[1]) as f32);
    let (dw, dh) = (dst[2] - dst[0], dst[3] - dst[1]);
    if sw <= 0.0 || sh <= 0.0 || dw <= 0.0 || dh <= 0.0 {
        return;
    }
    let (sin, cos) = p.rot.sin_cos();
    // screen = c + R * S * (local - c), local in unscaled screen units
    let fwd = |x: f32, y: f32| {
        let (dx, dy) = ((x - p.cx) * p.sx, (y - p.cy) * p.sy);
        (p.cx + dx * cos - dy * sin, p.cy + dx * sin + dy * cos)
    };
    let corners = [
        fwd(dst[0], dst[1]),
        fwd(dst[2], dst[1]),
        fwd(dst[0], dst[3]),
        fwd(dst[2], dst[3]),
    ];
    let c = p.clip;
    let x0 = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::MAX, f32::min)
        .max(c[0])
        .floor()
        .max(0.0) as i32;
    let x1 = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::MIN, f32::max)
        .min(c[2])
        .ceil()
        .min(out.w as f32) as i32;
    let y0 = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::MAX, f32::min)
        .max(c[1])
        .floor()
        .max(0.0) as i32;
    let y1 = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::MIN, f32::max)
        .min(c[3])
        .ceil()
        .min(out.h as f32) as i32;
    let plain = p.rot == 0.0 && p.sx == 1.0 && p.sy == 1.0 && dw == sw && dh == sh;
    let a = (p.alpha.clamp(0.0, 1.0) * 256.0) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // back to local
            let (rx, ry) = (fx - p.cx, fy - p.cy);
            let (lx, ly) = (
                (rx * cos + ry * sin) / p.sx + p.cx,
                (-rx * sin + ry * cos) / p.sy + p.cy,
            );
            if lx < dst[0]
                || ly < dst[1]
                || lx >= dst[2]
                || ly >= dst[3]
                || fx < c[0]
                || fy < c[1]
                || fx >= c[2]
                || fy >= c[3]
            {
                continue;
            }
            let (lx, ly) = (
                if p.flip.0 { dst[0] + dst[2] - lx } else { lx },
                if p.flip.1 { dst[1] + dst[3] - ly } else { ly },
            );
            let u = sr[0] as f32 + (lx - dst[0]) * sw / dw;
            let v = sr[1] as f32 + (ly - dst[1]) * sh / dh;
            let s = if plain {
                src.at(u as i32, v as i32)
            } else {
                bilinear(src, u - 0.5, v - 0.5, sr)
            };
            let sa = (s[3] as u32 * a) >> 8;
            if sa == 0 {
                continue;
            }
            let i = ((y as u32 * out.w + x as u32) * 4) as usize;
            let d = &mut out.px[i..i + 4];
            if p.add {
                for k in 0..3 {
                    d[k] = (d[k] as u32 + ((s[k] as u32 * sa) >> 8)).min(255) as u8;
                }
            } else {
                for k in 0..3 {
                    d[k] = ((s[k] as u32 * sa + d[k] as u32 * (255 - sa)) / 255) as u8;
                }
                d[3] = (sa + d[3] as u32 * (255 - sa) / 255).min(255) as u8;
            }
        }
    }
}

fn bilinear(img: &Image, u: f32, v: f32, sr: [i32; 4]) -> [u8; 4] {
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = (u - x0, v - y0);
    let clampx = |x: i32| x.clamp(sr[0], sr[2] - 1);
    let clampy = |y: i32| y.clamp(sr[1], sr[3] - 1);
    let (x0, y0) = (x0 as i32, y0 as i32);
    let p = [
        img.at(clampx(x0), clampy(y0)),
        img.at(clampx(x0 + 1), clampy(y0)),
        img.at(clampx(x0), clampy(y0 + 1)),
        img.at(clampx(x0 + 1), clampy(y0 + 1)),
    ];
    let w = [
        (1.0 - fx) * (1.0 - fy),
        fx * (1.0 - fy),
        (1.0 - fx) * fy,
        fx * fy,
    ];
    // weight colours by alpha so transparent edges stay clean
    let a: f32 = (0..4).map(|i| p[i][3] as f32 * w[i]).sum();
    let mut out = [0u8; 4];
    if a > 0.0 {
        for k in 0..3 {
            out[k] = ((0..4)
                .map(|i| p[i][k] as f32 * p[i][3] as f32 * w[i])
                .sum::<f32>()
                / a)
                .round() as u8;
        }
    }
    out[3] = a.round() as u8;
    out
}
