//! WARC archives ("WARC 1.x", the ShiinaRio engine's .WAR files), after GARbro's
//! ArcFormats/ShiinaRio (ArcWARC.cs, WarcEncryption.cs, MIT). The index and the entries are
//! encrypted with per-game keys (a "scheme": a key string, a picture region, a decode table and
//! the "Shiina" picture), which schemes/*.bin carry (tools/garbro_db.py writes them from GARbro's
//! database).

use std::collections::HashMap;

/// The per-game keys.
pub struct Scheme {
    pub name: &'static str,
    pub version: u32,
    pub entry_name_size: usize,
    pub crypt_key: Vec<u8>,
    pub helper_key: Vec<u32>,
    pub region: Vec<u8>,
    pub decode_bin: Vec<u8>,
    pub shiina_image: Vec<u8>,
    pub extra: String,
}

/// The schemes this build knows, by the title GARbro gives them.
/// (`tools/garbro_db.py Formats.dat warc schemes/<file> <name> [helper key]`)
const SCHEMES: &[(&str, &[u8])] = &[
    // 種憑け村 序章体験版: the full game's keys with the trial exe's helper key ("gy||OWey|diODD")
    (
        "Tanetsuke Mura (trial)",
        include_bytes!("../schemes/tanetsuke_mura_trial.bin"),
    ),
];

impl Scheme {
    fn parse(name: &'static str, b: &[u8]) -> Option<Scheme> {
        if b.get(..4)? != b"SRWS" {
            return None;
        }
        let u32_at = |at: usize| -> Option<u32> {
            Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
        };
        let version = u32_at(4)?;
        let entry_name_size = u32_at(8)? as usize;
        let mut at = 12;
        let mut blob = || -> Option<Vec<u8>> {
            let n = u32_at(at)? as usize;
            let v = b.get(at + 4..at + 4 + n)?.to_vec();
            at += 4 + n;
            Some(v)
        };
        let crypt_key = blob()?;
        let helper_key = blob()?
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let region = blob()?;
        let decode_bin = blob()?;
        let shiina_image = blob()?;
        let extra = String::from_utf8(blob()?).ok()?;
        Some(Scheme {
            name,
            version,
            entry_name_size,
            crypt_key,
            helper_key,
            region,
            decode_bin,
            shiina_image,
            extra,
        })
    }

    pub fn all() -> Vec<Scheme> {
        SCHEMES
            .iter()
            .filter_map(|(n, b)| Scheme::parse(n, b))
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub unpacked_size: u32,
    pub flags: u32,
}

pub struct Warc {
    pub version: u32,
    pub entries: Vec<Entry>,
    by_name: HashMap<String, usize>,
    scheme: Option<std::rc::Rc<Scheme>>,
}

struct Decoder<'a> {
    scheme: &'a Scheme,
    warc_version: u32,
    max_index_length: u32,
    rand: u32,
}

fn rot_r8(v: u8, n: u32) -> u8 {
    v.rotate_right(n)
}

impl<'a> Decoder<'a> {
    fn new(version: u32, scheme: &'a Scheme) -> Self {
        let entries = if version < 150 || scheme.version < 2310 {
            8192
        } else {
            16384
        };
        Decoder {
            scheme,
            warc_version: version,
            max_index_length: (scheme.entry_name_size as u32 + 0x18) * entries,
            rand: 0,
        }
    }

    fn next_rand(&mut self) -> u32 {
        self.rand = 1566083941u32.wrapping_mul(self.rand).wrapping_add(1);
        self.rand
    }

