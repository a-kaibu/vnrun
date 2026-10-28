//! Pages (pictures the scripts draw into) and sprites (pages, or parts of them, shown on the
//! screen above the front buffer page).

use std::collections::HashMap;
use std::rc::Rc;

use nexas::gfx::Image;

use crate::rct::Pic;

pub struct Page {
    pub img: Rc<Image>,
    /// the page has its own alpha (made "withalfa" or from a mask); else it is opaque
    pub alpha: bool,
    /// another page whose brightness is this one's alpha (`$page_set_antidata`)
    pub anti: Option<i32>,
}

impl Page {
    pub fn new(w: u32, h: u32) -> Page {
        let mut img = Image::new(w.max(1), h.max(1));
        img.fill([0, 0, 0, 255]);
        Page {
            img: Rc::new(img),
            alpha: false,
            anti: None,
        }
    }

    pub fn w(&self) -> i32 {
        self.img.w as i32
    }
    pub fn h(&self) -> i32 {
        self.img.h as i32
    }

    /// The pixels to change; the picture gets a new id so its texture is uploaded again.
    pub fn px(&mut self) -> &mut Image {
        let img = Rc::make_mut(&mut self.img);
        img.renew();
        img
    }
}

/// A movement of a sprite over time.
#[derive(Clone, Debug)]
pub struct Anim {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub start: f64,
    pub ms: f64,
}

#[derive(Clone, Debug)]
pub struct Sprite {
    pub page: i32,
    /// the part of the page: x, y, w, h
    pub src: [i32; 4],
    pub x: f32,
    pub y: f32,
    pub alpha: f32,
    pub fade: Option<(f32, f32, f64, f64)>,
    pub mv: Option<Anim>,
    pub rot: f32,
    pub sx: f32,
    pub sy: f32,
    pub visible: bool,
    /// frames of an animation: (x offset in the page, y offset, ms)
    pub frames: Vec<(i32, i32, f64)>,
    pub frame_start: f64,
    pub frame_loop: bool,
    pub add: bool,
    /// the page goes with the sprite
    pub owns_page: bool,
    /// alpha keys (value 0..1, ms; -1 ms holds) from `alpha_start`
    pub alpha_def: Vec<(f32, f64)>,
    pub alpha_start: f64,
    /// made by the "_ext" calls: its position is its centre, and it can turn and scale
    pub ext: bool,
}

impl Sprite {
    pub fn new(page: i32, src: [i32; 4]) -> Sprite {
        Sprite {
            page,
            src,
            // off the screen until moved
            x: 100000.0,
            y: 100000.0,
            alpha: 1.0,
            fade: None,
            mv: None,
            rot: 0.0,
            sx: 1.0,
            sy: 1.0,
            visible: true,
            frames: Vec::new(),
            frame_start: 0.0,
            frame_loop: false,
            add: false,
            owns_page: false,
            alpha_def: Vec::new(),
            alpha_start: 0.0,
            ext: false,
        }
    }

    /// Brings movements and fades up to `now`.
    pub fn tick(&mut self, now: f64) {
        if let Some(m) = &self.mv {
            let t = if m.ms <= 0.0 {
                1.0
            } else {
                ((now - m.start) / m.ms).clamp(0.0, 1.0)
            } as f32;
            self.x = m.from.0 + (m.to.0 - m.from.0) * t;
            self.y = m.from.1 + (m.to.1 - m.from.1) * t;
            if t >= 1.0 {
                self.mv = None;
            }
        }
        if !self.alpha_def.is_empty() {
            // each key goes from the previous value to its own over its time
            let mut t = now - self.alpha_start;
            let mut prev = self.alpha;
            let mut done = true;
            for &(v, ms) in &self.alpha_def {
                if ms < 0.0 {
                    prev = v;
                    break;
                }
                if t < ms {
                    prev += (v - prev) * (t / ms) as f32;
                    done = false;
                    break;
                }
                t -= ms;
                prev = v;
            }
            self.alpha = prev;
            if done {
                self.alpha_def.clear();
            }
        }
        if let Some((a, b, start, ms)) = self.fade {
            let t = if ms <= 0.0 {
                1.0
            } else {
                ((now - start) / ms).clamp(0.0, 1.0)
            } as f32;
            self.alpha = a + (b - a) * t;
            if t >= 1.0 {
                self.fade = None;
            }
        }
    }

    pub fn moving(&self) -> bool {
        self.mv.is_some() || !self.alpha_def.is_empty()
    }
}

