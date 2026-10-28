//! The objects of a QLIE game: pictures (`img`), buttons (`btn`, `sbtn`), message windows
//! (`msg`), sounds (`snd`), lists, and the built-in ones (`sys`, `eff`, `inp`, ...). A picture is
//! a still image or an ABMP animation; it shows from `\e`, playing its "enable" motion, and hides
//! after its "disable" motion from `\d`.

use std::rc::Rc;

use nexas::gfx::{Draw, Image, Place};

use crate::abmp::{Abmp, Head, Show};
use crate::text::{Item, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OKind {
    Img,
    Btn,
    /// a button of the system: its second string is a label of the system thread
    SBtn,
    Msg,
    Bar,
    List,
    Snd,
    Movie,
    Sys,
    Eff,
    Cam,
    Rep,
    SKey,
    Inp,
    Misc,
    Cfg,
    Log,
    Flags,
    Dat,
    Math,
    Other,
}

impl OKind {
    /// The kind of a `\var` type name.
    pub fn of(name: &str) -> OKind {
        match name.to_ascii_lowercase().as_str() {
            "img" | "obj" => OKind::Img,
            "btn" => OKind::Btn,
            "sbtn" => OKind::SBtn,
            "msg" => OKind::Msg,
            "bar" => OKind::Bar,
            "list" | "table" => OKind::List,
            "snd" => OKind::Snd,
            "theora" => OKind::Movie,
            _ => OKind::Other,
        }
    }
    pub fn shows(self) -> bool {
        matches!(
            self,
            OKind::Img | OKind::Btn | OKind::SBtn | OKind::Msg | OKind::Bar
        )
    }
    pub fn button(self) -> bool {
        matches!(self, OKind::Btn | OKind::SBtn)
    }
}

pub enum Pic {
    Still(Rc<Image>),
    Anim {
        abmp: Rc<Abmp>,
        /// by number: the `\l` files after the .b, else the .b's own pictures
        images: Vec<Option<Rc<Image>>>,
    },
}

/// A value moving to a target over time.
#[derive(Debug, Clone, Copy)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    pub start: f64,
    pub ms: f64,
}

impl Tween {
    pub fn new(v: f32) -> Tween {
        Tween {
            from: v,
            to: v,
            start: 0.0,
            ms: 0.0,
        }
    }
    pub fn at(&self, now: f64) -> f32 {
        if self.ms <= 0.0 || now >= self.start + self.ms {
            return self.to;
        }
        let f = ((now - self.start) / self.ms).clamp(0.0, 1.0) as f32;
        self.from + (self.to - self.from) * f
    }
    pub fn set(&mut self, v: f32, ms: f64, now: f64) {
        self.from = self.at(now);
        self.to = v;
        self.start = now;
        self.ms = ms;
    }
}

/// The text of a message window or button.
#[derive(Default)]
pub struct TextBox {
    pub items: Vec<Item>,
    pub style: Style,
    /// characters shown (typing)
    pub shown: f32,
    pub rect: [i32; 4],
    pub image: Option<(usize, Rc<Image>)>,
    pub dirty: bool,
}

#[derive(Default)]
pub struct Sound {
    pub file: String,
    pub data: Option<Rc<Vec<u8>>>,
    pub volume: Tween,
    pub playing: bool,
    pub repeat: bool,
}

impl Default for Tween {
    fn default() -> Tween {
        Tween::new(255.0)
    }
}

pub struct Obj {
    pub kind: OKind,
    /// the thread whose variable made it
    pub thread: usize,
    pub x: f32,
    pub y: f32,
    pub z: i32,
    /// enabled (`\e`)
    pub shown: bool,
    /// playing its disable motion, after which it hides
    pub hiding: bool,
    pub alpha: Tween,
    pub files: Vec<String>,
    pub pic: Option<Pic>,
    pub motion: Option<usize>,
    pub heads: Vec<Head>,
    /// motion speed, percent
    pub speed: f32,
    /// the motion was started by \e (enable) and the button has not settled yet
    pub entering: bool,
    // buttons
    pub us: Vec<String>,
    pub ui: i64,
    pub hover: bool,
    pub down: bool,
    pub lock: bool,
    // text
    pub text: Option<TextBox>,
    // sounds
    pub snd: Sound,
    // lists
    pub list: Vec<(String, String)>,
    // a picture's sounds (by number) to play
    pub cues: Vec<u8>,
    /// `\ds,left,top,right,bottom`: the button is there only while the pointer is in this area
    pub sense: Option<[i32; 4]>,
    /// a bar's value and its most
    pub bar: (i64, i64),
    /// sounds `\ls` put in place of the picture's own, by number
    pub sounds: Vec<Option<Rc<Vec<u8>>>>,
}

