//! MGR pictures (GARbro Propeller/ArcMGR.cs and ImageMGR.cs, MIT; BB.exe 0x405620):
//!
//! ```text
//! count:i16, then (count > 1) count offsets:u32
//! at each offset: unpacked:u32 packed:u32 and an LZ-packed BMP
//! ```
//!
//! LZ: a byte c < 0x20 copies c+1 bytes; otherwise the length is 2 plus c>>5 (7: plus the
//! next byte) and the distance ((c&0x1f)<<8) + next byte + 1.
//! 32-bit BMPs carry alpha; 24-bit ones are opaque.

fn u32_(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

pub fn unpack(src: &[u8], n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    let mut p = 0;
    while out.len() < n && p < src.len() {
        let c = src[p] as usize;
        p += 1;
        if c < 0x20 {
            let k = (c + 1).min(src.len() - p);
            out.extend_from_slice(&src[p..p + k]);
            p += k;
        } else {
            let mut cnt = c >> 5;
            if cnt == 7 {
                cnt += *src.get(p).unwrap_or(&0) as usize;
                p += 1;
            }
            let off = ((c & 0x1f) << 8) + *src.get(p).unwrap_or(&0) as usize + 1;
            p += 1;
            if off > out.len() {
                break;
            }
            for _ in 0..cnt + 2 {
                let b = out[out.len() - off];
                out.push(b);
            }
        }
    }
    out.truncate(n);
    out
}

/// The frames of a picture, unpacked to BMP files.
pub fn frames(b: &[u8]) -> Vec<Vec<u8>> {
    let Some(cnt) = b.get(..2).map(|c| i16::from_le_bytes([c[0], c[1]])) else {
        return vec![];
    };
    if cnt <= 0 {
        return vec![];
    }
    let offs: Vec<usize> = if cnt > 1 {
        (0..cnt as usize)
            .filter_map(|i| u32_(b, 2 + 4 * i).map(|o| o as usize))
            .collect()
    } else {
        vec![2]
    };
    offs.into_iter()
        .filter_map(|o| {
            let un = u32_(b, o)? as usize;
            let pk = u32_(b, o + 4)? as usize;
            let src = b.get(o + 8..(o + 8 + pk).min(b.len()))?;
            Some(unpack(src, un.min(64 << 20)))
        })
        .collect()
}

pub use vncommon::bmp::decode as bmp;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz() {
        // 3 literal bytes, then 4 bytes from 3 back
        let src = [2, b'a', b'b', b'c', (2 << 5), 2];
        assert_eq!(unpack(&src, 7), b"abcabca");
    }
}
