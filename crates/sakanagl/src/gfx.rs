//! Pictures (RGBA, not premultiplied) and what the frame draws: quads given by their corners.

use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

pub struct Img {
    /// Unique per picture, so the browser can keep its GPU copy.
    pub id: u32,
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

fn next_id() -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Img {
    pub fn new(w: u32, h: u32) -> Img {
        Img {
            id: next_id(),
            w,
            h,
            px: vec![0; (w * h * 4) as usize],
        }
    }

    pub fn solid(rgba: [u8; 4]) -> Img {
        Img {
            id: next_id(),
            w: 4,
            h: 4,
            px: rgba.repeat(16),
        }
    }

    /// WebP, PNG or JPEG.
    pub fn decode(b: &[u8]) -> Result<Img, String> {
        let fmt = if b.starts_with(b"RIFF") {
            image::ImageFormat::WebP
        } else if b.starts_with(&[0x89, b'P']) {
            image::ImageFormat::Png
        } else {
            image::ImageFormat::Jpeg
        };
        let i = image::load_from_memory_with_format(b, fmt)
            .map_err(|e| e.to_string())?
            .into_rgba8();
        let (w, h) = i.dimensions();
        Ok(Img {
            id: next_id(),
            w,
            h,
            px: i.into_raw(),
        })
    }

    /// Multiplied by an RGB tint.
    pub fn tinted(&self, rgb: [u8; 3]) -> Img {
        let mut px = self.px.clone();
        for p in px.chunks_exact_mut(4) {
            for k in 0..3 {
                p[k] = (p[k] as u32 * rgb[k] as u32 / 255) as u8;
            }
        }
        Img {
            id: next_id(),
            w: self.w,
            h: self.h,
            px,
        }
    }
}

/// One picture on the screen.
#[derive(Clone)]
pub struct Draw {
    pub img: Rc<Img>,
    /// top left, top right, bottom left, bottom right (screen pixels)
    pub corners: [[f32; 2]; 4],
    /// the part of the picture: x0 y0 x1 y1
    pub src: [f32; 4],
    pub alpha: f32,
    pub add: bool,
    /// the part of the screen it may cover: x0 y0 x1 y1
    pub clip: [f32; 4],
}

/// Draws `d` onto `out` (for the native tools and tests): nearest sampling, laid over or added.
pub fn paint(out: &mut Img, d: &Draw) {
    let [tl, tr, bl, _] = d.corners;
    // The inverse of the map from (u, v) in 0..1 to the screen.
    let (ax, ay) = (tr[0] - tl[0], tr[1] - tl[1]);
    let (bx, by) = (bl[0] - tl[0], bl[1] - tl[1]);
    let det = ax * by - ay * bx;
    if det.abs() < 1e-6 || d.alpha <= 0.0 {
        return;
    }
    let xs = d.corners.iter().map(|c| c[0]);
    let ys = d.corners.iter().map(|c| c[1]);
    let x0 = xs.clone().fold(f32::MAX, f32::min).max(d.clip[0]).max(0.0) as i32;
    let x1 = xs
        .fold(f32::MIN, f32::max)
        .min(d.clip[2])
        .min(out.w as f32)
        .ceil() as i32;
    let y0 = ys.clone().fold(f32::MAX, f32::min).max(d.clip[1]).max(0.0) as i32;
    let y1 = ys
        .fold(f32::MIN, f32::max)
        .min(d.clip[3])
        .min(out.h as f32)
        .ceil() as i32;
    let (sw, sh) = (d.src[2] - d.src[0], d.src[3] - d.src[1]);
    for y in y0..y1 {
        for x in x0..x1 {
            let (px, py) = (x as f32 + 0.5 - tl[0], y as f32 + 0.5 - tl[1]);
            let u = (px * by - py * bx) / det;
            let v = (ax * py - ay * px) / det;
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }
            let sx = (d.src[0] + u * sw) as i32;
            let sy = (d.src[1] + v * sh) as i32;
            if sx < 0 || sy < 0 || sx >= d.img.w as i32 || sy >= d.img.h as i32 {
                continue;
            }
            let si = ((sy as u32 * d.img.w + sx as u32) * 4) as usize;
            let di = ((y as u32 * out.w + x as u32) * 4) as usize;
            let a = d.img.px[si + 3] as f32 / 255.0 * d.alpha;
            for k in 0..3 {
                let s = d.img.px[si + k] as f32;
                let o = out.px[di + k] as f32;
                out.px[di + k] = if d.add {
                    (o + s * a).min(255.0)
                } else {
                    o + (s - o) * a
                } as u8;
            }
            out.px[di + 3] = 255;
        }
    }
}
