//! LiveMaker's own pictures (.gal, "Gale"): frames of layers of BGR(A) pixels, zlib packed or
//! JPEG, with a separate 8-bit alpha plane. Rows are top first, each padded to 4 bytes. With a
//! block size, a layer is a grid of blocks, each stored raw or copied from an earlier block.
//!
//! ```text
//! "Gale" version[3]                 100..=102: fixed header; 103+: header_size:u32 header
//! "GaleX200" xml_size:u32 zlib(xml)  (the same fields as XML attributes)
//! frame = name, mask:u32, 9 bytes, layers:u32, w:i32 h:i32 bpp:u32 [palette BGRX x 2^bpp]
//!         layers x { left top:i32 visible:u8 trans:i32 alpha:i32 alpha_on:u8 name [lock:u8]
//!                    size:u32 pixels  size:u32 alpha }
//! ```

use nexas::gfx::Image;

use crate::archive::Reader;

struct Info {
    w: usize,
    h: usize,
    bpp: usize,
    comp: Option<u8>,
    block: (usize, usize),
}

struct Layer {
    left: i32,
    top: i32,
    visible: bool,
    trans: i32,
    alpha_on: bool,
}

pub fn is_gal(b: &[u8]) -> bool {
    b.starts_with(b"Gale")
}

/// The first frame of a .gal picture.
pub fn decode(b: &[u8]) -> Result<Image, String> {
    if b.starts_with(b"GaleX") {
        return decode_x(b);
    }
    let mut r = Reader { b, at: 4 };
    let version: u32 = std::str::from_utf8(r.bytes(3)?)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or("gal: version")?;
    let (info, offset) = if version > 102 {
        let n = r.u32()? as usize;
        let h = r.bytes(n)?;
        let u = |at: usize| u32::from_le_bytes(h[at..at + 4].try_into().unwrap()) as usize;
        if h.len() < 0x24 {
            return Err("gal: short header".into());
        }
        if h[0x15] != 0 {
            return Err("gal: LiveMaker Pro scrambled picture".into());
        }
        (
            Info {
                w: u(4),
                h: u(8),
                bpp: u(0xc),
                comp: Some(h[0x16]),
                block: (u(0x1c), u(0x20)),
            },
            n + 11,
        )
    } else {
        let h = r.bytes(0x10)?;
        let u = |at: usize| u32::from_le_bytes(h[at..at + 4].try_into().unwrap()) as usize;
        (
            Info {
                w: u(4),
                h: u(8),
                bpp: u(0xc),
                comp: None,
                block: (0, 0),
            },
            u(0) + 45,
        )
    };
    let mut r = Reader { b, at: offset };
    let n = r.u32()? as usize;
    r.bytes(n)?;
    r.u32()?;
    r.bytes(9)?;
    let layers = r.u32()? as usize;
    let w = r.i32()?.max(0) as usize;
    let h = r.i32()?.max(0) as usize;
    let bpp = r.u32()? as usize;
    let palette = if bpp <= 8 {
        r.bytes((1 << bpp) * 4)?.to_vec()
    } else {
        Vec::new()
    };
    let info = Info { w, h, bpp, ..info };
    let mut out = Image::new(w as u32, h as u32);
    for i in 0..layers {
        let left = r.i32()?;
        let top = r.i32()?;
        let visible = r.u8()? != 0;
        let trans = r.i32()?;
        r.i32()?;
        let alpha_on = r.u8()? != 0;
        let n = r.u32()? as usize;
        r.bytes(n)?;
        if version >= 107 {
            r.u8()?;
        }
        let n = r.u32()? as usize;
        let px = r.bytes(n)?;
        let n = r.u32()? as usize;
        let alpha = r.bytes(n)?;
        let layer = Layer {
            left,
            top,
            visible,
            trans,
            alpha_on: alpha_on && !alpha.is_empty(),
        };
        if i == 0 || layer.visible {
            paint(&mut out, &info, &layer, px, alpha, &palette, i == 0)?;
        }
    }
    Ok(out)
}

fn attr(tag: &str, name: &str) -> Option<i64> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let end = tag[at..].find('"')? + at;
    tag[at..end].parse().ok()
}

