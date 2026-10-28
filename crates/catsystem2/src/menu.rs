//! Our own save/load screen and backlog, drawn over the game. The games' save screens are
//! screen scripts of SAVEPANEL objects laid out per game; this one stands in for them: the
//! game's `execfes saveload` opens it, and so does a right click during the story (save) or
//! the wheel turned up (backlog).

use std::rc::Rc;

use nexas::gfx::{Draw, Image, Place};

use crate::engine::Engine;
use crate::save::{SaveData, slot_name};

pub const SLOTS: usize = 12;
const COLS: usize = 2;
const TEXT: u32 = 20;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Save,
    Load,
    Log,
}

pub struct Menu {
    pub mode: Mode,
    /// the screen script that opened it (`execfes saveload`), answered when it closes
    from_fes: Option<usize>,
    /// what each slot holds, read when the menu opens or a slot is saved
    slots: Vec<Option<SaveData>>,
    /// backlog: pages back from the newest
    scroll: usize,
    /// the pictures of the labels, made again when `dirty`
    labels: Vec<(Rc<Image>, [f32; 2])>,
    dirty: bool,
}

fn inside(r: [f32; 4], (x, y): (i32, i32)) -> bool {
    let (x, y) = (x as f32, y as f32);
    x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3]
}

fn put(out: &mut Vec<Draw>, image: Rc<Image>, r: [f32; 4], alpha: f32, clip: [f32; 4]) {
    let (w, h) = (image.w as i32, image.h as i32);
    out.push(Draw {
        image,
        src: [0, 0, w, h],
        dst: [r[0], r[1], r[0] + r[2], r[1] + r[3]],
        place: Place {
            x: 0.0,
            y: 0.0,
            sx: 1.0,
            sy: 1.0,
            cx: 0.0,
            cy: 0.0,
            rot: 0.0,
            alpha,
            add: false,
            flip: (false, false),
            clip,
        },
    });
}

/// The first `n` characters of a page's text on one line.
fn short(s: &str, n: usize) -> String {
    let one: String = s.chars().filter(|&c| c != '\n').collect();
    if one.chars().count() > n {
        one.chars().take(n).chain("…".chars()).collect()
    } else {
        one
    }
}

impl Engine {
    pub(crate) fn open_menu(&mut self, mode: Mode, from_fes: Option<usize>) {
        let slots = (0..SLOTS)
            .map(|k| {
                self.g
                    .files
                    .read_save(&slot_name(k))
                    .and_then(|b| SaveData::decode(&b))
            })
            .collect();
        self.menu = Some(Menu {
            mode,
            from_fes,
            slots,
            scroll: 0,
            labels: Vec::new(),
            dirty: true,
        });
    }

    /// Closes the menu; a screen script that opened it hears `code`.
    fn close_menu(&mut self, code: i64) {
        if let Some(m) = self.menu.take()
            && let Some(r) = m.from_fes
            && let Some(run) = self.runs.get_mut(r)
        {
            run.exit_code = code;
        }
    }

    // ---- places on the screen

    fn slot_rect(&self, k: usize) -> [f32; 4] {
        let (w, h) = (self.width as f32, self.height as f32);
        let rows = SLOTS.div_ceil(COLS);
        let (x0, y0) = (w * 0.06, h * 0.16);
        let (cw, ch) = (w * 0.88 / COLS as f32, h * 0.72 / rows as f32);
        let (c, r) = (k % COLS, k / COLS);
        [
            x0 + c as f32 * cw + 6.0,
            y0 + r as f32 * ch + 4.0,
            cw - 12.0,
            ch - 8.0,
        ]
    }

    fn tab_rect(&self, mode: Mode) -> [f32; 4] {
        let w = self.width as f32;
        let x = match mode {
            Mode::Save => w * 0.06,
            Mode::Load => w * 0.06 + 150.0,
            Mode::Log => w * 0.06 + 300.0,
        };
        [x, self.height as f32 * 0.05, 140.0, 44.0]
    }

    fn back_rect(&self) -> [f32; 4] {
        let (w, h) = (self.width as f32, self.height as f32);
        [w * 0.94 - 140.0, h * 0.91, 140.0, 44.0]
    }

    /// The tabs shown: saving needs the story, a screen script's menu only loads.
    fn tabs(&self) -> Vec<Mode> {
        let m = self.menu.as_ref().unwrap();
        if m.from_fes.is_some() || self.adv.is_none() {
            vec![Mode::Load]
        } else {
            vec![Mode::Save, Mode::Load, Mode::Log]
        }
    }

    // ---- input

