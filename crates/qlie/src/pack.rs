//! QLIE `.pack` archives (after GARbro's ArcFormats/Qlie): a trailer "FilePackVerX.Y" with the
//! file count and the index offset; names and data XOR-encrypted by version. 1.0/2.0 use a fixed
//! key, 3.0 and 3.1 a hash of 0x100 bytes before the trailer; 3.1 names are UTF-16 and its
//! second method mixes in a key file (key.fkey, the exe's RESKEY resource, or the archive's own
//! `pack_keyfile` decrypted with those). Data may then be packed with the "1PC" byte-pair code.

use nexas::files::Files;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    raw_name: Vec<u8>,
    pub offset: u64,
    pub size: u32,
    pub unpacked: u32,
    pub packed: bool,
    pub method: i32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Version {
    V1,
    V2,
    V3,
    V31,
}

pub struct Pack {
    pub path: String,
    pub version: Version,
    pub entries: Vec<Entry>,
    arc_key: u32,
    /// key.fkey / the exe's RESKEY, or the archive's pack_keyfile decrypted with it
    key_file: Option<Vec<u8>>,
    /// 3.0: the game's own key (the exe's TFORM1 IconKeyImage), mixed in with the key file
    game_key: Option<Vec<u8>>,
    /// 2.0 data in a 1.0 archive
    v2_data: bool,
}

// ---- MMX lane arithmetic

fn padd(x: u64, y: u64, bits: u32) -> u64 {
    let lanes = 64 / bits;
    let mask = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let mut out = 0;
    for i in 0..lanes {
        let s = i * bits;
        out |= (((x >> s) & mask).wrapping_add((y >> s) & mask) & mask) << s;
    }
    out
}

fn padd_b(x: u64, y: u64) -> u64 {
    padd(x, y, 8)
}
fn padd_w(x: u64, y: u64) -> u64 {
    padd(x, y, 16)
}
fn padd_d(x: u64, y: u64) -> u64 {
    padd(x, y, 32)
}
fn psll_d(x: u64, n: u32) -> u64 {
    let lo = (x as u32) << n;
    let hi = ((x >> 32) as u32) << n;
    lo as u64 | (hi as u64) << 32
}
fn psrl_d(x: u64, n: u32) -> u64 {
    let lo = (x as u32) >> n;
    let hi = ((x >> 32) as u32) >> n;
    lo as u64 | (hi as u64) << 32
}

fn qwords(data: &[u8]) -> impl Iterator<Item = u64> + '_ {
    data.chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
}

/// The key of a 3.x archive: a hash of its 0x100 key bytes.
fn hash_v3(data: &[u8]) -> u32 {
    let (mut hash, mut key) = (0u64, 0u64);
    for q in qwords(data) {
        hash = padd_w(hash, 0x0307_0307_0307_0307);
        key = padd_w(key, q ^ hash);
    }
    (key ^ (key >> 32)) as u32
}

fn hash_v31(data: &[u8]) -> u32 {
    let (mut hash, mut key) = (0u64, 0u64);
    for q in qwords(data) {
        hash = padd_w(hash, 0xA357_93A7_A357_93A7);
        key = padd_w(key, q ^ hash);
        key = psll_d(key, 3) | psrl_d(key, 29);
    }
    let w = |s: u32| (key >> s) as u16 as i16 as i32;
    (w(0).wrapping_mul(w(32))).wrapping_add(w(16).wrapping_mul(w(48))) as u32
}

/// QLIE's Mersenne Twister: 64 words, its own shift (`|` after `>> 1`) and tempering.
struct Mt {
    mt: [u32; 64],
    i: usize,
}