    fn decrypt(&mut self, data: &mut [u8]) {
        let data_length = data.len() as u32;
        if data_length < 3 || self.warc_version < 120 {
            return;
        }
        let mut effective_length = data_length.min(1024);
        let mut index = 0usize;
        let (a, b);
        let mut fac: u32 = 0;
        self.rand = data_length;
        if self.warc_version > 120 {
            a = (data[0] as i8 as i32) ^ (data_length as u8 as i8 as i32);
            b = (data[1] as i8 as i32) ^ ((data_length / 2) as u8 as i8 as i32);
            if data_length != self.max_index_length
                && (self.warc_version > 130 || self.scheme.version > 2150)
            {
                let image = &self.scheme.shiina_image;
                let idx = (self.next_rand() as f64 * (image.len() as f64 / 4294967296.0)) as usize;
                let img = |i: usize| image.get(i).copied().unwrap_or(0) as u32;
                if self.warc_version >= 160 {
                    fac = self.rand.wrapping_add(img(idx));
                    fac = helper3(fac) & 0xfffffff;
                    if effective_length > 0x80 && self.scheme.version > 2350 {
                        self.helper4(data, 4);
                        index += 0x80;
                        effective_length -= 0x80;
                    }
                } else if self.warc_version == 150 {
                    fac = self.rand.wrapping_add(img(idx));
                    fac ^= (fac & 0xfff).wrapping_mul(fac & 0xfff);
                    let mut v: u32 = 0;
                    for _ in 0..32 {
                        let bit = fac & 1;
                        fac >>= 1;
                        if bit != 0 {
                            v = v.wrapping_add(fac);
                        }
                    }
                    fac = v;
                } else if self.warc_version == 140 {
                    fac = img(idx);
                } else if self.warc_version == 130 {
                    fac = img(idx & 0xff);
                }
            }
        } else {
            a = data[0] as i32;
            b = data[1] as i32;
        }
        self.rand ^= (helper1(a as f64) * 100000000.0) as i64 as u32;
        let mut token = 0.0f64;
        if (a | b) != 0 {
            token = ((a as f64) / ((a * a + b * b) as f64).sqrt()).acos();
            token = token / std::f64::consts::PI * 180.0;
        }
        if b < 0 {
            token = 360.0 - token;
        }
        let key_len = self.scheme.crypt_key.len() as u32;
        let h2 = self.helper2(token) as u8 as u32;
        let mut x = (fac.wrapping_add(h2) % key_len) as usize;
        // DecryptContent
        let key = &self.scheme.crypt_key;
        let mut n = 0usize;
        let len = effective_length as usize;
        for i in 2..len {
            let mut d = data[index + i];
            if self.warc_version > 120 {
                d ^= (self.next_rand() as f64 / 16777216.0) as u32 as u8;
            }
            d = rot_r8(d, 1);
            d ^= key[n] ^ key[x];
            n += 1;
            data[index + i] = d;
            x = d as usize % key.len();
            if n >= key.len() {
                n = 0;
            }
        }
    }

    fn helper2(&mut self, a: f64) -> u32 {
        let (v0, mut v1, mut v2, mut v3);
        if a > 1.0 {
            v0 = (a * 2.0 - 1.0).sqrt();
            loop {
                v1 = 1.0 - self.next_rand() as f64 / 4294967296.0;
                v2 = 2.0 * self.next_rand() as f64 / 4294967296.0 - 1.0;
                if v1 * v1 + v2 * v2 > 1.0 {
                    continue;
                }
                v2 /= v1;
                v3 = v2 * v0 + a - 1.0;
                if v3 <= 0.0 {
                    continue;
                }
                v1 = (a - 1.0) * (v3 / (a - 1.0)).ln() - v2 * v0;
                if v1 < -50.0 {
                    continue;
                }
                if (self.next_rand() as f64 / 4294967296.0) <= v1.exp() * (v2 * v2 + 1.0) {
                    break;
                }
            }
        } else {
            let e = 1.0f64.exp();
            v0 = e / (a + e);
            loop {
                v1 = self.next_rand() as f64 / 4294967296.0;
                v2 = self.next_rand() as f64 / 4294967296.0;
                if v1 < v0 {
                    v3 = v2.powf(1.0 / a);
                    v1 = (-v3).exp();
                } else {
                    v3 = 1.0 - v2.ln();
                    v1 = v3.powf(a - 1.0);
                }
                if (self.next_rand() as f64 / 4294967296.0) < v1 {
                    break;
                }
            }
        }
        if self.warc_version > 120 {
            (v3 * 256.0) as i64 as u32
        } else {
            (self.next_rand() as f64 / 4294967296.0) as u8 as u32
        }
    }

