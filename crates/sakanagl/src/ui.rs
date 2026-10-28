//! Views from the editor's .ui files (SKView and its kinds in system/ui/*.sk): a tree of groups,
//! pictures, labels and buttons, each with a size, a pivot, and motions (keyed properties; we
//! take the pose at time 0).
//!
//! A view's box is `-pivot .. -pivot + size` around its position; its children are placed from
//! the box's top left corner. A child that links another .ui (buttons) takes that file's root:
//! its size, pivot and children.

use crate::obj::Value;

pub const NAME: i64 = 1000;
pub const POSITION: i64 = 1001;
pub const SCALE: i64 = 1002;
pub const ROTATION: i64 = 1003;
pub const VISIBLE: i64 = 1004;
pub const SIZE: i64 = 1006;
pub const PIVOT: i64 = 1007;
pub const BUFFERCOLOR: i64 = 1010;
pub const IMAGE: i64 = 1100;
pub const IMAGECOLOR: i64 = 1101;
pub const IMAGEBLEND: i64 = 1102;
pub const IMAGEALIGN: i64 = 1103;
pub const LABEL: i64 = 1200;
pub const FONT: i64 = 1201;
pub const TEXTCOLOR: i64 = 1202;
pub const TEXTALIGN: i64 = 1203;
pub const SIZE_STATIC: i64 = 2000;
pub const PIVOT_STATIC: i64 = 2001;

#[derive(Clone, Debug, Default)]
pub struct Label {
    pub text: String,
    pub size: f32,
    /// RGBA
    pub color: u32,
    pub stroke: u32,
    pub stroke_size: f32,
    /// 0 left, 1 centre, 2 right (alignlist % 3); rows by alignlist / 3
    pub align: i64,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    /// 0 group, 1 label, 2 picture, 4 button
    pub kind: i64,
    pub image: Option<String>,
    pub size: [f32; 2],
    pub pivot: [f32; 2],
    pub pos: [f32; 2],
    pub scale: [f32; 2],
    /// degrees
    pub rot: f32,
    pub visible: bool,
    /// RGBA, the picture's tint and opacity
    pub color: u32,
    /// RGBA, a group's tint and opacity
    pub bufcolor: u32,
    pub align: i64,
    pub blend: i64,
    pub label: Option<Label>,
    pub children: Vec<Node>,
    /// The linked .ui (buttons), resolved by [`Node::link`].
    pub link: Option<String>,
    /// Opacity set by the engine (fades), times the colours'.
    pub alpha: f32,
    /// A plain colour box (the engine's fades).
    pub fill: Option<u32>,
}

impl Default for Node {
    fn default() -> Node {
        Node {
            name: String::new(),
            kind: 0,
            image: None,
            size: [0.0; 2],
            pivot: [0.0; 2],
            pos: [0.0; 2],
            scale: [1.0; 2],
            rot: 0.0,
            visible: true,
            color: 0xffff_ffff,
            bufcolor: 0xffff_ffff,
            align: 9,
            blend: 2,
            label: None,
            children: Vec::new(),
            link: None,
            alpha: 1.0,
            fill: None,
        }
    }
}

fn f2(v: &Value) -> Option<[f32; 2]> {
    Some([v.at(0)?.num()? as f32, v.at(1)?.num()? as f32])
}

fn rgba(v: &Value) -> Option<u32> {
    v.num().map(|n| n as i64 as u32)
}

impl Node {
    /// A .ui file's root view.
    pub fn from_ui(v: &Value) -> Node {
        let root = v.get("rootdata").unwrap_or(v);
        Node::from_value(root)
    }

    pub fn from_value(v: &Value) -> Node {
        let mut n = Node {
            kind: v.get("type").and_then(Value::num).unwrap_or(0.0) as i64,
            ..Node::default()
        };
        let mut font: Option<&Value> = None;
        let mut text_color: Option<u32> = None;
        let mut text_align = 0;
        let mut is_label = false;
        for p in v.get("staticparams").map(Value::list).unwrap_or(&[]) {
            let (Some(id), Some(val)) = (p.at(0).and_then(Value::num), p.at(1)) else {
                continue;
            };
            match id as i64 {
                NAME => n.name = val.str().unwrap_or("").to_string(),
                IMAGE => n.image = val.str().map(str::to_string),
                SIZE_STATIC => n.size = f2(val).unwrap_or(n.size),
                PIVOT_STATIC => n.pivot = f2(val).unwrap_or(n.pivot),
                IMAGEALIGN => n.align = val.num().unwrap_or(9.0) as i64,
                IMAGEBLEND => n.blend = val.num().unwrap_or(2.0) as i64,
                LABEL => {
                    is_label = true;
                    n.label.get_or_insert_with(Label::default).text =
                        val.str().unwrap_or("").into();
                }
                FONT => {
                    is_label = true;
                    font = Some(val);
                }
                TEXTALIGN => text_align = val.num().unwrap_or(0.0) as i64,
                _ => {}
            }
        }
        for m in v.get("motion").map(Value::list).unwrap_or(&[]) {
            let Some(pid) = m.get("pid").and_then(Value::num) else {
                continue;
            };
            // The pose at time 0: the first key.
            let Some(p) = m.get("key").and_then(|k| k.at(0)).and_then(|k| k.get("p")) else {
                continue;
            };
            match pid as i64 {
                POSITION => n.pos = f2(p).unwrap_or(n.pos),
                SCALE => {
                    if let Some(s) = f2(p) {
                        n.scale = [s[0] / 100.0, s[1] / 100.0];
                    }
                }
                ROTATION => n.rot = p.at(2).and_then(Value::num).unwrap_or(0.0) as f32,
                VISIBLE => n.visible = p.truthy(),
                SIZE => n.size = f2(p).unwrap_or(n.size),
                PIVOT => n.pivot = f2(p).unwrap_or(n.pivot),
                IMAGECOLOR => n.color = rgba(p).unwrap_or(n.color),
                BUFFERCOLOR => n.bufcolor = rgba(p).unwrap_or(n.bufcolor),
                TEXTCOLOR => text_color = rgba(p),
                _ => {}
            }
        }
        if is_label {
            let f = |k: &str| font.and_then(|f| f.get(k)).and_then(Value::num);
            let l = n.label.get_or_insert_with(Label::default);
            l.size = f("fontsize").unwrap_or(24.0) as f32;
            l.color = text_color
                .or(f("fontcolor").map(|c| c as i64 as u32))
                .unwrap_or(0xffff_ffff);
            l.stroke = f("strokecolor").map_or(0, |c| c as i64 as u32);
            l.stroke_size = f("strokesize").unwrap_or(0.0) as f32;
            l.align = text_align;
            n.kind = 1;
        }
        if let Some(link) = v.get("link").and_then(Value::str)
            && link.starts_with('/') {
                n.link = Some(link.to_string());
            }
        for c in v.get("children").map(Value::list).unwrap_or(&[]) {
            n.children.push(Node::from_value(c));
        }
        n
    }

