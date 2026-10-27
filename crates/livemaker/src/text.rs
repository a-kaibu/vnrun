//! Drawing text with a TrueType font (the games name Windows fonts such as ＭＳ Ｐゴシック; the
//! page's font stands in), with an optional one-pixel border.

use std::collections::HashMap;

use nexas::gfx::Image;

pub struct Fonts {
    font: Option<fontdue::Font>,
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
}

/// Delphi TColor ($00BBGGRR) as RGB.
pub fn rgb(c: i64) -> [u8; 3] {
    let c = c as u32;
    [c as u8, (c >> 8) as u8, (c >> 16) as u8]
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    pub fn new() -> Fonts {
        Fonts {
            font: None,
            cache: HashMap::new(),
        }
    }

    pub fn set(&mut self, ttf: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        self.font = Some(f);
        self.cache.clear();
        Ok(())
    }

    fn glyph(&mut self, c: char, size: u32) -> Option<&(fontdue::Metrics, Vec<u8>)> {
        let font = self.font.as_ref()?;
        Some(
            self.cache
                .entry((c, size))
                .or_insert_with(|| font.rasterize(c, size as f32)),
        )
    }

    /// How far the pen moves for `c`.
    pub fn advance(&mut self, c: char, size: u32) -> i32 {
        match self.glyph(c, size) {
            Some((m, _)) => m.advance_width.round() as i32,
            None => {
                if (c as u32) < 0x100 {
                    size as i32 / 2
                } else {
                    size as i32
                }
            }
        }
    }

    /// Draws `c` with its cell's top left at (x, y).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        out: &mut Image,
        x: i32,
        y: i32,
        c: char,
        size: u32,
        color: [u8; 3],
        border: Option<[u8; 3]>,
    ) {
        let Some(font) = self.font.as_ref() else {
            return;
        };
        let ascent = font
            .horizontal_line_metrics(size as f32)
            .map_or(size as f32 * 0.88, |m| m.ascent);
        let Some((m, bmp)) = self.glyph(c, size) else {
            return;
        };
        let (m, bmp) = (*m, bmp.clone());
        let gx = x + m.xmin;
        let gy = y + (ascent - m.height as f32 - m.ymin as f32).round() as i32;
        let (w, h) = (m.width as i32, m.height as i32);
        let at = |px: i32, py: i32| -> u32 {
            if px < 0 || py < 0 || px >= w || py >= h {
                0
            } else {
                bmp[(py * w + px) as usize] as u32
            }
        };
        let mut put = |px: i32, py: i32, c: [u8; 3], a: u32| {
            if a == 0 || px < 0 || py < 0 || px >= out.w as i32 || py >= out.h as i32 {
                return;
            }
            let i = ((py as u32 * out.w + px as u32) * 4) as usize;
            let d = &mut out.px[i..i + 4];
            let da = d[3] as u32;
            let oa = a + da * (255 - a) / 255;
            if oa == 0 {
                return;
            }
            for k in 0..3 {
                d[k] = ((c[k] as u32 * a + d[k] as u32 * da * (255 - a) / 255) / oa) as u8;
            }
            d[3] = oa as u8;
        };
        if let Some(b) = border {
            for py in -1..=h {
                for px in -1..=w {
                    let mut a = 0;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            a = a.max(at(px + dx, py + dy));
                        }
                    }
                    put(gx + px, gy + py, b, a);
                }
            }
        }
        for py in 0..h {
            for px in 0..w {
                put(gx + px, gy + py, color, at(px, py));
            }
        }
    }
}
