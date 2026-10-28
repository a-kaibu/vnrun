//! YKG000 pictures: a 0x40-byte header whose words at 0x28 say where three parts are (offset,
//! size each): the picture (a PNG whose signature has "GNP" for "PNG"), a mask picture (white
//! where the picture does not show) and a table of patterns: frames of 0x20 bytes (x, y, width
//! and height in the picture, then how many ms it shows), a pattern's frames ending with an empty
//! one. A pattern of several frames is an animation. Character pictures come without a
//! mask and name a second file that has only the mask (`*_M1.YKG`).

use nexas::gfx::Image;

pub struct Ykg {
    pub image: Option<Image>,
    pub mask: Option<Image>,
    /// the patterns: their frames (x, y, w, h in the picture) and how long each shows
    pub frames: Vec<Vec<([i32; 4], u32)>>,
}

fn png(b: &[u8]) -> Option<Image> {
    let mut v = b.to_vec();
    if v.len() < 8 {
        return None;
    }
    v[1..4].copy_from_slice(b"PNG");
    Image::decode(&v).ok()
}

pub fn parse(b: &[u8]) -> Result<Ykg, String> {
    if !b.starts_with(b"YKG000") || b.len() < 0x40 {
        // a plain PNG
        if b.starts_with(b"\x89PNG") {
            return Ok(Ykg {
                image: Some(Image::decode(b)?),
                mask: None,
                frames: Vec::new(),
            });
        }
        return Err("not a YKG picture".into());
    }
    let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let part = |i: usize| {
        let (o, n) = (u(0x28 + i * 8), u(0x2c + i * 8));
        (n > 0).then(|| b.get(o..o + n)).flatten()
    };
    let image = part(0).and_then(png);
    let mask = part(1).and_then(png);
    let mut frames = Vec::new();
    if let Some(t) = part(2) {
        let mut cur = Vec::new();
        for f in t.chunks_exact(0x20) {
            let w = |k: usize| i32::from_le_bytes(f[k * 4..k * 4 + 4].try_into().unwrap());
            if w(2) <= 0 || w(3) <= 0 {
                frames.push(std::mem::take(&mut cur));
            } else {
                cur.push(([w(0), w(1), w(2), w(3)], w(4).max(0) as u32));
            }
        }
        if !cur.is_empty() {
            frames.push(cur);
        }
    }
    Ok(Ykg {
        image,
        mask,
        frames,
    })
}

/// The picture with a mask's darkness as its opacity (the mask white where it does not show).
pub fn apply_mask(img: &mut Image, mask: &Image) {
    let (w, h) = (img.w.min(mask.w), img.h.min(mask.h));
    for y in 0..h {
        for x in 0..w {
            let m = ((y * mask.w + x) * 4) as usize;
            let d = ((y * img.w + x) * 4) as usize;
            let v = (mask.px[m] as u32 + mask.px[m + 1] as u32 + mask.px[m + 2] as u32) / 3;
            img.px[d + 3] = (255 - v) as u8;
        }
    }
}
