//! Pictures: "AP" (u32 width, u32 height, u16 bits, then BGRA rows bottom up) and plain
//! 24/32-bit BMPs.

use nexas::gfx::Image;

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

pub fn decode(b: &[u8]) -> Result<Image, String> {
    if b.starts_with(b"AP") {
        let w = u32le(b, 2).ok_or("short AP")?;
        let h = u32le(b, 6).ok_or("short AP")?;
        if w == 0 || h == 0 || w > 0x4000 || h > 0x4000 {
            return Err("bad AP size".into());
        }
        let data = b.get(12..12 + (w * h * 4) as usize).ok_or("short AP")?;
        let mut img = Image::new(w, h);
        let stride = w as usize * 4;
        for y in 0..h as usize {
            let s = &data[(h as usize - 1 - y) * stride..][..stride];
            let d = &mut img.px[y * stride..][..stride];
            for (d, s) in d.chunks_exact_mut(4).zip(s.chunks_exact(4)) {
                d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
            }
        }
        return Ok(img);
    }
    if b.starts_with(b"BM") {
        let off = u32le(b, 10).ok_or("short BMP")? as usize;
        let w = u32le(b, 18).ok_or("short BMP")? as i32;
        let h = u32le(b, 22).ok_or("short BMP")? as i32;
        let bpp = u16::from_le_bytes(b.get(28..30).ok_or("short BMP")?.try_into().unwrap());
        if w <= 0 || h == 0 || w > 0x4000 || h.abs() > 0x4000 || !(bpp == 24 || bpp == 32) {
            return Err("unsupported BMP".into());
        }
        let (uh, flip) = (h.unsigned_abs(), h > 0);
        let n = (bpp / 8) as usize;
        let stride = (w as usize * n + 3) & !3;
        let mut img = Image::new(w as u32, uh);
        for y in 0..uh as usize {
            let row = if flip { uh as usize - 1 - y } else { y };
            let Some(s) = b.get(off + row * stride..off + row * stride + w as usize * n) else {
                break;
            };
            let d = &mut img.px[y * w as usize * 4..][..w as usize * 4];
            for (d, s) in d.chunks_exact_mut(4).zip(s.chunks_exact(n)) {
                d.copy_from_slice(&[s[2], s[1], s[0], 255]);
            }
        }
        return Ok(img);
    }
    Err("unknown picture".into())
}
