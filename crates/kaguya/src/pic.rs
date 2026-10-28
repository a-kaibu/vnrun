//! Pictures: "AP" (u32 width, u32 height, u16 bits, then BGRA rows bottom up), "AO" (the
//! same with an i32 x and y where it goes on the screen), "\x04APS3" (parts of one AP, each
//! with where it goes) and plain 24/32-bit BMPs.

use nexas::gfx::Image;

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A picture and where its top left goes on the screen.
pub struct Pic {
    pub img: Image,
    pub x: i32,
    pub y: i32,
}

/// A part of an APS3: which state of a button it is (or 0), its place in the atlas
/// (x0, y0, x1, y1) and where it goes on the screen.
#[derive(Clone, Debug)]
pub struct Part {
    pub state: i32,
    pub name: String,
    pub src: [i32; 4],
    pub x: i32,
    pub y: i32,
}

pub struct Parts {
    pub atlas: Image,
    pub parts: Vec<Part>,
}

fn i32le(b: &[u8], at: usize) -> Option<i32> {
    Some(u32le(b, at)? as i32)
}

/// The parts of 2008 (no magic): u16 count of source files, each a u32 length and a path;
/// u16 count of patterns, each a u32 length, a name and 68 bytes (u16 0, u16 2, u32 index,
/// i32 -1, the source rectangle x0 y0 x1 y1, ...); then u16 packed, u32 packed length,
/// u32 length and the AP. The places on the screen are not in the file.
fn parts_2008(b: &[u8]) -> Result<Parts, String> {
    let u16at = |at: usize| -> Option<usize> {
        Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().unwrap()) as usize)
    };
    let short = || "short parts".to_string();
    let n = u16at(0).ok_or_else(short)?;
    let mut at = 2;
    for _ in 0..n {
        at += 4 + u32le(b, at).ok_or_else(short)? as usize;
    }
    let n = u16at(at).ok_or_else(short)?;
    at += 2;
    let mut parts = Vec::new();
    for k in 0..n {
        let len = u32le(b, at).ok_or_else(short)? as usize;
        let name = encoding_rs::SHIFT_JIS
            .decode(b.get(at + 4..at + 4 + len).ok_or_else(short)?)
            .0
            .into_owned();
        at += 4 + len;
        let src = [12, 16, 20, 24].map(|o| i32le(b, at + o).unwrap_or(0));
        parts.push(Part {
            state: k as i32,
            name,
            src,
            x: 0,
            y: 0,
        });
        at += 68;
    }
    let comp = u16at(at).ok_or_else(short)?;
    let packed = u32le(b, at + 2).ok_or_else(short)? as usize;
    let unpacked = u32le(b, at + 6).ok_or_else(short)? as usize;
    let raw = b.get(at + 10..at + 10 + packed).ok_or_else(short)?;
    let data = if comp == 1 {
        crate::arc::unpack(raw, unpacked)
    } else {
        raw.to_vec()
    };
    Ok(Parts {
        atlas: decode(&data)?.img,
        parts,
    })
}

