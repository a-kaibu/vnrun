//! Message text with a TrueType font (the games name Windows fonts such as メイリオ; the page's
//! font stands in), white with a dark border.

use std::collections::HashMap;

use nexas::gfx::Image;

#[derive(Default)]
pub struct Font {
    font: Option<fontdue::Font>,
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
}

impl Font {
    pub fn set(&mut self, ttf: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        self.font = Some(f);
        self.cache.clear();
        Ok(())
    }

    fn glyph(&mut self, c: char, size: u32) -> Option<(fontdue::Metrics, &Vec<u8>)> {
        let font = self.font.as_ref()?;
        let g = self
            .cache
            .entry((c, size))
            .or_insert_with(|| font.rasterize(c, size as f32));
        Some((g.0, &g.1))
    }

    /// How far the pen moves for `c`.
    pub fn advance(&mut self, c: char, size: u32) -> i32 {
        match self.glyph(c, size) {
            Some((m, _)) => m.advance_width.round() as i32,
            None => size as i32,
        }
    }

    /// Lines of text in `w` x `h`, each `pitch` below the last; only the first `shown`
    /// characters. Text that does not fit in a line goes on to the next.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        text: &str,
        shown: usize,
        w: u32,
        h: u32,
        size: u32,
        pitch: i32,
        color: [u8; 3],
    ) -> Image {
        let mut out = Image::new(w, h);
        let Some(font) = self.font.as_ref() else {
            return out;
        };
        let ascent = font
            .horizontal_line_metrics(size as f32)
            .map_or(size as f32 * 0.88, |m| m.ascent);
        let (mut x, mut y) = (2, 2);
        // coverage of the text and of its border
        let mut cover = vec![0u8; (w * h) as usize];
        for c in text.chars().take(shown) {
            if c == '\n' {
                x = 2;
                y += pitch;
                continue;
            }
            let adv = self.advance(c, size);
            if x + adv > w as i32 - 2 {
                x = 2;
                y += pitch;
            }
            if let Some((m, bmp)) = self.glyph(c, size) {
                let gx = x + m.xmin;
                let gy = y + (ascent - m.height as f32 - m.ymin as f32).round() as i32;
                for (k, &v) in bmp.iter().enumerate() {
                    let (px, py) = (gx + (k % m.width) as i32, gy + (k / m.width) as i32);
                    if px >= 0 && py >= 0 && px < w as i32 && py < h as i32 {
                        let i = (py as u32 * w + px as u32) as usize;
                        cover[i] = cover[i].max(v);
                    }
                }
            }
            x += adv;
        }
        for py in 0..h as i32 {
            for px in 0..w as i32 {
                let mut border = 0u8;
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        let (sx, sy) = (px + dx, py + dy);
                        if sx >= 0 && sy >= 0 && sx < w as i32 && sy < h as i32 {
                            border = border.max(cover[(sy as u32 * w + sx as u32) as usize]);
                        }
                    }
                }
                let v = cover[(py as u32 * w + px as u32) as usize] as u32;
                let i = ((py as u32 * w + px as u32) * 4) as usize;
                // the border is a dark blue-grey, the text over it
                let bc = [40u32, 50, 80];
                for k in 0..3 {
                    out.px[i + k] = ((color[k] as u32 * v + bc[k] * (255 - v)) / 255) as u8;
                }
                out.px[i + 3] = (border as u32 * 3 / 4).max(v) as u8;
            }
        }
        out
    }
}
