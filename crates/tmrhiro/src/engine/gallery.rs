//! The CG and scene galleries. A page is a picture (and a lit copy of it) with places on it:
//! a CG place shows a small copy of the first of its pictures that has been seen (or the
//! game's "no data" picture) and shows them one by one when clicked, a scene place plays its
//! script again, and the other places go to a label (the pages, back). What has been seen is
//! kept in seen.dat.
//!
//! The 2010+ form: 40.003c [?][?][count:u16] "nodata,incomplete,base,lit", then for each place
//! 40.0046 [kind][?][x0 y0 x1 y1: u16] "names" (kind 0x0a CGs, 0x14 a scene, 0x01 a label,
//! 0x02 the label of a right click). The older form is one command, 0xc8:
//! [?][?][?][?] ",base,lit,nodata" then " [kind]x0y0x1y1(hex, 3 each),names" for each place
//! (kind 1 CGs, 2 a label, 3 the label of a right click).

use std::collections::HashSet;
use std::rc::Rc;

use nexas::gfx::{Draw, Image};

use super::{Engine, Wait, draw, inside};
use crate::script::Command;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Cgs,
    Scene,
    Label,
    Cancel,
}

struct Place {
    kind: Kind,
    rect: [i32; 4],
    names: Vec<String>,
}

pub(super) struct Gallery {
    base: String,
    lit: String,
    nodata: String,
    incomplete: String,
    places: Vec<Place>,
    /// where the page starts (to come back after a scene)
    start: usize,
    /// the CGs being looked at and which one
    viewing: Option<(Vec<String>, usize)>,
}

/// What has been seen: pictures and scripts, by name (lower case).
#[derive(Default)]
pub(super) struct Seen {
    names: HashSet<String>,
    changed: bool,
}

impl Engine {
    pub(super) fn load_seen(&mut self) {
        if let Some(b) = self.files.read_save("seen.dat") {
            self.seen.names = String::from_utf8_lossy(&b)
                .lines()
                .map(|l| l.to_string())
                .collect();
        }
    }

    pub(super) fn mark_seen(&mut self, name: &str) {
        if !name.is_empty() && self.seen.names.insert(name.to_lowercase()) {
            self.seen.changed = true;
        }
    }

    /// Writes seen.dat when something new was seen (called once a frame).
    pub(super) fn save_seen(&mut self) {
        if !self.seen.changed {
            return;
        }
        self.seen.changed = false;
        let mut names: Vec<&String> = self.seen.names.iter().collect();
        names.sort();
        let text: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let b = text.join("\n").into_bytes();
        self.files.write_save("seen.dat", &b);
    }

    fn seen(&self, name: &str) -> bool {
        self.seen.names.contains(&name.to_lowercase())
    }

    /// 40.003c: a new page.
    pub(super) fn gallery_begin(&mut self, c: &Command) {
        let s = c.str(4);
        let mut f = s.split(',').map(|s| s.trim().to_string());
        let (nodata, incomplete) = (f.next().unwrap_or_default(), f.next().unwrap_or_default());
        let (base, lit) = (f.next().unwrap_or_default(), f.next().unwrap_or_default());
        self.gallery = Some(Gallery {
            base,
            lit,
            nodata,
            incomplete,
            places: Vec::new(),
            start: self.pc - 1,
            viewing: None,
        });
    }

    /// 40.0046: a place on the page; the page waits after its last one.
    pub(super) fn gallery_place(&mut self, c: &Command) {
        let kind = match c.u8(0) {
            0x0a => Kind::Cgs,
            0x14 => Kind::Scene,
            0x01 => Kind::Label,
            _ => Kind::Cancel,
        };
        let (x0, y0, x1, y1) = (c.i16(2), c.i16(4), c.i16(6), c.i16(8));
        let names = c.str(10).split(',').map(|s| s.trim().to_string()).collect();
        if let Some(g) = &mut self.gallery {
            g.places.push(Place {
                kind,
                rect: [x0, y0, x1 - x0, y1 - y0],
                names,
            });
        }
        let next = self.script.commands.get(self.pc);
        if !next.is_some_and(|n| n.group == 0x40 && n.op == 0x46) {
            self.wait = Wait::Gallery;
        }
    }

