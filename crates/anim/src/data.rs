//! The game's files: the GAX cipher of the scenario, CWP pictures, PCK archives and EOG sounds.

use nexas::gfx::Image;

/// Decrypts a file that starts 00 00 00 01 (the scenario, the define table and GAX pictures):
/// a 16-byte key follows, then the data XORed with it 16 bytes at a time; after each block the
/// key changes by the block's decrypted byte 14 (GARbro's GaxTransform).
pub fn gax_decrypt(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 20 || b[..4] != [0, 0, 0, 1] {
        return None;
    }
    let mut k: [u8; 16] = b[4..20].try_into().ok()?;
    let src = &b[20..];
    let mut out = vec![0u8; src.len()];
    let mut i = 0;
    while i < src.len() {
        let n = (src.len() - i).min(16);
        for j in 0..n {
            out[i + j] = src[i + j] ^ k[j];
        }
        if n < 16 {
            break;
        }
        let m = out[i + 14];
        i += 16;
        let a = |x: u8, y: u8| x.wrapping_add(y);
        match m & 7 {
            0 => {
                k[0] = a(k[0], m);
                k[3] = a(k[3], a(m, 2));
                k[4] = a(a(k[2], m), 11);
                k[8] = a(k[6], 7);
            }
            1 => {
                k[2] = a(k[9], k[10]);
                k[6] = a(k[7], k[15]);
                k[8] = a(k[8], k[1]);
                k[15] = a(k[3], k[5]);
            }
            2 => {
                k[1] = a(k[1], k[2]);
                k[5] = a(k[5], k[6]);
                k[7] = a(k[7], k[8]);
                k[10] = a(k[10], k[11]);
            }
            3 => {
                k[9] = a(k[1], k[2]);
                k[11] = a(k[5], k[6]);
                k[12] = a(k[7], k[8]);
                k[13] = a(k[10], k[11]);
            }
            4 => {
                k[0] = a(k[1], 0x6F);
                k[3] = a(k[4], 0x47);
                k[4] = a(k[5], 0x11);
                k[14] = a(k[15], 0x40);
            }
            5 => {
                k[2] = a(k[2], k[10]);
                k[4] = a(k[5], k[12]);
                k[6] = a(k[8], k[14]);
                k[8] = a(k[0], k[11]);
            }
            t => {
                if t == 6 {
                    k[9] = a(k[1], k[11]);
                    k[11] = a(k[3], k[13]);
                    k[13] = a(k[5], k[15]);
                    k[15] = a(k[7], k[9]);
                }
                k[1] = a(k[5], k[9]);
                k[2] = a(k[6], k[10]);
                k[3] = a(k[7], k[11]);
                k[4] = a(k[8], k[12]);
            }
        }
    }
    Some(out)
}

