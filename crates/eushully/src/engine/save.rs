//! Our own saves, in ten slots of a menu over the game (Escape opens it, also on the title).
//! The games' own save screens are their scripts' and are not used. A save is taken while a
//! message waits for a click: the machine as it is (variables, the scripts running and where,
//! text windows, sprites, the music) and every texture as the list of how it was made (a file,
//! blank, filled, written on, drawn over), so that a load makes the same pictures again.

use super::*;

const SLOTS: usize = 10;
const MAGIC: &[u8; 8] = b"AGESAVE1";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Page {
    Main,
    Save,
    Load,
}

#[derive(Clone, Debug)]
pub(super) struct Menu {
    page: Page,
    /// what each slot holds (its last line of text)
    labels: Vec<Option<String>>,
}

fn slot_name(i: usize) -> String {
    format!("age{:02}.sav", i + 1)
}

// ---- bytes ----

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend(v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend(v.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(n as u32);
    }
    fn bytes(&mut self, b: &[u8]) {
        self.len(b.len());
        self.0.extend(b);
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn rgb(&mut self, c: [u8; 3]) {
        self.0.extend(c);
    }
    fn ptr(&mut self, p: Ptr) {
        match p {
            Ptr::G(v) => {
                self.u8(0);
                self.u32(v);
            }
            Ptr::L(d, v) => {
                self.u8(1);
                self.len(d);
                self.u32(v);
            }
            Ptr::Gs(v) => {
                self.u8(2);
                self.u32(v);
            }
            Ptr::Ls(d, v) => {
                self.u8(3);
                self.len(d);
                self.u32(v);
            }
            Ptr::Null => self.u8(4),
        }
    }
    fn ops(&mut self, ops: &[TexOp]) {
        self.len(ops.len());
        for op in ops {
            match op {
                TexOp::File(n) => {
                    self.u8(0);
                    self.i32(*n);
                }
                TexOp::New(w, h) => {
                    self.u8(1);
                    self.u32(*w);
                    self.u32(*h);
                }
                TexOp::Pixels(p) => {
                    self.u8(2);
                    self.u32(p.w);
                    self.u32(p.h);
                    self.bytes(&miniz_oxide::deflate::compress_to_vec(&p.px, 6));
                }
                TexOp::Fill(r, al, c) => {
                    self.u8(3);
                    r.iter().for_each(|v| self.i32(*v));
                    self.i32(*al);
                    self.i32(*c);
                }
                TexOp::Blit(src, r, dx, dy) => {
                    self.u8(4);
                    self.ops(src);
                    r.iter().for_each(|v| self.i32(*v));
                    self.i32(*dx);
                    self.i32(*dy);
                }
                TexOp::Text {
                    x,
                    y,
                    s,
                    size,
                    rgb,
                    shadow,
                } => {
                    self.u8(5);
                    self.i32(*x);
                    self.i32(*y);
                    self.str(s);
                    self.f32(*size);
                    self.rgb(*rgb);
                    self.rgb(*shadow);
                }
            }
        }
    }
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}

impl R<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(self.u32()? as i32)
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_bits(self.u32()?))
    }
    fn len(&mut self) -> Option<usize> {
        let n = self.u32()? as usize;
        // no list is longer than what is left of the file
        (n <= self.b.len()).then_some(n)
    }
    fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.len()?;
        Some(self.take(n)?.to_vec())
    }
    fn str(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
    fn rgb(&mut self) -> Option<[u8; 3]> {
        self.take(3)?.try_into().ok()
    }
    fn ptr(&mut self) -> Option<Ptr> {
        Some(match self.u8()? {
            0 => Ptr::G(self.u32()?),
            1 => Ptr::L(self.len()?, self.u32()?),
            2 => Ptr::Gs(self.u32()?),
            3 => Ptr::Ls(self.len()?, self.u32()?),
            _ => Ptr::Null,
        })
    }
    fn ops(&mut self) -> Option<Vec<TexOp>> {
        let n = self.len()?;
        let mut out = Vec::with_capacity(n.min(64));
        for _ in 0..n {
            out.push(match self.u8()? {
                0 => TexOp::File(self.i32()?),
                1 => TexOp::New(self.u32()?, self.u32()?),
                2 => {
                    let (w, h) = (self.u32()?, self.u32()?);
                    let px = miniz_oxide::inflate::decompress_to_vec(&self.bytes()?).ok()?;
                    if px.len() != (w as usize) * (h as usize) * 4 {
                        return None;
                    }
                    let mut img = Image::new(w, h);
                    img.px = px;
                    TexOp::Pixels(Rc::new(img))
                }
                3 => TexOp::Fill(
                    [self.i32()?, self.i32()?, self.i32()?, self.i32()?],
                    self.i32()?,
                    self.i32()?,
                ),
                4 => {
                    let src = Rc::new(self.ops()?);
                    let r = [self.i32()?, self.i32()?, self.i32()?, self.i32()?];
                    TexOp::Blit(src, r, self.i32()?, self.i32()?)
                }
                5 => TexOp::Text {
                    x: self.i32()?,
                    y: self.i32()?,
                    s: self.str()?,
                    size: self.f32()?,
                    rgb: self.rgb()?,
                    shadow: self.rgb()?,
                },
                _ => return None,
            });
        }
        Some(out)
    }
}

