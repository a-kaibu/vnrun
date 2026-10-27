//! LiveMaker archives ("vf"): appended to the game's exe (the exe ends with the archive's
//! start as u32 and "lv"), or a standalone `game.dat`, or an index `game.ext` with the data in
//! `game.dat`, `game.001`, `game.002`, ... (1 GiB each).
//!
//! ```text
//! "vf" version:u32 count:u32
//! count x { len:u32 name[len] }            names, XOR the key stream (version >= 100)
//! (count+1) x { low:u32 high:u32 }         offsets from the archive start, each half XOR its own
//!                                          key stream; only bit 31 of high counts (bit 63)
//! count x { kind:u8 }                      0 zlib, 1 stored, 2 scrambled, 3 scrambled + zlib
//! count x u32, count x { checksum:u32 }, count x { encrypted:u8 }
//! ```
//!
//! The key stream: k = k*5 + 0x75d6ee39 (u32), starting from 0; a name byte takes the low byte of
//! one key, an offset half a whole key.

use std::collections::HashMap;

use nexas::files::Files;

const PART: u64 = 1 << 30;

#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub offset: u64,
    pub size: u64,
    pub kind: u8,
}

pub struct Archive {
    /// The files holding the data, 1 GiB each after the first, and where the archive starts in
    /// the first.
    parts: Vec<String>,
    base: u64,
    pub entries: HashMap<String, Entry>,
    /// names as they are in the archive, in order
    pub names: Vec<String>,
}

struct Keys(u32);

impl Keys {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(5).wrapping_add(0x75d6_ee39);
        self.0
    }
}

/// How names are looked up: lower case, `/` as `\`.
pub fn key(name: &str) -> String {
    name.replace('/', "\\").to_lowercase()
}

impl Archive {
    /// Finds the game's archive among the files at the top of the game folder.
    pub fn open(files: &mut dyn Files) -> Result<Archive, String> {
        let names = files.list();
        let find = |ext: &str| {
            names
                .iter()
                .find(|n| n.to_lowercase().ends_with(ext))
                .cloned()
        };
        // An index with its data beside it.
        if let Some(ext) = find(".ext") {
            let stem = &ext[..ext.len() - 4];
            let index = files.read(&ext).ok_or(format!("{ext}: cannot read"))?;
            let mut parts = vec![format!("{stem}.dat")];
            parts.extend(split_parts(&names, stem));
            return Archive::parse(&index, parts, 0);
        }
        // A standalone archive.
        for n in &names {
            if !n.to_lowercase().ends_with(".dat") {
                continue;
            }
            if files.read_range(n, 0, 2).as_deref() == Some(b"vf") {
                let size = files.size(n).unwrap_or(0);
                let index = files
                    .read_range(n, 0, size.min(64 << 20) as usize)
                    .ok_or(format!("{n}: cannot read"))?;
                let stem = &n[..n.len() - 4];
                let mut parts = vec![n.clone()];
                parts.extend(split_parts(&names, stem));
                return Archive::parse(&index, parts, 0);
            }
        }
        // An exe with the archive at its end.
        for n in &names {
            if !n.to_lowercase().ends_with(".exe") {
                continue;
            }
            let Some(size) = files.size(n) else { continue };
            if size < 6 {
                continue;
            }
            let Some(tail) = files.read_range(n, size - 6, 6) else {
                continue;
            };
            if &tail[4..] != b"lv" {
                continue;
            }
            let base = u32::from_le_bytes(tail[..4].try_into().unwrap()) as u64;
            let len = (size - base).min(64 << 20) as usize;
            let index = files
                .read_range(n, base, len)
                .ok_or(format!("{n}: cannot read"))?;
            return Archive::parse(&index, vec![n.clone()], base);
        }
        Err("no LiveMaker archive (an exe ending in \"lv\", game.dat or game.ext)".into())
    }

    fn parse(b: &[u8], parts: Vec<String>, base: u64) -> Result<Archive, String> {
        let mut r = Reader { b, at: 0 };
        if r.bytes(2)? != b"vf" {
            return Err("not a LiveMaker archive".into());
        }
        let version = r.u32()?;
        let count = r.u32()? as usize;
        if count > 1_000_000 {
            return Err("archive: too many files".into());
        }
        let mut keys = Keys(0);
        let mut names = Vec::with_capacity(count);
        for _ in 0..count {
            let n = r.u32()? as usize;
            let mut raw = r.bytes(n)?.to_vec();
            if version >= 100 {
                for c in &mut raw {
                    *c ^= keys.next() as u8;
                }
            }
            names.push(nexas::sjis::decode(&raw));
        }
        let (mut low_keys, mut high_keys) = (Keys(0), Keys(0));
        let mut offsets = Vec::with_capacity(count + 1);
        for _ in 0..=count {
            let mut low = r.u32()?;
            let mut high = r.u32()?;
            if version >= 100 {
                low ^= low_keys.next();
            }
            if version >= 101 {
                high ^= high_keys.next();
            }
            offsets.push(((high as u64 & 0x8000_0000) << 32) | low as u64);
        }
        let kinds = r.bytes(count)?.to_vec();
        let mut entries = HashMap::with_capacity(count);
        for (i, name) in names.iter().enumerate() {
            entries.insert(
                key(name),
                Entry {
                    offset: offsets[i],
                    size: offsets[i + 1].saturating_sub(offsets[i]),
                    kind: kinds[i],
                },
            );
        }
        Ok(Archive {
            parts,
            base,
            entries,
            names,
        })
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.contains_key(&key(name))
    }

