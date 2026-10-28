//! The pictures. Type 'a' (and 'd', drawn over the picture before it) starts with the
//! scheme's id byte and a header of numbers in a little code of three marker bytes (one adds
//! `v1`, one ends the number, one stands for zero), then pixel bits and BGR(A) rows (bottom up in some games);
//! types 'b' and 'c' carry a PNG at their end. After GARbro (morkt, MIT), ImageTINK.cs.

use nexas::gfx::Image;

/// The marker bytes and where each header number goes, which differ by game.
#[derive(Debug, Clone)]
pub struct Scheme {
    pub v1: u8,
    pub v2: u8,
    pub v3: u8,
    /// header slot (0 flags, 3 height, 4 width, 5 unpacked size, 6 alpha size, 7 bits size)
    /// of each number in turn, None for numbers not used
    pub order: Vec<Option<usize>>,
    /// rows stored bottom up
    pub flipped: bool,
}

impl Scheme {
    /// グランギニョルの夜 (2018)
    pub fn tinkerbell_2018() -> Scheme {
        let mut order = vec![None; 20];
        for (k, v) in [(3, 4), (5, 3), (9, 6), (11, 5), (13, 0), (17, 7)] {
            order[k] = Some(v);
        }
        Scheme {
            v1: 0xe9,
            v2: 0xef,
            v3: 0xfb,
            order,
            flipped: false,
        }
    }
}

/// Pixels as stored: bytes per pixel 1, 3 (BGR) or 4 (BGRA).
pub struct Raw {
    pub w: usize,
    pub h: usize,
    pub bpp: usize,
    pub px: Vec<u8>,
}

impl Raw {
    pub fn to_image(&self, flipped: bool) -> Image {
        let mut img = Image::new(self.w as u32, self.h as u32);
        for y in 0..self.h {
            let row = if flipped { self.h - 1 - y } else { y };
            let src = row * self.w * self.bpp;
            for x in 0..self.w {
                let s = &self.px[src + x * self.bpp..];
                let o = (y * self.w + x) * 4;
                img.px[o..o + 4].copy_from_slice(&match self.bpp {
                    1 => [s[0], s[0], s[0], 255],
                    3 => [s[2], s[1], s[0], 255],
                    _ => [s[2], s[1], s[0], s[3]],
                });
            }
        }
        img
    }
}

impl Raw {
    /// The pixels of a decoded PNG as a base for 'd' pictures.
    pub fn from_image(img: &Image) -> Raw {
        let mut px = img.px.clone();
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        Raw {
            w: img.w as usize,
            h: img.h as usize,
            bpp: 4,
            px,
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.at).ok_or("picture: cut short")?;
        self.at += 1;
        Ok(v)
    }
    fn take(&mut self, n: usize) -> &[u8] {
        let s = &self.b[self.at.min(self.b.len())..(self.at + n).min(self.b.len())];
        self.at += n;
        s
    }
    fn number(&mut self, s: &Scheme) -> Result<usize, String> {
        let mut a = self.byte()? as usize;
        if a == s.v3 as usize {
            a = 0;
        }
        let (mut c, mut d) = (0usize, 0usize);
        loop {
            let x = self.byte()?;
            if x == s.v2 {
                break;
            }
            if x == s.v1 {
                d += 1;
            } else {
                c = if x == s.v3 { 0 } else { x as usize };
            }
        }
        let v1 = s.v1 as usize;
        Ok(a + (c + d * v1) * v1)
    }
}

/// The header of an 'a' or 'd' picture: [flags, -, -, height, width, unpacked, alpha, bits]
/// and where its data starts.
pub fn header(b: &[u8], s: &Scheme) -> Result<([usize; 8], usize), String> {
    if b.len() < 22 || b[1] != s.v2 {
        return Err("picture: not of this scheme".into());
    }
    let mut r = Reader { b, at: 2 };
    let mut h = [0usize; 8];
    for slot in &s.order {
        let v = r.number(s)?;
        if let Some(k) = slot {
            h[*k] = v;
        }
    }
    Ok((h, r.at))
}

fn bit(m: &[u8], k: usize) -> bool {
    m.get(k >> 3).is_some_and(|&x| x >> (k & 7) & 1 == 1)
}

/// Decodes an 'a' or 'd' picture; `base` is the picture a 'd' one (or an 'a' one with flags
/// 1) is drawn over.
pub fn decode_a(b: &[u8], s: &Scheme, base: Option<&Raw>) -> Result<Raw, String> {
    let (h, at) = header(b, s)?;
    decode_body(b, h, at, base)
}