    fn helper4(&self, data: &mut [u8], index: usize) {
        let be = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().unwrap());
        let mut buf = [0u32; 0x50];
        for (i, b) in buf.iter_mut().enumerate().take(0x10) {
            *b = be(index + 40 + 4 * i);
        }
        for i in 0x10..0x50 {
            let v = buf[i - 16] ^ buf[i - 14] ^ buf[i - 8] ^ buf[i - 3];
            buf[i] = v.rotate_left(1);
        }
        let mut key = [0u32; 10];
        for (k, h) in key.iter_mut().zip(self.scheme.helper_key.iter()).take(5) {
            *k = *h;
        }
        let (mut k0, mut k1, mut k2, mut k3, mut k4) = (key[0], key[1], key[2], key[3], key[4]);
        for (i, &w) in buf.iter().enumerate() {
            let (f, c) = if i < 0x10 {
                (k1 ^ k2 ^ k3, 0)
            } else if i < 0x20 {
                (k1 & k2 | k3 & !k1, 0x5A827999)
            } else if i < 0x30 {
                (k3 ^ (k1 | !k2), 0x6ED9EBA1)
            } else if i < 0x40 {
                (k1 & k3 | k2 & !k3, 0x8F1BBCDCu32)
            } else {
                (k1 ^ (k2 | !k3), 0xA953FD4Eu32)
            };
            let new_k0 = w
                .wrapping_add(k4)
                .wrapping_add(f)
                .wrapping_add(c)
                .wrapping_add(k0.rotate_left(5));
            let new_k2 = k1.rotate_right(2);
            k1 = k0;
            k4 = k3;
            k3 = k2;
            k2 = new_k2;
            k0 = new_k0;
        }
        key[0] = key[0].wrapping_add(k0);
        key[1] = key[1].wrapping_add(k1);
        key[2] = key[2].wrapping_add(k2);
        key[3] = key[3].wrapping_add(k3);
        key[4] = key[4].wrapping_add(k4);
        let ft = ((key[0] & 0x7FFFFFFF) as u64) << 32 | key[1] as u64;
        let t = system_time(ft);
        key[5] = t.year | t.month << 16;
        key[7] = t.hour | t.minute << 16;
        key[8] = t.second | t.millis << 16;
        let mut flags =
            u32::from_le_bytes(data[index + 40..index + 44].try_into().unwrap()) | 0x80000000;
        let rgb = buf[1] >> 8;
        if flags & 0x78000000 == 0 {
            flags |= 0x98000000;
        }
        key[6] = region_crc32(&self.scheme.region, flags, rgb);
        key[9] = ((key[2] as i32 as i64 * key[3] as i32 as i64) >> 8) as u32;
        if self.scheme.version >= 2390 {
            key[6] = key[6].wrapping_add(key[9]);
        }
        for (i, k) in key.iter().enumerate() {
            let at = index + i * 4;
            let v = u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) ^ k;
            data[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
    }

    fn decrypt2(&self, data: &mut [u8]) {
        if data.len() < 0x400 || self.scheme.decode_bin.is_empty() {
            return;
        }
        let crc = !crc32(&data[..0x100]); // the running value, without the final inversion
        let mut index = 0x100;
        for _ in 0..0x40 {
            let src = u32::from_le_bytes(data[index..index + 4].try_into().unwrap()) & 0x1ffc;
            let src = self
                .scheme
                .decode_bin
                .get(src as usize..src as usize + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .unwrap_or(0);
            let key = src ^ crc;
            for k in 0..4 {
                data[index + 0x100] ^= (key >> (8 * k)) as u8;
                index += 1;
            }
        }
    }
}

/// Crc32Normal.UpdateCrc (0xFFFFFFFF, data): the standard table, no final inversion.
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = (i as u32) << 24;
            for _ in 0..8 {
                c = if c & 0x80000000 != 0 {
                    (c << 1) ^ 0x04C11DB7
                } else {
                    c << 1
                };
            }
            *e = c;
        }
        t
    });
    let mut crc = 0xFFFFFFFFu32;
    for &b in data {
        crc = table[((crc >> 24) ^ b as u32) as usize] ^ (crc << 8);
    }
    !crc
}

