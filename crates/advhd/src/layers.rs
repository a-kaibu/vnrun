//! What is on the screen: sets of layers. The scenario's layers (`bg01`, `st01`, ...) are one
//! set, and the interface in Lua makes its own ("system" for the message window, one per menu).
//! Sets are drawn in the order they were made, the layers of a set in their order.
//!
//! A layer shows a picture, a PNA (whose parts, "sub layers", each have their own visibility
//! and parameters) or text. Effects change a parameter by an amount over a time; a set can be
//! frozen as a snapshot and later fade from it to what it shows by then (a transition).

use crate::pna::Pna;
use nexas::gfx::{Draw, Image, Place};
use std::rc::Rc;

/// Parameter numbers (LUA_PARAM_*, and script command 0x46).
pub const POS: i32 = 0;
pub const ROTATE: i32 = 1;
pub const SCALE: i32 = 2;
pub const CENTER: i32 = 3;
pub const OFFSET: i32 = 5;
pub const COLOR: i32 = 6;
pub const RECT: i32 = 7;
pub const SIZE: i32 = 8;

#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub pos: [f32; 3],
    /// degrees
    pub rot: [f32; 3],
    pub scale: [f32; 3],
    /// the point that scaling and turning keep in place, from the layer's position; None: the
    /// middle of the picture
    pub center: Option<[f32; 3]>,
    pub offset: [f32; 3],
    /// multiplies the picture; the last is the opacity
    pub color: [f32; 4],
    /// the part of the picture shown: x y w h
    pub rect: Option<[f32; 4]>,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            pos: [0.0; 3],
            rot: [0.0; 3],
            scale: [1.0; 3],
            center: None,
            offset: [0.0; 3],
            color: [1.0; 4],
            rect: None,
        }
    }
}

fn v3(v: [f32; 4]) -> [f32; 3] {
    [v[0], v[1], v[2]]
}

impl Params {
    pub fn get(&self, kind: i32) -> [f32; 4] {
        let p = |a: [f32; 3]| [a[0], a[1], a[2], 0.0];
        match kind {
            POS => p(self.pos),
            ROTATE => p(self.rot),
            SCALE => p(self.scale),
            CENTER => p(self.center.unwrap_or_default()),
            OFFSET => p(self.offset),
            COLOR => self.color,
            RECT => self.rect.unwrap_or_default(),
            _ => [0.0; 4],
        }
    }

    pub fn set(&mut self, kind: i32, v: [f32; 4]) {
        match kind {
            POS => self.pos = v3(v),
            ROTATE => self.rot = v3(v),
            SCALE => self.scale = v3(v),
            CENTER => self.center = Some(v3(v)),
            OFFSET => self.offset = v3(v),
            COLOR => self.color = v,
            RECT => self.rect = Some(v),
            _ => {}
        }
    }

    /// Adds an effect's change (LUA_EFFECT_*: 0 opacity, 1 position, 2 scale, 3 rotation,
    /// 4 offset, 5 part shown).
    fn add(&mut self, kind: i32, d: [f32; 4], k: f32) {
        let add3 = |a: &mut [f32; 3]| (0..3).for_each(|i| a[i] += d[i] * k);
        match kind {
            0 => self.color[3] = (self.color[3] + d[3] * k).clamp(0.0, 1.0),
            1 => add3(&mut self.pos),
            2 => add3(&mut self.scale),
            3 => add3(&mut self.rot),
            4 => add3(&mut self.offset),
            5 => {
                let r = self.rect.get_or_insert([0.0; 4]);
                (0..4).for_each(|i| r[i] += d[i] * k);
            }
            _ => {}
        }
    }
}

/// A change of a parameter by `delta` over `time` ms, started by name.
#[derive(Debug, Clone)]
pub struct Effect {
    pub name: String,
    pub kind: i32,
    pub delta: [f32; 4],
    /// 0 linear, 1 ease in, 2 ease out, 3 both
    pub ease: i32,
    pub time: f64,
    pub delay: f64,
    /// when it was started (ms); None: set but not started
    pub start: Option<f64>,
    /// how much of `delta` has been added so far (0..1)
    pub done: f32,
}

