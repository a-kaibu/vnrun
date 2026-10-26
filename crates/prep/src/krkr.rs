//! Recover classic cxdec parameters from KiriKiri's bres resources, without
//! executing the EXE or its embedded plugin. Resource crypto follows xp3-brute
//! (MIT, Copyright xmoezzz): https://github.com/xmoezzz/xp3-brute
// MIT: Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
// of the Software, and to permit persons to whom the Software is furnished to do
// so, subject to the following conditions: The above copyright notice and this
// permission notice shall be included in all copies or substantial portions of
// the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO
// EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES
// OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
// ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.
use flate2::read::ZlibDecoder;
use sha3::{Digest, Sha3_384};
use std::io::Read;

const MAX_RESOURCE: usize = 4 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"VNCXDEC1";

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
struct Pe<'a> {
    bytes: &'a [u8],
    base: u32,
    sections: Vec<(u32, usize, usize)>,
    resources: usize,
}
impl<'a> Pe<'a> {
    fn new(bytes: &'a [u8]) -> Option<Self> {
        if !bytes.starts_with(b"MZ") {
            return None;
        }
        let pe = u32_at(bytes, 60)? as usize;
        if bytes.get(pe..pe.checked_add(4)?)? != b"PE\0\0" {
            return None;
        }
        let opt = pe.checked_add(24)?;
        if u16_at(bytes, opt)? != 0x10b {
            return None;
        }
        let section_table = opt.checked_add(u16_at(bytes, pe + 20)? as usize)?;
        let count = u16_at(bytes, pe + 6)? as usize;
        if count > 96 {
            return None;
        }
        let mut sections = Vec::new();
        for i in 0..count {
            let at = section_table.checked_add(i * 40)?;
            let rva = u32_at(bytes, at + 12)?;
            let size = u32_at(bytes, at + 16)? as usize;
            let offset = u32_at(bytes, at + 20)? as usize;
            bytes.get(offset..offset.checked_add(size)?)?;
            sections.push((rva, offset, size));
        }
        let mut result = Self {
            bytes,
            base: u32_at(bytes, opt + 28)?,
            sections,
            resources: 0,
        };
        result.resources = result.offset(u32_at(bytes, opt + 112)?, 16)?;
        Some(result)
    }
    fn offset(&self, rva: u32, size: usize) -> Option<usize> {
        self.sections.iter().find_map(|&(start, off, len)| {
            let delta = rva.checked_sub(start)? as usize;
            if delta.checked_add(size)? > len {
                return None;
            }
            Some(off + delta)
        })
    }
    fn resource(&self, kind: &str, name: &str) -> Option<&'a [u8]> {
        let mut directory = self.resources;
        for wanted in [Some(kind), Some(name), None] {
            let count = usize::from(u16_at(self.bytes, directory + 12)?)
                + usize::from(u16_at(self.bytes, directory + 14)?);
            if count > 4096 {
                return None;
            }
            let mut selected = None;
            for i in 0..count {
                let entry = directory.checked_add(16 + 8 * i)?;
                let id = u32_at(self.bytes, entry)?;
                let matches = match wanted {
                    None => true,
                    Some("RCDATA") if id == 10 => true,
                    Some(value) if id & 0x80000000 != 0 => {
                        let at = self.resources.checked_add((id & 0x7fffffff) as usize)?;
                        let len = usize::from(u16_at(self.bytes, at)?);
                        if len > 128 {
                            return None;
                        }
                        let raw = self.bytes.get(at + 2..at.checked_add(2 + len * 2)?)?;
                        let words: Vec<_> = raw
                            .chunks_exact(2)
                            .map(|x| u16::from_le_bytes([x[0], x[1]]))
                            .collect();
                        String::from_utf16(&words).ok().as_deref() == Some(value)
                    }
                    Some("127") if id == 127 => true,
                    _ => false,
                };
                if matches {
                    selected = Some(u32_at(self.bytes, entry + 4)?);
                    break;
                }
            }
            let value = selected?;
            let at = self.resources.checked_add((value & 0x7fffffff) as usize)?;
            if wanted.is_none() {
                if value & 0x80000000 != 0 {
                    return None;
                }
                let len = u32_at(self.bytes, at + 4)? as usize;
                if len > MAX_RESOURCE {
                    return None;
                }
                let off = self.offset(u32_at(self.bytes, at)?, len)?;
                return self.bytes.get(off..off + len);
            }
            if value & 0x80000000 == 0 {
                return None;
            }
            directory = at;
        }
        None
    }
}
fn quarter(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[a] = x[a].wrapping_add(x[b]);
    x[d] = (x[d] ^ x[a]).rotate_left(16);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] = (x[b] ^ x[c]).rotate_left(12);
    x[a] = x[a].wrapping_add(x[b]);
    x[d] = (x[d] ^ x[a]).rotate_left(8);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] = (x[b] ^ x[c]).rotate_left(7);
}
fn decrypt(input: &[u8], key: &str, salt: &[u8]) -> Vec<u8> {
    let mut hash = Sha3_384::new();
    for word in key.encode_utf16() {
        hash.update(word.to_le_bytes());
    }
    hash.update(salt);
    let material = hash.finalize();
    let mut initial = [0; 16];
    for (i, chunk) in b"expand 32-byte k".chunks_exact(4).enumerate() {
        initial[i] = u32::from_le_bytes(chunk.try_into().unwrap());
    }
    for i in 0..8 {
        initial[i + 4] = u32_at(&material, 4 * i).unwrap();
    }
    initial[14] = u32_at(&material, 32).unwrap();
    initial[15] = u32_at(&material, 36).unwrap();
    let mut output = Vec::with_capacity(input.len());
    for (block, src) in input.chunks(64).enumerate() {
        initial[12] = u32_at(&material, 40).unwrap() ^ block as u32;
        initial[13] = u32_at(&material, 44).unwrap();
        let mut x = initial;
        for _ in 0..4 {
            for (a, b, c, d) in [
                (0, 4, 8, 12),
                (1, 5, 9, 13),
                (2, 6, 10, 14),
                (3, 7, 11, 15),
                (0, 5, 10, 15),
                (1, 6, 11, 12),
                (2, 7, 8, 13),
                (3, 4, 9, 14),
            ] {
                quarter(&mut x, a, b, c, d);
            }
        }
        let stream: Vec<_> = x
            .iter()
            .zip(initial)
            .flat_map(|(a, b)| a.wrapping_add(b).to_le_bytes())
            .collect();
        output.extend(src.iter().zip(stream).map(|(a, b)| a ^ b));
    }
    output
}
fn utf16(bytes: &[u8]) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let words: Vec<_> = bytes
        .chunks_exact(2)
        .map(|x| u16::from_le_bytes([x[0], x[1]]))
        .collect();
    Some(
        String::from_utf16(&words)
            .ok()?
            .trim_end_matches('\0')
            .to_owned(),
    )
}
fn item<'a>(bytes: &'a [u8], name: &[u8], len: usize) -> Option<&'a [u8]> {
    for (at, found) in bytes.windows(name.len()).enumerate() {
        if found == name && u16_at(bytes, at + name.len())? as usize == len {
            let start = at + name.len() + 2;
            return bytes.get(start..start.checked_add(len)?);
        }
    }
    None
}
fn permutation(bytes: &[u8]) -> bool {
    let mut sorted = bytes.to_vec();
    sorted.sort_unstable();
    sorted.into_iter().eq(0..bytes.len() as u8)
}
/// Sidecar contains parameters and a control table, never executable bytes.
/// Unrecognized engines/versions return None and retain the normal detectors.
pub(super) fn recover(bytes: &[u8]) -> Option<Vec<u8>> {
    let pe = Pe::new(bytes)?;
    let encrypted = pe.resource("RCDATA", "STARTUP.TJS")?;
    let bootstrap = pe.resource("RCDATA", "BOOTSTRAP")?;
    let path = utf16(pe.resource("TEXT", "127")?)?;
    let key = path.strip_prefix("bres://./")?.strip_suffix('/')?;
    if key.is_empty() || key.len() > 128 || key.contains(['/', '\\']) {
        return None;
    }
    // The salt pointer and its 0x2000-byte length are adjacent MOVs. Validate
    // every candidate by the decrypted TJS signature; no fixed game addresses.
    for code in bytes.windows(20) {
        if code[..2] != [0xc7, 0x05]
            || code[10..12] != [0xc7, 0x05]
            || code[16..20] != [0, 0x20, 0, 0]
        {
            continue;
        }
        let Some(rva) = u32_at(code, 6).and_then(|va| va.checked_sub(pe.base)) else {
            continue;
        };
        let Some(off) = pe.offset(rva, 8192) else {
            continue;
        };
        let salt = &bytes[off..off + 8192];
        let startup = decrypt(encrypted, key, salt);
        if !startup.starts_with(b"TJS2100\0") {
            continue;
        }
        // Bootstrap paths are ASCII UTF-16 constants in the compiled script.
        let marker: Vec<_> = "bres://./"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        for at in 0..startup.len().saturating_sub(marker.len()) {
            if startup[at..at + marker.len()] != marker {
                continue;
            }
            let text: Vec<_> = startup[at..]
                .chunks_exact(2)
                .take(160)
                .map(|x| u16::from_le_bytes([x[0], x[1]]))
                .take_while(|&c| (32..127).contains(&c))
                .collect();
            let text = String::from_utf16(&text).ok()?;
            let Some(key) = text
                .strip_prefix("bres://./")
                .and_then(|s| s.strip_suffix("/bootstrap"))
            else {
                continue;
            };
            if key.is_empty() || key.contains(['/', '\\']) {
                continue;
            }
            let packed = decrypt(bootstrap, key, salt);
            let size = u32_at(&packed, 4)? as usize;
            if size > MAX_RESOURCE || u32_at(&packed, 0)? as usize != packed.len().checked_sub(8)? {
                continue;
            }
            let mut dll = Vec::new();
            ZlibDecoder::new(packed.get(8..)?)
                .take((MAX_RESOURCE + 1) as u64)
                .read_to_end(&mut dll)
                .ok()?;
            if dll.len() != size || !dll.starts_with(b"MZ") {
                continue;
            }
            let params = item(&dll, b"PARAMS\0", 22)?;
            // Other PRNG modes need a separate evaluator.
            if params[17] != 0
                || !permutation(&params[..8])
                || !permutation(&params[8..14])
                || !permutation(&params[14..17])
            {
                return None;
            }
            let table = item(&dll, b"TABLE\0", 4096)?;
            return Some([MAGIC.as_slice(), params, table].concat());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    fn put16(b: &mut [u8], at: usize, v: u16) {
        b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put32(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn wide(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    // Entirely synthetic PE/resource tree: no game bytes or real key material.
    fn fixture(params: &[u8; 22]) -> (Vec<u8>, Vec<u8>) {
        let table: Vec<_> = (0..4096).map(|i| (i * 73 + (i >> 3) * 19) as u8).collect();
        let mut dll = b"MZsynthetic-plugin\0PARAMS\0\x16\0".to_vec();
        dll.extend(params);
        dll.extend(b"TABLE\0\0\x10");
        dll.extend(&table);
        let mut zip = ZlibEncoder::new(Vec::new(), Compression::default());
        zip.write_all(&dll).unwrap();
        let zip = zip.finish().unwrap();
        let packed = [
            (zip.len() as u32).to_le_bytes().as_slice(),
            (dll.len() as u32).to_le_bytes().as_slice(),
            &zip,
        ]
        .concat();
        let salt: Vec<_> = (0..8192).map(|i| (i % 251) as u8).collect();
        let startup = [
            b"TJS2100\0".as_slice(),
            &wide("bres://./synthetic-bootstrap/bootstrap\0"),
        ]
        .concat();
        let payloads = [
            decrypt(&startup, "synthetic-startup", &salt),
            decrypt(&packed, "synthetic-bootstrap", &salt),
            wide("bres://./synthetic-startup/\0"),
        ];
        let mut b = vec![0u8; 0x10000 + 0x200];
        b[..2].copy_from_slice(b"MZ");
        put32(&mut b, 60, 0x80);
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        put16(&mut b, 0x86, 1);
        put16(&mut b, 0x94, 224);
        put16(&mut b, 0x98, 0x10b);
        put32(&mut b, 0xb4, 0x400000);
        put32(&mut b, 0x108, 0x1100);
        put32(&mut b, 0x178 + 12, 0x1000);
        put32(&mut b, 0x178 + 16, 0x10000);
        put32(&mut b, 0x178 + 20, 0x200);
        b[0x220..0x234].copy_from_slice(&[
            0xc7, 5, 0, 0, 0, 0, 0, 0x50, 0x40, 0, 0xc7, 5, 0, 0, 0, 0, 0, 0x20, 0, 0,
        ]);
        b[0x4200..0x6200].copy_from_slice(&salt);
        let r = 0x300;
        // Root -> RCDATA/TEXT -> resource name/id -> language -> data entry.
        put16(&mut b, r + 12, 1);
        put16(&mut b, r + 14, 1);
        for (at, id, target) in [
            (16, 0x80000180, 0x70),
            (24, 10, 0x30),
            (0x40, 0x800001a0, 0xa0),
            (0x48, 0x800001c0, 0xc0),
            (0x80, 127, 0xe0),
        ] {
            put32(&mut b, r + at, id);
            put32(&mut b, r + at + 4, 0x80000000 | target);
        }
        put16(&mut b, r + 0x30 + 12, 2);
        put16(&mut b, r + 0x70 + 14, 1);
        for (directory, data) in [(0xa0, 0x100), (0xc0, 0x110), (0xe0, 0x120)] {
            put16(&mut b, r + directory + 14, 1);
            put32(&mut b, r + directory + 16, 1041);
            put32(&mut b, r + directory + 20, data);
        }
        for (at, text) in [
            (0x180, "TEXT"),
            (0x1a0, "STARTUP.TJS"),
            (0x1c0, "BOOTSTRAP"),
        ] {
            put16(&mut b, r + at, text.len() as u16);
            let w = wide(text);
            b[r + at + 2..r + at + 2 + w.len()].copy_from_slice(&w);
        }
        let mut at = 0x1200;
        for (i, payload) in payloads.iter().enumerate() {
            put32(&mut b, r + 0x100 + i * 16, (at - 0x200 + 0x1000) as u32);
            put32(&mut b, r + 0x104 + i * 16, payload.len() as u32);
            b[at..at + payload.len()].copy_from_slice(payload);
            at += payload.len();
        }
        (b, [MAGIC.as_slice(), params.as_slice(), &table].concat())
    }
    const PARAMS: [u8; 22] = [
        0, 1, 2, 3, 4, 5, 6, 7, 0, 1, 2, 3, 4, 5, 0, 1, 2, 0, 0x55, 1, 0x23, 1,
    ];

    #[test]
    fn resource_cipher_matches_independent_multiblock_vector() {
        // Reference computed with a separate Python SHA3/ChaCha8 implementation.
        let salt: Vec<_> = (0..8192).map(|i| (i % 251) as u8).collect();
        let source: Vec<_> = (0..130).collect();
        let bytes = decrypt(&source, "synthetic-key", &salt);
        let hash = bytes.iter().fold(14695981039346656037u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(1099511628211)
        });
        assert_eq!(hash, 0x27cb6ae6517219ab);
    }

    #[test]
    fn recovers_parameters_and_rejects_wrong_keys_and_malformed_resources() {
        let (bytes, expected) = fixture(&PARAMS);
        assert_eq!(recover(&bytes), Some(expected));
        let mut wrong_key = bytes.clone();
        wrong_key[0x4200] ^= 1;
        assert_eq!(recover(&wrong_key), None);
        let mut huge = bytes.clone();
        put32(&mut huge, 0x404, u32::MAX);
        assert_eq!(recover(&huge), None);
        for end in [
            0,
            2,
            63,
            0x80,
            0x98,
            0x178,
            0x200,
            0x300,
            0x450,
            0x1200,
            bytes.len() - 1,
        ] {
            assert_eq!(recover(&bytes[..end]), None, "truncated at {end:x}");
        }
    }
    #[test]
    fn rejects_unsupported_generators_and_duplicate_selectors() {
        let mut params = PARAMS;
        params[17] = 1;
        assert_eq!(recover(&fixture(&params).0), None);
        params = PARAMS;
        params[2] = params[1];
        assert_eq!(recover(&fixture(&params).0), None);
    }
}