impl Mt {
    fn new(seed: u32) -> Mt {
        let mut mt = [0u32; 64];
        mt[0] = seed;
        for i in 1..64 {
            mt[i] = 0x6611_BC19u32
                .wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        Mt { mt, i: 64 }
    }

    fn xor_state(&mut self, b: &[u8]) {
        for (k, c) in b.chunks_exact(4).take(64).enumerate() {
            self.mt[k] ^= u32::from_le_bytes(c.try_into().unwrap());
        }
    }

    fn rand(&mut self) -> u32 {
        const N: usize = 64;
        const M: usize = 39;
        let mag = |y: u32| if y & 1 != 0 { 0x9908_B0DF } else { 0 };
        let mt = &mut self.mt;
        if self.i >= N {
            let mut kk = 0;
            while kk < N - M {
                let y = (mt[kk] & 0x8000_0000) | ((mt[kk + 1] & 0x7FFF_FFFF) >> 1);
                mt[kk] = mt[kk + M] ^ y ^ mag(mt[kk + 1]);
                kk += 1;
            }
            while kk < N - 1 {
                let y = (mt[kk] & 0x8000_0000) | ((mt[kk + 1] & 0x7FFF_FFFF) >> 1);
                mt[kk] = mt[kk + M - N] ^ y ^ mag(mt[kk + 1]);
                kk += 1;
            }
            let y = (mt[N - 1] & 0x8000_0000) | ((mt[0] & 0x7FFF_FFFF) >> 1);
            mt[N - 1] = mt[M - 1] ^ y ^ mag(mt[kk - 1]);
            self.i = 0;
        }
        let mut y = mt[self.i];
        self.i += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9C4F_88E3;
        y ^= (y << 15) & 0xE7F7_0000;
        y ^= y >> 18;
        y
    }

    fn rand64(&mut self) -> u64 {
        let lo = self.rand() as u64;
        lo | (self.rand() as u64) << 32
    }
}

/// Decrypts 1.0/2.0 data (and 3.0 without a key file).
fn decrypt_v2(data: &mut [u8], arc_key: u32, with_length: bool) {
    let mut hash = 0xA73C_5F9D_A73C_5F9Du64;
    let base = if with_length {
        (data.len() as u32).wrapping_add(arc_key)
    } else {
        arc_key
    };
    let mut xor = (base ^ 0xFEC9_753E) as u64;
    xor |= xor << 32;
    for c in data.chunks_exact_mut(8) {
        hash = padd_d(hash, 0xCE24_F523_CE24_F523) ^ xor;
        xor = u64::from_le_bytes((&*c).try_into().unwrap()) ^ hash;
        c.copy_from_slice(&xor.to_le_bytes());
    }
}

fn mix(data: &mut [u8], mut hash64: u64, mut table: impl FnMut(usize) -> u64) {
    for (i, c) in data.chunks_exact_mut(8).enumerate() {
        let t = table(i);
        hash64 = padd_d(hash64 ^ t, t);
        let d = u64::from_le_bytes((&*c).try_into().unwrap()) ^ hash64;
        c.copy_from_slice(&d.to_le_bytes());
        hash64 = padd_b(hash64, d) ^ d;
        hash64 = padd_w(psll_d(hash64, 1), d);
    }
}

/// 3.0 data with a key file.
fn decrypt_v3_key(
    data: &mut [u8],
    raw_name: &[u8],
    arc_key: u32,
    key_file: &[u8],
    game_key: Option<&[u8]>,
) {
    let len = data.len() as u32;
    let (mut hash, mut seed) = (0x85F532u32, 0x33F641u32);
    for (i, &b) in raw_name.iter().enumerate() {
        hash = hash.wrapping_add((i as u32 & 0xFF) * b as u32);
        seed ^= hash;
    }
    seed = seed.wrapping_add(
        arc_key
            ^ (7u32
                .wrapping_mul(len & 0xFF_FFFF)
                .wrapping_add(len)
                .wrapping_add(hash)
                .wrapping_add(hash ^ len ^ 0x8F32DC)),
    );
    seed = 9u32.wrapping_mul(seed & 0xFF_FFFF);
    if game_key.is_some() {
        seed ^= 0x453A;
    }
    let mut mt = Mt::new(seed);
    mt.xor_state(key_file);
    if let Some(g) = game_key {
        mt.xor_state(g);
    }
    let table: Vec<u64> = (0..16).map(|_| mt.rand64()).collect();
    for _ in 0..9 {
        mt.rand();
    }
    let hash64 = mt.rand64();
    let t0 = (mt.rand() & 0xF) as usize;
    mix(data, hash64, |i| table[(t0 + i) & 0xF]);
}

fn table_v31(n: usize, mut seed: u32, key: u32) -> Vec<u32> {
    (0..n)
        .map(|_| {
            let x = key as u64 * (seed ^ key) as u64;
            seed = ((x >> 32) as u32).wrapping_add(x as u32);
            seed
        })
        .collect()
}

fn name_hash(name: &str, mut hash: u32, mut seed: u32) -> (u32, u32) {
    for (i, c) in name.encode_utf16().enumerate() {
        hash = hash.wrapping_add((c as u32) << (i & 7));
        seed ^= hash;
    }
    (hash, seed)
}

fn decrypt_v31(data: &mut [u8], e: &Entry, arc_key: u32, key_file: Option<&[u8]>) {
    let len = data.len() as u32;
    if e.method == 1 {
        let (hash, seed) = name_hash(&e.name.replace('/', "\\"), 0x85F532, 0x33F641);
        let seed = seed.wrapping_add(
            arc_key
                ^ (7u32
                    .wrapping_mul(len & 0xFF_FFFF)
                    .wrapping_add(len)
                    .wrapping_add(hash)
                    .wrapping_add(hash ^ len ^ 0x8F32DC)),
        );
        let t = table_v31(0x20, 9u32.wrapping_mul(seed & 0xFF_FFFF), 0x8DF2_1431);
        let k0 = 2 * (t[0xD] & 0xF) as usize;
        let h = t[6] as u64 | (t[7] as u64) << 32;
        mix(data, h, |i| {
            let k = (k0 + 2 * i) & 0x1F;
            t[k] as u64 | (t[k + 1] as u64) << 32
        });
    } else {
        let (hash, seed) = name_hash(&e.name.replace('/', "\\"), 0x86F7E2, 0x4437F1);
        let seed = seed.wrapping_add(
            arc_key
                ^ (13u32
                    .wrapping_mul(len & 0xFF_FFFF)
                    .wrapping_add(len)
                    .wrapping_add(hash)
                    .wrapping_add(hash ^ len ^ 0x56E213)),
        );
        let t = table_v31(0x20, 13u32.wrapping_mul(seed & 0xFF_FFFF), 0x8A77_F473);
        let mut kd = vec![0u8; 0x400];
        for i in 0..0x100i32 {
            let h = if i % 3 != 0 {
                (i + 7).wrapping_mul(-(i + 3))
            } else {
                (i + 7) * (i + 3)
            };
            kd[i as usize * 4..i as usize * 4 + 4].copy_from_slice(&h.to_le_bytes());
        }
        if let Some(kf) = key_file.filter(|k| k.len() >= 128) {
            let mut k = kf[49] as usize % 73 + 128;
            let l = kf[79] as usize % 7 + 7;
            for b in kd.iter_mut() {
                k = (k + l) % kf.len();
                *b ^= kf[k];
            }
        }
        let k0 = (8 * (t[8] as usize & 0xD)) & 0x7F;
        let h = t[6] as u64 | (t[7] as u64) << 32;
        mix(data, h, |i| {
            let k = (k0 + i) & 0x7F;
            let ti = 2 * (k & 0xF);
            let tv = t[ti] as u64 | (t[ti + 1] as u64) << 32;
            tv ^ u64::from_le_bytes(kd[8 * k..8 * k + 8].try_into().unwrap())
        });
    }
}

/// Unpacks "1PC\xFF" data.
pub fn decompress(input: &[u8]) -> Option<Vec<u8>> {
    if input.get(..4)? != b"1PC\xFF" {
        return None;
    }
    let wide = input[4] & 1 != 0;
    let out_len = u32::from_le_bytes(input.get(8..12)?.try_into().ok()?) as usize;
    let mut out = Vec::with_capacity(out_len);
    let mut src = 12;
    let mut node = [[0u8; 256]; 2];
    let mut stack = [0u8; 256];
    while src < input.len() {
        for (i, n) in node[0].iter_mut().enumerate() {
            *n = i as u8;
        }
        let mut i = 0usize;
        while i < 256 {
            let mut count = *input.get(src)? as usize;
            src += 1;
            if count > 127 {
                i += count - 127;
                count = 0;
            }
            if i > 255 {
                break;
            }
            for _ in 0..=count {
                if i > 255 {
                    break;
                }
                node[0][i] = *input.get(src)?;
                src += 1;
                if node[0][i] as usize != i {
                    node[1][i] = *input.get(src)?;
                    src += 1;
                }
                i += 1;
            }
        }
        let mut count = if wide {
            let c = u16::from_le_bytes(input.get(src..src + 2)?.try_into().ok()?) as usize;
            src += 2;
            c
        } else {
            let c = u32::from_le_bytes(input.get(src..src + 4)?.try_into().ok()?) as usize;
            src += 4;
            c
        };
        let mut k = 0usize;
        loop {
            let index = if k > 0 {
                k -= 1;
                stack[k] as usize
            } else {
                if count == 0 {
                    break;
                }
                count -= 1;
                let b = *input.get(src)? as usize;
                src += 1;
                b
            };
            if node[0][index] as usize == index {
                out.push(index as u8);
            } else {
                *stack.get_mut(k)? = node[1][index];
                *stack.get_mut(k + 1)? = node[0][index];
                k += 2;
            }
        }
    }
    (out.len() == out_len).then_some(out)
}

fn read_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn sjis(b: &[u8]) -> String {
    nexas::sjis::decode(b)
}

impl Pack {
    /// Opens an archive; `key_file` is key.fkey or the exe's RESKEY resource, when there is one.
    pub fn open(
        files: &mut dyn Files,
        path: &str,
        key_file: Option<Vec<u8>>,
    ) -> Result<Pack, String> {
        Pack::open_with(files, path, key_file, None)
    }

