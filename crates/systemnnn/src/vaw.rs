//! VAW / WGQ sounds: a 0x40-byte header ("PACKTYPE=n" at 0x30) and the sound: 0 a RIFF WAVE,
//! 1 a WAVE whose samples are packed (each a 4-bit length and that many bits of difference),
//! 2 Ogg Vorbis at 0x6c, 6 ("OGG " at 0x10) Ogg Vorbis. [`playable`] gives bytes a browser can
//! decode.

use crate::dwq::pack_type;

pub fn playable(b: &[u8]) -> Option<Vec<u8>> {
    if b.starts_with(b"RIFF") || b.starts_with(b"OggS") {
        return Some(b.to_vec());
    }
    let (kind, _) = pack_type(b)?;
    match kind {
        0 => Some(b.get(0x40..)?.to_vec()),
        1 => unpack(b.get(0x40..)?),
        _ => {
            // the Ogg stream, wherever it starts after the header
            let at = b.windows(4).skip(0x40).position(|w| w == b"OggS")? + 0x40;
            Some(b[at..].to_vec())
        }
    }
}

/// Type 1: the RIFF header up to the end of "fmt ", then the packed samples (16-bit mono or
/// stereo, one running value).
fn unpack(b: &[u8]) -> Option<Vec<u8>> {
    let fmt_size = u32::from_le_bytes(b.get(0x10..0x14)?.try_into().ok()?) as usize;
    let header = 0x14 + fmt_size;
    let riff = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?) as usize + 8;
    let fmt = b.get(0x14..header)?;
    let size = riff.checked_sub(header + 8)? & !1;
    if size > 64 << 20 {
        return None;
    }
    let mut out = Vec::with_capacity(44 + size);
    out.extend(b"RIFF");
    out.extend(((36 + size) as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(fmt.get(..16)?);
    out.extend(b"data");
    out.extend((size as u32).to_le_bytes());
    let src = &b[header..];
    let (mut pos, mut sample) = (0usize, 0i16);
    let mut bits = |n: u32| -> i32 {
        let mut v = 0i32;
        for i in 0..n {
            let byte = src.get(pos / 8).copied().unwrap_or(0);
            v |= (((byte >> (pos % 8)) & 1) as i32) << i;
            pos += 1;
        }
        v
    };
    for _ in 0..size / 2 {
        let c = bits(4) as u32;
        let mut code = 0i32;
        if c > 0 {
            code = bits(c) << (32 - c) >> (32 - c);
            let sign = code >> 31;
            code ^= 1 << (c - 1);
            code -= sign;
        }
        sample = sample.wrapping_add(code as i16);
        out.extend(sample.to_le_bytes());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ogg_after_header() {
        let mut b = vec![b' '; 0x40];
        b[0x30..0x3a].copy_from_slice(b"PACKTYPE=6");
        b.extend(b"OggS....");
        assert_eq!(playable(&b).unwrap(), b"OggS....");
    }
}
