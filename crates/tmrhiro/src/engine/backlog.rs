//! The backlog: the lines read so far, a page at a time, with their voices to play again. It
//! opens with the wheel or the window's button and is laid out by the ini's [BACKLOG] section
//! (the older games get our own, see `old::SETTINGS`).

use std::rc::Rc;

use nexas::gfx::{Draw, Image};

use super::{Engine, Wait, draw, inside, part};

/// How many lines the backlog keeps.
const KEEP: usize = 300;

pub(super) struct LogLine {
    pub name: String,
    pub text: String,
    pub voice: String,
}

/// The open backlog: how many lines up from the newest it shows, and their pictures.
pub(super) struct LogView {
    back: usize,
    shown: Vec<Shown>,
}

struct Shown {
    line: usize,
    name: Option<Rc<Image>>,
    text: Vec<Rc<Image>>,
}

struct Layout {
    max: usize,
    size: i32,
    step: i32,
    at: [i32; 2],
    next: [i32; 2],
    name: [i32; 2],
    voice: Option<[i32; 6]>,
    back: Option<[i32; 6]>,
}

impl Engine {
    pub(super) fn log_line(&mut self, name: &str, text: &str, voice: &str) {
        if text.is_empty() {
            return;
        }
        self.backlog.push(LogLine {
            name: name.to_string(),
            text: text.to_string(),
            voice: voice.to_string(),
        });
        if self.backlog.len() > KEEP {
            self.backlog.remove(0);
        }
    }

    fn log_layout(&self) -> Layout {
        let i = &self.ini;
        let six = |key: &str| {
            let v = i.ints(&format!("BACKLOG/{key}"));
            (v.len() >= 6).then(|| [v[0], v[1], v[2], v[3], v[4], v[5]])
        };
        Layout {
            max: i.int("BACKLOG/BACKLOG_MAX", 5).max(1) as usize,
            size: i.int("BACKLOG/BACKLOG_FONT_SIZE", 24),
            step: i.int("BACKLOG/BACKLOG_NEXT_LINE", 30),
            at: [
                i.int("BACKLOG/BACKLOG_POS_X", 100),
                i.int("BACKLOG/BACKLOG_POS_Y", 80),
            ],
            next: [
                i.int("BACKLOG/BACKLOG_NEXT_X", 0),
                i.int("BACKLOG/BACKLOG_NEXT_Y", self.screen.1 as i32 / 6),
            ],
            name: [
                i.int("BACKLOG/BACKLOG_NAME_X", 0),
                i.int("BACKLOG/BACKLOG_NAME_Y", -30),
            ],
            voice: six("BACKLOG_VOICE"),
            back: six("BACKLOG_RETURN_POS"),
        }
    }

    /// The pictures of the backlog: the screen and its sheet of buttons.
    fn log_images(&self) -> (String, String) {
        match self
            .ini
            .get("BACKLOG/BACKLOG_IMAGES")
            .and_then(|v| v.split_once(','))
        {
            Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
            None => ("sys_backlog_base".into(), "sys_backlog_button".into()),
        }
    }

    pub(super) fn log_open(&self) -> bool {
        self.log_view.is_some()
    }

    /// Opens the backlog when a line or a choice waits.
    pub(super) fn open_log(&mut self) {
        if self.backlog.is_empty() || !matches!(self.wait, Wait::Click | Wait::Choice(_)) {
            return;
        }
        self.log_view = Some(LogView {
            back: 0,
            shown: Vec::new(),
        });
        self.log_page();
    }

    /// Makes the pictures of the lines on the page.
    fn log_page(&mut self) {
        let Some(back) = self.log_view.as_ref().map(|v| v.back) else {
            return;
        };
        let l = self.log_layout();
        let Some(font) = self.text_font(l.size) else {
            return;
        };
        let end = self.backlog.len() - back;
        let start = end.saturating_sub(l.max);
        // as many letters as fit between the line's place and the right edge
        let fit = ((self.screen.0 as i32 - l.at[0] - 80) / l.size.max(1)).max(4) as usize;
        let lines_per = ((l.next[1] - 4) / l.step.max(1)).max(1) as usize;
        let color = [255, 255, 255];
        let mut shown = Vec::new();
        for k in start..end {
            let line = &self.backlog[k];
            let name = (!line.name.is_empty())
                .then(|| Rc::new(font.render(&nexas::sjis::encode(&line.name), color, 2)));
            let mut rows = Vec::new();
            for part in line.text.split("\\n") {
                let chars: Vec<char> = super::strip_ruby(part).chars().collect();
                for chunk in chars.chunks(fit) {
                    rows.push(chunk.iter().collect::<String>());
                }
            }
            let text = rows
                .iter()
                .take(lines_per)
                .map(|r| Rc::new(font.render(&nexas::sjis::encode(r), color, 2)))
                .collect();
            shown.push(Shown {
                line: k,
                name,
                text,
            });
        }
        if let Some(v) = &mut self.log_view {
            v.shown = shown;
        }
    }