    /// [`Pack::open`] with a 3.0 game's own key ([`crate::game::game_key`]) too.
    pub fn open_with(
        files: &mut dyn Files,
        path: &str,
        key_file: Option<Vec<u8>>,
        game_key: Option<Vec<u8>>,
    ) -> Result<Pack, String> {
        let size = files.size(path).ok_or(format!("{path}: not found"))?;
        if size < 0x1c {
            return Err(format!("{path}: too small"));
        }
        let tail = files
            .read_range(path, size - 0x1c, 0x1c)
            .ok_or(format!("{path}: unreadable"))?;
        if &tail[..11] != b"FilePackVer" || tail[12] != b'.' {
            return Err(format!("{path}: not a QLIE pack"));
        }
        let (major, minor) = (tail[11], tail[13]);
        let count = read_u32(&tail, 0x10).unwrap() as usize;
        let index_at = u64::from_le_bytes(tail[0x14..0x1c].try_into().unwrap());
        let version = match (major, minor) {
            (b'1', _) => Version::V1,
            (b'2', _) => Version::V2,
            (b'3', b'0') => Version::V3,
            (b'3', b'1') => Version::V31,
            _ => {
                return Err(format!(
                    "{path}: pack version {}.{}",
                    major as char, minor as char
                ));
            }
        };
        let arc_key = if matches!(version, Version::V3 | Version::V31) {
            let k = files
                .read_range(path, size - 0x41c, 0x100)
                .ok_or(format!("{path}: no key"))?;
            (if version == Version::V31 {
                hash_v31(&k)
            } else {
                hash_v3(&k)
            }) & 0x0FFF_FFFF
        } else {
            0
        };
        let index = files
            .read_range(path, index_at, (size - 0x1c - index_at) as usize)
            .ok_or(format!("{path}: no index"))?;
        let mut pack = Pack {
            path: path.to_string(),
            version,
            entries: Vec::new(),
            arc_key,
            key_file: None,
            game_key: if version == Version::V3 {
                game_key
            } else {
                None
            },
            v2_data: version != Version::V1,
        };
        // 1.0 archives may have any of three layouts: try them in turn
        let layouts: &[(bool, bool)] = if version == Version::V1 {
            &[(false, false), (false, true), (true, true)]
        } else {
            &[(true, true)]
        };
        for &(with_hash, v2) in layouts {
            pack.v2_data = v2;
            if let Some(es) = pack.read_index(&index, count, size, with_hash, v2) {
                pack.entries = es;
                break;
            }
        }
        if pack.entries.is_empty() && count > 0 {
            return Err(format!("{path}: unreadable index"));
        }
        if let Some(kf) = key_file
            && matches!(version, Version::V3 | Version::V31)
        {
            pack.key_file = Some(kf);
            // 3.x archives may hold their own key, encrypted with the key file
            if let Some(e) = pack
                .entries
                .iter()
                .find(|e| e.name.contains("pack_keyfile"))
                .cloned()
                && let Some(k) = pack.read_entry(files, &e)
            {
                pack.key_file = Some(k);
            }
        }
        Ok(pack)
    }

