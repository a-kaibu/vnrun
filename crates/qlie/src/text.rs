//! Message text: the markup of `\o` (`[n]` new line, `[c,$AARRGGBB]` colour, `[s,w,h]` size,
//! `[pi,x,y]` spacing, `[f,n]` font, `[rb,text,ruby]` ruby, `[pc,text]` centred text) and drawing
//! it with a TrueType font (the games name Windows fonts; the page's font stands in), with an
//! edge in the font's edge colour.

use std::collections::HashMap;

use nexas::gfx::Image;

/// One character to draw, or a new line.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Char {
        c: char,
        size: u32,
        color: [u8; 4],
        /// gap after it
        gap: i32,
        /// the line's height (size and line spacing)
        line: i32,
        font: u8,
        /// small text above (ruby) starts at this character: its text
        ruby: Option<String>,
        /// the characters the ruby spans
        ruby_len: usize,
    },
    Newline,
    /// the rest of this line is centred
    Center,
}

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub size: u32,
    pub line: i32,
    pub gap: i32,
    pub color: [u8; 4],
    pub font: u8,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            size: 24,
            line: 30,
            gap: 0,
            color: [255, 255, 255, 255],
            font: 0,
        }
    }
}

fn argb(v: &str) -> Option<[u8; 4]> {
    let n = crate::expr::str_int(v) as u32;
    if v.trim().is_empty() {
        return None;
    }
    let a = (n >> 24) as u8;
    Some([
        (n >> 16) as u8,
        (n >> 8) as u8,
        n as u8,
        if a == 0 { 255 } else { a },
    ])
}

/// The items of marked-up text, the style carried on from `style` (and left as at the end).
pub fn parse(text: &str, style: &mut Style, out: &mut Vec<Item>) {
    let cs: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c == '['
            && let Some(j) = cs[i..].iter().position(|&c| c == ']')
        {
            {
                let tag: String = cs[i + 1..i + j].iter().collect();
                i += j + 1;
                let parts: Vec<&str> = tag.split(',').collect();
                let num = |k: usize, d: i64| {
                    parts
                        .get(k)
                        .filter(|s| !s.trim().is_empty())
                        .map_or(d, |s| crate::expr::str_int(s))
                };
                match parts[0].trim().to_lowercase().as_str() {
                    "n" => out.push(Item::Newline),
                    "c" => {
                        if let Some(c) = parts.get(1).and_then(|s| argb(s)) {
                            style.color = c;
                        }
                    }
                    "s" => {
                        let w = num(1, style.size as i64);
                        let h = num(2, w);
                        style.size = h.clamp(6, 200) as u32;
                    }
                    "pi" => {
                        style.gap = num(1, 0) as i32;
                        style.line = style.size as i32 + num(2, 4) as i32;
                    }
                    "f" => style.font = num(1, 0) as u8,
                    "rb" => {
                        let base = parts.get(1).copied().unwrap_or("");
                        let ruby = parts.get(2).copied().unwrap_or("");
                        let n = base.chars().count();
                        for (k, c) in base.chars().enumerate() {
                            out.push(Item::Char {
                                c,
                                size: style.size,
                                color: style.color,
                                gap: style.gap,
                                line: style.line.max(style.size as i32),
                                font: style.font,
                                ruby: (k == 0 && !ruby.is_empty()).then(|| ruby.to_string()),
                                ruby_len: n,
                            });
                        }
                    }
                    "pc" => {
                        out.push(Item::Center);
                        let rest = parts[1..].join(",");
                        parse(&rest, style, out);
                    }
                    _ => {}
                }
                continue;
            }
        }
        out.push(Item::Char {
            c,
            size: style.size,
            color: style.color,
            gap: style.gap,
            line: style.line.max(style.size as i32),
            font: style.font,
            ruby: None,
            ruby_len: 0,
        });
        i += 1;
    }
}

/// How many characters there are to show (for typing them out).
pub fn count(items: &[Item]) -> usize {
    items
        .iter()
        .filter(|i| matches!(i, Item::Char { .. }))
        .count()
}

#[derive(Default)]
pub struct Font {
    font: Option<fontdue::Font>,
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
    /// edge colour by font number, when the font has an edge
    pub edges: HashMap<u8, [u8; 3]>,
}

impl Font {
    pub fn set(&mut self, ttf: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        self.font = Some(f);
        self.cache.clear();
        Ok(())
    }

    pub fn has(&self) -> bool {
        self.font.is_some()
    }

    fn glyph(&mut self, c: char, size: u32) -> Option<(fontdue::Metrics, &Vec<u8>)> {
        let font = self.font.as_ref()?;
        let g = self
            .cache
            .entry((c, size))
            .or_insert_with(|| font.rasterize(c, size as f32));
        Some((g.0, &g.1))
    }

    fn advance(&mut self, c: char, size: u32) -> i32 {
        match self.glyph(c, size) {
            Some((m, _)) => m.advance_width.round() as i32,
            None => size as i32,
        }
    }

