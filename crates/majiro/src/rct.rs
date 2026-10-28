//! Pictures: RCT (24-bit, "TC00"/"TC01"; TC01 names a base picture whose pixels show through
//! pure red) and RC8 (8-bit with a palette; "name_.rc8" is the alpha mask of "name.rct").
//! The packing follows GARbro's ImageRCT.cs / ImageRC8.cs.

const MAGIC: [u8; 4] = [0x98, 0x5a, 0x92, 0x9a];

fn u32le(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| "short picture".to_string())
}

pub enum Pic {
    /// BGR pixels, and the name of the base picture (TC01)
    Rgb {
        w: u32,
        h: u32,
        bgr: Vec<u8>,
        base: Option<String>,
    },
    /// palette indices and the palette (RGB)
    Indexed {
        w: u32,
        h: u32,
        px: Vec<u8>,
        palette: Vec<[u8; 3]>,
    },
}

pub fn is_pic(b: &[u8]) -> bool {
    b.len() > 8 && b[..4] == MAGIC
}

pub fn parse(b: &[u8]) -> Result<Pic, String> {
    if !is_pic(b) {
        return Err("not an RCT/RC8 picture".into());
    }
    let w = u32le(b, 8)?;
    let h = u32le(b, 12)?;
    if w > 0x8000 || h > 0x8000 {
        return Err("bad picture size".into());
    }
    match &b[4..8] {
        b"TC00" | b"TC01" => {
            let mut at = 0x14;
            let mut base = None;
            if b[7] == b'1' {
                let n = b.get(0x14..0x16).ok_or("short picture")?;
                let n = u16::from_le_bytes([n[0], n[1]]) as usize;
                at = 0x16;
                let raw = b.get(at..at + n).ok_or("short picture")?;
                let end = raw.iter().position(|&c| c == 0).unwrap_or(n);
                if end > 0 {
                    base = Some(encoding_rs::SHIFT_JIS.decode(&raw[..end]).0.into_owned());
                }
                at += n;
            }
            let bgr = unpack(&b[at..], (w * h) as usize, 3, w as i32)?;
            Ok(Pic::Rgb { w, h, bgr, base })
        }
        b"8_00" => {
            let pal = b.get(0x14..0x314).ok_or("short palette")?;
            let palette = pal.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            let px = unpack(&b[0x314..], (w * h) as usize, 1, w as i32)?;
            Ok(Pic::Indexed { w, h, px, palette })
        }
        b"TS00" | b"TS01" => Err("encrypted RCT pictures are not supported".into()),
        _ => Err("unknown RCT kind".into()),
    }
}

const SHIFT3: [i8; 32] = [
    -16, -32, -48, -64, -80, -96, 49, 33, 17, 1, -15, -31, -47, 50, 34, 18, 2, -14, -30, -46, 51,
    35, 19, 3, -13, -29, -45, 36, 20, 4, -12, -28,
];
const SHIFT1: [i8; 16] = [
    -16, -32, -48, -64, 49, 33, 17, 1, -15, -31, -47, 34, 18, 2, -14, -30,
];

/// The LZ-like packing of both kinds: runs of literal pixels and copies from a nearby earlier
/// pixel (to the left or on the rows above).
fn unpack(src: &[u8], pixels: usize, bpp: usize, width: i32) -> Result<Vec<u8>, String> {
    let total = pixels * bpp;
    let mut out = vec![0u8; total];
    let mut p = 0usize;
    let mut s = 0usize;
    let short = || "picture data ends early".to_string();
    let byte = |s: &mut usize| -> Result<usize, String> {
        let b = *src.get(*s).ok_or_else(short)?;
        *s += 1;
        Ok(b as usize)
    };
    let word = |s: &mut usize| -> Result<usize, String> {
        let b = src.get(*s..*s + 2).ok_or_else(short)?;
        *s += 2;
        Ok(u16::from_le_bytes([b[0], b[1]]) as usize)
    };
    let mut n = 0usize;
    while p < total {
        let count = (n + 1) * bpp;
        if p + count > total || s + count > src.len() {
            return Err(short());
        }
        out[p..p + count].copy_from_slice(&src[s..s + count]);
        p += count;
        s += count;
        while p < total {
            let mut e = byte(&mut s)?;
            if e & 0x80 == 0 {
                if e == 0x7f {
                    e += word(&mut s)?;
                }
                n = e;
                break;
            }
            let (idx, mut len, min) = if bpp == 3 {
                ((e >> 2) & 0x1f, e & 3, 1)
            } else {
                ((e >> 3) & 0x0f, e & 7, 3)
            };
            let lim = if bpp == 3 { 3 } else { 7 };
            if len == lim {
                len += word(&mut s)?;
            }
            let count = if bpp == 3 { (len + min) * 3 } else { len + min };
            if p + count > total {
                return Err("picture copy past the end".into());
            }
            let shift = if bpp == 3 { SHIFT3[idx] } else { SHIFT1[idx] } as i32;
            let row = shift & 0x0f;
            let dx = shift >> 4;
            let back = (dx - row * width) * bpp as i32;
            if back >= 0 || (p as i32 + back) < 0 {
                return Err("bad picture copy".into());
            }
            let from = (p as i32 + back) as usize;
            for k in 0..count {
                out[p + k] = out[from + k];
            }
            p += count;
        }
    }
    Ok(out)
}