fn helper1(a: f64) -> f64 {
    if a < 0.0 {
        return -helper1(-a);
    }
    if a < 18.0 {
        let mut v0 = a;
        let mut v1 = a;
        let v2 = -(a * a);
        let mut j = 3i32;
        while j < 1000 {
            v1 *= v2 / (j * (j - 1)) as f64;
            v0 += v1 / j as f64;
            if v0 == v2 {
                break;
            }
            j += 2;
        }
        return v0;
    }
    let mut flags = 0;
    let (mut v0_l, mut v1_l) = (0.0f64, 0.0f64);
    let (mut v0_h, mut v1_h) = (2.0f64, 2.0f64);
    let (mut v0, mut v1) = (0.0f64, 0.0f64);
    let mut div = 1.0 / a;
    let mut i = 0i32;
    loop {
        v0 += div;
        i += 1;
        div *= i as f64 / a;
        if v0 < v0_h {
            v0_h = v0;
        } else {
            flags |= 1;
        }
        v1 += div;
        i += 1;
        div *= i as f64 / a;
        if v1 < v1_h {
            v1_h = v1;
        } else {
            flags |= 2;
        }
        v0 -= div;
        i += 1;
        div *= i as f64 / a;
        if v0 > v0_l {
            v0_l = v0;
        } else {
            flags |= 4;
        }
        v1 -= div;
        i += 1;
        div *= i as f64 / a;
        if v1 > v1_l {
            v1_l = v1;
        } else {
            flags |= 8;
        }
        if flags == 0xf {
            break;
        }
    }
    ((std::f64::consts::PI - a.cos() * (v0_l + v0_h)) - (a.sin() * (v1_l + v1_h))) / 2.0
}

fn helper3(key: u32) -> u32 {
    let b = key.to_le_bytes();
    let f = |x: u8| (1.5 * x as f64 + 0.1) as f32;
    let v0 = f(b[0]).to_bits().swap_bytes();
    let v1 = f(b[1]) as u32;
    let v2 = (f(b[2]).to_bits() as i32).wrapping_neg() as u32;
    let v3 = !f(b[3]).to_bits();
    v0.wrapping_add(v1) | v2.wrapping_sub(v3)
}

struct SysTime {
    year: u32,
    month: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
}

/// FileTimeToSystemTime: 100 ns ticks since 1601-01-01.
fn system_time(ft: u64) -> SysTime {
    let ms_total = ft / 10_000;
    let millis = (ms_total % 1000) as u32;
    let secs = ms_total / 1000;
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // days since 1601-01-01 -> civil date (days_from_civil inverse, epoch shifted to 1970)
    let z = days - 134774 + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    SysTime {
        year: y as u32,
        month: m as u32,
        hour: (rem / 3600) as u32,
        minute: (rem / 60 % 60) as u32,
        second: (rem % 60) as u32,
        millis,
    }
}

fn region_crc32(src: &[u8], flags: u32, rgb: u32) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut poly = i as u32;
            for _ in 0..8 {
                let bit = poly & 1;
                poly = poly.rotate_right(1);
                if bit == 0 {
                    poly ^= 0x6DB88320;
                }
            }
            *e = poly;
        }
        t
    });
    if src.len() < 48 * 48 * 4 {
        return 0;
    }
    let mut src_alpha = (flags & 0x1ff) as i32;
    let mut dst_alpha = ((flags >> 12) & 0x1ff) as i32;
    let f = flags >> 24;
    if f & 0x10 == 0 {
        dst_alpha = 0;
    }
    if f & 8 == 0 {
        src_alpha = 0x100;
    }
    let mut y_step: i32 = 0;
    let mut x_step: i32 = 4;
    let width = 48;
    let mut pos: i32 = 0;
    if f & 0x40 != 0 {
        y_step += width;
        pos += (width - 1) * 4;
        x_step = -x_step;
    }
    if f & 0x20 != 0 {
        y_step -= width;
        pos += width * 0x2f * 4;
    }
    y_step <<= 3;
    let mut checksum: u32 = 0;
    for _ in 0..48 {
        for _ in 0..48 {
            let p = pos as usize;
            let alpha = (src[p + 3] as i32 * src_alpha) >> 8;
            let mut color = rgb;
            for i in 0..3 {
                let v = src[p + i] as i32;
                let mut c = (color & 0xff) as i32;
                c -= v;
                c = (c * dst_alpha) >> 8;
                c = (c + v) & 0xff;
                c = (c * alpha) >> 8;
                checksum = (checksum >> 8) ^ table[((c as u32 ^ checksum) & 0xff) as usize];
                color >>= 8;
            }
            pos += x_step;
        }
        pos += y_step;
    }
    checksum
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn inflate(b: &[u8], size: usize) -> Option<Vec<u8>> {
    let mut out = miniz_oxide::inflate::decompress_to_vec_zlib(b)
        .or_else(|e| {
            if e.output.is_empty() {
                Err(())
            } else {
                Ok(e.output)
            }
        })
        .ok()?;
    if size > 0 {
        out.resize(size, 0);
    }
    Some(out)
}

