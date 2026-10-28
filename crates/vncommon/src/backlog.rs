//! A plain backlog for engines that draw their own menus: the last [`KEEP`] messages (a name
//! and its text), shown over the screen newest at the bottom. The wheel turned up opens it and
//! goes further back, turned down comes forward and closes it at the newest; a click or a
//! right click closes it. The engine gives the text pictures, so the backlog uses its font.

use std::collections::VecDeque;
use std::rc::Rc;

use nexas::gfx::{Draw, Image, Place};

/// How many messages the backlog keeps.
pub const KEEP: usize = 100;

const NAME_RGB: [u8; 3] = [255, 214, 150];
const TEXT_RGB: [u8; 3] = [255, 255, 255];
const HINT_RGB: [u8; 3] = [190, 190, 190];

#[derive(Default)]
pub struct Backlog {
    lines: VecDeque<(String, String)>,
    open: bool,
    /// how many messages up from the newest the bottom one is
    back: usize,
}

impl Backlog {
    /// Keeps a message. A repeat of the newest is dropped (a message shown again after a load
    /// or a menu is the same one), and one that goes on from the newest (a page that waited
    /// for a click halfway) takes its place.
    pub fn push(&mut self, name: &str, text: &str) {
        let text = text.trim_end();
        if text.trim().is_empty() {
            return;
        }
        if let Some((n, t)) = self.lines.back_mut()
            && n == name
            && text.starts_with(t.as_str())
        {
            *t = text.to_string();
            return;
        }
        self.lines.push_back((name.to_string(), text.to_string()));
        while self.lines.len() > KEEP {
            self.lines.pop_front();
        }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.close();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens it at the newest message (when there is one).
    pub fn open(&mut self) -> bool {
        self.open = !self.lines.is_empty();
        self.back = 0;
        self.open
    }

    pub fn close(&mut self) {
        self.open = false;
        self.back = 0;
    }

    /// The wheel: up opens it or goes back a message, down comes forward and closes it at the
    /// newest. Says whether the backlog took the turn (it is open, or just opened).
    pub fn wheel(&mut self, up: bool) -> bool {
        if !self.open {
            return up && self.open();
        }
        if up {
            self.back = (self.back + 1).min(self.lines.len().saturating_sub(1));
        } else if self.back == 0 {
            self.close();
        } else {
            self.back -= 1;
        }
        true
    }

    /// The backlog over a `w` x `h` screen, text `size` pixels high; `text` makes the picture
    /// of a line of text in a colour.
    pub fn draws(
        &self,
        w: u32,
        h: u32,
        size: i32,
        text: &mut dyn FnMut(&str, [u8; 3]) -> Rc<Image>,
    ) -> Vec<Draw> {
        let mut out = Vec::new();
        let mut shade = Image::new(1, 1);
        shade.fill([0, 0, 0, 230]);
        let mut d = draw(&Rc::new(shade), 0.0, 0.0);
        d.dst = [0.0, 0.0, w as f32, h as f32];
        out.push(d);
        let margin = (w as i32 / 16).max(16);
        let step = size + size / 2;
        let head = text("バックログ", TEXT_RGB);
        out.push(draw(&head, margin as f32, (step / 2) as f32));
        let hint = text("ホイールで前後 ・ クリックで閉じる", HINT_RGB);
        out.push(draw(
            &hint,
            (w as i32 - margin - hint.w as i32) as f32,
            (step / 2) as f32,
        ));
        let top = step * 2;
        let width = (w as i32 - margin * 2).max(size);
        let cols = (width * 2 / size.max(1)).max(4) as usize;
        let mut y = h as i32 - step;
        let newest = self.lines.len().saturating_sub(1 + self.back);
        for i in (0..=newest).rev() {
            let Some((name, body)) = self.lines.get(i) else {
                break;
            };
            let rows = wrap(body, cols);
            let mut lines: Vec<(&str, [u8; 3])> = Vec::new();
            if !name.is_empty() {
                lines.push((name, NAME_RGB));
            }
            lines.extend(rows.iter().map(|r| (r.as_str(), TEXT_RGB)));
            let block = lines.len() as i32 * step;
            if y - block < top && i != newest {
                break;
            }
            y -= block;
            for (k, (t, rgb)) in lines.iter().enumerate() {
                let img = text(t, *rgb);
                let x = if k == 0 && !name.is_empty() {
                    margin
                } else {
                    margin + size
                };
                let mut d = draw(&img, x as f32, (y + k as i32 * step) as f32);
                d.place.clip = [0.0, top as f32, w as f32, h as f32];
                out.push(d);
            }
            y -= step / 2;
        }
        out
    }
}

/// Splits text into rows of at most `cols` half-width columns (a full-width character takes
/// two), at the text's own line breaks too.
fn wrap(s: &str, cols: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for line in s.split('\n') {
        let mut row = String::new();
        let mut n = 0;
        for c in line.chars() {
            let cw = if (c as u32) < 0x100 || ('\u{ff61}'..='\u{ff9f}').contains(&c) {
                1
            } else {
                2
            };
            if n + cw > cols && !row.is_empty() {
                rows.push(std::mem::take(&mut row));
                n = 0;
            }
            row.push(c);
            n += cw;
        }
        rows.push(row);
    }
    rows.retain(|r| !r.trim().is_empty());
    rows
}

fn draw(image: &Rc<Image>, x: f32, y: f32) -> Draw {
    Draw {
        image: image.clone(),
        src: [0, 0, image.w as i32, image.h as i32],
        dst: [x, y, x + image.w as f32, y + image.h as f32],
        place: Place {
            x,
            y,
            sx: 1.0,
            sy: 1.0,
            cx: 0.0,
            cy: 0.0,
            rot: 0.0,
            alpha: 1.0,
            add: false,
            flip: (false, false),
            clip: [0.0, 0.0, 1e6, 1e6],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_and_scrolls() {
        let mut b = Backlog::default();
        assert!(!b.wheel(true));
        for i in 0..150 {
            b.push("", &format!("line {i}"));
        }
        b.push("", "line 149");
        assert_eq!(b.len(), KEEP);
        b.push("", "line 149 and on");
        assert_eq!(b.len(), KEEP);
        assert!(b.wheel(true) && b.is_open());
        b.wheel(true);
        b.wheel(false);
        b.wheel(false);
        assert!(!b.is_open());
        assert_eq!(wrap("あいうえお", 4), ["あい", "うえ", "お"]);
        let mut n = 0;
        b.open();
        let ds = b.draws(800, 600, 20, &mut |_, _| {
            n += 1;
            Rc::new(Image::new(10, 20))
        });
        assert!(ds.len() > 10);
    }
}
