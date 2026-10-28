//! CPB pictures ("CPB\x1a"): 24 or 32 bit, each channel a CRC32 and a zlib stream.
//!
//! The layout found in Triptych: type at 4, bits at 5, 0 1 at 6, width (u16) at 8, height (u16)
//! at 14, the four stream sizes at 16 and the streams from 32, in the order alpha, blue, green,
//! red (GARbro's version 1 map). A size of 0 leaves that channel out (no alpha: opaque).

use nexas::gfx::Image;

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

pub fn decode(d: &[u8]) -> Result<Image, String> {
    if !d.starts_with(b"CPB\x1a") || d.len() < 32 {
        return Err("not a CPB".into());
    }
    let bpp = d[5];
    if bpp != 24 && bpp != 32 {
        return Err(format!("CPB: {bpp} bits"));
    }
    let w = u16::from_le_bytes([d[8], d[9]]) as u32;
    let h = u16::from_le_bytes([d[14], d[15]]) as u32;
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err("CPB: bad size".into());
    }
    let sizes: Vec<usize> = (0..4)
        .map(|i| u32_at(d, 16 + i * 4).unwrap() as usize)
        .collect();
    let n = (w * h) as usize;
    let mut img = Image::new(w, h);
    let mut alpha = false;
    let mut pos = 32usize;
    // streams in this order hold these RGBA channels
    for (stream, chan) in [(0, 3), (3, 2), (1, 1), (2, 0)] {
        let size = sizes[stream];
        if size == 0 {
            continue;
        }
        let end = pos.checked_add(size).ok_or("CPB: short")?;
        let z = d.get(pos + 4..end).ok_or("CPB: short")?;
        let plane =
            miniz_oxide::inflate::decompress_to_vec_zlib(z).map_err(|e| format!("CPB: {e:?}"))?;
        for (i, v) in plane.iter().take(n).enumerate() {
            img.px[i * 4 + chan] = *v;
        }
        alpha |= chan == 3;
        pos = end;
    }
    if !alpha || bpp == 24 {
        for p in img.px.chunks_exact_mut(4) {
            p[3] = 255;
        }
    }
    Ok(img)
}