/// A loaded save before it replaces the running game.
struct Saved {
    now: f64,
    g: Vec<i32>,
    gs: HashMap<u32, String>,
    gp: HashMap<u32, Ptr>,
    frames: Vec<Frame>,
    switch: Vec<(i32, u32)>,
    window: i32,
    font_size: f32,
    text_rgb: [u8; 3],
    shadow_rgb: [u8; 3],
    windows: HashMap<i32, Window>,
    textures: Vec<(i32, Vec<TexOp>)>,
    sprites: BTreeMap<i32, Sprite>,
    bgm: Option<i32>,
}

impl Engine {
    // ---- the menu ----

    /// Escape: opens our menu, or closes it.
    pub fn menu(&mut self) {
        if self.menu.take().is_some() {
            return;
        }
        let labels = (0..SLOTS).map(|i| self.slot_label(i)).collect();
        self.menu = Some(Menu {
            page: Page::Main,
            labels,
        });
    }

    /// Whether the game is where a save can be taken (a message waiting for a click).
    pub fn can_save(&self) -> bool {
        self.wait == Wait::Click && !self.frames.is_empty()
    }

    fn main_items(&self) -> Vec<(String, [i32; 4], bool)> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        let can_save = self.can_save();
        [("セーブ", can_save), ("ロード", true), ("閉じる", true)]
            .iter()
            .enumerate()
            .map(|(i, &(n, on))| {
                let y = h / 2 - 110 + i as i32 * 80;
                (n.to_string(), [w / 2 - 160, y, w / 2 + 160, y + 60], on)
            })
            .collect()
    }

    fn slot_rects(&self) -> Vec<[i32; 4]> {
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        let row = (h - 80) / SLOTS as i32;
        (0..SLOTS as i32)
            .map(|i| {
                let y = 40 + i * row;
                [w / 8, y + 3, w - w / 8, y + row - 3]
            })
            .collect()
    }

    fn slot_label(&mut self, i: usize) -> Option<String> {
        let b = self.files.read_save(&slot_name(i))?;
        let mut r = R { b: &b, p: 0 };
        if r.take(8)? != MAGIC {
            return None;
        }
        r.str()
    }

    pub(super) fn menu_draws(&mut self, out: &mut Vec<Draw>) {
        let Some(m) = self.menu.clone() else {
            return;
        };
        let (w, h) = (self.screen.0 as i32, self.screen.1 as i32);
        self.rect([0, 0, w, h], [0, 0, 0, 160], out);
        let items: Vec<(String, [i32; 4], bool)> = match m.page {
            Page::Main => self.main_items(),
            Page::Save | Page::Load => {
                let head = if m.page == Page::Save {
                    "セーブ"
                } else {
                    "ロード"
                };
                self.label(head, [w / 8, 4, w, 40], [255, 255, 255], out);
                self.slot_rects()
                    .into_iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let l = m.labels[i].as_deref();
                        let text = match l {
                            Some(t) => {
                                format!("{:02}  {}", i + 1, t.chars().take(30).collect::<String>())
                            }
                            None => format!("{:02}  ----", i + 1),
                        };
                        (text, r, m.page == Page::Save || l.is_some())
                    })
                    .collect()
            }
        };
        let hit = self.pointer;
        for (text, r, on) in items {
            let over = on && inside(r, hit.0, hit.1);
            let bg = if over {
                [90, 70, 40, 230]
            } else {
                [30, 30, 40, 220]
            };
            self.rect(r, bg, out);
            let c = if on { [255, 255, 255] } else { [120, 120, 120] };
            self.label(&text, r, c, out);
        }
    }

    fn rect(&mut self, r: [i32; 4], rgba: [u8; 4], out: &mut Vec<Draw>) {
        let mut img = Image::new(1, 1);
        img.fill(rgba);
        let dst = [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32];
        out.push(Draw {
            image: Rc::new(img),
            src: [0, 0, 1, 1],
            dst,
            place: self.flat(),
        });
    }

    fn label(&mut self, text: &str, r: [i32; 4], c: [u8; 3], out: &mut Vec<Draw>) {
        let size = ((r[3] - r[1]) as f32 * 0.5).clamp(14.0, 28.0);
        let Some(img) = self.line_image(text, size, c, [0, 0, 0]) else {
            return;
        };
        let x = r[0] as f32 + 16.0;
        let y = (r[1] + r[3]) as f32 / 2.0 - img.h as f32 / 2.0;
        out.push(Draw {
            image: img.clone(),
            src: [0, 0, img.w as i32, img.h as i32],
            dst: [x, y, x + img.w as f32, y + img.h as f32],
            // a long line ends at the button's edge
            place: Place {
                clip: [r[0] as f32, r[1] as f32, r[2] as f32 - 8.0, r[3] as f32],
                ..self.flat()
            },
        });
    }

    pub(super) fn flat(&self) -> Place {
        Place {
            x: 0.0,
            y: 0.0,
            sx: 1.0,
            sy: 1.0,
            cx: 0.0,
            cy: 0.0,
            rot: 0.0,
            alpha: 1.0,
            add: false,
            flip: (false, false),
            clip: [0.0, 0.0, self.screen.0 as f32, self.screen.1 as f32],
        }
    }

    pub(super) fn menu_click(&mut self, x: i32, y: i32) {
        let Some(mut m) = self.menu.clone() else {
            return;
        };
        let (x, y) = if x < 0 { self.pointer } else { (x, y) };
        match m.page {
            Page::Main => {
                let hit = self
                    .main_items()
                    .into_iter()
                    .position(|(_, r, on)| on && inside(r, x, y));
                match hit {
                    Some(0) => m.page = Page::Save,
                    Some(1) => m.page = Page::Load,
                    Some(_) => {
                        self.menu = None;
                        return;
                    }
                    None => {}
                }
                self.menu = Some(m);
            }
            Page::Save | Page::Load => {
                let Some(i) = self.slot_rects().into_iter().position(|r| inside(r, x, y)) else {
                    return;
                };
                if m.page == Page::Save {
                    self.save_slot(i);
                    self.menu = None;
                } else if m.labels[i].is_some() && self.load_slot(i) {
                    self.menu = None;
                }
            }
        }
    }

    // ---- saving ----

    /// Writes slot `i` (0..10); false where a save cannot be taken.
    pub fn save_slot(&mut self, i: usize) -> bool {
        if !self.can_save() || i >= SLOTS {
            return false;
        }
        self.settle();
        let mut w = W::default();
        w.0.extend(MAGIC);
        let label = self.text_log.last().cloned().unwrap_or_default();
        w.str(&label);
        // the rest is deflated: the variable arrays are long and mostly zero
        let head = w.0.len();
        w.0.extend(self.now.to_le_bytes());
        w.len(self.g.len());
        self.g.iter().for_each(|v| w.i32(*v));
        w.len(self.gs.len());
        for (k, v) in &self.gs {
            w.u32(*k);
            w.str(v);
        }
        w.len(self.gp.len());
        for (k, v) in &self.gp {
            w.u32(*k);
            w.ptr(*v);
        }
        w.len(self.frames.len());
        for f in &self.frames {
            w.str(&f.script.name);
            w.len(f.pc);
            w.len(f.ret.len());
            f.ret.iter().for_each(|v| w.len(*v));
            w.len(f.l.len());
            f.l.iter().for_each(|v| w.i32(*v));
            w.len(f.ls.len());
            for (k, v) in &f.ls {
                w.u32(*k);
                w.str(v);
            }
            w.len(f.lp.len());
            for (k, v) in &f.lp {
                w.u32(*k);
                w.ptr(*v);
            }
            for m in [&f.mouse, &f.joy] {
                w.len(m.len());
                for (k, v) in m {
                    w.i32(*k);
                    w.u32(*v);
                }
            }
        }
        w.len(self.switch.len());
        for (a, b) in &self.switch {
            w.i32(*a);
            w.u32(*b);
        }
        w.i32(self.window);
        w.f32(self.font_size);
        w.rgb(self.text_rgb);
        w.rgb(self.shadow_rgb);
        w.len(self.windows.len());
        for (k, win) in &self.windows {
            w.i32(*k);
            for v in [win.x, win.y, win.w, win.h, win.tx, win.ty, win.right] {
                w.i32(v);
            }
            w.len(win.lines.len());
            win.lines.iter().for_each(|l| w.str(l));
            w.f32(win.size);
            w.rgb(win.rgb);
            w.rgb(win.shadow);
        }
        w.len(self.textures.len());
        for (id, t) in &self.textures {
            w.i32(*id);
            match self.recipes.get(id) {
                Some(r) => w.ops(r),
                None => w.ops(&[TexOp::Pixels(t.clone())]),
            }
        }
        w.len(self.sprites.len());
        for (id, s) in &self.sprites {
            // animations end where they go
            let mut s = s.clone();
            for a in std::mem::take(&mut s.anims) {
                s.put(a.kind, a.to);
            }
            w.i32(*id);
            w.i32(s.tex);
            s.src.iter().for_each(|v| w.i32(*v));
            for v in [
                s.x, s.y, s.alpha, s.pivot.0, s.pivot.1, s.scale[0], s.scale[1], s.off[0], s.off[1],
            ] {
                w.f32(v);
            }
            w.u8(s.add as u8);
            w.i32(s.tint);
        }
        w.i32(self.bgm.unwrap_or(-1));
        let body = miniz_oxide::deflate::compress_to_vec(&w.0[head..], 6);
        w.0.truncate(head);
        w.0.extend(body);
        self.files.write_save(&slot_name(i), &w.0);
        true
    }

    /// Loads slot `i`; false (and the game as it was) when it is empty or does not read.
    pub fn load_slot(&mut self, i: usize) -> bool {
        let Some(b) = self.files.read_save(&slot_name(i)) else {
            return false;
        };
        let Some(s) = self.read_saved(&b) else {
            return false;
        };
        self.now = s.now;
        self.g = s.g;
        self.gs = s.gs;
        self.gp = s.gp;
        self.frames = s.frames;
        self.switch = s.switch;
        self.window = s.window;
        self.font_size = s.font_size;
        self.text_rgb = s.text_rgb;
        self.shadow_rgb = s.shadow_rgb;
        self.windows = s.windows;
        self.textures.clear();
        self.recipes.clear();
        self.tinted.clear();
        for (id, ops) in s.textures {
            if let Some(img) = self.build(&ops) {
                self.textures.insert(id, Rc::new(img));
                self.recipes.insert(id, Rc::new(ops));
            }
        }
        self.sprites = s.sprites;
        self.wait = Wait::Click;
        self.clicked = false;
        self.keys.clear();
        self.buttons = 0;
        self.release = 0;
        self.ended = false;
        self.stop(-1, 300);
        self.bgm = s.bgm;
        if let Some(n) = s.bgm {
            self.play(n, BGM, true);
        }
        true
    }

    fn read_saved(&mut self, b: &[u8]) -> Option<Saved> {
        let mut r = R { b, p: 0 };
        if r.take(8)? != MAGIC {
            return None;
        }
        r.str()?;
        let body = miniz_oxide::inflate::decompress_to_vec(&b[r.p..]).ok()?;
        let mut r = R { b: &body, p: 0 };
        let now = f64::from_le_bytes(r.take(8)?.try_into().ok()?);
        let g = (0..r.len()?).map(|_| r.i32()).collect::<Option<Vec<_>>>()?;
        let mut gs = HashMap::new();
        for _ in 0..r.len()? {
            gs.insert(r.u32()?, r.str()?);
        }
        let mut gp = HashMap::new();
        for _ in 0..r.len()? {
            gp.insert(r.u32()?, r.ptr()?);
        }
        let mut frames = Vec::new();
        for _ in 0..r.len()? {
            let name = r.str()?;
            let n = self.index.find(&name)?;
            let script = self.script(n).ok()?;
            let pc = r.len()?;
            let ret = (0..r.len()?).map(|_| r.len()).collect::<Option<Vec<_>>>()?;
            let l = (0..r.len()?).map(|_| r.i32()).collect::<Option<Vec<_>>>()?;
            let mut ls = HashMap::new();
            for _ in 0..r.len()? {
                ls.insert(r.u32()?, r.str()?);
            }
            let mut lp = HashMap::new();
            for _ in 0..r.len()? {
                lp.insert(r.u32()?, r.ptr()?);
            }
            let mut maps = [HashMap::new(), HashMap::new()];
            for m in &mut maps {
                for _ in 0..r.len()? {
                    m.insert(r.i32()?, r.u32()?);
                }
            }
            let [mouse, joy] = maps;
            frames.push(Frame {
                script,
                pc,
                ret,
                l,
                ls,
                lp,
                mouse,
                joy,
            });
        }
        let mut switch = Vec::new();
        for _ in 0..r.len()? {
            switch.push((r.i32()?, r.u32()?));
        }
        let window = r.i32()?;
        let font_size = r.f32()?;
        let text_rgb = r.rgb()?;
        let shadow_rgb = r.rgb()?;
        let mut windows = HashMap::new();
        for _ in 0..r.len()? {
            let k = r.i32()?;
            let mut v = [0; 7];
            for x in &mut v {
                *x = r.i32()?;
            }
            let lines = (0..r.len()?).map(|_| r.str()).collect::<Option<Vec<_>>>()?;
            let win = Window {
                x: v[0],
                y: v[1],
                w: v[2],
                h: v[3],
                tx: v[4],
                ty: v[5],
                right: v[6],
                lines,
                size: r.f32()?,
                rgb: r.rgb()?,
                shadow: r.rgb()?,
            };
            windows.insert(k, win);
        }
        let mut textures = Vec::new();
        for _ in 0..r.len()? {
            textures.push((r.i32()?, r.ops()?));
        }
        let mut sprites = BTreeMap::new();
        for _ in 0..r.len()? {
            let id = r.i32()?;
            let tex = r.i32()?;
            let src = [r.i32()?, r.i32()?, r.i32()?, r.i32()?];
            let mut f = [0.0; 9];
            for x in &mut f {
                *x = r.f32()?;
            }
            let s = Sprite {
                tex,
                src,
                x: f[0],
                y: f[1],
                alpha: f[2],
                pivot: (f[3], f[4]),
                scale: [f[5], f[6]],
                off: [f[7], f[8]],
                add: r.u8()? != 0,
                tint: r.i32()?,
                anims: Vec::new(),
            };
            sprites.insert(id, s);
        }
        let bgm = Some(r.i32()?).filter(|&n| n >= 0);
        Some(Saved {
            now,
            g,
            gs,
            gp,
            frames,
            switch,
            window,
            font_size,
            text_rgb,
            shadow_rgb,
            windows,
            textures,
            sprites,
            bgm,
        })
    }
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}