impl Effect {
    pub fn new(name: &str, kind: i32, delta: [f32; 4], ease: i32, time: f64, delay: f64) -> Effect {
        Effect {
            name: name.to_string(),
            kind,
            delta,
            ease,
            time: time.max(0.0),
            delay: delay.max(0.0),
            start: None,
            done: 0.0,
        }
    }

    /// Brings `p` to time `now`; true once the effect is over.
    fn step(&mut self, p: &mut Params, now: f64) -> bool {
        let Some(start) = self.start else { return false };
        let t = if self.time <= 0.0 {
            if now >= start + self.delay { 1.0 } else { 0.0 }
        } else {
            ((now - start - self.delay) / self.time).clamp(0.0, 1.0) as f32
        };
        let e = match self.ease {
            1 => t * t,
            2 => 1.0 - (1.0 - t) * (1.0 - t),
            3 => t * t * (3.0 - 2.0 * t),
            _ => t,
        };
        p.add(self.kind, self.delta, e - self.done);
        self.done = e;
        t >= 1.0
    }

    fn finish(&mut self, p: &mut Params) {
        p.add(self.kind, self.delta, 1.0 - self.done);
        self.done = 1.0;
    }
}

fn step_effects(effects: &mut Vec<Effect>, p: &mut Params, now: f64) {
    effects.retain_mut(|e| !e.step(p, now));
}

/// A text layer's box: its size and margins; the engine draws the text into `image`.
#[derive(Debug, Clone, Default)]
pub struct TextBox {
    pub w: i32,
    pub h: i32,
    /// left top right bottom
    pub margin: [i32; 4],
    pub line_gap: f32,
    pub image: Option<Rc<Image>>,
}

pub enum Content {
    None,
    Image(Rc<Image>),
    Pna(Rc<Pna>),
    Text(TextBox),
}

pub struct Sub {
    pub id: i32,
    pub visible: bool,
    pub p: Params,
    /// hit tested (setSubLayerHitTest), whether shown or not
    pub hit: bool,
    pub effects: Vec<Effect>,
}

pub struct Layer {
    pub name: String,
    pub content: Content,
    pub visible: bool,
    pub p: Params,
    /// for a PNA, one per part, in the same order
    pub subs: Vec<Sub>,
    pub hit: bool,
    pub effects: Vec<Effect>,
    pub parent: Option<String>,
    /// the picture file shown, as the script named it
    pub file: String,
}

impl Layer {
    pub fn new(name: &str, content: Content) -> Layer {
        let subs = match &content {
            Content::Pna(pna) => pna
                .parts
                .iter()
                .map(|p| Sub {
                    id: p.id,
                    visible: false,
                    p: Params { pos: [p.x as f32, p.y as f32, 0.0], ..Params::default() },
                    hit: false,
                    effects: Vec::new(),
                })
                .collect(),
            _ => Vec::new(),
        };
        Layer {
            name: name.to_string(),
            file: String::new(),
            content,
            visible: true,
            p: Params::default(),
            subs,
            hit: false,
            effects: Vec::new(),
            parent: None,
        }
    }

    pub fn sub(&mut self, id: i32) -> Option<&mut Sub> {
        self.subs.iter_mut().find(|s| s.id == id)
    }

    /// The size of what the layer shows.
    pub fn size(&self) -> (f32, f32) {
        match &self.content {
            Content::None => (0.0, 0.0),
            Content::Image(i) => (i.w as f32, i.h as f32),
            Content::Pna(p) => (p.w as f32, p.h as f32),
            Content::Text(t) => (t.w as f32, t.h as f32),
        }
    }

    pub fn busy(&self) -> bool {
        self.effects.iter().chain(self.subs.iter().flat_map(|s| &s.effects)).any(|e| e.start.is_some())
    }

    pub fn start_effect(&mut self, name: &str, now: f64) {
        for e in self.effects.iter_mut().filter(|e| e.name == name && e.start.is_none()) {
            e.start = Some(now);
        }
    }

    /// Ends the effects of that name (all with ""), as if their time had passed.
    pub fn finish_effects(&mut self, name: &str) {
        let p = &mut self.p;
        self.effects.retain_mut(|e| {
            if name.is_empty() || e.name == name {
                e.finish(p);
                false
            } else {
                true
            }
        });
        for s in &mut self.subs {
            let p = &mut s.p;
            s.effects.retain_mut(|e| {
                if name.is_empty() || e.name == name {
                    e.finish(p);
                    false
                } else {
                    true
                }
            });
        }
    }
}