impl Warc {
    /// Reads the index of an archive whose bytes are `file` (only the header and the index are
    /// read, through `read(offset, len)`).
    pub fn open(
        read: &mut dyn FnMut(u64, usize) -> Vec<u8>,
        file_len: u64,
        schemes: &[std::rc::Rc<Scheme>],
    ) -> Option<Warc> {
        let head = read(0, 12);
        if head.len() < 12 || &head[..4] != b"WARC" || &head[4..7] != b" 1." {
            return None;
        }
        let v = head[7].wrapping_sub(b'0') as u32;
        if !(1..=7).contains(&v) {
            return None;
        }
        let version = 100 + v * 10;
        let index_offset = 0xF182AD82u32 ^ le32(&head, 8);
        if index_offset as u64 >= file_len {
            return None;
        }
        let candidates: Vec<Option<std::rc::Rc<Scheme>>> = if version > 110 {
            schemes.iter().cloned().map(Some).collect()
        } else {
            vec![None]
        };
        for scheme in candidates {
            let plain = Scheme {
                name: "",
                version: 0,
                entry_name_size: 0x10,
                crypt_key: vec![],
                helper_key: vec![],
                region: vec![],
                decode_bin: vec![],
                shiina_image: vec![],
                extra: String::new(),
            };
            let s = scheme.as_deref().unwrap_or(&plain);
            let mut dec = Decoder::new(version, s);
            let max = dec.max_index_length as usize;
            let len = (max as u64).min(file_len - index_offset as u64) as usize;
            if len < 8 {
                continue;
            }
            let mut idx = read(index_offset as u64, len);
            idx.resize(max, 0);
            dec.decrypt(&mut idx);
            for c in idx.chunks_exact_mut(4) {
                let v = u32::from_le_bytes(c.try_into().unwrap()) ^ index_offset;
                c.copy_from_slice(&v.to_le_bytes());
            }
            if version >= 170 {
                let key = !(version as u8);
                for b in idx.iter_mut() {
                    *b ^= key;
                }
            }
            let index = if version >= 170 {
                if idx[8] != 0x78 {
                    continue;
                }
                match inflate(&idx[8..len], 0) {
                    Some(v) => v,
                    None => continue,
                }
            } else if version >= 120 {
                // Kogado range coder: not needed by the supported titles yet.
                continue;
            } else {
                idx[..len].to_vec()
            };
            let name_size = s.entry_name_size;
            let rec = name_size + 24;
            let mut entries = Vec::new();
            let mut by_name = HashMap::new();
            for r in index.chunks_exact(rec) {
                let raw = &r[..name_size];
                let end = raw.iter().position(|&c| c == 0).unwrap_or(name_size);
                if end == 0 || raw[0] >= 0x80 {
                    continue;
                }
                let name = encoding_rs::SHIFT_JIS.decode(&raw[..end]).0.into_owned();
                let e = Entry {
                    name: name.clone(),
                    offset: le32(r, name_size),
                    size: le32(r, name_size + 4),
                    unpacked_size: le32(r, name_size + 8),
                    flags: le32(r, name_size + 20),
                };
                if e.offset as u64 + e.size as u64 > file_len {
                    entries.clear();
                    break;
                }
                let key = name.to_ascii_lowercase();
                if let std::collections::hash_map::Entry::Vacant(v) = by_name.entry(key) {
                    v.insert(entries.len());
                    entries.push(e);
                }
            }
            if entries.is_empty() {
                continue;
            }
            return Some(Warc {
                version,
                entries,
                by_name,
                scheme,
            });
        }
        None
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.by_name
            .get(&name.to_ascii_lowercase())
            .map(|&i| &self.entries[i])
    }

