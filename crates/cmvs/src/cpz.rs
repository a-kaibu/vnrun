//! CPZ5 archives. The steps follow GARbro's ArcCPZ.cs and CmvsMD5.cs (MIT, (C) morkt); the
//! MD5 variant and the secret are the ones of the fourth CPZ5 reader of cmvs32.exe
//! (未来ノスタルジア web 体験版 v2), which GARbro does not list.
//!
//! ```text
//! "CPZ5" dir_count dir_size file_size index_md5[16] md5[4] master enc entry_key? checksum
//! index (dir_size + file_size bytes): directories, then their entries
//! ```

use std::collections::HashMap;

use nexas::files::Files;

const SECRET: [u32; 24] = [
    0xCD90F089, 0xE982B782, 0xA282AB88, 0xCD82718E, 0x52838A83, 0xA882AA82, 0x7592648E, 0xB582AB82,
    0xE182BF82, 0xDC82A282, 0x4281B782, 0x62838183, 0xA9824981, 0xA282ED82, 0xC682A282, 0xBE8CA982,
    0xC482C182, 0x968BE082, 0xC482B582, 0xB082A082, 0xA282C882, 0xBE82F182, 0xE782A982, 0x49819F82,
];
const FACTOR: u32 = 0x1A743125;
const DIR_ADD: [u32; 4] = [0, 0x00112233, 0, 0x34258765];

