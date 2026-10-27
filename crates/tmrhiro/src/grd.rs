//! GRD pictures. The header gives the canvas the picture belongs to and the part of it the
//! picture covers; the colour channels follow one after another, each run-length coded,
//! Huffman coded, or Huffman coded over a small LZ77.
//!
//! ```text
//! kind:u8 (1 opaque, 2 with alpha) packing:u8 (1 RLE, 0xA1 Huffman+RLE, 0xA2 Huffman+LZ77)
//! canvas_w:u16 canvas_h:u16 bpp:u16 left:u16 right:u16 top:u16 bottom:u16
//! alpha_size:u32 r_size:u32 g_size:u32 b_size:u32   (then the channels in that order)
//! ```
//!
//! Rows are stored bottom up and `top`/`bottom` count from the canvas's bottom edge (after
//! GARbro's ImageGRD.cs).

use std::rc::Rc;

use nexas::gfx::Image;

pub struct Grd {
    pub canvas: (u32, u32),
    /// where the picture's top left corner is on the canvas
    pub x: i32,
    pub y: i32,
    pub image: Rc<Image>,
}

fn u16_at(b: &[u8], at: usize) -> u32 {
    u16::from_le_bytes([b[at], b[at + 1]]) as u32
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
}

pub fn is_grd(b: &[u8]) -> bool {
    b.len() >= 0x20 && matches!(b[0], 1 | 2) && matches!(b[1], 1 | 0xa1 | 0xa2)
}

pub fn decode(b: &[u8]) -> Result<Grd, String> {
    if !is_grd(b) {
        return Err("not a GRD picture".into());
    }
    let packing = b[1];
    let (cw, ch, bpp) = (u16_at(b, 2), u16_at(b, 4), u16_at(b, 6));
    let (left, right, top, bottom) = (u16_at(b, 8), u16_at(b, 10), u16_at(b, 12), u16_at(b, 14));
    let (w, h) = (right.abs_diff(left), bottom.abs_diff(top));
    if bpp != 24 && bpp != 32 {
        return Err(format!("GRD: {bpp} bits per pixel"));
    }
    let sizes = [u32_at(b, 16), u32_at(b, 20), u32_at(b, 24), u32_at(b, 28)];
    if 0x20 + sizes.iter().sum::<usize>() > b.len() {
        return Err("GRD is cut short".into());
    }
    let n = (w * h) as usize;
    let mut image = Image::new(w, h);
    for p in image.px.chunks_exact_mut(4) {
        p[3] = 255;
    }
    let mut at = 0x20;
    // alpha, red, green, blue
    for (k, &size) in sizes.iter().enumerate() {
        let part = &b[at..at + size];
        at += size;
        if k == 0 && (bpp != 32 || size == 0) {
            continue;
        }
        let channel = match packing {
            1 => rle(part, n),
            0xa2 => lz77(&huffman(part)?, n),
            _ => rle(&huffman(part)?, n),
        };
        let dst = if k == 0 { 3 } else { k - 1 };
        for y in 0..h as usize {
            let row = &channel[(h as usize - 1 - y) * w as usize..][..w as usize];
            for (x, &v) in row.iter().enumerate() {
                image.px[(y * w as usize + x) * 4 + dst] = v;
            }
        }
    }
    Ok(Grd {
        canvas: (cw, ch),
        x: left as i32,
        y: ch as i32 - bottom as i32,
        image: Rc::new(image),
    })
}

/// count:u8 below 0x80 copies that many bytes; with the top bit, repeats the next byte.
fn rle(src: &[u8], n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while i < src.len() && out.len() < n {
        let c = src[i] as usize;
        i += 1;
        if c > 0x7f {
            let v = src.get(i).copied().unwrap_or(0);
            i += 1;
            out.extend(std::iter::repeat_n(v, c & 0x7f));
        } else {
            let end = (i + c).min(src.len());
            out.extend_from_slice(&src[i..end]);
            i = end;
        }
    }
    out.resize(n, 0);
    out
}

/// unpacked:u32 packed:u32 freq:[u32; 256], then the bits, low bit first.
fn huffman(src: &[u8]) -> Result<Vec<u8>, String> {
    if src.len() < 8 + 1024 {
        return Err("GRD: Huffman table is cut short".into());
    }
    let unpacked = u32_at(src, 0);
    let mut freq = [0u64; 0x200];
    let mut kids = [(0usize, 0usize); 0x200];
    // nodes kept in order of frequency, a new one after those of the same frequency
    let mut order: Vec<usize> = Vec::with_capacity(0x100);
    let add = |order: &mut Vec<usize>, freq: &[u64; 0x200], i: usize| {
        let at = order
            .iter()
            .position(|&o| freq[o] > freq[i])
            .unwrap_or(order.len());
        order.insert(at, i);
    };
    for i in 0..0x100 {
        freq[i] = u32_at(src, 8 + i * 4) as u64;
        add(&mut order, &freq, i);
    }
    let mut next = 0x100;
    while order.len() > 1 {
        let (l, r) = (order.remove(0), order.remove(0));
        freq[next] = freq[l] + freq[r];
        kids[next] = (l, r);
        add(&mut order, &freq, next);
        next += 1;
    }
    let root = order[0];
    let bits = &src[8 + 1024..];
    let mut out = Vec::with_capacity(unpacked);
    let mut pos = 0usize;
    while out.len() < unpacked {
        let mut node = root;
        while node > 0xff {
            let Some(&byte) = bits.get(pos / 8) else {
                return Err("GRD: Huffman data is cut short".into());
            };
            let bit = byte >> (pos % 8) & 1;
            pos += 1;
            node = if bit != 0 { kids[node].1 } else { kids[node].0 };
        }
        out.push(node as u8);
    }
    Ok(out)
}

/// A marker byte (at 8) starts a back reference: distance:u8 count:u8, or the marker itself
/// when the distance is the marker; distances above it are one too large. Data from 12.
fn lz77(src: &[u8], n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    let Some(&special) = src.get(8) else {
        return vec![0; n];
    };
    let mut i = 12;
    while out.len() < n && i < src.len() {
        let b = src[i];
        i += 1;
        if b != special {
            out.push(b);
            continue;
        }
        let Some(&dist) = src.get(i) else { break };
        i += 1;
        if dist == special {
            out.push(dist);
            continue;
        }
        let Some(&count) = src.get(i) else { break };
        i += 1;
        let dist = if dist > special { dist - 1 } else { dist } as usize;
        if dist == 0 || dist > out.len() {
            break;
        }
        for _ in 0..count {
            out.push(out[out.len() - dist]);
        }
    }
    out.resize(n, 0);
    out
}
