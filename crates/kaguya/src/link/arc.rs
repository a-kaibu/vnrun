//! LINK archives (bgd.arc, spd.arc, scr.arc, ...) and their pictures. The layout, the BMR
//! compression and the XOR with the key of params.dat follow GARbro's ArcLINK.cs (morkt, MIT):
//!
//! ```text
//! "LINK" '6' ...; entries from 8 + byte[7]:
//!   u32 size (from the entry's start; 0 ends), u16 flags, 7 bytes, u16 name length, UTF-16 name,
//!   the data up to start + size
//! ```
//!
//! flags & 3 with "BMR" at the start: packed (Huffman, move to front, a Burrows-Wheeler
//! inverse and an RLE with a step); flags & 4: the pixels are XORed with the key, cycling,
//! after the header (0x18 for AP-2 / AP-3, 0x36 for BM, 0xC for AP).

use std::collections::HashMap;

use nexas::files::Files;
use nexas::gfx::Image;

#[derive(Clone)]
pub struct Entry {
    pub offset: u64,
    pub size: usize,
    pub flags: u16,
}

pub struct LinkArc {
    file: String,
    /// by the name without its extension
    entries: HashMap<String, Entry>,
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn i32le(b: &[u8], at: usize) -> Option<i32> {
    u32le(b, at).map(|v| v as i32)
}

/// The name without its extension: the scripts name "X.spd" what spd.arc holds as "X.alp".
pub fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

impl LinkArc {
    pub fn open(files: &mut dyn Files, file: &str) -> Result<LinkArc, String> {
        let total = files.size(file).ok_or_else(|| format!("no {file}"))?;
        let head = files.read_range(file, 0, 8).ok_or("short LINK")?;
        if !head.starts_with(b"LINK") {
            return Err(format!("{file}: not LINK"));
        }
        let version = head[4].wrapping_sub(b'0');
        if version != 6 {
            return Err(format!("{file}: LINK{version} is not read yet"));
        }
        let mut at = 8 + head[7] as u64;
        let mut entries = HashMap::new();
        while at + 4 < total {
            let h = files
                .read_range(file, at, 15.min((total - at) as usize))
                .ok_or("short LINK entry")?;
            let size = u32le(&h, 0).ok_or("short LINK entry")? as u64;
            if size == 0 {
                break;
            }
            if size < 0x10 {
                return Err(format!("{file}: bad entry at {at}"));
            }
            let flags = u16le(&h, 4).ok_or("short LINK entry")?;
            let n = u16le(&h, 13).ok_or("short LINK entry")? as usize;
            let raw = files
                .read_range(file, at + 15, n)
                .ok_or("short LINK name")?;
            let name: Vec<u16> = raw
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let name = String::from_utf16_lossy(&name);
            let offset = at + 15 + n as u64;
            entries.insert(
                stem(&name).to_string(),
                Entry {
                    offset,
                    size: (at + size).saturating_sub(offset) as usize,
                    flags,
                },
            );
            at += size;
        }
        Ok(LinkArc {
            file: file.to_string(),
            entries,
        })
    }

    pub fn has(&self, stem: &str) -> bool {
        self.entries.contains_key(stem)
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    /// An entry by the name without extension, unpacked and decrypted with `key`.
    pub fn read(&self, files: &mut dyn Files, stem: &str, key: &[u8]) -> Option<Vec<u8>> {
        let e = self.entries.get(stem)?;
        let mut b = files.read_range(&self.file, e.offset, e.size)?;
        if e.flags & 3 != 0 && b.starts_with(b"BMR") {
            b = bmr(&b).ok()?;
        }
        if e.flags & 4 != 0 && !key.is_empty() {
            let start = if b.starts_with(b"AP-2") || b.starts_with(b"AP-3") {
                0x18
            } else if b.starts_with(b"BM") {
                0x36
            } else if b.starts_with(b"AP") {
                0xc
            } else {
                b.len()
            };
            for (i, v) in b.iter_mut().skip(start).enumerate() {
                *v ^= key[i % key.len()];
            }
        }
        Some(b)
    }
}

struct Bits<'a> {
    b: &'a [u8],
    at: usize,
    acc: u32,
    n: u32,
}

impl Bits<'_> {
    fn get(&mut self, k: u32) -> Option<u32> {
        while self.n < k {
            self.acc = (self.acc << 8) | *self.b.get(self.at)? as u32;
            self.at += 1;
            self.n += 8;
        }
        self.n -= k;
        Some((self.acc >> self.n) & ((1 << k) - 1))
    }
}