    /// Takes this frame's input when the menu is open, or opens it; true when it took it.
    pub(crate) fn menu_input(&mut self, clicked: bool, rclicked: bool, wheel_up: bool) -> bool {
        if self.menu.is_none() {
            let reading = self.skip_to.is_none()
                && self
                    .adv
                    .as_ref()
                    .is_some_and(|a| !a.waits_fes() && a.reading());
            let on_page = self.skip_to.is_none()
                && self
                    .adv
                    .as_ref()
                    .is_some_and(|a| !a.waits_fes() && a.on_page());
            if on_page && rclicked {
                self.open_menu(Mode::Save, None);
                return true;
            }
            if reading && wheel_up {
                self.open_menu(Mode::Log, None);
                return true;
            }
            return false;
        }
        let mouse = self.mouse;
        let mode = self.menu.as_ref().unwrap().mode;
        if rclicked {
            self.close_menu(-2);
            return true;
        }
        if mode == Mode::Log {
            let pages = self.adv.as_ref().map_or(0, |a| a.backlog.len());
            let m = self.menu.as_mut().unwrap();
            if wheel_up && m.scroll + 1 < pages {
                m.scroll += 1;
                m.dirty = true;
            }
        }
        if !clicked {
            return true;
        }
        if inside(self.back_rect(), mouse) {
            self.close_menu(-2);
            return true;
        }
        for t in self.tabs() {
            if inside(self.tab_rect(t), mouse) {
                let m = self.menu.as_mut().unwrap();
                m.mode = t;
                m.scroll = 0;
                m.dirty = true;
                return true;
            }
        }
        if mode == Mode::Log {
            // a click below the newest page closes the backlog
            let m = self.menu.as_mut().unwrap();
            if m.scroll == 0 {
                self.close_menu(-2);
            } else {
                m.scroll -= 1;
                m.dirty = true;
            }
            return true;
        }
        let Some(k) = (0..SLOTS).find(|&k| inside(self.slot_rect(k), mouse)) else {
            return true;
        };
        match mode {
            Mode::Save => {
                if let Some(d) = self.save_data() {
                    self.g.files.write_save(&slot_name(k), &d.encode());
                    let m = self.menu.as_mut().unwrap();
                    m.slots[k] = Some(d);
                    m.dirty = true;
                }
            }
            Mode::Load => {
                let m = self.menu.as_ref().unwrap();
                if m.slots[k].is_none() {
                    return true;
                }
                if m.from_fes.is_some() {
                    self.close_menu(1000 + k as i64);
                } else if let Some(d) = self.menu.as_mut().unwrap().slots[k].take() {
                    self.menu = None;
                    let parent = self.adv.as_ref().map_or(0, |a| a.parent);
                    self.load_save(parent, d);
                }
            }
            Mode::Log => {}
        }
        true
    }

    // ---- drawing

    fn label(&mut self, text: &str, w: f32, h: f32, size: u32, color: [u8; 3]) -> Rc<Image> {
        Rc::new(self.font.render(
            text,
            usize::MAX,
            w.max(1.0) as u32,
            h.max(1.0) as u32,
            size,
            size as i32 + 8,
            color,
        ))
    }

    /// Makes the labels' pictures again.
    fn menu_labels(&mut self) -> Vec<(Rc<Image>, [f32; 2])> {
        let mode = self.menu.as_ref().unwrap().mode;
        let mut out = Vec::new();
        for t in self.tabs() {
            let r = self.tab_rect(t);
            let name = match t {
                Mode::Save => "セーブ",
                Mode::Load => "ロード",
                Mode::Log => "ログ",
            };
            let color = if t == mode {
                [255, 255, 255]
            } else {
                [150, 160, 180]
            };
            let img = self.label(name, r[2], r[3], 24, color);
            out.push((img, [r[0] + 20.0, r[1] + 6.0]));
        }
        let b = self.back_rect();
        let img = self.label("戻る", b[2], b[3], 24, [255, 255, 255]);
        out.push((img, [b[0] + 40.0, b[1] + 6.0]));
        if mode == Mode::Log {
            let (w, h) = (self.width as f32, self.height as f32);
            let scroll = self.menu.as_ref().unwrap().scroll;
            let pages: Vec<(String, String)> = self
                .adv
                .as_ref()
                .map(|a| {
                    a.backlog
                        .iter()
                        .rev()
                        .skip(scroll)
                        .take(6)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            let ph = h * 0.72 / 6.0;
            for (k, (who, text)) in pages.iter().rev().enumerate() {
                let y = h * 0.16 + k as f32 * ph;
                let page = if who.is_empty() {
                    text.clone()
                } else {
                    format!("【{who}】{text}")
                };
                let img = self.label(&page, w * 0.86, ph, TEXT, [255, 255, 255]);
                out.push((img, [w * 0.07, y]));
            }
            return out;
        }
        for k in 0..SLOTS {
            let r = self.slot_rect(k);
            let line = match &self.menu.as_ref().unwrap().slots[k] {
                Some(d) => {
                    let who = if d.speaker.is_empty() {
                        String::new()
                    } else {
                        format!("【{}】", d.speaker)
                    };
                    format!(
                        "No.{:02}  {}\n{}{}",
                        k + 1,
                        d.scene,
                        who,
                        short(&d.text, 40)
                    )
                }
                None => format!("No.{:02}  ----", k + 1),
            };
            let img = self.label(&line, r[2] - 16.0, r[3] - 8.0, TEXT, [255, 255, 255]);
            out.push((img, [r[0] + 10.0, r[1] + 6.0]));
        }
        out
    }

    pub(crate) fn menu_draws(&mut self, out: &mut Vec<Draw>) {
        let Some(m) = self.menu.as_ref() else { return };
        let (mode, dirty) = (m.mode, m.dirty);
        if dirty {
            let labels = self.menu_labels();
            let m = self.menu.as_mut().unwrap();
            m.labels = labels;
            m.dirty = false;
        }
        let (w, h) = (self.width as f32, self.height as f32);
        let full = [0.0, 0.0, w, h];
        let black = self.color_image([0, 0, 0]);
        put(out, black, full, 0.8, full);
        let panel = self.color_image([40, 52, 78]);
        let lit = self.color_image([80, 104, 150]);
        let mouse = self.mouse;
        let mut boxes: Vec<[f32; 4]> = self.tabs().iter().map(|&t| self.tab_rect(t)).collect();
        boxes.push(self.back_rect());
        if mode != Mode::Log {
            boxes.extend((0..SLOTS).map(|k| self.slot_rect(k)));
        }
        for r in boxes {
            let img = if inside(r, mouse) {
                lit.clone()
            } else {
                panel.clone()
            };
            put(out, img, r, 0.9, full);
        }
        for (img, [x, y]) in self.menu.as_ref().unwrap().labels.clone() {
            let r = [x, y, img.w as f32, img.h as f32];
            put(out, img, r, 1.0, full);
        }
    }
}