pub fn parts(b: &[u8]) -> Result<Parts, String> {
    if !b.starts_with(b"\x04APS3") {
        return parts_2008(b).map_err(|_| "not APS3".to_string());
    }
    let n = i32le(b, 5).ok_or("short APS3")?;
    let mut at = 9;
    let mut parts = Vec::new();
    for _ in 0..n.clamp(0, 4096) {
        let state = i32le(b, at).ok_or("short APS3")?;
        let len = *b.get(at + 4).ok_or("short APS3")? as usize;
        let name = encoding_rs::SHIFT_JIS
            .decode(b.get(at + 5..at + 5 + len).ok_or("short APS3")?)
            .0
            .into_owned();
        at += 5 + len;
        let r = [0, 4, 8, 12].map(|k| i32le(b, at + k).unwrap_or(0));
        let (x, y) = (
            i32le(b, at + 20).unwrap_or(0),
            i32le(b, at + 24).unwrap_or(0),
        );
        at += 28;
        if len > 0 {
            parts.push(Part {
                state,
                name,
                src: r,
                x,
                y,
            });
        }
    }
    let _size = u32le(b, at).ok_or("short APS3")?;
    let comp = u16::from_le_bytes(
        b.get(at + 4..at + 6)
            .ok_or("short APS3")?
            .try_into()
            .unwrap(),
    );
    let data = if comp == 1 {
        let packed = u32le(b, at + 6).ok_or("short APS3")? as usize;
        let unpacked = u32le(b, at + 10).ok_or("short APS3")? as usize;
        let raw = b.get(at + 14..at + 14 + packed).ok_or("short APS3")?;
        crate::arc::unpack(raw, unpacked)
    } else {
        let n = u32le(b, at + 6).ok_or("short APS3")? as usize;
        b.get(at + 10..at + 10 + n).ok_or("short APS3")?.to_vec()
    };
    Ok(Parts {
        atlas: decode(&data)?.img,
        parts,
    })
}

fn rows(b: &[u8], start: usize, w: u32, h: u32) -> Result<Image, String> {
    if w == 0 || h == 0 || w > 0x4000 || h > 0x4000 {
        return Err("bad picture size".into());
    }
    let data = b
        .get(start..start + (w * h * 4) as usize)
        .ok_or("short picture")?;
    let mut img = Image::new(w, h);
    let stride = w as usize * 4;
    for y in 0..h as usize {
        let s = &data[(h as usize - 1 - y) * stride..][..stride];
        let d = &mut img.px[y * stride..][..stride];
        for (d, s) in d.chunks_exact_mut(4).zip(s.chunks_exact(4)) {
            d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
        }
    }
    Ok(img)
}

pub fn decode(b: &[u8]) -> Result<Pic, String> {
    if b.starts_with(b"AO") {
        let w = u32le(b, 2).ok_or("short AO")?;
        let h = u32le(b, 6).ok_or("short AO")?;
        let x = i32le(b, 12).ok_or("short AO")?;
        let y = i32le(b, 16).ok_or("short AO")?;
        return Ok(Pic {
            img: rows(b, 20, w, h)?,
            x,
            y,
        });
    }
    if b.starts_with(b"AP") {
        let w = u32le(b, 2).ok_or("short AP")?;
        let h = u32le(b, 6).ok_or("short AP")?;
        return Ok(Pic {
            img: rows(b, 12, w, h)?,
            x: 0,
            y: 0,
        });
    }
    let img = bmp(b)?;
    Ok(Pic { img, x: 0, y: 0 })
}

fn bmp(b: &[u8]) -> Result<Image, String> {
    if b.starts_with(b"BM") {
        let off = u32le(b, 10).ok_or("short BMP")? as usize;
        let w = u32le(b, 18).ok_or("short BMP")? as i32;
        let h = u32le(b, 22).ok_or("short BMP")? as i32;
        let bpp = u16::from_le_bytes(b.get(28..30).ok_or("short BMP")?.try_into().unwrap());
        if w <= 0 || h == 0 || w > 0x4000 || h.abs() > 0x4000 || !(bpp == 24 || bpp == 32) {
            return Err("unsupported BMP".into());
        }
        let (uh, flip) = (h.unsigned_abs(), h > 0);
        let n = (bpp / 8) as usize;
        let stride = (w as usize * n + 3) & !3;
        let mut img = Image::new(w as u32, uh);
        for y in 0..uh as usize {
            let row = if flip { uh as usize - 1 - y } else { y };
            let Some(s) = b.get(off + row * stride..off + row * stride + w as usize * n) else {
                break;
            };
            let d = &mut img.px[y * w as usize * 4..][..w as usize * 4];
            for (d, s) in d.chunks_exact_mut(4).zip(s.chunks_exact(n)) {
                d.copy_from_slice(&[s[2], s[1], s[0], 255]);
            }
        }
        return Ok(img);
    }
    Err("unknown picture".into())
}