/// BMR: "BMR", u8 step, i32 final size, i32 key, i32 size, then from 0x14 the bits.
pub fn bmr(d: &[u8]) -> Result<Vec<u8>, String> {
    let step = *d.get(3).ok_or("short BMR")? as usize;
    let final_size = i32le(d, 4).ok_or("short BMR")? as usize;
    let key = i32le(d, 8).ok_or("short BMR")? as usize;
    let size = i32le(d, 12).ok_or("short BMR")? as usize;
    if size > 0x4000_0000 || final_size > 0x4000_0000 {
        return Err("BMR too big".into());
    }
    let mut bits = Bits {
        b: d,
        at: 0x14,
        acc: 0,
        n: 0,
    };
    let mut tree = [[0u16; 256]; 2];
    let mut token = 256u16;
    fn make(bits: &mut Bits, tree: &mut [[u16; 256]; 2], token: &mut u16) -> Option<u16> {
        if bits.get(1)? != 0 {
            let v = *token;
            if v >= 512 {
                return None;
            }
            *token += 1;
            let l = make(bits, tree, token)?;
            let r = make(bits, tree, token)?;
            tree[0][(v - 256) as usize] = l;
            tree[1][(v - 256) as usize] = r;
            Some(v)
        } else {
            Some(bits.get(8)? as u16)
        }
    }
    let root = make(&mut bits, &mut tree, &mut token).ok_or("bad BMR tree")?;
    let mut out = vec![0u8; size];
    for o in out.iter_mut() {
        let mut s = root;
        while s >= 256 {
            let bit = bits.get(1).ok_or("short BMR")?;
            s = tree[bit as usize][(s - 256) as usize];
        }
        *o = s as u8;
    }
    // move to front
    let mut dict: [u8; 256] = std::array::from_fn(|i| i as u8);
    for o in out.iter_mut() {
        let v = *o as usize;
        let c = dict[v];
        dict.copy_within(0..v, 1);
        dict[0] = c;
        *o = c;
    }
    // the inverse transform
    let mut freq = [0usize; 256];
    for &c in &out {
        freq[c as usize] += 1;
    }
    for i in 1..256 {
        freq[i] += freq[i - 1];
    }
    let mut dist = vec![0usize; size];
    for i in (0..size).rev() {
        let v = out[i] as usize;
        freq[v] -= 1;
        dist[freq[v]] = i;
    }
    let mut pos = key;
    let mut res = vec![0u8; size];
    for r in res.iter_mut() {
        pos = *dist.get(pos).ok_or("bad BMR key")?;
        *r = out[pos];
    }
    if step == 0 {
        return Ok(res);
    }
    let mut o = vec![0u8; final_size];
    let mut src = 0;
    let mut next = || -> Result<u8, String> {
        let v = *res.get(src).ok_or("short BMR run")?;
        src += 1;
        Ok(v)
    };
    for i in 0..step.min(final_size) {
        let mut v1 = next()?;
        o[i] = v1;
        let mut dst = i + step;
        while dst < final_size {
            let mut v2 = next()?;
            o[dst] = v2;
            dst += step;
            if v2 == v1 {
                let mut count = next()? as usize;
                if count & 0x80 != 0 {
                    count = next()? as usize + ((count & 0x7f) << 8) + 128;
                }
                while count > 0 && dst < final_size {
                    o[dst] = v2;
                    dst += step;
                    count -= 1;
                }
                if dst < final_size {
                    v2 = next()?;
                    o[dst] = v2;
                    dst += step;
                }
            }
            v1 = v2;
        }
    }
    Ok(o)
}

