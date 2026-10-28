//! The `fPK` archives of Ivory's engine (`misc/*.pk`, `sound/music.pk`, `voice/voice.pk`): the
//! file size, then sections (id, size, header size): `cLST` the index (name offset, offset,
//! size, scrambled with a key), `cNAM` the names (scrambled the same way), `cDAT` the data.
//! Follows GARbro's `Ivory/ArcPK.cs` (MIT, by morkt). `fPK2` has 64-bit numbers.

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Debug, Clone)]
pub struct Entry {
    pub pack: String,
    pub name: String,
    pub offset: u64,
    pub size: u32,
}

/// Every archive of the game folder, looked up by file name (case-insensitive), with or
/// without its folder (`SYSTEM/FRAME_BASE.SG` or `FRAME_BASE.SG`).
#[derive(Default)]
pub struct Packs {
    files: HashMap<String, Entry>,
    /// a name without its folder: the full name
    base: HashMap<String, String>,
}

/// The archives Ivory's games keep beside the exe.
pub const PACKS: &[&str] = &[
    "misc/script.pk",
    "misc/grp.pk",
    "misc/bg.pk",
    "misc/chr.pk",
    "misc/se.pk",
    "misc/ev.pk",
    "misc/etc.pk",
    "sound/music.pk",
    "sound/env.pk",
    "voice/voice.pk",
];

/// Unscrambles 32-bit words: each word's bit pairs are swapped where a control word (from the
/// key) has a 1, then the word is xored with the key, both turning 32 times.
pub fn decrypt(data: &mut [u8], mut seed: u32) {
    let mut ctl = [0u16; 32];
    let mut key = [0u32; 32];
    for i in 0..32 {
        let mut code: u32 = 0;
        let mut k = seed;
        for _ in 0..16 {
            code = ((k ^ (k >> 1)) << 15) | ((code & 0xffff) >> 1);
            k >>= 2;
        }
        key[i] = seed;
        ctl[i] = code as u16;
        seed = seed.rotate_left(1);
    }
    for (i, w) in data.chunks_exact_mut(4).enumerate() {
        let s = u32::from_le_bytes(w.try_into().unwrap());
        let mut code = ctl[i & 31];
        let mut d = 0u32;
        let (mut v3, mut v2, mut v1) = (3u32, 2u32, 1u32);
        for _ in 0..16 {
            if code & 1 != 0 {
                d |= ((s & v1) << 1) | ((s >> 1) & (v2 >> 1));
            } else {
                d |= s & v3;
            }
            code >>= 1;
            v3 <<= 2;
            v2 <<= 2;
            v1 <<= 2;
        }
        w.copy_from_slice(&(d ^ key[i & 31]).to_le_bytes());
    }
}

/// Reads an archive's index (the sections before `cDAT`'s data).
pub fn index(files: &mut dyn Files, pack: &str) -> Option<Vec<Entry>> {
    let total = files.size(pack)?;
    let head = files.read_range(pack, 0, 4)?;
    let wide = match &head[..] {
        b"fPK " => false,
        b"fPK2" => true,
        _ => return None,
    };
    let n = if wide { 8 } else { 4 };
    let num = |b: &[u8], at: usize| -> Option<u64> {
        let s = b.get(at..at + n)?;
        Some(if wide {
            u64::from_le_bytes(s.try_into().ok()?)
        } else {
            u32::from_le_bytes(s.try_into().ok()?) as u64
        })
    };
    let mut pos = 4 + n as u64;
    let mut list: Vec<(u64, u64, u64)> = Vec::new();
    let mut names = Vec::new();
    let mut base = 0;
    while pos + 4 + 2 * n as u64 <= total {
        let h = files.read_range(pack, pos, 4 + 2 * n + 12)?;
        let size = num(&h, 4)?;
        let head_size = num(&h, 4 + n)?;
        if size < 4 || head_size > size {
            return None;
        }
        let content = pos + head_size;
        match &h[..4] {
            b"cLST" => {
                let o = 4 + 2 * n;
                let count = u32::from_le_bytes(h.get(o + 4..o + 8)?.try_into().ok()?) as usize;
                let key = u32::from_le_bytes(h.get(o + 8..o + 12)?.try_into().ok()?);
                let mut b = files.read_range(pack, content, (size - head_size) as usize)?;
                decrypt(&mut b, key);
                for i in 0..count {
                    let at = i * 3 * n;
                    list.push((num(&b, at)?, num(&b, at + n)?, num(&b, at + 2 * n)?));
                }
            }
            b"cNAM" => {
                let o = 4 + 2 * n;
                let key = u32::from_le_bytes(h.get(o + 4..o + 8)?.try_into().ok()?);
                let mut b = files.read_range(pack, content, (size - head_size) as usize)?;
                decrypt(&mut b, key);
                names = b;
            }
            b"cDAT" => {
                base = content;
                break;
            }
            _ => {}
        }
        pos += size;
    }
    if base == 0 {
        return None;
    }
    let out = list
        .into_iter()
        .map(|(no, off, size)| {
            let s = names.get(no as usize..).unwrap_or(&[]);
            let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            Entry {
                pack: pack.to_string(),
                name: nexas::sjis::decode(&s[..end]),
                offset: base + off,
                size: size as u32,
            }
        })
        .collect();
    Some(out)
}

impl Packs {
    pub fn open(files: &mut dyn Files) -> Packs {
        let mut p = Packs::default();
        for pack in PACKS {
            for e in index(files, pack).unwrap_or_default() {
                let full = e.name.to_lowercase();
                if let Some((_, b)) = full.rsplit_once('/') {
                    p.base.entry(b.to_string()).or_insert(full.clone());
                }
                p.files.entry(full).or_insert(e);
            }
        }
        p
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        let n = name.to_lowercase();
        self.files
            .get(&n)
            .or_else(|| self.files.get(self.base.get(&n)?))
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.files.values()
    }

    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = self.entry(name)?;
        files.read_range(&e.pack, e.offset, e.size as usize)
    }
}