/// A frozen picture of a set, and the fade from it once a transition starts: (start, length).
/// A copied snapshot (copySnap, a menu's backdrop) is drawn under the set's own layers instead
/// of in place of them.
pub struct Snap {
    pub image: Rc<Image>,
    pub fade: Option<(f64, f64)>,
    pub under: bool,
}

pub struct Set {
    pub name: String,
    pub layers: Vec<Layer>,
    pub snap: Option<Snap>,
    /// a transition without a snapshot (erasing) runs until then (ms)
    pub until: f64,
}

impl Set {
    pub fn layer(&mut self, name: &str) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.name.eq_ignore_ascii_case(name))
    }

    pub fn remove(&mut self, name: &str) {
        self.layers.retain(|l| !l.name.eq_ignore_ascii_case(name));
    }

    /// Adds a layer in place of one of the same name (keeping its place in the order).
    pub fn insert(&mut self, layer: Layer) {
        match self.layers.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&layer.name)) {
            Some(l) => *l = layer,
            None => self.layers.push(layer),
        }
    }

    pub fn busy(&self) -> bool {
        self.layers.iter().any(|l| l.busy())
    }

    pub fn fading(&self) -> bool {
        self.snap.as_ref().is_some_and(|s| s.fade.is_some()) || self.until > 0.0
    }

    /// The position of a layer's top left, with its parents'.
    fn origin(&self, l: &Layer, depth: u32) -> [f32; 2] {
        let mut o = [l.p.pos[0] + l.p.offset[0], l.p.pos[1] + l.p.offset[1]];
        if let Some(parent) = &l.parent
            && depth < 8
            && let Some(pl) = self.layers.iter().find(|p| p.name.eq_ignore_ascii_case(parent))
        {
            let po = self.origin(pl, depth + 1);
            o[0] += po[0];
            o[1] += po[1];
        }
        o
    }

    fn draws(&self, out: &mut Vec<Draw>, w: f32, h: f32) {
        let clip = [0.0, 0.0, w, h];
        for l in self.layers.iter().filter(|l| l.visible) {
            let alpha = l.p.color[3];
            if alpha <= 0.0 {
                continue;
            }
            let [ox, oy] = self.origin(l, 0);
            let (lw, lh) = l.size();
            let c = l.p.center.map_or([lw / 2.0, lh / 2.0], |c| [c[0], c[1]]);
            let place = Place {
                x: 0.0,
                y: 0.0,
                sx: l.p.scale[0],
                sy: l.p.scale[1],
                cx: ox + c[0],
                cy: oy + c[1],
                rot: l.p.rot[2].to_radians(),
                alpha,
                add: false,
                flip: (false, false),
                clip,
            };
            let mut one = |image: Rc<Image>, x: f32, y: f32, rect: Option<[f32; 4]>, place: Place| {
                let src = match rect {
                    Some(r) if r[2] > 0.0 && r[3] > 0.0 => {
                        [r[0] as i32, r[1] as i32, (r[0] + r[2]) as i32, (r[1] + r[3]) as i32]
                    }
                    _ => [0, 0, image.w as i32, image.h as i32],
                };
                let dst = [x, y, x + (src[2] - src[0]) as f32, y + (src[3] - src[1]) as f32];
                out.push(Draw { image, src, dst, place });
            };
            match &l.content {
                Content::None => {}
                Content::Image(img) => one(img.clone(), ox, oy, l.p.rect, place),
                Content::Text(t) => {
                    if let Some(img) = &t.image {
                        one(img.clone(), ox, oy, None, place);
                    }
                }
                Content::Pna(pna) => {
                    for (part, sub) in pna.parts.iter().zip(&l.subs) {
                        let a = alpha * sub.p.color[3];
                        if !sub.visible || a <= 0.0 {
                            continue;
                        }
                        if let Some(img) = pna.image(part) {
                            let x = ox + sub.p.pos[0] + sub.p.offset[0];
                            let y = oy + sub.p.pos[1] + sub.p.offset[1];
                            one(img, x, y, sub.p.rect, Place { alpha: a, ..place });
                        }
                    }
                }
            }
        }
    }

    /// What is at a point, top first: the sub layer (-1 for a whole layer) and layer.
    pub fn hit(&self, x: f32, y: f32) -> Option<(i32, &str)> {
        for l in self.layers.iter().rev().filter(|l| l.visible) {
            let [ox, oy] = self.origin(l, 0);
            if let Content::Pna(pna) = &l.content {
                for (part, sub) in pna.parts.iter().zip(&l.subs).rev() {
                    if sub.hit {
                        let (px, py) = (ox + sub.p.pos[0], oy + sub.p.pos[1]);
                        if x >= px && y >= py && x < px + part.w as f32 && y < py + part.h as f32 {
                            return Some((sub.id, &l.name));
                        }
                    }
                }
            }
            if l.hit {
                let (w, h) = l.size();
                if x >= ox && y >= oy && x < ox + w && y < oy + h {
                    return Some((-1, &l.name));
                }
            }
        }
        None
    }
}

