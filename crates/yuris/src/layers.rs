//! CG layers: named by paths like folders. `ABC/` is a layer set (a folder of layers),
//! `ABC/CG1` a layer in it, `:` the root. The root holds the set `W0`, the screen, which is
//! also where relative names start.

use std::collections::HashMap;

use crate::image::{Image, Rect};

pub const SCREEN: &str = ":/W0/";

#[derive(Debug, Clone)]
pub struct Layer {
    /// texture number for the renderer; stays while the layer exists
    pub tex: u32,
    /// bumped when the pixels change
    pub version: u32,
    pub set: bool,
    /// the picture; for a set, its size (SX, SY) with no pixels
    pub img: Image,
    pub e: bool,
    pub a: i64,
    pub x: i64,
    pub y: i64,
    pub z: i64,
    pub mode: i64,
    /// mask layer (MID)
    pub mid: String,
    /// transition: rule layer (TID), progress (T, 0..=256) and softness (TA)
    pub tid: String,
    pub t: i64,
    pub ta: i64,
    /// order of creation, for layers with the same Z
    pub seq: u64,
}

impl Layer {
    fn new(set: bool, img: Image) -> Layer {
        Layer {
            tex: 0,
            version: 1,
            set,
            img,
            e: true,
            a: 256,
            x: 0,
            y: 0,
            z: 0,
            mode: 0,
            mid: String::new(),
            tid: String::new(),
            t: 256,
            ta: 0,
            seq: 0,
        }
    }
    pub fn w(&self) -> u32 {
        self.img.w
    }
    pub fn h(&self) -> u32 {
        self.img.h
    }
    /// Marks the pixels as changed.
    pub fn touch(&mut self) {
        self.version = self.version.wrapping_add(1);
    }
}

/// What the renderer draws for one layer, in screen pixels.
#[derive(Debug, Clone)]
pub struct Draw {
    pub tex: u32,
    pub version: u32,
    pub x: i64,
    pub y: i64,
    /// alpha with the sets' alpha, 0..=256
    pub a: i64,
    pub mode: i64,
    /// the sets it is in cut it to this
    pub clip: Rect,
    /// mask: its texture and top left
    pub mask: Option<(u32, i64, i64)>,
    /// transition: rule texture, top left, progress and softness
    pub rule: Option<(u32, i64, i64, i64, i64)>,
}

pub struct Layers {
    pub map: HashMap<String, Layer>,
    next_tex: u32,
    seq: u64,
    /// the screen's size
    pub w: u32,
    pub h: u32,
}

/// The set a path is in: ":/W0/ABC/CG1" -> ":/W0/ABC/", ":/W0/ABC/" -> ":/W0/".
pub fn parent(path: &str) -> &str {
    let p = path.strip_suffix('/').unwrap_or(path);
    match p.rfind('/') {
        Some(i) => &path[..=i],
        None => ":/",
    }
}

/// The full path of a layer name: relative names are in the screen set; `.`, `..` and `:` (the
/// root) work as in folder paths. Names are matched without regard to case.
pub fn resolve(name: &str) -> String {
    let set = name.ends_with('/');
    let (base, rest) = match name.strip_prefix(':') {
        Some(r) => (Vec::new(), r),
        None => (vec!["W0".to_string()], name),
    };
    let mut parts = base;
    for p in rest.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p.to_uppercase()),
        }
    }
    let mut out = format!(":/{}", parts.join("/"));
    if set && !out.ends_with('/') {
        out.push('/');
    }
    out
}

impl Layers {
    pub fn new(w: u32, h: u32) -> Layers {
        let mut l = Layers { map: HashMap::new(), next_tex: 1, seq: 0, w, h };
        let screen = l.make(true, Image { w, h, px: Vec::new() });
        l.map.insert(SCREEN.to_string(), screen);
        l
    }

    fn make(&mut self, set: bool, img: Image) -> Layer {
        let mut layer = Layer::new(set, img);
        layer.tex = self.next_tex;
        self.next_tex += 1;
        self.seq += 1;
        layer.seq = self.seq;
        layer
    }