fn decode_x(b: &[u8]) -> Result<Image, String> {
    let mut r = Reader { b, at: 8 };
    let n = r.u32()? as usize;
    let xml = miniz_oxide::inflate::decompress_to_vec_zlib(r.bytes(n)?)
        .map_err(|e| format!("gal xml: {e:?}"))?;
    let xml = nexas::sjis::decode(&xml);
    let root = &xml[..xml.find('>').unwrap_or(xml.len())];
    let get = |k: &str| attr(root, k).unwrap_or(0);
    if attr(root, "Randomized").unwrap_or(0) != 0 {
        return Err("gal: LiveMaker Pro scrambled picture".into());
    }
    let mut info = Info {
        w: get("Width") as usize,
        h: get("Height") as usize,
        bpp: get("Bpp") as usize,
        comp: Some(get("CompType") as u8),
        block: (get("BlockWidth") as usize, get("BlockHeight") as usize),
    };
    // The first frame's layers.
    let frame_start = xml.find("<Frame").unwrap_or(0);
    let frame_end = xml[frame_start..]
        .find("</Frame>")
        .map_or(xml.len(), |e| e + frame_start);
    let frame = &xml[frame_start..frame_end];
    if let Some(at) = frame.find("<Layers") {
        let tag = &frame[at..frame[at..].find('>').map_or(frame.len(), |e| e + at)];
        info.w = attr(tag, "Width").map_or(info.w, |v| v as usize);
        info.h = attr(tag, "Height").map_or(info.h, |v| v as usize);
        info.bpp = attr(tag, "Bpp").map_or(info.bpp, |v| v as usize);
    }
    let palette: Vec<u8> = frame
        .find("<RGB>")
        .and_then(|at| {
            let s = &frame[at + 5..];
            let hex = &s[..s.find('<')?];
            (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
                .collect::<Option<Vec<u8>>>()
        })
        .map(|bgr| {
            bgr.chunks(3)
                .flat_map(|c| [c[0], c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0), 0])
                .collect()
        })
        .unwrap_or_default();
    let mut out = Image::new(info.w as u32, info.h as u32);
    let mut r = Reader { b, at: n + 12 };
    for (i, (at, _)) in frame.match_indices("<Layer ").enumerate() {
        let tag = &frame[at..frame[at..].find('>').map_or(frame.len(), |e| e + at)];
        let layer = Layer {
            left: attr(tag, "Left").unwrap_or(0) as i32,
            top: attr(tag, "Top").unwrap_or(0) as i32,
            visible: attr(tag, "Visible").unwrap_or(1) != 0,
            trans: attr(tag, "TransColor").unwrap_or(-1) as i32,
            alpha_on: attr(tag, "AlphaOn").unwrap_or(0) != 0,
        };
        let n = r.u32()? as usize;
        let px = r.bytes(n)?;
        let alpha = if layer.alpha_on {
            let n = r.u32()? as usize;
            r.bytes(n)?
        } else {
            &[]
        };
        if i == 0 || layer.visible {
            paint(&mut out, &info, &layer, px, alpha, &palette, i == 0)?;
        }
    }
    Ok(out)
}

/// Undoes the block layout of a plane: `stride` bytes a row, `bpp` bits a pixel.
fn unblock(packed: &[u8], info: &Info, stride: usize, bpp: usize) -> Result<Vec<u8>, String> {
    let (bw, bh) = info.block;
    if bw == 0 || bh == 0 {
        return Ok(packed.to_vec());
    }
    let (w, h) = (info.w, info.h);
    let cols = w.div_ceil(bw);
    let count = cols * h.div_ceil(bh);
    let mut r = Reader { b: packed, at: 0 };
    let mut refs = Vec::with_capacity(count);
    for _ in 0..count {
        refs.push((r.i32()?, r.i32()?));
    }
    let mut data = vec![0u8; stride * h];
    let mut i = 0;
    for y in (0..h).step_by(bh) {
        let rows = bh.min(h - y);
        for x in (0..w).step_by(bw) {
            let (frame, layer) = refs[i];
            i += 1;
            let run = (bw.min(w - x) * bpp).div_ceil(8);
            let mut dst = y * stride + (x * bpp).div_ceil(8);
            match frame {
                -1 => {
                    for _ in 0..rows {
                        data[dst..dst + run].copy_from_slice(r.bytes(run)?);
                        dst += stride;
                    }
                }
                -2 => {
                    let layer = layer as usize;
                    let sx = bw * (layer % cols);
                    let sy = bh * (layer / cols);
                    let mut src = sy * stride + (sx * bpp).div_ceil(8);
                    for _ in 0..rows {
                        data.copy_within(src..src + run, dst);
                        src += stride;
                        dst += stride;
                    }
                }
                _ => return Err("gal: block from another frame".into()),
            }
        }
    }
    Ok(data)
}