/// Converts a decoded RCT/RC8 to RGBA; `mask` is the matching "_" RC8.
pub fn to_image(pic: &Pic, mask: Option<&Pic>) -> Image {
    let (w, h) = match pic {
        Pic::Rgb { w, h, .. } | Pic::Indexed { w, h, .. } => (*w, *h),
    };
    let mut img = Image::new(w, h);
    match pic {
        Pic::Rgb { bgr, .. } => {
            for (d, s) in img.px.chunks_exact_mut(4).zip(bgr.chunks_exact(3)) {
                d.copy_from_slice(&[s[2], s[1], s[0], 255]);
            }
        }
        Pic::Indexed { px, palette, .. } => {
            for (d, &i) in img.px.chunks_exact_mut(4).zip(px) {
                let c = palette[i as usize];
                d.copy_from_slice(&[c[2], c[1], c[0], 255]);
            }
        }
    }
    if let Some(Pic::Indexed {
        w: mw,
        h: mh,
        px,
        palette,
    }) = mask
        && *mw == w
        && *mh == h
    {
        for (d, &i) in img.px.chunks_exact_mut(4).zip(px) {
            let c = palette[i as usize];
            d[3] = !(((c[0] as u32 + c[1] as u32 + c[2] as u32) / 3) as u8);
        }
    }
    img
}

pub struct Gfx {
    pub pages: HashMap<i32, Page>,
    pub next_page: i32,
    pub sprites: HashMap<i32, Sprite>,
    /// sprite ids, bottom first
    pub order: Vec<i32>,
    pub next_sprite: i32,
    pub front: i32,
    /// pages with an alpha page, combined: (page id, its picture id, the alpha's id, result)
    pub combined: HashMap<i32, (u32, u32, Rc<Image>)>,
    pub w: i32,
    pub h: i32,
}

impl Gfx {
    pub fn new(w: i32, h: i32) -> Gfx {
        let mut pages = HashMap::new();
        pages.insert(1, Page::new(w as u32, h as u32));
        Gfx {
            pages,
            next_page: 2,
            sprites: HashMap::new(),
            order: Vec::new(),
            next_sprite: 1,
            front: 1,
            combined: HashMap::new(),
            w,
            h,
        }
    }

    pub fn add_page(&mut self, p: Page) -> i32 {
        let id = self.next_page;
        self.next_page += 1;
        self.pages.insert(id, p);
        id
    }

    pub fn add_sprite(&mut self, s: Sprite) -> i32 {
        let id = self.next_sprite;
        self.next_sprite += 1;
        self.sprites.insert(id, s);
        self.order.push(id);
        id
    }

    pub fn release_sprite(&mut self, id: i32) {
        self.sprites.remove(&id);
        self.order.retain(|&s| s != id);
    }

    /// Moves `id` just above (`high`) or below `other` (0: to the very top or bottom).
    pub fn reorder(&mut self, id: i32, other: i32, high: bool) {
        if !self.sprites.contains_key(&id) {
            return;
        }
        self.order.retain(|&s| s != id);
        let at = match self.order.iter().position(|&s| s == other) {
            Some(i) => {
                if high {
                    i + 1
                } else {
                    i
                }
            }
            None => {
                if high {
                    self.order.len()
                } else {
                    0
                }
            }
        };
        self.order.insert(at, id);
    }
}

impl Gfx {
    /// What a page looks like on the screen: its pixels, with the alpha of its alpha page.
    pub fn shown(&mut self, id: i32) -> Option<Rc<Image>> {
        let p = self.pages.get(&id)?;
        let Some(anti) = p.anti.and_then(|a| self.pages.get(&a)) else {
            return Some(p.img.clone());
        };
        let (img, aimg) = (p.img.clone(), anti.img.clone());
        if let Some((i, a, out)) = self.combined.get(&id)
            && *i == img.id
            && *a == aimg.id
        {
            return Some(out.clone());
        }
        let mut out = (*img).clone();
        if aimg.w == out.w && aimg.h == out.h {
            for (d, s) in out.px.chunks_exact_mut(4).zip(aimg.px.chunks_exact(4)) {
                // the alpha page holds transparency, in the low byte of its colours (blue)
                d[3] = 255 - s[2];
            }
        }
        let out = Rc::new(out);
        self.combined.insert(id, (img.id, aimg.id, out.clone()));
        Some(out)
    }
}
