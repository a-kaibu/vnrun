//! Text: the console (message window) and `$fontout`.
//!
//! The engine only draws the letters on the console's page (`$console_set`) and tells the
//! scripts with events: `CONSOLE_WROTE(page, x, y, w, h, zerotime)` after each run of letters
//! (console.mjo copies them, letter by letter over time, onto the page it shows),
//! `CONSOLE_CLS`, `CONSOLE_ON`/`CONSOLE_OFF`, `PAUSE` (the scripts wait for the click),
//! `CRLF` and `NAME_DISP(name)`.

use crate::engine::Engine;
use crate::gfx::Page;

/// How letters look: size, advance, line step, colours.
#[derive(Clone, Copy, Debug)]
pub struct Font {
    /// letter height in pixels
    pub yl: i32,
    /// advance of a half-width letter (a full-width one takes twice) unless proportional
    pub xl: i32,
    pub step: i32,
    pub prop: bool,
    pub fore: u32,
    /// shadow or outline colour, and in the top byte which of them
    pub back: u32,
}

impl Default for Font {
    fn default() -> Font {
        Font {
            yl: 24,
            xl: 12,
            step: 36,
            prop: false,
            fore: 0xffffff,
            back: 0,
        }
    }
}

/// The console: where its letters go and where the next one goes.
#[derive(Default, Clone, Debug)]
pub struct Console {
    pub on: bool,
    pub page: i32,
    pub area: [i32; 4],
    pub font: Font,
    pub x: i32,
    pub y: i32,
    /// the text of this page and the name it is said by (for the log and the backlog)
    pub text: String,
    pub name: String,
    pub last: String,
}

impl Console {
    /// Back to the top left of the area.
    pub fn home(&mut self) {
        self.x = self.area[0];
        self.y = self.area[1] + (self.font.step - self.font.yl).max(0) / 2;
    }

    pub fn newline(&mut self) {
        self.x = self.area[0];
        self.y += self.font.step.max(self.font.yl);
    }
}

#[derive(Default, Clone, Debug)]
pub struct Fontout {
    pub page: i32,
    pub x: i32,
    pub y: i32,
    pub font: Font,
}

fn half_width(c: char) -> bool {
    (c as u32) < 0x100 || ('\u{ff61}'..='\u{ff9f}').contains(&c)
}

/// How far a letter moves the pen.
pub fn advance(e: &Engine, f: &Font, c: char) -> i32 {
    if f.prop
        && let Some(font) = &e.font
    {
        return font.metrics(c, f.yl as f32).advance_width.round() as i32;
    }
    if half_width(c) { f.xl } else { f.xl * 2 }
}

