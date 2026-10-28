//! Lines of text drawn with a TrueType font, a dark border around the letters.

use nexas::gfx::Image;

/// One line of `text` in `size` pixel letters; (0, 0) of the result is `PAD` up and left of
/// the first letter's cell.
pub const PAD: i32 = 2;

pub fn render(font: &fontdue::Font, text: &str, size: f32, rgb: [u8; 3]) -> Image {
    let ascent = font
        .horizontal_line_metrics(size)
        .map_or(size * 0.88, |m| m.ascent);
    let chars: Vec<char> = text.chars().collect();
    let advance = |c: char| {
        if (c as u32) < 0x80 || ('\u{ff61}'..='\u{ff9f}').contains(&c) {
            size / 2.0
        } else {
            size
        }
    };
    let width: f32 = chars.iter().map(|&c| advance(c)).sum();
    let (w, h) = (
        (width.ceil() as i32 + PAD * 2).max(1),
        (size.ceil() as i32 + PAD * 2).max(1),
    );
    let mut cover = vec![0u8; (w * h) as usize];
    let mut pen = PAD as f32;
    for &c in &chars {
        let (m, bm) = font.rasterize(c, size);
        let ox =
            pen.round() as i32 + m.xmin + ((advance(c) - m.advance_width) / 2.0).round() as i32;
        let oy = PAD + (ascent.round() as i32) - m.height as i32 - m.ymin;
        for y in 0..m.height {
            for x in 0..m.width {
                let (px, py) = (ox + x as i32, oy + y as i32);
                if px < 0 || py < 0 || px >= w || py >= h {
                    continue;
                }
                let i = (py * w + px) as usize;
                cover[i] = cover[i].max(bm[y * m.width + x]);
            }
        }
        pen += advance(c);
    }
    let mut out = Image::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let c = cover[i] as u32;
            // the border: the most coverage within 2 pixels
            let mut b = 0u32;
            for dy in -PAD..=PAD {
                for dx in -PAD..=PAD {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w && ny < h {
                        b = b.max(cover[(ny * w + nx) as usize] as u32);
                    }
                }
            }
            let a = c.max(b * 3 / 4);
            if a == 0 {
                continue;
            }
            let o = i * 4;
            for (k, &v) in rgb.iter().enumerate() {
                out.px[o + k] = ((v as u32 * c) / a.max(1)).min(255) as u8;
            }
            out.px[o + 3] = a as u8;
        }
    }
    out.renew();
    out
}

/// Splits a message into lines of at most `per_line` full-width letters (half-width ones
/// count half); `\n` breaks a line.
pub fn wrap(text: &str, per_line: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut used = 0usize;
        for c in para.chars() {
            let n = if (c as u32) < 0x80 { 1 } else { 2 };
            // closing punctuation may hang past the end
            let hang = "、。」』）！？…―ー".contains(c);
            if used + n > per_line * 2 && !hang {
                out.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push(c);
            used += n;
        }
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_full_width() {
        let l = wrap("あいうえお。かきく\nさ", 3);
        assert_eq!(l, vec!["あいう", "えお。", "かきく", "さ"]);
    }
}
