//! Drawing text into layers (CGACT TEXT) with the settings of FONT. Every font name is drawn
//! with the one font the player has.

use crate::image::Image;

#[derive(Debug, Clone)]
pub struct Font {
    pub name: String,
    pub sx: i64,
    pub sy: i64,
    pub color: u32,
    /// the colour at the bottom of each character (a vertical gradient), when set
    pub color2: Option<u32>,
    pub bold: bool,
}

impl Default for Font {
    fn default() -> Self {
        Font { name: String::new(), sx: 24, sy: 24, color: 0xffffff, color2: None, bold: false }
    }
}

pub struct Text {
    font: Option<fontdue::Font>,
}

fn rgb(c: u32) -> [f32; 3] {
    [((c >> 16) & 0xff) as f32, ((c >> 8) & 0xff) as f32, (c & 0xff) as f32]
}

impl Text {
    pub fn new(font: Option<&[u8]>) -> Text {
        let font = font.and_then(|b| fontdue::Font::from_bytes(b, fontdue::FontSettings::default()).ok());
        Text { font }
    }

    /// Whether a character is drawn half width (ASCII and half-width katakana).
    pub fn half(c: char) -> bool {
        (c as u32) < 0x80 || ('\u{ff61}'..='\u{ff9f}').contains(&c)
    }

    /// Draws a string with its top left at (x, y). Full-width characters take SX pixels,
    /// half-width ones SX/2.
    pub fn draw(&self, img: &mut Image, s: &str, x: i64, y: i64, f: &Font) {
        let Some(font) = &self.font else { return };
        let size = f.sy.max(1) as f32;
        let ascent = font.horizontal_line_metrics(size).map_or(size * 0.88, |m| m.ascent);
        let mut pen = x;
        for c in s.chars() {
            let adv = if Self::half(c) { f.sx / 2 } else { f.sx };
            let (m, cov) = font.rasterize(c, size);
            // centre the glyph in its cell
            let gx = pen + ((adv as f32 - m.advance_width) / 2.0).round() as i64 + m.xmin as i64;
            let gy = y + (ascent - m.height as f32 - m.ymin as f32).round() as i64;
            for by in 0..m.height as i64 {
                let t = if f.sy > 1 { ((gy + by - y) as f32 / (f.sy - 1) as f32).clamp(0.0, 1.0) } else { 0.0 };
                let top = rgb(f.color);
                let col = match f.color2 {
                    Some(c2) => {
                        let b = rgb(c2);
                        [0, 1, 2].map(|k| top[k] + (b[k] - top[k]) * t)
                    }
                    None => top,
                };
                for bx in 0..m.width as i64 {
                    let mut a = cov[(by * m.width as i64 + bx) as usize] as f32 / 255.0;
                    if f.bold && bx > 0 {
                        a = a.max(cov[(by * m.width as i64 + bx - 1) as usize] as f32 / 255.0);
                    }
                    let (px, py) = (gx + bx, gy + by);
                    if px < 0 || py < 0 || px >= img.w as i64 || py >= img.h as i64 {
                        continue;
                    }
                    if a <= 0.0 {
                        continue;
                    }
                    let i = (py as usize * img.w as usize + px as usize) * 4;
                    let p = &mut img.px[i..i + 4];
                    let da = p[3] as f32 / 255.0;
                    let oa = a + da * (1.0 - a);
                    for k in 0..3 {
                        p[k] = ((col[k] * a + p[k] as f32 * da * (1.0 - a)) / oa.max(1e-6)).round() as u8;
                    }
                    p[3] = (oa * 255.0).round() as u8;
                }
            }
            pen += adv;
        }
    }
}
