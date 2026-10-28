//! Labels: text drawn with the game's own font (or the page's), with a border.

use std::collections::HashMap;

use crate::gfx::Img;

#[derive(Default)]
pub struct Font {
    font: Option<fontdue::Font>,
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
}

/// How a label is drawn.
pub struct Style {
    pub size: f32,
    pub color: [u8; 4],
    pub stroke: [u8; 4],
    pub stroke_px: i32,
    /// 0 left, 1 centre, 2 right
    pub align: i64,
    /// 0 top, 1 middle, 2 bottom
    pub valign: i64,
    pub pitch: f32,
}

impl Font {
    /// A TrueType/OpenType font, or the first font of a collection.
    pub fn set(&mut self, ttf: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(
            ttf,
            fontdue::FontSettings {
                collection_index: 0,
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
        self.font = Some(f);
        self.cache.clear();
        Ok(())
    }

    pub fn ready(&self) -> bool {
        self.font.is_some()
    }

    fn glyph(&mut self, c: char, size: u32) -> Option<&(fontdue::Metrics, Vec<u8>)> {
        let font = self.font.as_ref()?;
        if self.cache.len() > 4000 {
            self.cache.clear();
        }
        Some(
            self.cache
                .entry((c, size))
                .or_insert_with(|| font.rasterize(c, size as f32)),
        )
    }

    fn advance(&mut self, c: char, size: u32) -> f32 {
        self.glyph(c, size)
            .map_or(size as f32, |g| g.0.advance_width)
    }

    /// Breaks `text` into lines no wider than `w`; a line break may not come before closing
    /// punctuation.
    fn lines(&mut self, text: &str, w: f32, size: u32) -> Vec<(Vec<char>, f32)> {
        let mut out: Vec<(Vec<char>, f32)> = vec![(Vec::new(), 0.0)];
        for c in text.chars() {
            if c == '\n' {
                out.push((Vec::new(), 0.0));
                continue;
            }
            let adv = self.advance(c, size);
            let last = out.last_mut().unwrap();
            let closing =
                "、。，．）」』】〉》！？…ー〜ぁぃぅぇぉっゃゅょァィゥェォッャュョ".contains(c);
            if last.1 + adv > w && !last.0.is_empty() && !closing {
                out.push((vec![c], adv));
            } else {
                last.0.push(c);
                last.1 += adv;
            }
        }
        out
    }

    /// `text` in a `w` x `h` box (pixels).
    pub fn render(&mut self, text: &str, w: u32, h: u32, st: &Style) -> Img {
        let mut out = Img::new(w.max(1), h.max(1));
        let (w, h) = (out.w as i32, out.h as i32);
        if self.font.is_none() || text.is_empty() {
            return out;
        }
        let size = st.size.round().max(4.0) as u32;
        let ascent = self
            .font
            .as_ref()
            .and_then(|f| f.horizontal_line_metrics(size as f32))
            .map_or(size as f32 * 0.88, |m| m.ascent);
        let pad = st.stroke_px.max(0);
        let lines = self.lines(text, (w - 2 * pad) as f32, size);
        let mut cover = vec![0u8; (w * h) as usize];
        let total = lines.len() as f32 * st.pitch - (st.pitch - size as f32 * 1.2).max(0.0);
        let mut y = match st.valign {
            1 => ((h as f32 - total) / 2.0).max(0.0),
            2 => (h as f32 - total - pad as f32).max(0.0),
            _ => pad as f32,
        };
        for (line, lw) in lines {
            let mut x = match st.align {
                1 => (w as f32 - lw) / 2.0,
                2 => w as f32 - lw - pad as f32,
                _ => pad as f32,
            };
            for c in line {
                let adv = self.advance(c, size);
                if let Some((m, bmp)) = self.glyph(c, size) {
                    let gx = (x + m.xmin as f32).round() as i32;
                    let gy = (y + ascent - m.height as f32 - m.ymin as f32).round() as i32;
                    for (k, &v) in bmp.iter().enumerate() {
                        let (px, py) = (gx + (k % m.width) as i32, gy + (k / m.width) as i32);
                        if px >= 0 && py >= 0 && px < w && py < h {
                            let i = (py * w + px) as usize;
                            cover[i] = cover[i].max(v);
                        }
                    }
                }
                x += adv;
            }
            y += st.pitch;
        }
        let r = st.stroke_px;
        for py in 0..h {
            for px in 0..w {
                let v = cover[(py * w + px) as usize] as u32;
                let mut border = 0u32;
                if r > 0 && st.stroke[3] > 0 {
                    'b: for dy in -r..=r {
                        for dx in -r..=r {
                            let (sx, sy) = (px + dx, py + dy);
                            if sx >= 0 && sy >= 0 && sx < w && sy < h {
                                border = border.max(cover[(sy * w + sx) as usize] as u32);
                                if border == 255 {
                                    break 'b;
                                }
                            }
                        }
                    }
                }
                let i = ((py * w + px) * 4) as usize;
                let ba = border * st.stroke[3] as u32 / 255;
                let ta = v * st.color[3] as u32 / 255;
                // text over its border
                let a = ta + ba * (255 - ta) / 255;
                if a == 0 {
                    continue;
                }
                for k in 0..3 {
                    let c =
                        (st.color[k] as u32 * ta + st.stroke[k] as u32 * ba * (255 - ta) / 255) / a;
                    out.px[i + k] = c.min(255) as u8;
                }
                out.px[i + 3] = a.min(255) as u8;
            }
        }
        out
    }
}

pub fn rgba(c: u32) -> [u8; 4] {
    c.to_be_bytes()
}