    /// The first `shown` characters of `items` in a `w` x `h` picture.
    pub fn render(&mut self, items: &[Item], shown: usize, w: u32, h: u32) -> Image {
        let mut out = Image::new(w.max(1), h.max(1));
        if self.font.is_none() {
            return out;
        }
        // lay out: lines of (x, y, item index)
        struct Placed {
            x: i32,
            y: i32,
            k: usize,
        }
        let mut placed: Vec<Placed> = Vec::new();
        let (mut x, mut y) = (2, 2);
        let mut line_h = 0;
        let mut line_start = 0;
        let mut center_from: Option<usize> = None;
        let mut n = 0;
        let finish_line =
            |placed: &mut Vec<Placed>, start: usize, center: Option<usize>, x: i32| {
                if let Some(c) = center {
                    let shift = (w as i32 - x) / 2;
                    if shift > 0 {
                        for p in &mut placed[c.max(start)..] {
                            p.x += shift;
                        }
                    }
                }
            };
        for (k, it) in items.iter().enumerate() {
            match it {
                Item::Newline => {
                    finish_line(&mut placed, line_start, center_from.take(), x);
                    x = 2;
                    y += if line_h > 0 { line_h } else { 30 };
                    line_h = 0;
                    line_start = placed.len();
                }
                Item::Center => center_from = Some(placed.len()),
                Item::Char {
                    c, size, gap, line, ..
                } => {
                    if n >= shown {
                        break;
                    }
                    n += 1;
                    let adv = self.advance(*c, *size) + gap;
                    if x + adv > w as i32 - 2 && x > 2 {
                        finish_line(&mut placed, line_start, center_from.take(), x);
                        x = 2;
                        y += if line_h > 0 { line_h } else { *line };
                        line_h = 0;
                        line_start = placed.len();
                    }
                    placed.push(Placed { x, y, k });
                    x += adv;
                    line_h = line_h.max(*line);
                }
            }
        }
        finish_line(&mut placed, line_start, center_from, x);
        // coverage and colour per pixel; then the edge
        let mut cover = vec![0u8; (w * h) as usize];
        let mut color = vec![[255u8; 4]; (w * h) as usize];
        let mut edge = vec![None::<[u8; 3]>; (w * h) as usize];
        let ascent_of = |f: &fontdue::Font, size: u32| {
            f.horizontal_line_metrics(size as f32)
                .map_or(size as f32 * 0.88, |m| m.ascent)
        };
        let mut blit = |s: &mut Font, c: char, size: u32, gx: i32, gy: i32, col: [u8; 4], e| {
            let ascent = ascent_of(s.font.as_ref().unwrap(), size);
            if let Some((m, bmp)) = s.glyph(c, size) {
                let gx = gx + m.xmin;
                let gy = gy + (ascent - m.height as f32 - m.ymin as f32).round() as i32;
                for (k, &v) in bmp.iter().enumerate() {
                    let (px, py) = (gx + (k % m.width) as i32, gy + (k / m.width) as i32);
                    if px >= 0 && py >= 0 && px < w as i32 && py < h as i32 && v > 0 {
                        let i = (py as u32 * w + px as u32) as usize;
                        if v >= cover[i] {
                            color[i] = col;
                            edge[i] = e;
                        }
                        cover[i] = cover[i].max(v);
                    }
                }
            }
        };
        for p in &placed {
            let Item::Char {
                c,
                size,
                color: col,
                font,
                ruby,
                ruby_len,
                line,
                ..
            } = &items[p.k]
            else {
                continue;
            };
            let e = self.edges.get(font).copied();
            // the text sits at the bottom of its line
            let top = p.y + (*line - *size as i32).max(0) - 2;
            blit(self, *c, *size, p.x, top, *col, e);
            if let Some(r) = ruby {
                let rs = (*size / 2).max(8);
                let span = *ruby_len as i32 * self.advance(*c, *size);
                let rw: i32 = r.chars().map(|c| self.advance(c, rs)).sum();
                let mut rx = p.x + (span - rw) / 2;
                for rc in r.chars() {
                    blit(self, rc, rs, rx, top - rs as i32, *col, e);
                    rx += self.advance(rc, rs);
                }
            }
        }
        for py in 0..h as i32 {
            for px in 0..w as i32 {
                let i = (py as u32 * w + px as u32) as usize;
                let v = cover[i] as u32;
                // the edge around: the strongest neighbour with an edge colour
                let mut border = 0u8;
                let mut bc = [0u8; 3];
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        let (sx, sy) = (px + dx, py + dy);
                        if sx >= 0 && sy >= 0 && sx < w as i32 && sy < h as i32 {
                            let j = (sy as u32 * w + sx as u32) as usize;
                            if let Some(ec) = edge[j]
                                && cover[j] > border
                            {
                                border = cover[j];
                                bc = ec;
                            }
                        }
                    }
                }
                let o = i * 4;
                let col = color[i];
                for k in 0..3 {
                    out.px[o + k] = ((col[k] as u32 * v + bc[k] as u32 * (255 - v)) / 255) as u8;
                }
                let a = (border as u32).max(v) * col[3] as u32 / 255;
                out.px[o + 3] = a as u8;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup() {
        let mut st = Style::default();
        let mut v = Vec::new();
        parse("[c,$FF102030]a[n][rb,瑞希,みずき]", &mut st, &mut v);
        assert_eq!(v.len(), 4);
        assert!(matches!(
            v[0],
            Item::Char {
                c: 'a',
                color: [0x10, 0x20, 0x30, 255],
                ..
            }
        ));
        assert_eq!(v[1], Item::Newline);
        assert!(matches!(&v[2], Item::Char { ruby: Some(r), .. } if r == "みずき"));
        assert_eq!(count(&v), 3);
    }
}
