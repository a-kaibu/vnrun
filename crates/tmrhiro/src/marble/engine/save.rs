//! Saves and the save/load screen. The trials draw no save screen of their own pictures, so
//! this is a plain list of slots. A save holds the place of the last `*mark` (the start of
//! the current page), the variables and what the scene was made from.

use std::rc::Rc;

use nexas::gfx::{Draw, Image};

use super::{Engine, Wait, draw, inside};

pub(super) const SLOTS: usize = 10;

pub(super) struct Menu {
    pub save: bool,
    /// what the game waited for before the screen opened
    pub back: Wait,
}

/// One slot as the list shows it: the chapter and the first words of the page.
fn slot_name(k: usize) -> String {
    format!("marble-{:02}.sav", k + 1)
}

fn field<'a>(b: &'a str, key: &str) -> Option<&'a str> {
    b.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
}

impl Engine {
    pub(super) fn open_menu(&mut self, save: bool) {
        let back = std::mem::replace(&mut self.wait, Wait::Menu);
        self.menu = Some(Menu { save, back });
    }

    pub(super) fn close_menu(&mut self) {
        if let Some(m) = self.menu.take() {
            self.wait = m.back;
        }
    }

    fn slot_rect(&self, k: usize) -> [i32; 4] {
        let (w, h) = (self.screen.0 as i32 - 120, 40);
        let y = 90 + k as i32 * (h + 6);
        [60, y, 60 + w, y + h]
    }

    fn tab_rect(&self, save: bool) -> [i32; 4] {
        let x = if save { 60 } else { 200 };
        [x, 30, x + 120, 70]
    }

    fn back_rect(&self) -> [i32; 4] {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        [w - 180, h - 60, w - 60, h - 20]
    }

    pub(super) fn menu_click(&mut self, x: i32, y: i32) {
        let Some(save) = self.menu.as_ref().map(|m| m.save) else {
            return;
        };
        if inside(self.back_rect(), x, y) {
            self.close_menu();
            return;
        }
        for tab in [true, false] {
            if inside(self.tab_rect(tab), x, y) {
                if let Some(m) = &mut self.menu {
                    m.save = tab;
                }
                return;
            }
        }
        let Some(k) = (0..SLOTS).find(|&k| inside(self.slot_rect(k), x, y)) else {
            return;
        };
        if save {
            if let Some(b) = self.save_data() {
                self.files.write_save(&slot_name(k), b.as_bytes());
            }
            self.slots.clear();
        } else if let Some(b) = self.files.read_save(&slot_name(k)) {
            self.menu = None;
            self.restore(&String::from_utf8_lossy(&b));
        }
    }

    /// The game at the last `*mark`, as text.
    fn save_data(&self) -> Option<String> {
        let (script, pc, stack) = self.mark.as_ref()?;
        let mut out = String::new();
        out += &format!("script={script}\npc={pc}\n");
        out += &format!(
            "stack={}\n",
            stack
                .iter()
                .map(|(s, pc)| format!("{s}:{pc}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let s = &self.shown;
        out += &format!(
            "bg={}\nbg_y={}\nspr={}\novr={}\nbgm={}\nnovel={}\n",
            s.bg_name, s.bg_y, s.spr_name, s.ovr_name, self.bgm, self.novel as u8
        );
        out += &format!("title={}\ntext={}\n", self.chapter, self.said);
        let mut vars: Vec<_> = self.vars.iter().collect();
        vars.sort();
        for (k, v) in vars {
            out += &format!("var.{k}={v}\n");
        }
        Some(out)
    }

    fn restore(&mut self, b: &str) {
        let num = |k: &str| field(b, k).map_or(0, |v| v.trim().parse().unwrap_or(0));
        let Some(script) = field(b, "script").and_then(|s| self.load(s)) else {
            return;
        };
        self.stack = field(b, "stack")
            .unwrap_or("")
            .split(',')
            .filter_map(|e| {
                let (s, pc) = e.split_once(':')?;
                Some((self.load(s)?, pc.parse().ok()?))
            })
            .collect();
        self.script = script;
        self.pc = num("pc") as usize;
        self.vars = b
            .lines()
            .filter_map(|l| {
                let (k, v) = l.strip_prefix("var.")?.split_once('=')?;
                Some((k.to_string(), v.parse().ok()?))
            })
            .collect();
        self.next = Default::default();
        let bg = field(b, "bg").unwrap_or("").to_string();
        if !bg.is_empty() {
            self.set_bg(&bg);
        }
        self.next.bg_y = field(b, "bg_y").and_then(|v| v.parse().ok()).unwrap_or(0.0);
        self.set_sprites(field(b, "spr").unwrap_or(""));
        let ovr = field(b, "ovr").unwrap_or("");
        self.set_ovr(if ovr.is_empty() { "OFF" } else { ovr });
        self.shown = self.next.clone();
        self.trans = None;
        self.pan = None;
        self.shake = None;
        self.bgm = 0;
        self.music(num("bgm"));
        self.novel = num("novel") == 1;
        self.chapter = field(b, "title").unwrap_or("").to_string();
        self.page.clear();
        self.speaker.clear();
        self.choice = None;
        self.map = None;
        self.parts = None;
        self.window = false;
        self.wait = Wait::None;
    }

    /// The list's line for each slot (read once while the screen is open).
    fn slot_lines(&mut self) -> Vec<String> {
        if self.slots.is_empty() {
            self.slots = (0..SLOTS)
                .map(|k| {
                    let b = self.files.read_save(&slot_name(k));
                    let b = b.map(|b| String::from_utf8_lossy(&b).into_owned());
                    let line = b.map_or_else(
                        || "―".to_string(),
                        |b| {
                            let text: String =
                                field(&b, "text").unwrap_or("").chars().take(24).collect();
                            format!("{}  {text}", field(&b, "title").unwrap_or(""))
                        },
                    );
                    format!("{:02}  {line}", k + 1)
                })
                .collect();
        }
        self.slots.clone()
    }

    pub(super) fn menu_draws(&mut self, out: &mut Vec<Draw>) {
        let Some(save) = self.menu.as_ref().map(|m| m.save) else {
            return;
        };
        let mut dark = Image::new(self.screen.0, self.screen.1);
        dark.fill([0, 0, 0, 210]);
        out.push(draw(&Rc::new(dark), 0.0, 0.0, 1.0));
        let Some(font) = self.text_font(20) else {
            return;
        };
        let (px, py) = self.pointer;
        let button = |r: [i32; 4], text: &str, lit: bool, out: &mut Vec<Draw>| {
            let mut bg = Image::new((r[2] - r[0]) as u32, (r[3] - r[1]) as u32);
            let a = if lit || inside(r, px, py) { 110 } else { 50 };
            bg.fill([255, 255, 255, a]);
            out.push(draw(&Rc::new(bg), r[0] as f32, r[1] as f32, 1.0));
            let img = Rc::new(font.render(&nexas::sjis::encode(text), [255; 3], 2));
            let y = (r[1] + r[3]) as f32 / 2.0 - img.h as f32 / 2.0;
            out.push(draw(&img, r[0] as f32 + 10.0, y, 1.0));
        };
        button(self.tab_rect(true), "セーブ", save, out);
        button(self.tab_rect(false), "ロード", !save, out);
        button(self.back_rect(), "戻る", false, out);
        for (k, line) in self.slot_lines().iter().enumerate() {
            button(self.slot_rect(k), line, false, out);
        }
    }
}