impl Obj {
    pub fn new(kind: OKind, thread: usize) -> Obj {
        Obj {
            kind,
            thread,
            x: 0.0,
            y: 0.0,
            z: 0,
            shown: false,
            hiding: false,
            alpha: Tween::new(255.0),
            files: Vec::new(),
            pic: None,
            motion: None,
            heads: Vec::new(),
            speed: 100.0,
            entering: false,
            us: Vec::new(),
            ui: 0,
            hover: false,
            down: false,
            lock: false,
            text: None,
            snd: Sound::default(),
            list: Vec::new(),
            cues: Vec::new(),
            sense: None,
            bar: (0, 0),
            sounds: Vec::new(),
        }
    }

    /// Away because the pointer is outside its `\ds` area.
    pub fn away(&self, (x, y): (i32, i32)) -> bool {
        self.sense
            .is_some_and(|[l, t, r, b]| x < l || y < t || x >= r || y >= b)
    }

    /// Back to nothing loaded (`\i`), keeping the place.
    pub fn reset(&mut self) {
        let (kind, thread) = (self.kind, self.thread);
        *self = Obj::new(kind, thread);
    }

    pub fn abmp(&self) -> Option<&Rc<Abmp>> {
        match &self.pic {
            Some(Pic::Anim { abmp, .. }) => Some(abmp),
            _ => None,
        }
    }

    /// Plays motion `m` from its start.
    pub fn play(&mut self, m: Option<usize>) {
        self.motion = m;
        self.heads.clear();
        let Some(a) = self.abmp().cloned() else {
            return;
        };
        let Some(m) = m.and_then(|m| a.motions.get(m)) else {
            return;
        };
        let mut cues = Vec::new();
        for l in &m.layers {
            let mut h = Head::default();
            l.start(&mut h, &mut cues);
            self.heads.push(h);
        }
        self.cues.extend(cues);
    }

    pub fn play_named(&mut self, name: &str) -> bool {
        let m = self.abmp().and_then(|a| a.motion(name));
        if m.is_some() {
            self.play(m);
        }
        m.is_some()
    }

    /// The motion has come to rest: every layer stopped, looped or past its last key.
    pub fn settled(&self) -> bool {
        let Some(a) = self.abmp() else { return true };
        let Some(m) = self.motion.and_then(|m| a.motions.get(m)) else {
            return true;
        };
        m.layers.iter().zip(&self.heads).all(|(l, h)| {
            h.stopped || h.looped || l.keys.last().is_none_or(|k| h.t >= k.time as f32)
        })
    }

    /// Moves the motion on `dt` ms.
    pub fn advance(&mut self, dt: f32) {
        let Some(a) = self.abmp().cloned() else {
            return;
        };
        let Some(m) = self.motion.and_then(|m| a.motions.get(m)) else {
            return;
        };
        let dt = dt * self.speed / 100.0;
        let mut cues = Vec::new();
        for (l, h) in m.layers.iter().zip(self.heads.iter_mut()) {
            l.advance(h, dt, &mut cues);
        }
        self.cues.extend(cues);
        if let Some(g) = self.heads.iter_mut().find_map(|h| h.goto.take())
            && (g as usize) < a.motions.len()
        {
            self.play(Some(g as usize));
        }
    }

    /// The size of picture `i`.
    pub fn size(&self, i: usize) -> (u32, u32) {
        match &self.pic {
            Some(Pic::Still(im)) => (im.w, im.h),
            Some(Pic::Anim { images, .. }) => images
                .get(i)
                .and_then(|o| o.as_ref())
                .map_or((0, 0), |im| (im.w, im.h)),
            None => (0, 0),
        }
    }

    /// The rectangles it shows (and, for buttons, where it can be clicked): x0 y0 x1 y1.
    pub fn bounds(&self, hit: bool) -> Vec<[f32; 4]> {
        let mut out = Vec::new();
        match &self.pic {
            Some(Pic::Still(im)) => {
                out.push([self.x, self.y, self.x + im.w as f32, self.y + im.h as f32])
            }
            Some(Pic::Anim { abmp, images }) => {
                let Some(m) = self.motion.and_then(|m| abmp.motions.get(m)) else {
                    return out;
                };
                let mut hits = Vec::new();
                for (l, h) in m.layers.iter().zip(&self.heads) {
                    let Some(s) = l.state(h) else { continue };
                    let (w, hh) = match s.show {
                        Show::Hit | Show::Fill => (s.sx, s.sy),
                        Show::Image | Show::Other(_) => {
                            let Some(Some(im)) = images.get(s.image as usize) else {
                                continue;
                            };
                            (im.w as f32 * s.sx / 100.0, im.h as f32 * s.sy / 100.0)
                        }
                        Show::None => continue,
                    };
                    let (ax, ay) = s.anchor;
                    let x0 = self.x + s.x - w * ax;
                    let y0 = self.y + s.y - hh * ay;
                    let r = [x0, y0, x0 + w, y0 + hh];
                    if s.show == Show::Hit {
                        hits.push(r);
                    } else {
                        out.push(r);
                    }
                }
                if hit && !hits.is_empty() {
                    return hits;
                }
            }
            None => {}
        }
        if let Some(t) = &self.text
            && (out.is_empty() || !hit)
        {
            let r = t.rect;
            out.push([
                self.x + r[0] as f32,
                self.y + r[1] as f32,
                self.x + (r[0] + r[2]) as f32,
                self.y + (r[1] + r[3]) as f32,
            ]);
        }
        out
    }

