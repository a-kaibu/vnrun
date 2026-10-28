//! SSXX containers: `SSXX` + a crypter tag (`DEFL`), a big-endian seed, then a body that is XORed
//! with a xorshift stream keyed by the seed and the body length (sakanagl.dll 0x1800eb6c0 and
//! 0x1800eb390). The plain body starts with the big-endian unpacked size and a zstd frame.

use std::io::Read;

/// XORs `buf` (whole little-endian words only) with the stream for `key`.
pub fn xor_stream(key: u64, buf: &mut [u8]) {
    let lo = key as u32 ^ 0x159a_55e5;
    let hi = (key >> 32) as u32 ^ 0x075b_cd15;
    let t = hi << 11 ^ hi;
    let mut x = t >> 8 ^ t ^ 0x0549_139a;
    let t = lo << 11 ^ lo;
    let mut y = (x >> 11 ^ t) >> 8 ^ t ^ x;
    let mut z = y >> 19 ^ y ^ 0x8e41_5c26;
    let mut w = z >> 19 ^ z ^ 0x4d9d_5bb8;
    for c in buf.chunks_exact_mut(4) {
        let t = x << 11 ^ x;
        let a = (w >> 11 ^ t) >> 8 ^ t ^ w;
        let t = y << 11 ^ y;
        let b = (a >> 11 ^ t) >> 8 ^ t ^ a;
        let o = b << 12 ^ a >> 4;
        let v = u32::from_le_bytes([c[0], c[1], c[2], c[3]]) ^ o;
        c.copy_from_slice(&v.to_le_bytes());
        (x, y, z, w) = (z, w, a, b);
    }
}

/// The key for a body of `len` bytes after the seed word pair.
pub fn key(seed: i32, len: usize) -> u64 {
    let s = seed as i64;
    let k = (len as i64 + s).wrapping_mul(0x3c1).wrapping_sub(0x1e775) ^ s ^ 0x2e6_2e76_034b;
    k as u64
}

/// Decrypts and unpacks a whole SSXX file.
pub fn open(file: &[u8]) -> Result<Vec<u8>, String> {
    if file.len() < 20 || &file[..4] != b"SSXX" {
        return Err("not an SSXX file".into());
    }
    if &file[4..8] != b"DEFL" {
        return Err(format!(
            "unknown crypter {:?}",
            String::from_utf8_lossy(&file[4..8])
        ));
    }
    let seed = i32::from_be_bytes(file[8..12].try_into().unwrap());
    let mut body = file[16..].to_vec();
    xor_stream(key(seed, body.len()), &mut body);
    let size = u32::from_be_bytes(body[..4].try_into().unwrap()) as usize;
    unzstd(&body[4..], size)
}

/// Decodes one stored file: `flags` bit 4 means plain (else XORed with the stream keyed by the
/// stored size and the 16-byte block offset, sakanagl.dll 0x1800eca00), and `flags & 3` means a
/// big-endian unpacked size plus a zstd frame.
pub fn file(raw: &[u8], flags: u16, block: u32) -> Result<Vec<u8>, String> {
    let mut d = raw.to_vec();
    if flags & 0x10 == 0 {
        let k = (raw.len() as u64) << 16 ^ block as u64 ^ 0x2e6_2e76_034b;
        xor_stream(k, &mut d);
    }
    if flags & 3 == 0 {
        return Ok(d);
    }
    if d.len() < 4 {
        return Err("short packed file".into());
    }
    let size = u32::from_be_bytes(d[..4].try_into().unwrap()) as usize;
    unzstd(&d[4..], size)
}

/// Unpacks one zstd frame.
pub fn unzstd(src: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(size);
    let mut d = ruzstd::decoding::StreamingDecoder::new(src).map_err(|e| e.to_string())?;
    d.read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}