    /// Resolves linked .ui files with `load` (paths as stored, such as `/ui/common/talk/x.ui`).
    pub fn link(&mut self, load: &mut dyn FnMut(&str) -> Option<Node>, depth: u32) {
        if depth > 8 {
            return;
        }
        if let Some(path) = self.link.take()
            && let Some(root) = load(&path) {
                if self.size == [0.0; 2] {
                    self.size = root.size;
                    self.pivot = root.pivot;
                }
                self.children.extend(root.children);
            }
        for c in &mut self.children {
            c.link(load, depth + 1);
        }
    }

    /// The first view named `name` at or under this one, depth first.
    pub fn find_mut(&mut self, name: &str) -> Option<&mut Node> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter_mut().find_map(|c| c.find_mut(name))
    }

    pub fn find(&self, name: &str) -> Option<&Node> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(name))
    }

    /// A dotted path (`GROUP.IMG.FG`): each name is looked for under the last.
    pub fn path_mut(&mut self, path: &str) -> Option<&mut Node> {
        let mut at = self;
        for seg in path.split('.') {
            at = at.children.iter_mut().find_map(|c| c.find_mut(seg))?;
        }
        Some(at)
    }

    pub fn path(&self, path: &str) -> Option<&Node> {
        let mut at = self;
        for seg in path.split('.') {
            at = at.children.iter().find_map(|c| c.find(seg))?;
        }
        Some(at)
    }

    /// The transform from this view's box (its top left at 0,0) to its parent's box.
    pub fn local(&self) -> Affine {
        Affine::translate(self.pos[0], self.pos[1])
            .then(&Affine::rotate(self.rot.to_radians()))
            .then(&Affine::scale(self.scale[0], self.scale[1]))
            .then(&Affine::translate(-self.pivot[0], -self.pivot[1]))
    }

    /// The view's own opacity (engine fade and colours).
    pub fn opacity(&self) -> f32 {
        let a = |c: u32| (c & 0xff) as f32 / 255.0;
        let own = if self.kind == 2 { a(self.color) } else { 1.0 };
        self.alpha * a(self.bufcolor) * own
    }
}

/// x' = a x + c y + e, y' = b x + d y + f
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine(pub [f32; 6]);

impl Affine {
    pub const ID: Affine = Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn translate(x: f32, y: f32) -> Affine {
        Affine([1.0, 0.0, 0.0, 1.0, x, y])
    }
    pub fn scale(x: f32, y: f32) -> Affine {
        Affine([x, 0.0, 0.0, y, 0.0, 0.0])
    }
    pub fn rotate(r: f32) -> Affine {
        let (s, c) = r.sin_cos();
        Affine([c, s, -s, c, 0.0, 0.0])
    }
    /// `self` after `o`: applies `o` first.
    pub fn then(&self, o: &Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = o.0;
        Affine([
            a * a2 + c * b2,
            b * a2 + d * b2,
            a * c2 + c * d2,
            b * c2 + d * d2,
            a * e2 + c * f2 + e,
            b * e2 + d * f2 + f,
        ])
    }
    pub fn apply(&self, x: f32, y: f32) -> [f32; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * x + c * y + e, b * x + d * y + f]
    }
    pub fn invert(&self) -> Option<Affine> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        if det.abs() < 1e-9 {
            return None;
        }
        let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
        Some(Affine([
            ia,
            ib,
            ic,
            id,
            -(ia * e + ic * f),
            -(ib * e + id * f),
        ]))
    }
    /// How much a unit along x grows.
    pub fn sx(&self) -> f32 {
        (self.0[0] * self.0[0] + self.0[1] * self.0[1]).sqrt()
    }
}