    pub fn inside(&self, (mx, my): (i32, i32)) -> bool {
        let (mx, my) = (mx as f32, my as f32);
        self.bounds(true)
            .iter()
            .any(|r| mx >= r[0] && my >= r[1] && mx < r[2] && my < r[3])
    }
}

/// Where the anchor of grid number `a` (0 top left .. 8 bottom right) is, as fractions.
pub fn anchor(a: u8) -> (f32, f32) {
    let a = a.min(8);
    ((a % 3) as f32 * 0.5, (a / 3) as f32 * 0.5)
}

pub fn place(alpha: f32, w: f32, h: f32) -> Place {
    Place {
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
    }
}

/// The draws of an object (with its layers' z, for ordering).
pub fn draws(
    o: &Obj,
    now: f64,
    (sw, sh): (f32, f32),
    fill: &mut dyn FnMut([u8; 3]) -> Rc<Image>,
    out: &mut Vec<(i32, Draw)>,
) {
    let alpha = o.alpha.at(now) / 255.0;
    if alpha <= 0.0 {
        return;
    }
    match &o.pic {
        Some(Pic::Still(im)) => {
            out.push((
                0,
                Draw {
                    image: im.clone(),
                    src: [0, 0, im.w as i32, im.h as i32],
                    dst: [o.x, o.y, o.x + im.w as f32, o.y + im.h as f32],
                    place: place(alpha, sw, sh),
                },
            ));
        }
        Some(Pic::Anim { abmp, images }) if abmp.motions.is_empty() => {
            // no motions read: the first picture, as a still one
            if let Some(Some(im)) = images.first() {
                out.push((
                    0,
                    Draw {
                        image: im.clone(),
                        src: [0, 0, im.w as i32, im.h as i32],
                        dst: [o.x, o.y, o.x + im.w as f32, o.y + im.h as f32],
                        place: place(alpha, sw, sh),
                    },
                ));
            }
        }
        Some(Pic::Anim { abmp, images }) => {
            if let Some(m) = o.motion.and_then(|m| abmp.motions.get(m)) {
                for (l, h) in m.layers.iter().zip(&o.heads) {
                    let Some(s) = l.state(h) else { continue };
                    let (image, w, hh) = match s.show {
                        Show::Fill => (fill(s.color), s.sx, s.sy),
                        Show::Image | Show::Other(_) => {
                            let Some(Some(im)) = images.get(s.image as usize) else {
                                continue;
                            };
                            (im.clone(), im.w as f32, im.h as f32)
                        }
                        _ => continue,
                    };
                    if s.alpha <= 0.0 || w <= 0.0 || hh <= 0.0 {
                        continue;
                    }
                    let (ax, ay) = s.anchor;
                    let (px, py) = (o.x + s.x, o.y + s.y);
                    let dst = [
                        px - w * ax,
                        py - hh * ay,
                        px - w * ax + w,
                        py - hh * ay + hh,
                    ];
                    let (scx, scy) = if s.show == Show::Fill {
                        (1.0, 1.0)
                    } else {
                        (s.sx / 100.0, s.sy / 100.0)
                    };
                    if scx == 0.0 || scy == 0.0 {
                        continue;
                    }
                    let (iw, ih) = (image.w as i32, image.h as i32);
                    let mut p = place(alpha * s.alpha, sw, sh);
                    p.sx = scx.abs();
                    p.sy = scy.abs();
                    p.flip = (scx < 0.0, scy < 0.0);
                    p.cx = px;
                    p.cy = py;
                    p.rot = s.rot.to_radians();
                    out.push((
                        s.z as i32,
                        Draw {
                            image,
                            src: [0, 0, iw, ih],
                            dst,
                            place: p,
                        },
                    ));
                }
            }
        }
        None => {}
    }
    if let Some(t) = &o.text
        && let Some((_, im)) = &t.image
    {
        let (x, y) = (o.x + t.rect[0] as f32, o.y + t.rect[1] as f32);
        out.push((
            1000,
            Draw {
                image: im.clone(),
                src: [0, 0, im.w as i32, im.h as i32],
                dst: [x, y, x + im.w as f32, y + im.h as f32],
                place: place(alpha, sw, sh),
            },
        ));
    }
}