fn u32_(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn words(b: &mut [u8]) -> impl Iterator<Item = &mut [u8]> {
    b.chunks_exact_mut(4)
}

fn get(w: &[u8]) -> u32 {
    u32::from_le_bytes(w.try_into().unwrap())
}

fn set(w: &mut [u8], v: u32) {
    w.copy_from_slice(&v.to_le_bytes());
}

/// MD5's compression of one block: `data` then the padding of a 16-byte message.
fn md5(init: [u32; 4], data: [u32; 4]) -> [u32; 4] {
    const S: [u32; 16] = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
    let mut buf = [0u32; 16];
    buf[..4].copy_from_slice(&data);
    buf[4] = 0x80;
    buf[14] = 0x80;
    let [mut a, mut b, mut c, mut d] = init;
    for i in 0..64 {
        let (f, g) = match i / 16 {
            0 => ((b & c) | (!b & d), i),
            1 => ((d & b) | (!d & c), (5 * i + 1) & 15),
            2 => (b ^ c ^ d, (3 * i + 5) & 15),
            _ => (c ^ (b | !d), (7 * i) & 15),
        };
        let k = ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32;
        let t = d;
        d = c;
        c = b;
        b = b.wrapping_add(
            a.wrapping_add(f)
                .wrapping_add(k)
                .wrapping_add(buf[g])
                .rotate_left(S[(i / 16) * 4 + i % 4]),
        );
        a = t;
    }
    [
        init[0].wrapping_add(a),
        init[1].wrapping_add(b),
        init[2].wrapping_add(c),
        init[3].wrapping_add(d),
    ]
}

fn cmvs_md5(data: [u32; 4]) -> [u32; 4] {
    let s = md5([0x64FE9A8C, 0xC2F93EA8, 0xEF81BA5D, 0x92C8A72F], data);
    [
        s[1] ^ 0x49876325,
        s[2].wrapping_add(0x54F46D7C),
        s[3] ^ 0xAC7958B7,
        s[0].wrapping_add(0x1C0638BD),
    ]
}

fn table(mut key: u32, summand: u32) -> [u8; 256] {
    let mut t = [0u8; 256];
    for (i, v) in t.iter_mut().enumerate() {
        *v = i as u8;
    }
    for _ in 0..256 {
        t.swap((key >> 16) as u8 as usize, key as u8 as usize);
        t.swap((key >> 8) as u8 as usize, (key >> 24) as usize);
        key = summand.wrapping_add(FACTOR.wrapping_mul(key.rotate_right(2)));
    }
    t
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub offset: u64,
    pub size: u32,
    key: u32,
}

pub struct Cpz {
    pub path: String,
    /// lower-case name ("dir/name", or "name" in the root) to its entry
    pub entries: HashMap<String, Entry>,
    md: [u32; 4],
    master: u32,
    dir_count: u32,
    encrypted: bool,
    table: [u8; 256],
}

impl Cpz {
    pub fn open(files: &mut dyn Files, path: &str) -> Result<Cpz, String> {
        let h = files.read_range(path, 0, 0x40).ok_or("cannot read")?;
        if h.len() < 0x40 || &h[..4] != b"CPZ5" {
            return Err("not a CPZ5 archive".into());
        }
        let dir_count = 0xFE3A53D9 ^ u32_(&h, 4);
        let dir_size = (0x37F298E7 ^ u32_(&h, 8)) as usize;
        let file_size = (0x7A6F3A2C ^ u32_(&h, 12)) as usize;
        let master = 0xAE7D39BF ^ u32_(&h, 0x30);
        let encrypted = (0xFB73A955 ^ u32_(&h, 0x34)) != 0;
        let md = cmvs_md5([
            0x43DE7C19 ^ u32_(&h, 0x20),
            0xCC65F415 ^ u32_(&h, 0x24),
            0xD016A93C ^ u32_(&h, 0x28),
            0x97A3BA9A ^ u32_(&h, 0x2C),
        ]);
        if dir_size > 1 << 24 || file_size > 1 << 26 {
            return Err("index too large".into());
        }
        let mut idx = files
            .read_range(path, 0x40, dir_size + file_size)
            .filter(|b| b.len() == dir_size + file_size)
            .ok_or("index cut short")?;
        // stage 1, over the whole index
        let key = master ^ 0x3795B39A;
        let sk: Vec<u32> = SECRET.iter().map(|s| s.wrapping_sub(key)).collect();
        let shift = (((key >> 24) ^ (key >> 16) ^ (key >> 8) ^ key ^ 0xB) & 0xF) + 7;
        let mut i = 5;
        let n = idx.len();
        for w in words(&mut idx) {
            let v = (sk[i] ^ get(w))
                .wrapping_add(0x784C5962)
                .rotate_right(shift)
                .wrapping_add(0x01010101);
            set(w, v);
            i = (i + 1) % 24;
        }
        for k in (1..=n & 3).rev() {
            let p = n - k;
            idx[p] = (idx[p] ^ (sk[i] >> (k * 4)) as u8).wrapping_sub(0x79);
            i = (i + 1) % 24;
        }
        let t = table(master, md[1]);
        for b in &mut idx[..dir_size] {
            *b = t[(*b ^ 0x3A) as usize];
        }
        let dk = [
            md[0] ^ master.wrapping_add(0x76A3BF29),
            md[1] ^ master,
            md[2] ^ master.wrapping_add(0x10000000),
            md[3] ^ master,
        ];
        let mut seed = 0x76548AEFu32;
        let whole = dir_size / 4;
        for (j, w) in words(&mut idx[..whole * 4]).enumerate() {
            let v = (get(w) ^ dk[j & 3])
                .wrapping_sub(0x4A91C262)
                .rotate_left(3)
                .wrapping_sub(seed);
            set(w, v);
            seed = seed.wrapping_add(0x10FB562A);
        }
        for k in 0..dir_size & 3 {
            let p = whole * 4 + k;
            idx[p] = (idx[p] ^ (dk[(whole + k) & 3] >> 6) as u8).wrapping_add(0x37);
        }
        let t = table(master, md[2]);
        let base = 0x40 + (dir_size + file_size) as u64;
        let mut entries = HashMap::new();
        let mut d = 0usize;
        for di in 0..dir_count {
            if d + 0x10 > dir_size {
                return Err("bad directory".into());
            }
            let (dsz, count, eoff, dkey) = (
                u32_(&idx, d) as usize,
                u32_(&idx, d + 4),
                u32_(&idx, d + 8) as usize,
                u32_(&idx, d + 12),
            );
            if dsz <= 0x10 || d + dsz > dir_size || count > 0x10000 {
                return Err("bad directory".into());
            }
            let name_end = idx[d + 0x10..d + dsz]
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(dsz - 0x10);
            let dname = nexas::sjis::decode(&idx[d + 0x10..d + 0x10 + name_end]);
            let next = if di + 1 == dir_count {
                file_size
            } else {
                u32_(&idx, d + dsz + 8) as usize
            };
            if next < eoff || dir_size + next > idx.len() {
                return Err("bad directory".into());
            }
            let (start, size) = (dir_size + eoff, next - eoff);
            let part = &mut idx[start..start + size];
            for b in part.iter_mut() {
                *b = t[(*b ^ 0x7E) as usize];
            }
            let key: Vec<u32> = (0..4)
                .map(|j| md[j] ^ dkey.wrapping_add(DIR_ADD[j]))
                .collect();
            let mut seed = 0x2A65CB4Eu32;
            let whole = size / 4;
            for (j, w) in words(&mut part[..whole * 4]).enumerate() {
                let v = (get(w) ^ key[j & 3])
                    .wrapping_sub(seed)
                    .rotate_left(2)
                    .wrapping_add(0x37A19E8B);
                set(w, v);
                seed = seed.wrapping_sub(0x139FA9B);
            }
            for k in 0..size & 3 {
                let p = whole * 4 + k;
                part[p] = (part[p] ^ (key[(whole + k) & 3] >> 4) as u8).wrapping_add(5);
            }
            let mut cur = 0usize;
            for _ in 0..count {
                if cur + 0x18 > size {
                    return Err("bad entry".into());
                }
                let esz = u32_(part, cur) as usize;
                if esz <= 0x18 || cur + esz > size {
                    return Err("bad entry".into());
                }
                let offset = u64::from_le_bytes(part[cur + 4..cur + 12].try_into().unwrap());
                let e = Entry {
                    offset: offset + base,
                    size: u32_(part, cur + 12),
                    key: u32_(part, cur + 0x14).wrapping_add(dkey),
                };
                let nm = &part[cur + 0x18..cur + esz];
                let nm = &nm[..nm.iter().position(|&c| c == 0).unwrap_or(nm.len())];
                let name = nexas::sjis::decode(nm);
                let full = if dname == "root" {
                    name
                } else {
                    format!("{dname}/{name}")
                };
                entries.insert(full.to_lowercase(), e);
                cur += esz;
            }
            d += dsz;
        }
        let table = if encrypted { table(md[3], master) } else { t };
        Ok(Cpz {
            path: path.to_string(),
            entries,
            md,
            master,
            dir_count,
            encrypted,
            table,
        })
    }

    /// The entry's bytes, decrypted; PS2A scripts are unpacked and PB3B headers decrypted.
    pub fn read(&self, files: &mut dyn Files, name: &str) -> Option<Vec<u8>> {
        let e = self.entries.get(&name.to_lowercase())?;
        let mut d = files.read_range(&self.path, e.offset, e.size as usize)?;
        if self.encrypted {
            let seed = (self.master ^ e.key)
                .wrapping_add(self.dir_count)
                .wrapping_sub(0x5C29E87B);
            self.decrypt(&mut d, seed);
        }
        if d.starts_with(b"PS2A") && d.len() > 0x30 {
            return unpack_ps2(&d);
        }
        if d.starts_with(b"PB3B") && d.len() > 0x40 {
            decrypt_pb3(&mut d);
        }
        Some(d)
    }

    fn decrypt(&self, d: &mut [u8], seed: u32) {
        let k = (self.md[1] >> 2) as u8;
        let mut kb = [0u8; 64];
        for (i, w) in SECRET[..16].iter().enumerate() {
            kb[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        for b in kb.iter_mut() {
            *b = k ^ self.table[*b as usize];
        }
        let sk: Vec<u32> = (0..16).map(|i| u32_(&kb, i * 4) ^ seed).collect();
        let mut key = 0x2547A39Eu32;
        let mut p = 9usize;
        let whole = d.len() / 4;
        for w in words(&mut d[..whole * 4]) {
            let x = get(w) ^ sk[((key >> 6) & 0xf) as usize] ^ (sk[p] >> 1);
            let v = self.md[(key & 3) as usize] ^ x.wrapping_sub(seed);
            set(w, v);
            p = (p + 1) & 0xf;
            key = key.wrapping_add(seed).wrapping_add(v);
        }
        for b in &mut d[whole * 4..] {
            *b = self.table[(*b ^ 0xBC) as usize];
        }
    }
}

/// PS2A: the body after the 0x30-byte header is XORed and rotated, then LZSS packed.
pub fn unpack_ps2(d: &[u8]) -> Option<Vec<u8>> {
    let key = u32_(d, 12);
    let shift = (key >> 20) % 5 + 1;
    let k = ((key >> 24).wrapping_add(key >> 3)) as u8;
    let body: Vec<u8> = d[0x30..]
        .iter()
        .map(|&b| (k ^ b.wrapping_sub(0x7C)).rotate_right(shift))
        .collect();
    let n = u32_(d, 0x28) as usize;
    if n > 1 << 26 {
        return None;
    }
    let mut out = d[..0x30].to_vec();
    out.reserve(n);
    let mut frame = [0u8; 0x800];
    let mut fp = 0x7DFusize;
    let (mut src, mut ctl) = (0usize, 1u32);
    while out.len() < 0x30 + n && src < body.len() {
        if ctl == 1 {
            ctl = body[src] as u32 | 0x100;
            src += 1;
        }
        if ctl & 1 != 0 {
            let b = *body.get(src)?;
            src += 1;
            out.push(b);
            frame[fp] = b;
            fp = (fp + 1) & 0x7ff;
        } else {
            let (lo, hi) = (*body.get(src)? as usize, *body.get(src + 1)? as usize);
            src += 2;
            let o = lo | ((hi & 0xe0) << 3);
            for i in 0..(hi & 0x1f) + 2 {
                let b = frame[(o + i) & 0x7ff];
                out.push(b);
                frame[fp] = b;
                fp = (fp + 1) & 0x7ff;
            }
        }
        ctl >>= 1;
    }
    Some(out)
}

/// PB3B keeps 0x2C header bytes from 8 on encrypted with its last bytes.
pub fn decrypt_pb3(d: &mut [u8]) {
    let n = d.len();
    let (k1, k2) = (d[n - 3], d[n - 2]);
    let mut src = n - 0x2F;
    for i in (8..0x34).step_by(2) {
        d[i] = (d[i] ^ k1).wrapping_sub(d[src]);
        d[i + 1] = (d[i + 1] ^ k2).wrapping_sub(d[src + 1]);
        src += 2;
    }
}