    fn read_index(
        &self,
        index: &[u8],
        count: usize,
        size: u64,
        with_hash: bool,
        v2: bool,
    ) -> Option<Vec<Entry>> {
        let mut at = 0usize;
        let mut out = Vec::with_capacity(count);
        let wide = self.version == Version::V31;
        for _ in 0..count {
            let n = u16::from_le_bytes(index.get(at..at + 2)?.try_into().ok()?) as usize;
            at += 2;
            if n > 0x100 {
                return None;
            }
            let bytes = if wide { n * 2 } else { n };
            let mut raw = index.get(at..at + bytes)?.to_vec();
            at += bytes;
            let name = if wide {
                let chars = n as u32;
                let mut hash = (chars.wrapping_mul(chars)) ^ chars;
                hash ^= 0x3E13 ^ (self.arc_key >> 16) ^ self.arc_key;
                hash &= 0xFFFF;
                let mut key = hash;
                for i in 0..n {
                    key = hash
                        .wrapping_add(i as u32)
                        .wrapping_add(key.wrapping_mul(8));
                    raw[i * 2] ^= key as u8;
                    raw[i * 2 + 1] ^= (key >> 8) as u8;
                }
                let u: Vec<u16> = raw
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&u)
            } else {
                let name_key = if self.version == Version::V1 && !v2 {
                    0xC4 ^ 0x3E
                } else {
                    let k = if matches!(self.version, Version::V3) {
                        self.arc_key as i32
                    } else {
                        0xC4
                    };
                    n as i32 + (k ^ 0x3E)
                };
                let mut dec = raw.clone();
                for (k, b) in dec.iter_mut().enumerate() {
                    let k = k as i32 + 1;
                    *b ^= ((k ^ name_key).wrapping_add(k)) as u8;
                }
                let s = sjis(&dec);
                raw = dec;
                s
            };
            let rec = index.get(at..at + 0x18)?;
            at += 0x18;
            let offset = u64::from_le_bytes(rec[0..8].try_into().ok()?);
            let size_ = read_u32(rec, 8)?;
            if offset + size_ as u64 > size {
                return None;
            }
            out.push(Entry {
                name: name.replace('\\', "/"),
                raw_name: raw,
                offset,
                size: size_,
                unpacked: read_u32(rec, 12)?,
                packed: read_u32(rec, 16)? != 0,
                method: read_u32(rec, 20)? as i32,
            });
            if with_hash {
                at += 4;
            }
        }
        Some(out)
    }