    /// A file of the archive, unpacked.
    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = *self.entries.get(&key(name))?;
        let mut data = Vec::with_capacity(e.size as usize);
        let mut at = self.base + e.offset;
        while (data.len() as u64) < e.size {
            let (part, off) = if self.parts.len() > 1 {
                ((at / PART) as usize, at % PART)
            } else {
                (0, at)
            };
            let want = (e.size - data.len() as u64).min(if self.parts.len() > 1 {
                PART - off
            } else {
                u64::MAX
            });
            let chunk = files.read_range(self.parts.get(part)?, off, want as usize)?;
            if chunk.is_empty() {
                return None;
            }
            at += chunk.len() as u64;
            data.extend_from_slice(&chunk);
        }
        if e.kind == 2 || e.kind == 3 {
            data = unscramble(&data)?;
        }
        if e.kind == 0 || e.kind == 3 {
            data = miniz_oxide::inflate::decompress_to_vec_zlib(&data)
                .map_err(|err| eprintln!("{name}: {err:?}"))
                .ok()?;
        }
        Some(data)
    }
}

fn split_parts(names: &[String], stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    for i in 1..100 {
        let want = format!("{stem}.{i:03}").to_lowercase();
        match names.iter().find(|n| n.to_lowercase() == want) {
            Some(n) => out.push(n.clone()),
            None => break,
        }
    }
    out
}

/// LiveMaker Pro's scrambling: `chunk:i32 seed:u32`, then the chunks of the file in an order
/// drawn from a random generator.
fn unscramble(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 8 {
        return Some(b.to_vec());
    }
    let chunk = i32::from_le_bytes(b[..4].try_into().unwrap());
    let seed = u32::from_le_bytes(b[4..8].try_into().unwrap());
    if chunk <= 0 {
        return None;
    }
    let chunk = chunk as usize;
    let body = &b[8..];
    let n = body.len().div_ceil(chunk);
    let mut out = Vec::with_capacity(body.len());
    for i in random_order(n, seed ^ 0xf8ea) {
        let at = i * chunk;
        out.extend_from_slice(&body[at..(at + chunk).min(body.len())]);
    }
    Some(out)
}

struct Random([u32; 5]);

impl Random {
    fn new(seed: u32) -> Random {
        let mut s = if seed == 0 { u32::MAX } else { seed };
        let mut state = [0; 5];
        for v in &mut state {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            *v = s;
        }
        let mut r = Random(state);
        for _ in 0..19 {
            r.next();
        }
        r
    }

    fn next(&mut self) -> u32 {
        let s = &mut self.0;
        let x = s[3] as u64 * 0x7dd4_ffc7
            + s[2] as u64 * 0x5d4
            + s[1] as u64 * 0x6f0
            + s[0] as u64 * 0x13fb
            + s[4] as u64;
        s[4] = (x >> 32) as u32;
        s[3] = s[2];
        s[2] = s[1];
        s[1] = s[0];
        s[0] = x as u32;
        s[0]
    }

    /// low..=high
    fn range(&mut self, low: usize, high: usize) -> usize {
        low + (self.next() as f64 / 4294967296.0 * (high - low + 1) as f64) as usize
    }
}

/// Where each of `n` chunks came from: the i-th chunk read is chunk `order[i]` of the stream.
fn random_order(n: usize, seed: u32) -> Vec<usize> {
    let mut r = Random::new(seed);
    let mut left: Vec<usize> = (0..n).collect();
    let mut seq = vec![0; n];
    let mut i = 0;
    while !left.is_empty() {
        let k = if left.len() == 1 {
            0
        } else {
            r.range(0, left.len() - 2)
        };
        seq[left.remove(k)] = i;
        i += 1;
    }
    seq
}

pub(crate) struct Reader<'a> {
    pub b: &'a [u8],
    pub at: usize,
}

impl<'a> Reader<'a> {
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| format!("cut short at {:#x}", self.at))?;
        let out = &self.b[self.at..end];
        self.at = end;
        Ok(out)
    }
    pub fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }
    pub fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    pub fn str(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        Ok(nexas::sjis::decode(self.bytes(n)?))
    }
}