fn paint(
    out: &mut Image,
    info: &Info,
    layer: &Layer,
    px: &[u8],
    alpha: &[u8],
    palette: &[u8],
    first: bool,
) -> Result<(), String> {
    let (w, h, bpp) = (info.w, info.h, info.bpp);
    let stride = ((w * bpp).div_ceil(8) + 3) & !3;
    let rgba: Vec<u8> = if info.comp == Some(2) {
        let img = image::load_from_memory_with_format(px, image::ImageFormat::Jpeg)
            .map_err(|e| format!("gal jpeg: {e}"))?
            .to_rgba8();
        img.into_raw()
    } else {
        let raw = if info.comp == Some(0) {
            miniz_oxide::inflate::decompress_to_vec_zlib(px)
                .map_err(|e| format!("gal: {e:?}"))?
        } else {
            px.to_vec()
        };
        let raw = unblock(&raw, info, stride, bpp)?;
        if raw.len() < stride * h {
            return Err("gal: pixels cut short".into());
        }
        let mut v = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            let row = &raw[y * stride..];
            for x in 0..w {
                let c = match bpp {
                    32 => [row[x * 4 + 2], row[x * 4 + 1], row[x * 4], row[x * 4 + 3]],
                    24 => [row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255],
                    16 => {
                        let p = u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]);
                        let f = |v: u16| ((v & 31) * 255 / 31) as u8;
                        [f(p >> 10), f(p >> 5), f(p), 255]
                    }
                    8 | 4 => {
                        let i = if bpp == 8 {
                            row[x] as usize
                        } else {
                            ((row[x / 2] >> if x % 2 == 0 { 4 } else { 0 }) & 15) as usize
                        };
                        let p = palette.get(i * 4..i * 4 + 3).unwrap_or(&[0, 0, 0]);
                        [p[2], p[1], p[0], 255]
                    }
                    _ => return Err(format!("gal: {bpp} bits a pixel")),
                };
                v.extend(c);
            }
        }
        if bpp != 32 && layer.trans >= 0 {
            let t = layer.trans as u32;
            let key = [t as u8, (t >> 8) as u8, (t >> 16) as u8];
            for p in v.chunks_exact_mut(4) {
                if p[..3] == key {
                    p[3] = 0;
                }
            }
        }
        v
    };
    let mut rgba = rgba;
    if layer.alpha_on {
        let astride = (w + 3) & !3;
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(alpha)
            .map_err(|e| format!("gal alpha: {e:?}"))?;
        let a = unblock(&raw, info, astride, 8)?;
        for y in 0..h {
            for x in 0..w {
                rgba[(y * w + x) * 4 + 3] = a.get(y * astride + x).copied().unwrap_or(255);
            }
        }
    }
    if first && layer.left == 0 && layer.top == 0 && rgba.len() == out.px.len() {
        out.px = rgba;
        return Ok(());
    }
    // Lay the layer over what is there.
    let (ow, oh) = (out.w as i32, out.h as i32);
    for y in 0..h as i32 {
        let oy = y + layer.top;
        if oy < 0 || oy >= oh {
            continue;
        }
        for x in 0..w as i32 {
            let ox = x + layer.left;
            if ox < 0 || ox >= ow {
                continue;
            }
            let s = &rgba[((y * w as i32 + x) * 4) as usize..][..4];
            let d = &mut out.px[((oy * ow + ox) * 4) as usize..][..4];
            let a = s[3] as u32;
            for k in 0..3 {
                d[k] = ((s[k] as u32 * a + d[k] as u32 * (255 - a)) / 255) as u8;
            }
            d[3] = (a + d[3] as u32 * (255 - a) / 255) as u8;
        }
    }
    Ok(())
}
