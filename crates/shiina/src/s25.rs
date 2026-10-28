//! S25 pictures, after GARbro's `ImageS25.cs` (MIT): "S25\0", a count, then the offset of
//! each frame (0: none). A frame is its width, height, x and y (where it goes on the screen),
//! a flags word (bit 31: rows are coded as differences, shared between frames) and the offset
//! of each row. A row is a u16 length, then runs: a u16 (method << 13 | skip << 11 | count,
//! count 0: a u32 follows) aligned to 2 bytes; method 2 BGR pixels, 3 one BGR repeated,
//! 4 ABGR pixels, 5 one ABGR repeated, others clear pixels.

use std::collections::HashMap;

use nexas::gfx::Image;

fn u16at(b: &[u8], at: usize) -> usize {
    b.get(at..at + 2)
        .map_or(0, |s| u16::from_le_bytes([s[0], s[1]]) as usize)
}

fn u32at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4)
        .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
}

/// A frame: the picture and where its top left goes.
pub struct Frame {
    pub image: Image,
    pub x: i32,
    pub y: i32,
}

/// The offsets of the frames (0: an empty frame).
pub fn frame_offsets(b: &[u8]) -> Vec<u32> {
    if b.len() < 8 || &b[..4] != b"S25\0" {
        return Vec::new();
    }
    let n = (u32at(b, 4) as usize).min(0xfffff).min((b.len() - 8) / 4);
    (0..n).map(|i| u32at(b, 8 + i * 4)).collect()
}

/// A row's bytes: from its offset, the u16 length, a padding byte when the offset is odd.
fn row_bytes(b: &[u8], offset: usize) -> Vec<u8> {
    let mut len = u16at(b, offset);
    let mut at = offset + 2;
    if offset & 1 != 0 {
        at += 1;
        len = len.saturating_sub(1);
    }
    let end = (at + len).min(b.len());
    b.get(at..end).map_or_else(Vec::new, |s| s.to_vec())
}

/// The runs of a row: (method, count, position of its pixel bytes).
fn runs(line: &[u8], width: usize) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let mut pos = 0;
    let mut x = width;
    while x > 0 && pos + 2 <= line.len() {
        if pos & 1 != 0 {
            pos += 1;
        }
        let word = u16at(line, pos);
        pos += 2;
        let method = word >> 13;
        pos += (word >> 11) & 3;
        let mut count = word & 0x7ff;
        if count == 0 {
            count = u32at(line, pos) as usize;
            pos += 4;
        }
        let count = count.min(x);
        x -= count;
        out.push((method, count, pos));
        pos += match method {
            2 => count * 3,
            3 => 3,
            4 => count * 4,
            5 => 4,
            _ => 0,
        };
    }
    out
}

fn unpack_line(line: &[u8], width: usize, out: &mut [u8]) {
    let px = |i: usize| line.get(i).copied().unwrap_or(0);
    let mut dst = 0;
    for (method, count, p) in runs(line, width) {
        for i in 0..count {
            let d = (dst + i) * 4;
            if d + 4 > out.len() {
                break;
            }
            let (b, g, r, a) = match method {
                2 => (px(p + i * 3), px(p + i * 3 + 1), px(p + i * 3 + 2), 255),
                3 => (px(p), px(p + 1), px(p + 2), 255),
                4 => (
                    px(p + i * 4 + 1),
                    px(p + i * 4 + 2),
                    px(p + i * 4 + 3),
                    px(p + i * 4),
                ),
                5 => (px(p + 1), px(p + 2), px(p + 3), px(p)),
                _ => continue,
            };
            out[d..d + 4].copy_from_slice(&[r, g, b, a]);
        }
        dst += count;
    }
}

/// Undoes the differences of a shared row, once for each frame that uses it (as the engine
/// does when it draws them in turn).
fn integrate(line: &mut [u8], width: usize, repeat: usize) {
    for (method, count, p) in runs(line, width) {
        let step = match method {
            2 => 3,
            4 => 4,
            _ => continue,
        };
        let n = count * step;
        for _ in 0..repeat {
            let mut i = step;
            while i < n && p + i < line.len() {
                line[p + i] = line[p + i].wrapping_add(line[p + i - step]);
                i += 1;
            }
        }
    }
}

/// Decodes frame `k`.
pub fn frame(b: &[u8], k: usize) -> Option<Frame> {
    let offsets = frame_offsets(b);
    let at = *offsets.get(k)? as usize;
    if at == 0 || at + 0x14 > b.len() {
        return None;
    }
    let w = u32at(b, at) as usize;
    let h = u32at(b, at + 4) as usize;
    let x = u32at(b, at + 8) as i32;
    let y = u32at(b, at + 12) as i32;
    let incremental = u32at(b, at + 16) & 0x8000_0000 != 0;
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return None;
    }
    let origin = at + 0x14;
    let rows: Vec<u32> = (0..h).map(|i| u32at(b, origin + i * 4)).collect();
    let mut image = Image::new(w as u32, h as u32);
    let row = w * 4;
    if !incremental {
        for (y, &r) in rows.iter().enumerate() {
            let line = row_bytes(b, r as usize);
            unpack_line(&line, w, &mut image.px[y * row..(y + 1) * row]);
        }
    } else {
        // how many times each row is used, by this frame and the others
        let mut count: HashMap<u32, usize> = HashMap::new();
        for &r in &rows {
            *count.entry(r).or_default() += 1;
        }
        for &o in &offsets {
            let o = o as usize;
            if o == 0 || o == at {
                continue;
            }
            let oh = u32at(b, o + 4) as usize;
            for i in 0..oh.min(8192) {
                let r = u32at(b, o + 0x14 + i * 4);
                if let Some(c) = count.get_mut(&r) {
                    *c += 1;
                }
            }
        }
        let mut lines: HashMap<u32, Vec<u8>> = HashMap::new();
        for (y, &r) in rows.iter().enumerate() {
            let line = lines.entry(r).or_insert_with(|| {
                let mut l = row_bytes(b, r as usize);
                integrate(&mut l, w, count[&r]);
                l
            });
            unpack_line(line, w, &mut image.px[y * row..(y + 1) * row]);
        }
    }
    Some(Frame { image, x, y })
}

/// The number of frames.
pub fn count(b: &[u8]) -> usize {
    frame_offsets(b).len()
}