    /// Decrypts and unpacks an entry's stored bytes.
    pub fn decode(&self, e: &Entry, mut data: Vec<u8>) -> Vec<u8> {
        if data.len() <= 8 {
            return data;
        }
        let mut sig = le32(&data, 0);
        let unpacked_size = le32(&data, 4) as usize;
        let plain_scheme;
        let scheme = match &self.scheme {
            Some(s) => &**s,
            None => {
                plain_scheme = Scheme {
                    name: "",
                    version: 0,
                    entry_name_size: 0x10,
                    crypt_key: vec![],
                    helper_key: vec![],
                    region: vec![],
                    decode_bin: vec![],
                    shiina_image: vec![],
                    extra: String::new(),
                };
                &plain_scheme
            }
        };
        let mut dec = Decoder::new(self.version, scheme);
        if self.version > 110 {
            sig ^= (unpacked_size as u32 ^ 0x82AD82) & 0xFFFFFF;
            if e.flags & 0x80000000 != 0 {
                dec.decrypt(&mut data[8..]);
            }
            if e.flags & 0x20000000 != 0 {
                dec.decrypt2(&mut data[8..]);
            }
        }
        let unpacked = match sig & 0xffffff {
            0x314859 => Some(unpack_yh1(&mut data, unpacked_size)),
            0x4b5059 => Some(unpack_ypk(&mut data, unpacked_size)),
            0x5a4c59 => Some(unpack_ylz(&mut data, unpacked_size)),
            _ => None,
        };
        match unpacked {
            Some(mut out) => {
                if self.version > 110 && e.flags & 0x40000000 != 0 {
                    dec.decrypt2(&mut out);
                }
                out
            }
            None => data,
        }
    }
}

