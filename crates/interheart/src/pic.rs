//! Pictures: KG (sprites with alpha), BMP (also the "BMP24RLE" packed kind) and HMP hover maps.
//!
//! ```text
//! KG:  "GCGK" w:u16 h:u16 packed:u32  h x row_offset:u32  rows: runs of alpha:u8 n:u8 (0 = 256)
//!      and, unless alpha is 0, n x {r g b}
//! HMP: w:u32 h:u32 w*h bytes: the number of the button under each pixel (0: none)
//! BMP24RLE: "BMP24RLE" then a BMP whose header is kept and whose pixels repeat a colour that
//!      comes twice in a row count:u16 more times
//! ```

use nexas::gfx::Image;

pub fn decode(b: &[u8]) -> Result<Image, String> {
    if b.starts_with(b"GCGK") {
        return kg(b);
    }
    if b.starts_with(b"BMP24RLE") {
        return Image::decode(&bmp_rle(b)?);
    }
    Image::decode(b)
}

fn kg(b: &[u8]) -> Result<Image, String> {
    if b.len() < 12 {
        return Err("KG: too short".into());
    }
    let w = u16::from_le_bytes([b[4], b[5]]) as usize;
    let h = u16::from_le_bytes([b[6], b[7]]) as usize;
    let base = 12 + 4 * h;
    if b.len() < base {
        return Err("KG: cut short".into());
    }
    let mut img = Image::new(w as u32, h as u32);
    for y in 0..h {
        let mut p =
            base + u32::from_le_bytes(b[12 + 4 * y..16 + 4 * y].try_into().unwrap()) as usize;
        let mut x = 0;
        while x < w && p + 2 <= b.len() {
            let a = b[p];
            let n = if b[p + 1] == 0 {
                256
            } else {
                b[p + 1] as usize
            };
            p += 2;
            if a != 0 {
                for i in 0..n {
                    if p + 3 > b.len() || x + i >= w {
                        break;
                    }
                    let q = (y * w + x + i) * 4;
                    img.px[q..q + 4].copy_from_slice(&[b[p], b[p + 1], b[p + 2], a]);
                    p += 3;
                }
            }
            x += n;
        }
    }
    Ok(img)
}

fn bmp_rle(b: &[u8]) -> Result<Vec<u8>, String> {
    if b.len() < 0x26 {
        return Err("BMP24RLE: too short".into());
    }
    let unpacked = u32::from_le_bytes(b[0x0A..0x0E].try_into().unwrap()) as usize;
    let header = u32::from_le_bytes(b[0x12..0x16].try_into().unwrap()) as usize;
    if 8 + header > b.len() || unpacked > 64 << 20 {
        return Err("BMP24RLE: bad header".into());
    }
    let mut out = vec![0u8; unpacked];
    out[..header].copy_from_slice(&b[8..8 + header]);
    let mut dst = header;
    let mut p = 8 + header;
    let take = |p: &mut usize| -> Option<[u8; 3]> {
        let v = b.get(*p..*p + 3)?;
        *p += 3;
        Some([v[0], v[1], v[2]])
    };
    let mut cur = take(&mut p).unwrap_or_default();
    while dst + 3 <= out.len() {
        out[dst..dst + 3].copy_from_slice(&[cur[2], cur[1], cur[0]]);
        dst += 3;
        let Some(next) = take(&mut p) else { break };
        if next == cur {
            if dst + 3 > out.len() {
                break;
            }
            out[dst..dst + 3].copy_from_slice(&[cur[2], cur[1], cur[0]]);
            dst += 3;
            let count = b
                .get(p..p + 2)
                .map_or(0, |c| u16::from_le_bytes([c[0], c[1]]));
            p += 2;
            for _ in 0..count {
                if dst + 3 > out.len() {
                    break;
                }
                out[dst..dst + 3].copy_from_slice(&[cur[2], cur[1], cur[0]]);
                dst += 3;
            }
            match take(&mut p) {
                Some(n) => cur = n,
                None => break,
            }
        } else {
            cur = next;
        }
    }
    Ok(out)
}

/// A hover map: which button is under each pixel.
pub struct HoverMap {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl HoverMap {
    pub fn parse(b: &[u8]) -> Option<HoverMap> {
        let w = u32::from_le_bytes(b.get(0..4)?.try_into().ok()?) as usize;
        let h = u32::from_le_bytes(b.get(4..8)?.try_into().ok()?) as usize;
        let px = b.get(8..8 + w * h)?.to_vec();
        Some(HoverMap { w, h, px })
    }

    pub fn at(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return 0;
        }
        self.px[y as usize * self.w + x as usize]
    }

    /// The buttons and the box each covers (x0, y0, x1, y1 exclusive), by number.
    pub fn boxes(&self) -> Vec<(u8, [i32; 4])> {
        let mut b: Vec<Option<[i32; 4]>> = vec![None; 256];
        for y in 0..self.h {
            for x in 0..self.w {
                let v = self.px[y * self.w + x] as usize;
                if v == 0 {
                    continue;
                }
                let (x, y) = (x as i32, y as i32);
                let r = b[v].get_or_insert([x, y, x + 1, y + 1]);
                r[0] = r[0].min(x);
                r[1] = r[1].min(y);
                r[2] = r[2].max(x + 1);
                r[3] = r[3].max(y + 1);
            }
        }
        b.into_iter()
            .enumerate()
            .filter_map(|(v, r)| Some((v as u8, r?)))
            .collect()
    }
}
