//! The save and load screen, drawn by the engine for both generations (the games' own
//! screens live in their programs): ten slots in two columns under a row of tabs. A slot is
//! a text file of `key=value` lines in the saves folder (`saveNN.txt`); `text` is the first
//! line of the message it was made at.

use nexas::files::Files;
use nexas::gfx::Image;

use crate::engine::text;

pub const SLOTS: usize = 10;

/// What a click on the screen hits.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    Save,
    Load,
    Title,
    Back,
    Slot(usize),
}

/// The screen while it is open.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Menu {
    pub save: bool,
    pub from_title: bool,
}

pub fn slot_name(i: usize) -> String {
    format!("save{:02}.txt", i + 1)
}

/// The `text` line of each slot (None: empty).
pub fn read_all(files: &mut dyn Files) -> Vec<Option<String>> {
    (0..SLOTS)
        .map(|i| {
            let b = files.read_save(&slot_name(i))?;
            let t = String::from_utf8_lossy(&b).to_string();
            Some(
                t.lines()
                    .find_map(|l| l.strip_prefix("text="))
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .collect()
}

/// A slot's lines as (key, value) in order.
pub fn read(files: &mut dyn Files, i: usize) -> Option<Vec<(String, String)>> {
    let b = files.read_save(&slot_name(i))?;
    Some(
        String::from_utf8_lossy(&b)
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    )
}

/// The tabs along the top: (what, label, x0, y0, x1, y1).
fn tabs(w: i32, from_title: bool) -> Vec<(Hit, &'static str, [i32; 4])> {
    let items: Vec<(Hit, &str)> = if from_title {
        vec![(Hit::Load, "ロード"), (Hit::Back, "戻る")]
    } else {
        vec![
            (Hit::Save, "セーブ"),
            (Hit::Load, "ロード"),
            (Hit::Title, "タイトルへ"),
            (Hit::Back, "戻る"),
        ]
    };
    let (bw, gap) = (w / 6, w / 40);
    let total = items.len() as i32 * (bw + gap) - gap;
    items
        .into_iter()
        .enumerate()
        .map(|(k, (hit, label))| {
            let x = (w - total) / 2 + k as i32 * (bw + gap);
            (hit, label, [x, 24, x + bw, 24 + 44])
        })
        .collect()
}

fn slot_rect(w: i32, h: i32, i: usize) -> [i32; 4] {
    let (col, row) = ((i / 5) as i32, (i % 5) as i32);
    let (mx, top) = (w / 20, 96);
    let cw = (w - 3 * mx) / 2;
    let rh = (h - top - 24) / 5;
    let x = mx + col * (cw + mx);
    let y = top + row * rh;
    [x, y, x + cw, y + rh - 12]
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && y >= r[1] && x < r[2] && y < r[3]
}

pub fn hit(w: i32, h: i32, menu: Menu, x: i32, y: i32) -> Option<Hit> {
    if let Some(t) = tabs(w, menu.from_title)
        .into_iter()
        .find(|t| inside(t.2, x, y))
    {
        return Some(t.0);
    }
    (0..SLOTS)
        .find(|&i| inside(slot_rect(w, h, i), x, y))
        .map(Hit::Slot)
}

fn fill(img: &mut Image, r: [i32; 4], c: [u8; 4]) {
    let a = c[3] as u32;
    for y in r[1].max(0)..r[3].min(img.h as i32) {
        for x in r[0].max(0)..r[2].min(img.w as i32) {
            let i = ((y as u32 * img.w + x as u32) * 4) as usize;
            let d = &mut img.px[i..i + 4];
            for k in 0..3 {
                d[k] = ((c[k] as u32 * a + d[k] as u32 * (255 - a)) / 255) as u8;
            }
            d[3] = (a + d[3] as u32 * (255 - a) / 255) as u8;
        }
    }
}

/// The whole screen as one picture, to lay over the game.
pub fn picture(
    font: Option<&fontdue::Font>,
    w: u32,
    h: u32,
    menu: Menu,
    hover: Option<Hit>,
    slots: &[Option<String>],
) -> Image {
    let (wi, hi) = (w as i32, h as i32);
    let mut img = Image::new(w, h);
    img.fill([8, 10, 24, 225]);
    let size = (h as f32 / 26.0).clamp(16.0, 28.0);
    for (what, label, r) in tabs(wi, menu.from_title) {
        let on = match what {
            Hit::Save => menu.save,
            Hit::Load => !menu.save,
            _ => false,
        };
        let c = if on {
            [200, 120, 150, 230]
        } else if hover == Some(what) {
            [110, 110, 150, 230]
        } else {
            [60, 60, 90, 230]
        };
        fill(&mut img, r, c);
        if let Some(f) = font {
            let tw: f32 = label
                .chars()
                .map(|ch| f.metrics(ch, size).advance_width)
                .sum();
            let x = r[0] as f32 + ((r[2] - r[0]) as f32 - tw) / 2.0;
            let y = r[1] as f32 + ((r[3] - r[1]) as f32 - size) / 2.0 - 2.0;
            text(&mut img, f, label, x, y, size, usize::MAX);
        }
    }
    for i in 0..SLOTS {
        let r = slot_rect(wi, hi, i);
        let c = if hover == Some(Hit::Slot(i)) {
            [90, 90, 140, 235]
        } else {
            [40, 40, 64, 235]
        };
        fill(&mut img, r, c);
        if let Some(f) = font {
            let small = size * 0.85;
            let head = format!("No.{:02}", i + 1);
            text(
                &mut img,
                f,
                &head,
                r[0] as f32 + 10.0,
                r[1] as f32 + 6.0,
                small,
                usize::MAX,
            );
            let body = match slots.get(i).cloned().flatten() {
                Some(t) if !t.is_empty() => t,
                Some(_) => "(セーブ済み)".to_string(),
                None => "―― 空き ――".to_string(),
            };
            // as many letters as fit on one line
            let room = (r[2] - r[0] - 20) as f32;
            let mut used = 0.0;
            let line: String = body
                .chars()
                .filter(|c| *c != '\n')
                .take_while(|ch| {
                    used += f.metrics(*ch, small).advance_width;
                    used <= room
                })
                .collect();
            text(
                &mut img,
                f,
                &line,
                r[0] as f32 + 10.0,
                r[1] as f32 + 12.0 + small,
                small,
                usize::MAX,
            );
        }
    }
    img
}

/// The first line of a message, to name a slot by.
pub fn first_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}