fn unpack_yh1(input: &mut [u8], size: usize) -> Vec<u8> {
    if input[3] != 0 {
        let key = 0x6393528eu32 ^ 0x4b4d;
        let n = input.len() / 4;
        for i in 2..n {
            let v = le32(input, i * 4) ^ key;
            input[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    huffman(&input[8..], size)
}

fn unpack_ypk(input: &mut [u8], size: usize) -> Vec<u8> {
    if input[3] != 0 {
        let key = !0x4b4d4b4du32;
        let n = input.len() / 4;
        for i in 2..n {
            let v = le32(input, i * 4) ^ key;
            input[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        for b in input.iter_mut().skip(n.max(2) * 4) {
            *b ^= key as u8;
        }
    }
    inflate(&input[8..], size).unwrap_or_else(|| vec![0; size])
}

fn unpack_ylz(input: &mut [u8], size: usize) -> Vec<u8> {
    if input[3] != 0 {
        let mut key = 0x4b4d4b4du32;
        let n = input.len() / 4;
        for i in 2..n {
            let v = le32(input, i * 4) ^ key;
            input[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        for b in input.iter_mut().skip(n.max(2) * 4) {
            *b ^= key as u8;
            key >>= 8;
        }
    }
    let mut out = vec![0u8; size];
    let mut src = 8usize;
    let mut ctl = 0u32;
    let mut mask = 0u32;
    let get = |src: &usize| input.get(*src).copied().unwrap_or(0);
    let bit = |src: &mut usize, ctl: &mut u32, mask: &mut u32| -> bool {
        let b = *ctl & *mask != 0;
        *mask >>= 1;
        if *mask == 0 {
            *ctl = if *src + 4 <= input.len() {
                le32(input, *src)
            } else {
                0
            };
            *src += 4;
            *mask = 0x80000000;
        }
        b
    };
    bit(&mut src, &mut ctl, &mut mask);
    let mut dst = 0usize;
    while dst < size {
        if bit(&mut src, &mut ctl, &mut mask) {
            out[dst] = get(&src);
            src += 1;
            dst += 1;
            continue;
        }
        let next = bit(&mut src, &mut ctl, &mut mask);
        let mut offset = get(&src) as i32 | !0xffff;
        src += 1;
        let mut ah: i32 = 0xff;
        let count;
        macro_rules! bits {
            ($n:expr) => {{
                let mut v = 0i32;
                for _ in 0..$n {
                    v = v << 1 | bit(&mut src, &mut ctl, &mut mask) as i32;
                }
                v
            }};
        }
        if next {
            if bit(&mut src, &mut ctl, &mut mask) {
                ah = ah << 1 | bits!(1);
            } else if bit(&mut src, &mut ctl, &mut mask) {
                ah = ah << 1 | bits!(1);
                offset -= 0x200;
            } else if bit(&mut src, &mut ctl, &mut mask) {
                ah = ah << 2 | bits!(2);
                offset -= 0x400;
            } else if bit(&mut src, &mut ctl, &mut mask) {
                ah = ah << 3 | bits!(3);
                offset -= 0x800;
            } else {
                ah = ah << 4 | bits!(4);
                offset -= 0x1000;
            }
            count = if bit(&mut src, &mut ctl, &mut mask) {
                3
            } else if bit(&mut src, &mut ctl, &mut mask) {
                4
            } else if bit(&mut src, &mut ctl, &mut mask) {
                5 + bits!(1)
            } else if bit(&mut src, &mut ctl, &mut mask) {
                7 + bits!(2)
            } else if bit(&mut src, &mut ctl, &mut mask) {
                0x0b + bits!(3)
            } else {
                let c = 0x13 + get(&src) as i32;
                src += 1;
                c
            };
        } else if bit(&mut src, &mut ctl, &mut mask) {
            ah <<= 3;
            ah |= bits!(3);
            ah = (ah - 1) & 0xff;
            count = 2;
        } else if offset & 0xff == 0xff {
            break;
        } else {
            count = 2;
        }
        offset += (ah & 0xff) << 8;
        let from = dst as i64 + offset as i64;
        for k in 0..count as usize {
            if dst >= size {
                break;
            }
            let s = from + k as i64;
            out[dst] = if s >= 0 { out[s as usize] } else { 0 };
            dst += 1;
        }
    }
    out
}

fn huffman(src: &[u8], size: usize) -> Vec<u8> {
    struct R<'a> {
        src: &'a [u8],
        pos: usize,
        bits: u32,
        cache: u32,
        tree: [[u16; 511]; 2],
        next: u16,
    }
    impl R<'_> {
        fn read32(&mut self) -> u32 {
            let rem = self.src.len().saturating_sub(self.pos);
            if rem >= 4 {
                let v = le32(self.src, self.pos);
                self.pos += 4;
                v
            } else {
                let mut v = 0u32;
                for i in 0..rem {
                    v |= (self.src[self.pos + i] as u32) << (8 * i);
                }
                self.pos += rem;
                v
            }
        }
        fn get(&mut self, mut req: u32) -> u32 {
            let mut ret = 0u32;
            if req > self.bits {
                req -= self.bits;
                ret |= (self.cache & ((1u64 << self.bits) - 1) as u32) << req;
                self.cache = self.read32();
                self.bits = 32;
            }
            self.bits -= req;
            ret | (((1u64 << req) - 1) as u32 & (self.cache.checked_shr(self.bits).unwrap_or(0)))
        }
        fn tree(&mut self) -> u16 {
            if self.get(1) != 0 {
                let i = self.next;
                self.next += 1;
                if (i as usize) < 511 {
                    let l = self.tree();
                    let r = self.tree();
                    self.tree[0][i as usize] = l;
                    self.tree[1][i as usize] = r;
                }
                i
            } else {
                self.get(8) as u16
            }
        }
    }
    let mut r = R {
        src,
        pos: 0,
        bits: 0,
        cache: 0,
        tree: [[0; 511]; 2],
        next: 256,
    };
    let root = r.tree();
    let mut out = vec![0u8; size];
    for o in out.iter_mut() {
        let mut s = root;
        while s >= 256 && (s as usize) < 511 {
            let b = r.get(1) as usize;
            s = r.tree[b][s as usize];
        }
        *o = s as u8;
    }
    out
}
