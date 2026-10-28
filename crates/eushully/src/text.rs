//! Lines of text drawn with a TrueType font and a shadow down and right of the letters, in the
//! colours the scripts set.

use nexas::gfx::Image;

/// (0, 0) of a rendered line is `PAD` up and left of the first letter's cell.
pub const PAD: i32 = 2;
const SHADOW: i32 = 2;

/// `extra` (the game's AGE-EXTEND.TTF in SYS5) draws the letters `font` lacks, such as the
/// private-use dashes and dots of the scripts.
pub fn render(
    font: &fontdue::Font,
    extra: Option<&fontdue::Font>,
    text: &str,
    size: f32,
    rgb: [u8; 3],
    shadow: [u8; 3],
) -> Image {
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
        (width.ceil() as i32 + PAD * 2 + SHADOW).max(1),
        (size.ceil() as i32 + PAD * 2 + SHADOW).max(1),
    );
    let mut cover = vec![0u8; (w * h) as usize];
    let mut pen = PAD as f32;
    for &c in &chars {
        let f = match extra {
            Some(x) if font.lookup_glyph_index(c) == 0 && x.lookup_glyph_index(c) != 0 => x,
            _ => font,
        };
        let (m, bm) = f.rasterize(c, size);
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
            let s = if x >= SHADOW && y >= SHADOW {
                cover[((y - SHADOW) * w + x - SHADOW) as usize] as u32
            } else {
                0
            };
            // the letter over its shadow
            let a = c + s * (255 - c) / 255;
            if a == 0 {
                continue;
            }
            let o = i * 4;
            for k in 0..3 {
                let v = (rgb[k] as u32 * c + shadow[k] as u32 * s * (255 - c) / 255) / a;
                out.px[o + k] = v.min(255) as u8;
            }
            out.px[o + 3] = a.min(255) as u8;
        }
    }
    out.renew();
    out
}