pub struct Screen {
    pub w: u32,
    pub h: u32,
    pub sets: Vec<Set>,
}

impl Screen {
    pub fn new(w: u32, h: u32) -> Screen {
        Screen { w, h, sets: Vec::new() }
    }

    pub fn set(&mut self, name: &str) -> Option<&mut Set> {
        self.sets.iter_mut().find(|s| s.name == name)
    }

    /// The set of that name, made (on top) if there is none.
    pub fn add(&mut self, name: &str) -> &mut Set {
        match self.sets.iter().position(|s| s.name == name) {
            Some(i) => &mut self.sets[i],
            None => {
                self.sets.push(Set { name: name.to_string(), layers: Vec::new(), snap: None, until: 0.0 });
                self.sets.last_mut().unwrap()
            }
        }
    }

    pub fn layer(&mut self, set: &str, name: &str) -> Option<&mut Layer> {
        self.set(set)?.layer(name)
    }

    /// Brings effects and transitions to time `now`.
    pub fn step(&mut self, now: f64) {
        for s in &mut self.sets {
            for l in &mut s.layers {
                step_effects(&mut l.effects, &mut l.p, now);
                for sub in &mut l.subs {
                    step_effects(&mut sub.effects, &mut sub.p, now);
                }
            }
            if s.until > 0.0 && now >= s.until {
                s.until = 0.0;
            }
            if let Some(snap) = &s.snap
                && let Some((start, len)) = snap.fade
                && now >= start + len
            {
                s.snap = None;
            }
        }
    }

    /// What to draw, bottom first, up to and including set `upto` (all when None).
    pub fn draws(&self, now: f64, upto: Option<&str>) -> Vec<Draw> {
        let (w, h) = (self.w as f32, self.h as f32);
        let mut out = Vec::new();
        for s in &self.sets {
            match &s.snap {
                Some(Snap { image, fade: None, under }) => {
                    full(&mut out, image.clone(), 1.0, w, h);
                    if *under {
                        s.draws(&mut out, w, h);
                    }
                }
                Some(Snap { image, fade: Some((start, len)), .. }) => {
                    s.draws(&mut out, w, h);
                    let t = if *len > 0.0 { ((now - start) / len).clamp(0.0, 1.0) as f32 } else { 1.0 };
                    full(&mut out, image.clone(), 1.0 - t, w, h);
                }
                None => s.draws(&mut out, w, h),
            }
            if upto == Some(s.name.as_str()) {
                break;
            }
        }
        out
    }

    /// The screen up to set `upto` as one picture.
    pub fn render(&self, now: f64, upto: Option<&str>) -> Image {
        let mut img = Image::new(self.w, self.h);
        img.fill([0, 0, 0, 255]);
        for d in self.draws(now, upto) {
            nexas::gfx::draw(&mut img, &d.image, d.src, d.dst, &d.place);
        }
        img
    }
}

fn full(out: &mut Vec<Draw>, image: Rc<Image>, alpha: f32, w: f32, h: f32) {
    if alpha <= 0.0 {
        return;
    }
    let src = [0, 0, image.w as i32, image.h as i32];
    out.push(Draw {
        image,
        src,
        dst: [0.0, 0.0, w, h],
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
            clip: [0.0, 0.0, w, h],
        },
    });
}