    /// The wheel: up opens the backlog or goes back a line, down goes forward and closes it at
    /// the newest line.
    pub fn wheel(&mut self, up: bool) {
        if self.menu.is_some() {
            return;
        }
        let Some(back) = self.log_view.as_ref().map(|v| v.back) else {
            if up {
                self.open_log();
            }
            return;
        };
        let max = self.log_layout().max;
        let most = self.backlog.len().saturating_sub(max);
        let back = if up {
            (back + 1).min(most)
        } else if back == 0 {
            self.log_view = None;
            return;
        } else {
            back - 1
        };
        if let Some(v) = &mut self.log_view {
            v.back = back;
        }
        self.log_page();
    }

    fn log_place(&self, l: &Layout, k: usize) -> [i32; 2] {
        [
            l.at[0] + k as i32 * l.next[0],
            l.at[1] + k as i32 * l.next[1],
        ]
    }

    /// A click on the backlog: a voice button plays its line, anything else closes it.
    pub(super) fn log_click(&mut self, x: i32, y: i32) {
        let l = self.log_layout();
        if let Some(v) = l.voice {
            let lines: Vec<usize> = self
                .log_view
                .as_ref()
                .map(|s| s.shown.iter().map(|s| s.line).collect())
                .unwrap_or_default();
            for (k, line) in lines.into_iter().enumerate() {
                let p = self.log_place(&l, k);
                let voice = self.backlog[line].voice.clone();
                if !voice.is_empty() && inside([p[0] + v[0], p[1] + v[1], v[4], v[5]], x, y) {
                    self.play(&["vic"], &voice, None, true);
                    return;
                }
            }
        }
        self.log_view = None;
    }

    pub(super) fn log_draws(&mut self, out: &mut Vec<Draw>) {
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let (base, sheet) = self.log_images();
        match self.image(&base) {
            Some(g) => out.push(draw(
                &g.image,
                g.x as f32,
                g.y as f32,
                1.0,
                [0.0, 0.0, w, h],
            )),
            None => {
                let p = Rc::new(super::panel(self.screen.0, self.screen.1));
                out.push(draw(&p, 0.0, 0.0, 1.0, [0.0, 0.0, w, h]));
            }
        }
        let sheet = self.image(&sheet);
        let l = self.log_layout();
        let hover = self.hover;
        let button = |out: &mut Vec<Draw>, r: [i32; 4], src: [i32; 2]| {
            if let Some(g) = &sheet {
                let lit = inside(r, hover.0, hover.1) as i32;
                let src = [src[0] - g.x, src[1] + lit * r[3] - g.y, r[2], r[3]];
                out.push(part(&g.image, src, r[0] as f32, r[1] as f32));
            }
        };
        let Some(view) = &self.log_view else {
            return;
        };
        for (k, s) in view.shown.iter().enumerate() {
            let p = self.log_place(&l, k);
            if let Some(n) = &s.name {
                let (x, y) = (p[0] + l.name[0], p[1] + l.name[1]);
                out.push(draw(n, x as f32, y as f32, 1.0, [0.0, 0.0, w, h]));
            }
            for (i, t) in s.text.iter().enumerate() {
                let y = p[1] + i as i32 * l.step;
                out.push(draw(t, p[0] as f32, y as f32, 1.0, [0.0, 0.0, w, h]));
            }
            if let Some(v) = l.voice
                && !self.backlog[s.line].voice.is_empty()
            {
                button(out, [p[0] + v[0], p[1] + v[1], v[4], v[5]], [v[2], v[3]]);
            }
        }
        if let Some(b) = l.back {
            button(out, [b[0], b[1], b[4], b[5]], [b[2], b[3]]);
        }
    }
}