/// A picture and where its top left goes (AP-2 and AP-3 carry it; the others are at 0, 0).
pub struct Pic {
    pub img: Image,
    pub x: i32,
    pub y: i32,
}

/// AP-2 / AP-3: the tag, i32 x, i32 y, u32 width, u32 height, u32 header size (0x18), then
/// BGRA rows bottom up. AP-0 (the rules): the tag, u32 width, u32 height, then a grey byte a
/// pixel, bottom up, the bright parts changing first (kept inverted here, so that the dark
/// parts go first as with the other engines' rules). BM: a BMP.
pub fn picture(b: &[u8]) -> Result<Pic, String> {
    let ap0 = b.starts_with(b"AP-0");
    if ap0 || b.starts_with(b"AP-2") || b.starts_with(b"AP-3") {
        let (x, y, w, h, start) = if ap0 {
            let w = u32le(b, 4).ok_or("short AP")?;
            let h = u32le(b, 8).ok_or("short AP")?;
            (0, 0, w, h, 12)
        } else {
            (
                i32le(b, 4).ok_or("short AP")?,
                i32le(b, 8).ok_or("short AP")?,
                u32le(b, 12).ok_or("short AP")?,
                u32le(b, 16).ok_or("short AP")?,
                (u32le(b, 20).unwrap_or(0x18) as usize).max(0x18),
            )
        };
        if w == 0 || h == 0 || w > 0x4000 || h > 0x4000 {
            return Err("bad AP size".into());
        }
        let n = if ap0 { 1 } else { 4 };
        let stride = w as usize * n;
        let data = b
            .get(start..start + stride * h as usize)
            .ok_or("short AP")?;
        let mut img = Image::new(w, h);
        for y in 0..h as usize {
            let s = &data[(h as usize - 1 - y) * stride..][..stride];
            let d = &mut img.px[y * w as usize * 4..][..w as usize * 4];
            if n == 1 {
                for (d, &g) in d.chunks_exact_mut(4).zip(s) {
                    d.copy_from_slice(&[!g, !g, !g, 255]);
                }
            } else {
                for (d, s) in d.chunks_exact_mut(4).zip(s.chunks_exact(4)) {
                    d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
                }
            }
        }
        return Ok(Pic { img, x, y });
    }
    if b.starts_with(b"BM") {
        let off = u32le(b, 10).ok_or("short BMP")? as usize;
        let w = i32le(b, 18).ok_or("short BMP")?;
        let h = i32le(b, 22).ok_or("short BMP")?;
        let bpp = u16le(b, 28).ok_or("short BMP")?;
        if w <= 0 || h == 0 || w > 0x4000 || h.abs() > 0x4000 {
            return Err("bad BMP size".into());
        }
        let (uh, flip) = (h.unsigned_abs() as usize, h > 0);
        let w = w as usize;
        let n = (bpp / 8) as usize;
        if !(n == 1 || n == 3 || n == 4) {
            return Err(format!("BMP of {bpp} bits"));
        }
        let pal = |i: u8| -> [u8; 4] {
            let p = 14 + u32le(b, 14).unwrap_or(40) as usize + i as usize * 4;
            match b.get(p..p + 3) {
                Some(c) => [c[2], c[1], c[0], 255],
                None => [i, i, i, 255],
            }
        };
        let stride = (w * n + 3) & !3;
        let mut img = Image::new(w as u32, uh as u32);
        for y in 0..uh {
            let row = if flip { uh - 1 - y } else { y };
            let Some(s) = b.get(off + row * stride..off + row * stride + w * n) else {
                break;
            };
            let d = &mut img.px[y * w * 4..][..w * 4];
            for (d, s) in d.chunks_exact_mut(4).zip(s.chunks_exact(n)) {
                let px = match n {
                    1 => pal(s[0]),
                    3 => [s[2], s[1], s[0], 255],
                    _ => [s[2], s[1], s[0], s[3]],
                };
                d.copy_from_slice(&px);
            }
        }
        return Ok(Pic { img, x: 0, y: 0 });
    }
    Err(format!(
        "unknown picture {:?}",
        String::from_utf8_lossy(&b[..b.len().min(4)])
    ))
}