    /// The older 0xc8: a whole page.
    pub(super) fn gallery_old(&mut self, c: &Command) {
        let s = c.str(0);
        let mut fields = s.split(' ');
        let head = fields.next().unwrap_or("");
        let mut f = head.split(',').skip(1).map(|s| s.trim().to_string());
        let (base, lit, nodata) = (
            f.next().unwrap_or_default(),
            f.next().unwrap_or_default(),
            f.next().unwrap_or_default(),
        );
        let mut places = Vec::new();
        for p in fields {
            let b = p.as_bytes();
            if b.len() < 13 {
                continue;
            }
            let kind = match b[0] {
                1 => Kind::Cgs,
                2 => Kind::Label,
                _ => Kind::Cancel,
            };
            let n = |k: usize| {
                std::str::from_utf8(&b[1 + k * 3..4 + k * 3])
                    .ok()
                    .and_then(|h| i32::from_str_radix(h, 16).ok())
                    .unwrap_or(0)
            };
            let names = p[13..]
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string())
                .collect();
            places.push(Place {
                kind,
                rect: [n(0), n(1), n(2) - n(0), n(3) - n(1)],
                names,
            });
        }
        self.gallery = Some(Gallery {
            base,
            lit,
            nodata,
            incomplete: String::new(),
            places,
            start: self.pc - 1,
            viewing: None,
        });
        self.window = false;
        self.wait = Wait::Gallery;
    }

    pub(super) fn gallery_click(&mut self, x: i32, y: i32) {
        let Some(g) = &mut self.gallery else {
            return;
        };
        if let Some((names, k)) = &mut g.viewing {
            *k += 1;
            if *k >= names.len() {
                g.viewing = None;
            }
            return;
        }
        let Some(p) = g
            .places
            .iter()
            .find(|p| p.kind != Kind::Cancel && inside(p.rect, x, y))
        else {
            return;
        };
        let (kind, names) = (p.kind, p.names.clone());
        let start = g.start;
        match kind {
            Kind::Cgs => {
                let seen: Vec<String> = names.into_iter().filter(|n| self.seen(n)).collect();
                if !seen.is_empty()
                    && let Some(g) = &mut self.gallery
                {
                    g.viewing = Some((seen, 0));
                }
            }
            Kind::Scene => {
                let Some(name) = names.first().filter(|n| self.seen(n)).cloned() else {
                    return;
                };
                self.gallery = None;
                self.wait = Wait::None;
                self.stack.push((self.script.clone(), start));
                self.jump_file(&name);
            }
            Kind::Label | Kind::Cancel => {
                let label = names.first().cloned().unwrap_or_default();
                self.gallery = None;
                self.wait = Wait::None;
                self.jump_label(&label);
            }
        }
    }

    /// A right click: stops looking at a CG, or goes to the page's right-click label.
    pub(super) fn gallery_cancel(&mut self) {
        let Some(g) = &mut self.gallery else {
            return;
        };
        if g.viewing.take().is_some() {
            return;
        }
        let label = g
            .places
            .iter()
            .find(|p| p.kind == Kind::Cancel)
            .and_then(|p| p.names.first().cloned());
        if let Some(label) = label {
            self.gallery = None;
            self.wait = Wait::None;
            self.jump_label(&label);
        }
    }

    /// A small copy of a picture, `w` x `h`.
    fn thumb(&mut self, name: &str, w: i32, h: i32) -> Option<Rc<Image>> {
        let key = format!("{name}@{w}x{h}");
        if let Some(t) = self.thumbs.get(&key) {
            return Some(t.clone());
        }
        let pics = self.pictures(name, 0.0, 0.0);
        let (sw, sh) = self.screen;
        let mut full = Image::new(sw, sh);
        full.fill([0, 0, 0, 255]);
        for p in &pics {
            let d = draw(&p.image, p.x, p.y, 1.0, [0.0, 0.0, sw as f32, sh as f32]);
            nexas::gfx::draw(&mut full, &d.image, d.src, d.dst, &d.place);
        }
        let (w, h) = (w.max(1) as u32, h.max(1) as u32);
        let mut t = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x * sw / w, y * sh / h);
                let s = ((fy * sw + fx) * 4) as usize;
                let d = ((y * w + x) * 4) as usize;
                t.px[d..d + 4].copy_from_slice(&full.px[s..s + 4]);
            }
        }
        let t = Rc::new(t);
        self.thumbs.insert(key, t.clone());
        Some(t)
    }

    pub(super) fn gallery_draws(&mut self, out: &mut Vec<Draw>) {
        let (w, h) = (self.screen.0 as f32, self.screen.1 as f32);
        let Some(g) = &self.gallery else {
            return;
        };
        if let Some((names, k)) = &g.viewing {
            let name = names[*k].clone();
            let mut black = Image::new(self.screen.0, self.screen.1);
            black.fill([0, 0, 0, 255]);
            out.push(draw(&Rc::new(black), 0.0, 0.0, 1.0, [0.0, 0.0, w, h]));
            for p in self.pictures(&name, 0.0, 0.0) {
                out.push(draw(&p.image, p.x, p.y, 1.0, [0.0, 0.0, w, h]));
            }
            return;
        }
        let (base, lit, nodata, incomplete) = (
            g.base.clone(),
            g.lit.clone(),
            g.nodata.clone(),
            g.incomplete.clone(),
        );
        let places: Vec<(Kind, [i32; 4], Vec<String>)> = g
            .places
            .iter()
            .map(|p| (p.kind, p.rect, p.names.clone()))
            .collect();
        for p in self.pictures(&base, 0.0, 0.0) {
            out.push(draw(&p.image, p.x, p.y, 1.0, [0.0, 0.0, w, h]));
        }
        let lit = self.image(&lit);
        let hover = self.hover;
        for (kind, r, names) in places {
            match kind {
                Kind::Cgs | Kind::Scene => {
                    // a CG place shows its first seen CG; a seen scene shows the "incomplete"
                    // picture, as we have no picture of it
                    let first = names.iter().find(|n| self.seen(n)).cloned();
                    let pic = match (&first, kind) {
                        (Some(n), Kind::Cgs) => self.thumb(n, r[2], r[3]),
                        _ => None,
                    };
                    let pic = pic.or_else(|| {
                        let name = if first.is_some() {
                            &incomplete
                        } else {
                            &nodata
                        };
                        self.image(name).map(|g| g.image.clone())
                    });
                    if let Some(p) = pic {
                        out.push(draw(&p, r[0] as f32, r[1] as f32, 1.0, [0.0, 0.0, w, h]));
                    }
                }
                Kind::Label => {
                    if inside(r, hover.0, hover.1)
                        && let Some(g) = &lit
                    {
                        let mut d = draw(&g.image, g.x as f32, g.y as f32, 1.0, [0.0; 4]);
                        d.place.clip = [
                            r[0] as f32,
                            r[1] as f32,
                            (r[0] + r[2]) as f32,
                            (r[1] + r[3]) as f32,
                        ];
                        out.push(d);
                    }
                }
                Kind::Cancel => {}
            }
        }
    }
}
