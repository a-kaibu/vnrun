//! Message text: the codes in script strings, and drawing text into a text layer's box.
//!
//! Codes: `%K` wait for a click, `%P` new page, `%LC` / `%LF` (before names) are dropped,
//! `\n` is a new line, `{base:ruby}` shows the base.

use crate::layers::TextBox;
use nexas::gfx::Image;

/// The characters of a message as shown ('\n' for new lines).
pub fn plain(s: &str) -> Vec<char> {
    let mut out = Vec::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '%' => {
                // %K, %P, %LC, %LF, ...: letters up to the next non-capital
                let mut n = 0;
                while it.peek().is_some_and(|c| c.is_ascii_uppercase()) && n < 2 {
                    it.next();
                    n += 1;
                }
                if n == 0 {
                    out.push('%');
                }
            }
            '\\' if it.peek() == Some(&'n') => {
                it.next();
                out.push('\n');
            }
            '{' => {
                let mut base = String::new();
                let mut in_base = true;
                for c in it.by_ref() {
                    match c {
                        '}' => break,
                        ':' => in_base = false,
                        c if in_base => base.push(c),
                        _ => {}
                    }
                }
                out.extend(base.chars());
            }
            c => out.push(c),
        }
    }
    out
}

/// Characters that don't begin a line (kinsoku); they hang past the margin instead.
const NO_LINE_START: &str =
    "、。，．・：；？！）」』】〉》〕｝ー～…‥ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ";

pub struct Style {
    pub size: f32,
    pub color: [u8; 3],
    pub shadow: bool,
}

/// Draws the first `shown` characters of `text` into a picture the size of the box, wrapping
/// at its right margin.
pub fn render(font: &fontdue::Font, text: &[char], shown: usize, b: &TextBox, st: &Style) -> Image {
    let (w, h) = (b.w.max(1) as u32, b.h.max(1) as u32);
    let mut img = Image::new(w, h);
    let line_h = st.size * (1.0 + b.line_gap.max(0.0)) + 4.0;
    let (left, top) = (b.margin[0] as f32, b.margin[1] as f32);
    let right = w as f32 - b.margin[2] as f32;
    let (mut x, mut y) = (left, top);
    let ascent = font
        .horizontal_line_metrics(st.size)
        .map_or(st.size * 0.88, |m| m.ascent);
    for &c in text.iter().take(shown) {
        if c == '\n' {
            x = left;
            y += line_h;
            continue;
        }
        let (m, bitmap) = font.rasterize(c, st.size);
        if x + m.advance_width > right && x > left && !NO_LINE_START.contains(c) {
            x = left;
            y += line_h;
        }
        let gx = (x + m.xmin as f32).round() as i32;
        let gy = (y + ascent - m.height as f32 - m.ymin as f32).round() as i32;
        if st.shadow {
            blit(&mut img, &bitmap, m.width, gx + 2, gy + 2, [0, 0, 0]);
        }
        blit(&mut img, &bitmap, m.width, gx, gy, st.color);
        x += m.advance_width;
    }
    img
}

fn blit(img: &mut Image, cov: &[u8], cw: usize, gx: i32, gy: i32, rgb: [u8; 3]) {
    if cw == 0 {
        return;
    }
    for (row, line) in cov.chunks_exact(cw).enumerate() {
        let y = gy + row as i32;
        if y < 0 || y >= img.h as i32 {
            continue;
        }
        for (col, &a) in line.iter().enumerate() {
            let x = gx + col as i32;
            if a == 0 || x < 0 || x >= img.w as i32 {
                continue;
            }
            let i = ((y as u32 * img.w + x as u32) * 4) as usize;
            let d = &mut img.px[i..i + 4];
            let da = d[3] as u32;
            let sa = a as u32;
            let oa = sa + da * (255 - sa) / 255;
            for k in 0..3 {
                d[k] =
                    ((rgb[k] as u32 * sa + d[k] as u32 * da * (255 - sa) / 255) / oa.max(1)) as u8;
            }
            d[3] = oa as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain() {
        let s: String = super::plain("「…{私:わたくし}は」\\n%K%P")
            .into_iter()
            .collect();
        assert_eq!(s, "「…私は」\n");
        let s: String = super::plain("%LC詩乃").into_iter().collect();
        assert_eq!(s, "詩乃");
    }
}