    pub fn get(&self, name: &str) -> Option<&Layer> {
        self.map.get(&resolve(name))
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Layer> {
        self.map.get_mut(&resolve(name))
    }

    /// The layer, made (with its sets) when it is missing.
    pub fn ensure(&mut self, name: &str) -> &mut Layer {
        let path = resolve(name);
        self.ensure_path(&path)
    }

    fn ensure_path(&mut self, path: &str) -> &mut Layer {
        if !self.map.contains_key(path) {
            let up = parent(path).to_string();
            if up != ":/" {
                self.ensure_path(&up);
            }
            let set = path.ends_with('/');
            let img = if set { Image { w: self.w, h: self.h, px: Vec::new() } } else { Image::black() };
            let layer = self.make(set, img);
            self.map.insert(path.to_string(), layer);
        }
        self.map.get_mut(path).unwrap()
    }

    /// Removes a layer, or a set with everything in it.
    pub fn remove(&mut self, name: &str) {
        let path = resolve(name);
        if path == SCREEN {
            let keep: Vec<String> = self.map.keys().filter(|k| k.starts_with(SCREEN) && *k != SCREEN).cloned().collect();
            keep.iter().for_each(|k| {
                self.map.remove(k);
            });
            return;
        }
        if path.ends_with('/') {
            self.map.retain(|k, _| !k.starts_with(&path));
        } else {
            self.map.remove(&path);
        }
    }

    /// Exchanges the pictures (and with `all` the settings) of two layers.
    pub fn swap(&mut self, a: &str, b: &str, all: bool) {
        let (pa, pb) = (resolve(a), resolve(b));
        let (Some(mut la), Some(mut lb)) = (self.map.remove(&pa), self.map.remove(&pb)) else { return };
        if all {
            std::mem::swap(&mut la, &mut lb);
            std::mem::swap(&mut la.tex, &mut lb.tex);
            std::mem::swap(&mut la.seq, &mut lb.seq);
        } else {
            std::mem::swap(&mut la.img, &mut lb.img);
        }
        la.touch();
        lb.touch();
        self.map.insert(pa, la);
        self.map.insert(pb, lb);
    }

    /// The layers directly in a set, back to front.
    pub fn children(&self, set: &str) -> Vec<&str> {
        let mut out: Vec<(&str, &Layer)> = self
            .map
            .iter()
            .filter(|(k, _)| k.as_str() != set && parent(k) == set)
            .map(|(k, l)| (k.as_str(), l))
            .collect();
        out.sort_by_key(|(_, l)| (l.z, l.seq));
        out.into_iter().map(|(k, _)| k).collect()
    }

    /// What to draw for a set, back to front, with its top left at (x, y).
    pub fn draws(&self, set: &str, x: i64, y: i64, a: i64, clip: Rect, out: &mut Vec<Draw>) {
        for path in self.children(set) {
            let l = &self.map[path];
            if !l.e || l.a <= 0 {
                continue;
            }
            let (lx, ly) = (x + l.x, y + l.y);
            let a = a * l.a.min(256) / 256;
            if l.set {
                let c = clip.and(&Rect::new(lx, ly, l.w() as i64, l.h() as i64));
                if !c.is_empty() {
                    self.draws(path, lx, ly, a, c, out);
                }
                continue;
            }
            let find = |name: &str| (!name.is_empty()).then(|| self.map.get(&resolve(name))).flatten();
            let mask = find(&l.mid).map(|m| (m.tex, x + m.x, y + m.y));
            let rule = find(&l.tid).filter(|_| l.t < 256).map(|r| (r.tex, x + r.x, y + r.y, l.t, l.ta));
            out.push(Draw { tex: l.tex, version: l.version, x: lx, y: ly, a, mode: l.mode, clip, mask, rule });
        }
    }

    /// The screen's draws.
    pub fn screen(&self) -> Vec<Draw> {
        let mut out = Vec::new();
        self.draws(SCREEN, 0, 0, 256, Rect::new(0, 0, self.w as i64, self.h as i64), &mut out);
        out
    }

    pub fn by_tex(&self) -> HashMap<u32, &Layer> {
        self.map.values().map(|l| (l.tex, l)).collect()
    }

    /// A set drawn into one picture of its size (CGACT COPY of a set, screenshots).
    pub fn render(&self, name: &str) -> Image {
        let path = resolve(name);
        let Some(set) = self.map.get(&path) else { return Image::black() };
        if !set.set {
            return set.img.clone();
        }
        let mut out = Image::new(set.w(), set.h());
        let mut draws = Vec::new();
        self.draws(&path, 0, 0, 256, out.rect(), &mut draws);
        let tex = self.by_tex();
        for d in draws {
            let Some(l) = tex.get(&d.tex) else { continue };
            let mut src = l.img.clone();
            if let Some((m, mx, my)) = d.mask {
                if let Some(m) = tex.get(&m) {
                    apply_mask(&mut src, d.x, d.y, &m.img, mx, my);
                }
            }
            if let Some((r, rx, ry, t, ta)) = d.rule {
                if let Some(r) = tex.get(&r) {
                    apply_rule(&mut src, d.x, d.y, &r.img, rx, ry, t, ta);
                }
            }
            out.draw(&src, d.x, d.y, d.a, d.mode, d.clip);
        }
        out
    }
}

/// How much of a pixel a transition shows: `rule` is the rule image's grey (0..=255), `t` the
/// progress (0..=256), `ta` the softness (0 = a hard edge).
pub fn rule_alpha(rule: u8, t: i64, ta: i64) -> f32 {
    let r = rule as f32 / 255.0;
    let t = t.clamp(0, 256) as f32 / 256.0;
    let soft = ta.max(0) as f32 / 256.0;
    if soft <= 0.0 {
        return if r < t { 1.0 } else { 0.0 };
    }
    // Spread the edge so that t=0 shows nothing and t=1 shows everything.
    ((t * (1.0 + soft) - r) / soft).clamp(0.0, 1.0)
}

/// Multiplies a layer's alpha by its mask's alpha (both at screen positions).
pub fn apply_mask(img: &mut Image, x: i64, y: i64, mask: &Image, mx: i64, my: i64) {
    let w = img.w as i64;
    for (i, p) in img.px.chunks_exact_mut(4).enumerate() {
        let (px, py) = (x + i as i64 % w - mx, y + i as i64 / w - my);
        let m = mask.at(px, py).map_or(0, |c| c[3]);
        p[3] = (p[3] as u32 * m as u32 / 255) as u8;
    }
}

pub fn apply_rule(img: &mut Image, x: i64, y: i64, rule: &Image, rx: i64, ry: i64, t: i64, ta: i64) {
    let w = img.w as i64;
    for (i, p) in img.px.chunks_exact_mut(4).enumerate() {
        let (px, py) = (x + i as i64 % w - rx, y + i as i64 / w - ry);
        let g = rule.at(px, py).map_or(255, |c| ((c[0] as u32 * 77 + c[1] as u32 * 150 + c[2] as u32 * 29) >> 8) as u8);
        p[3] = (p[3] as f32 * rule_alpha(g, t, ta)) as u8;
    }
}