    /// The bytes of an entry, decrypted and unpacked.
    pub fn read_entry(&self, files: &mut dyn Files, e: &Entry) -> Option<Vec<u8>> {
        let mut data = files.read_range(&self.path, e.offset, e.size as usize)?;
        if e.method != 0 {
            match self.version {
                Version::V1 => decrypt_v2(&mut data, 0, self.v2_data),
                Version::V2 => decrypt_v2(&mut data, 0, true),
                Version::V3 => match &self.key_file {
                    Some(k) => decrypt_v3_key(
                        &mut data,
                        &e.raw_name,
                        self.arc_key,
                        k,
                        self.game_key.as_deref(),
                    ),
                    None => decrypt_v2(&mut data, self.arc_key, true),
                },
                Version::V31 => decrypt_v31(&mut data, e, self.arc_key, self.key_file.as_deref()),
            }
        }
        if e.packed {
            return Some(decompress(&data).unwrap_or(data));
        }
        Some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lanes() {
        assert_eq!(padd_b(0x00ff, 0x0001), 0x0000);
        assert_eq!(padd_w(0xffff, 1), 0);
        assert_eq!(padd_d(0xffff_ffff, 1), 0);
        assert_eq!(psll_d(0x8000_0001_8000_0001, 1), 0x0000_0002_0000_0002);
    }

    #[test]
    fn one_pc() {
        // one block: no pairs, two literal bytes
        let mut b = b"1PC\xFF\0\0\0\0\x02\0\0\0".to_vec();
        // skip 128 entries, one entry (128, itself: no pair), skip past the end
        b.extend([0xFF, 0x80, 0xFF]);
        b.extend(2u32.to_le_bytes());
        b.extend(b"hi");
        assert_eq!(decompress(&b).as_deref(), Some(&b"hi"[..]));
    }
}