/// Draws `s` on `page` with its top left at (x, y) and returns the width drawn. The page's
/// antidata page (if any) gets the letters' opacity too.
pub fn draw(e: &mut Engine, page: i32, s: &str, x: i32, y: i32, f: &Font) -> i32 {
    let Some(font) = e.font.clone() else {
        return s.chars().map(|c| advance(e, f, c)).sum();
    };
    let advances: Vec<i32> = s.chars().map(|c| advance(e, f, c)).collect();
    let width = advances.iter().sum();
    let anti_id = e.gfx.pages.get(&page).and_then(|p| p.anti);
    let mut anti: Option<(i32, Page)> =
        anti_id.and_then(|a| e.gfx.pages.remove(&a).map(|p| (a, p)));
    if let Some(p) = e.gfx.pages.get_mut(&page) {
        let alpha_too = anti.is_none() && p.alpha;
        let img = p.px();
        let mut anti_img = anti
            .as_mut()
            .filter(|(_, a)| a.img.w == img.w && a.img.h == img.h)
            .map(|(_, a)| a.px());
        let size = f.yl.max(6) as f32;
        let ascent = font
            .horizontal_line_metrics(size)
            .map_or(size * 0.88, |m| m.ascent);
        let fore = [(f.fore >> 16) as u8, (f.fore >> 8) as u8, f.fore as u8];
        let back = [(f.back >> 16) as u8, (f.back >> 8) as u8, f.back as u8];
        // the top byte: 0 plain letters, 1 a drop shadow, 2 and up an outline
        let edge: Vec<(i32, i32)> = match f.back >> 24 {
            0 => Vec::new(),
            1 => vec![(2, 2)],
            _ => (-2..=2)
                .flat_map(|dy| (-2..=2).map(move |dx| (dx, dy)))
                .filter(|&(dx, dy)| (dx, dy) != (0, 0) && dx * dx + dy * dy <= 5)
                .collect(),
        };
        let passes: Vec<(i32, i32, [u8; 3])> = edge
            .into_iter()
            .map(|(dx, dy)| (dx, dy, back))
            .chain([(0, 0, fore)])
            .collect();
        let mut pen = x;
        for (c, adv) in s.chars().zip(&advances) {
            let (m, bmp) = font.rasterize(c, size);
            // centre fixed-pitch letters in their cell
            let cell = if f.prop {
                0
            } else {
                (adv - m.advance_width.round() as i32) / 2
            };
            let gx = pen + cell + m.xmin;
            let gy = y + (ascent - m.height as f32 - m.ymin as f32).round() as i32;
            for &(ox, oy, rgb) in &passes {
                for by in 0..m.height {
                    for bx in 0..m.width {
                        let a = bmp[by * m.width + bx] as u32;
                        if a == 0 {
                            continue;
                        }
                        let (px, py) = (gx + bx as i32 + ox, gy + by as i32 + oy);
                        if px < 0 || py < 0 || px >= img.w as i32 || py >= img.h as i32 {
                            continue;
                        }
                        let i = ((py as u32 * img.w + px as u32) * 4) as usize;
                        let d = &mut img.px[i..i + 4];
                        for k in 0..3 {
                            d[k] = ((rgb[k] as u32 * a + d[k] as u32 * (255 - a)) / 255) as u8;
                        }
                        if alpha_too {
                            d[3] = (a + d[3] as u32 * (255 - a) / 255) as u8;
                        }
                        if let Some(t) = anti_img.as_deref_mut() {
                            let v = (t.px[i + 2] as u32 * (255 - a) / 255) as u8;
                            t.px[i..i + 3].copy_from_slice(&[v, v, v]);
                        }
                    }
                }
            }
            pen += adv;
        }
    }
    if let Some((a, p)) = anti {
        e.gfx.pages.insert(a, p);
    }
    width
}

/// Writes `s` into the console from its cursor, wrapping at the area's right edge, and returns
/// each run written as (x, y, width) for `CONSOLE_WROTE`.
pub fn write(e: &mut Engine, s: &str) -> Vec<(i32, i32, i32)> {
    let mut runs = Vec::new();
    let c = e.sys.console.clone();
    let right = c.area[0] + c.area[2];
    let (mut x, mut y) = (c.x, c.y);
    let mut run = String::new();
    let mut run_x = x;
    let mut flush = |e: &mut Engine, run: &mut String, rx: i32, ry: i32| {
        if !run.is_empty() {
            let w = draw(e, c.page, run, rx, ry, &c.font);
            runs.push((rx, ry, w));
            run.clear();
        }
    };
    for ch in s.chars() {
        let adv = advance(e, &c.font, ch);
        // a line may not start with closing punctuation: let it hang over instead
        let hangs = "、。」』）・…ー！？".contains(ch);
        if x + adv > right && x > c.area[0] && !hangs {
            flush(e, &mut run, run_x, y);
            x = c.area[0];
            y += c.font.step.max(c.font.yl);
            run_x = x;
        }
        run.push(ch);
        x += adv;
    }
    flush(e, &mut run, run_x, y);
    let con = &mut e.sys.console;
    con.x = x;
    con.y = y;
    con.text.push_str(s);
    runs
}

/// `$fontout`: the text at the locate position of the fontout page; the position moves on.
pub fn fontout(e: &mut Engine, s: &str) {
    let f = e.sys.fontout.clone();
    let w = draw(e, f.page, s, f.x, f.y, &f.font);
    e.sys.fontout.x += w;
}