/// The pixels of an 'a' or 'd' picture from its header numbers (see [`header`]).
fn decode_body(b: &[u8], h: [usize; 8], at: usize, base: Option<&Raw>) -> Result<Raw, String> {
    let (flags, height, width, unp, asz, bits) = (h[0], h[3], h[4], h[5], h[6], h[7]);
    if b[0] != b'd' && width == 0 || width >= 0x8000 || height == 0 || height >= 0x8000 {
        return Err(format!("picture: size {width}x{height}"));
    }
    // a 'd' picture has the size of its base (its own numbers there mean something else)
    let (w, ht) = match base {
        Some(x) if b[0] == b'd' => (x.w, x.h),
        _ => (width, height),
    };
    let plane = w * ht;
    let mut r = Reader { b, at };
    let base = base.filter(|x| x.w == w && x.h == ht);
    let raw = |bpp, px| Raw { w, h: ht, bpp, px };
    if unp == 0 {
        // only the pixels that differ from the base
        let base = base.ok_or("picture: no base")?;
        if asz != 0 || flags & 1 == 0 {
            return Err("picture: empty".into());
        }
        let mut out = base.px.clone();
        let bpp = base.bpp;
        let map = r.take(bits).to_vec();
        for k in 0..(out.len() / bpp).min(bits * 8) {
            if bit(&map, k) {
                let px = r.take(3);
                let n = px.len();
                out[k * bpp..k * bpp + n].copy_from_slice(px);
            }
        }
        return Ok(raw(bpp, out));
    }
    let copy = |r: &mut Reader| -> Result<Raw, String> {
        for bpp in [1, 3, 4] {
            if unp == plane * bpp {
                return Ok(raw(bpp, r.take(unp).to_vec()));
            }
        }
        let stride = (w * 3 + 3) & !3;
        if stride * ht != unp {
            return Err("picture: bad size".into());
        }
        let d = r.take(unp);
        let mut out = Vec::with_capacity(plane * 3);
        for y in 0..ht {
            out.extend_from_slice(d.get(y * stride..y * stride + w * 3).ok_or("cut")?);
        }
        Ok(raw(3, out))
    };
    if flags == 0 {
        return copy(&mut r);
    }
    match flags & 6 {
        2 => {
            let rgb = r.take(bits).to_vec();
            let alpha = r.take(bits).to_vec();
            let mut out = vec![0; plane * 3];
            for k in 0..plane {
                if !bit(&alpha, k) && bit(&rgb, k) {
                    let px = r.take(3);
                    out[k * 3..k * 3 + px.len()].copy_from_slice(px);
                }
            }
            Ok(raw(3, out))
        }
        6 if bits == 0 => {
            // an alpha plane, then the colour plane, rows padded to 4 bytes
            let stride = (w * 3 + 3) & !3;
            let a = r.take(stride * ht).to_vec();
            let c = r.take(stride * ht).to_vec();
            let mut out = vec![0; plane * 4];
            for y in 0..ht {
                for x in 0..w {
                    let o = (y * w + x) * 4;
                    let s = y * stride + x * 3;
                    if s + 3 <= c.len() && s < a.len() {
                        out[o..o + 3].copy_from_slice(&c[s..s + 3]);
                        out[o + 3] = a[s];
                    }
                }
            }
            Ok(raw(4, out))
        }
        6 if flags & 1 == 1 && b[0] == b'd' && base.is_some() => {
            let base = base.unwrap();
            let mut out = if base.bpp == 4 {
                base.px.clone()
            } else {
                base.px
                    .chunks_exact(base.bpp)
                    .flat_map(|p| {
                        let c = if p.len() >= 3 {
                            [p[0], p[1], p[2]]
                        } else {
                            [p[0]; 3]
                        };
                        [c[0], c[1], c[2], 255]
                    })
                    .collect()
            };
            let map = r.take(bits).to_vec();
            let alpha = r.take(asz).to_vec();
            let mut ai = 0;
            for k in 0..plane.min(bits * 8) {
                if bit(&map, k) {
                    let px = r.take(3);
                    out[k * 4..k * 4 + px.len()].copy_from_slice(px);
                    out[k * 4 + 3] = alpha.get(ai).copied().unwrap_or(255);
                    ai += 1;
                }
            }
            Ok(raw(4, out))
        }
        6 => {
            let rgb = r.take(bits).to_vec();
            let amap = r.take(bits).to_vec();
            let alpha = r.take(asz).to_vec();
            let mut out = vec![0; plane * 4];
            let mut ai = 0;
            for k in 0..plane {
                let has = bit(&amap, k);
                if has || bit(&rgb, k) {
                    let px = r.take(3);
                    out[k * 4..k * 4 + px.len()].copy_from_slice(px);
                    out[k * 4 + 3] = if has && ai < alpha.len() {
                        let v = alpha[ai];
                        ai += 3;
                        v
                    } else {
                        255
                    };
                }
            }
            Ok(raw(4, out))
        }
        _ if bits == 0 => copy(&mut r),
        _ => {
            let map = r.take(bits).to_vec();
            let (bpp, mut out) = match base {
                Some(b) if b.bpp >= 3 => (b.bpp, b.px.clone()),
                _ => (4, vec![0; plane * 4]),
            };
            for k in 0..plane {
                let on = bit(&map, k);
                if on {
                    let px = r.take(3);
                    out[k * bpp..k * bpp + px.len()].copy_from_slice(px);
                }
                if bpp == 4 && base.is_none() {
                    out[k * 4 + 3] = if on { 255 } else { 0 };
                }
            }
            Ok(raw(bpp, out))
        }
    }
}