/// A CWP picture: a PNG without its signature and chunk names ("CWDP", the IHDR fields and
/// CRC, then the IDAT chunks), with red and blue swapped. The alpha channel is not used.
pub fn cwp(b: &[u8]) -> Result<Image, String> {
    let png = if b.starts_with(b"CWDP") || b.starts_with(b"AMNP") {
        if b.len() < 0x19 {
            return Err("CWP: too short".into());
        }
        let mut png = Vec::with_capacity(b.len() + 40);
        png.extend_from_slice(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR");
        png.extend_from_slice(&b[4..0x19]);
        png.extend_from_slice(b"IDAT");
        png.extend_from_slice(&b[0x19..]);
        png.extend_from_slice(b"\0\0\0IEND\xaeB`\x82");
        png
    } else if let Some(d) = gax_decrypt(b) {
        // GAX (2011): an encrypted PNG, colours and alpha as they are
        return Image::decode(&d);
    } else {
        b.to_vec()
    };
    let mut img = Image::decode(&png)?;
    for p in img.px.chunks_exact_mut(4) {
        p.swap(0, 2);
        p[3] = 255;
    }
    Ok(img)
}

/// The left half of a picture with its mask (the right half, white where it shows) as alpha.
pub fn masked(img: &Image) -> Image {
    let w = img.w / 2;
    let mut out = Image::new(w, img.h);
    for y in 0..img.h {
        for x in 0..w {
            let s = ((y * img.w + x) * 4) as usize;
            let m = ((y * img.w + x + w) * 4) as usize;
            let d = ((y * w + x) * 4) as usize;
            out.px[d..d + 3].copy_from_slice(&img.px[s..s + 3]);
            let a = (img.px[m] as u32 + img.px[m + 1] as u32 + img.px[m + 2] as u32) / 3;
            out.px[d + 3] = a as u8;
        }
    }
    out
}

/// The top half of a picture with its mask (the bottom half) as alpha, like the choice bars of
/// p_select.cwp: two bars, then their two masks.
pub fn masked_below(img: &Image) -> Image {
    let h = img.h / 2;
    let mut out = Image::new(img.w, h);
    for y in 0..h {
        for x in 0..img.w {
            let s = ((y * img.w + x) * 4) as usize;
            let m = (((y + h) * img.w + x) * 4) as usize;
            out.px[s..s + 3].copy_from_slice(&img.px[s..s + 3]);
            let a = (img.px[m] as u32 + img.px[m + 1] as u32 + img.px[m + 2] as u32) / 3;
            out.px[s + 3] = a as u8;
        }
    }
    out
}

/// An EOG sound: "CRM\0", a size, then Ogg Vorbis.
pub fn eog(b: Vec<u8>) -> Vec<u8> {
    if b.starts_with(b"CRM\0") && b.len() > 8 {
        b[8..].to_vec()
    } else {
        b
    }
}

/// A PCK archive: a count, then (0, offset, size) for each file, then the names.
pub struct Pck {
    pub entries: Vec<(String, u64, usize)>,
}

impl Pck {
    pub fn parse(head: &[u8]) -> Option<Pck> {
        let n = u32::from_le_bytes(head.get(..4)?.try_into().ok()?) as usize;
        if n == 0 || n > 0xfffff {
            return None;
        }
        let mut names = 4 + 12 * n;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let e = 4 + 12 * i;
            let off = u32::from_le_bytes(head.get(e + 4..e + 8)?.try_into().ok()?) as u64;
            let size = u32::from_le_bytes(head.get(e + 8..e + 12)?.try_into().ok()?) as usize;
            let end = names + head.get(names..)?.iter().position(|&c| c == 0)?;
            let name = String::from_utf8_lossy(&head[names..end]).to_lowercase();
            names = end + 1;
            entries.push((name, off, size));
        }
        Some(Pck { entries })
    }

    /// The offset and size of a file, by its name without extension.
    pub fn find(&self, stem: &str) -> Option<(u64, usize)> {
        let s = stem.to_lowercase();
        self.entries
            .iter()
            .find(|(n, _, _)| n.split('.').next() == Some(s.as_str()))
            .map(|e| (e.1, e.2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pck_index() {
        let mut b = vec![2, 0, 0, 0];
        for (off, size) in [(40u32, 3u32), (43, 5)] {
            b.extend(0u32.to_le_bytes());
            b.extend(off.to_le_bytes());
            b.extend(size.to_le_bytes());
        }
        b.extend(b"SE001.eog\0w000001a.eog\0");
        let p = Pck::parse(&b).unwrap();
        assert_eq!(p.find("W000001A"), Some((43, 5)));
        assert_eq!(p.find("se001"), Some((40, 3)));
    }

    #[test]
    fn gax_roundtrip_first_block() {
        // with a zero key the first block is the data itself
        let mut b = vec![0, 0, 0, 1];
        b.extend([0u8; 16]);
        b.extend(b"0123456789abcdef");
        assert_eq!(gax_decrypt(&b).unwrap(), b"0123456789abcdef");
    }
}
