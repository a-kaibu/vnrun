//! Sets of buttons (10.0029 and the like, then 10.0032 or 10.0033 for each) and the map of the
//! 2010 games. The n-th button of a set shows only while variable n of bank 3 is set (10.0028
//! sets them all). On a map (10.002a "base,sheet,"), 10.0033 is one of:
//!
//! - 0x1010 [area:u16 1-10][x][y][step x][step y][row step x][row step y][rows][10 row counts]:
//!   an area of the map, where the characters line up in rows;
//! - 0x0020 [src x][src y][w][h][lit dx][lit dy] label: a character in the area that its
//!   variable names (1 the first), in the next free place there;
//! - 0x0010 [x][y][src x][src y][w][h][lit dx][lit dy] label: a button where it says;
//! - 0x0030 [x1][y1][x2][y2][src x][src y][w][h][lit dx][lit dy] label: a button at one of two
//!   places, as its variable says.
//!
//! The pictures of the buttons come from the sheet, the lit one as far away as it says.

use std::rc::Rc;

use nexas::gfx::Draw;

use super::{Button, Engine, Wait, inside, part};
use crate::script::Command;

#[derive(Clone, Default)]
pub(super) struct Area {
    at: [i32; 2],
    step: [i32; 2],
    row_step: [i32; 2],
    rows: Vec<i32>,
    /// how many places are taken
    taken: i32,
}

#[derive(Default)]
pub(super) struct Buttons {
    /// the next button's number
    next: u16,
    areas: Vec<Area>,
}

impl Engine {
    /// 10.0029, 10.002a and 10.005b: [?][count:u16] "pictures": a new set of buttons.
    pub(super) fn buttons_begin(&mut self, c: &Command) {
        let s = c.str(4);
        let mut it = s.split(',');
        self.button_images = (
            it.next().unwrap_or("").to_string(),
            it.next().unwrap_or("").to_string(),
        );
        self.button_set = Buttons::default();
        self.window = false;
    }

    /// Whether the next button of the set shows (it takes its number).
    pub(super) fn button_shown(&mut self) -> bool {
        let n = self.button_set.next;
        self.button_set.next += 1;
        self.var(3, n) != 0
    }

    /// The buttons wait once the last of them is set up (if any shows).
    pub(super) fn buttons_end(&mut self) {
        let next = self.script.commands.get(self.pc);
        let more = next.is_some_and(|n| n.group == 0x10 && matches!(n.op, 0x32 | 0x33));
        if !more && !self.buttons.is_empty() {
            self.wait = Wait::Buttons;
        }
    }

    /// 10.0033: an area or a button of the map.
    pub(super) fn map_place(&mut self, c: &Command) {
        let n = self.button_set.next;
        let v = c.u16(0);
        if v == 0x1010 {
            let k = c.u16(2) as usize;
            if (1..=10).contains(&k) {
                let rows = c.u16(16).min(10) as usize;
                let area = Area {
                    at: [c.i16(4), c.i16(6)],
                    step: [c.i16(8), c.i16(10)],
                    row_step: [c.i16(12), c.i16(14)],
                    rows: (0..rows).map(|r| c.i16(18 + r * 2)).collect(),
                    taken: 0,
                };
                let areas = &mut self.button_set.areas;
                if areas.len() < k {
                    areas.resize(k, Area::default());
                }
                areas[k - 1] = area;
            }
            self.vars.insert((3, n), 1);
            self.button_set.next += 1;
            self.buttons_end();
            return;
        }
        let which = self.var(3, n);
        if !self.button_shown() {
            self.buttons_end();
            return;
        }
        let (at, rest) = match v {
            0x20 => {
                let Some(a) = self.button_set.areas.get_mut(which as usize - 1) else {
                    self.buttons_end();
                    return;
                };
                (a.place(), 2)
            }
            0x30 => {
                let k = if which == 2 { 6 } else { 2 };
                ([c.i16(k), c.i16(k + 2)], 10)
            }
            _ => ([c.i16(2), c.i16(4)], 6),
        };
        let f = |k: usize| c.i16(rest + k * 2);
        self.buttons.push(Button {
            rect: [at[0], at[1], f(2), f(3)],
            label: c.str(rest + 12),
            icon: None,
            sheet: Some([f(0), f(1), f(4), f(5)]),
        });
        self.buttons_end();
    }

    /// The buttons drawn from the sheet, lit under the pointer.
    pub(super) fn sheet_button_draws(
        &mut self,
        sheet: &str,
        hit: Option<[i32; 4]>,
        out: &mut Vec<Draw>,
    ) {
        if self.buttons.iter().all(|b| b.sheet.is_none()) {
            return;
        }
        let Some(g) = self.image(sheet) else {
            return;
        };
        let image = Rc::clone(&g.image);
        for b in &self.buttons {
            let Some([sx, sy, lx, ly]) = b.sheet else {
                continue;
            };
            let r = b.rect;
            let lit = hit == Some(r) && inside(r, self.hover.0, self.hover.1);
            let (sx, sy) = if lit { (sx + lx, sy + ly) } else { (sx, sy) };
            let src = [sx - g.x, sy - g.y, r[2], r[3]];
            out.push(part(&image, src, r[0] as f32, r[1] as f32));
        }
    }
}

impl Area {
    /// The next free place: along a row, then the next row.
    fn place(&mut self) -> [i32; 2] {
        let mut k = self.taken;
        self.taken += 1;
        for (r, &count) in self.rows.iter().enumerate() {
            if k < count {
                let r = r as i32;
                return [
                    self.at[0] + r * self.row_step[0] + k * self.step[0],
                    self.at[1] + r * self.row_step[1] + k * self.step[1],
                ];
            }
            k -= count;
        }
        self.at
    }
}