/// The PNG of a 'b' or 'c' picture.
pub fn png_of(b: &[u8]) -> Option<&[u8]> {
    let n = u32::from_be_bytes(b.get(1..5)?.try_into().ok()?) as usize;
    b.get(b.len().checked_sub(n)?..)
}

/// Whether a picture needs the one before it.
pub fn wants_base(b: &[u8], s: &Scheme) -> bool {
    match b.first() {
        Some(b'd') => true,
        Some(b'a') => header(b, s).is_ok_and(|(h, _)| h[0] == 1),
        _ => false,
    }
}

/// An 'a' or 'd' picture of the newer games (2023 on): the type letter, u32 number (for 'd'
/// and flags 1: the picture it is drawn over), u32 0, u32 width, u32 height, u32 flags; then
/// with flags 7 u32 alpha size and the data, else u32 alpha size, u32 bits size, u32 size and
/// the data. The data is as in the older pictures ([`decode_a`]).
pub fn decode_a2023(b: &[u8], base: Option<&Raw>) -> Result<Raw, String> {
    let u = |at: usize| -> usize {
        b.get(at..at + 4)
            .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()) as usize)
    };
    if b.len() < 33 {
        return Err("picture: cut short".into());
    }
    let (mut w, mut ht, flags) = (u(9), u(13), u(17));
    if b[0] == b'd'
        && let Some(x) = base
    {
        (w, ht) = (x.w, x.h);
    }
    // the 2026 games: alpha bytes, then the bitmap and the colours, each after its size
    if b[0] == b'd'
        && flags & 6 == 6
        && let Some(x) = base
    {
        let asz = u(21);
        let bits = (w * ht).div_ceil(8);
        if u(25 + asz) == bits {
            return diff_sized(b, x, asz, bits);
        }
    }
    // [flags, -, -, height, width, unpacked, alpha, bits]
    let (h, at) = if flags & 6 == 6 {
        let bits = if b[0] == b'd' {
            (w * ht).div_ceil(8)
        } else {
            0
        };
        ([flags, 0, 0, ht, w, 1, u(21), bits], 25)
    } else {
        ([flags, 0, 0, ht, w, u(29), u(21), u(25)], 33)
    };
    decode_body(b, h, at, base)
}

/// A 2026 'd' picture: `asz` alpha bytes, u32 size + bitmap, u32 size + 3 bytes per set bit.
fn diff_sized(b: &[u8], base: &Raw, asz: usize, bits: usize) -> Result<Raw, String> {
    let alpha = b.get(25..25 + asz).ok_or("picture: cut short")?;
    let map = b
        .get(29 + asz..29 + asz + bits)
        .ok_or("picture: cut short")?;
    let rgb = b.get(33 + asz + bits..).ok_or("picture: cut short")?;
    let mut out: Vec<u8> = if base.bpp == 4 {
        base.px.clone()
    } else {
        base.px
            .chunks_exact(base.bpp)
            .flat_map(|p| {
                let c = if p.len() >= 3 {
                    [p[0], p[1], p[2]]
                } else {
                    [p[0]; 3]
                };
                [c[0], c[1], c[2], 255]
            })
            .collect()
    };
    let mut n = 0;
    for k in 0..(base.w * base.h).min(bits * 8) {
        if bit(map, k) {
            let Some(px) = rgb.get(n * 3..n * 3 + 3) else {
                break;
            };
            out[k * 4..k * 4 + 3].copy_from_slice(px);
            out[k * 4 + 3] = alpha.get(n).copied().unwrap_or(255);
            n += 1;
        }
    }
    Ok(Raw {
        w: base.w,
        h: base.h,
        bpp: 4,
        px: out,
    })
}

/// The picture an 'a' (flags 1) or 'd' picture of the newer games is drawn over.
pub fn base_id_2023(b: &[u8]) -> Option<u32> {
    let wants = match b.first() {
        Some(b'd') => true,
        Some(b'a') => b.get(17).is_some_and(|f| f & 1 == 1 && f & 6 != 6),
        _ => false,
    };
    if !wants {
        return None;
    }
    Some(u32::from_le_bytes(b.get(1..5)?.try_into().ok()?))
}
