//! The engine's objects (Resource, Sprite, Window, MessageSprite, ResourceManager, InputFilter,
//! threads, ...) and the member functions of the basic types.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use nexas::gfx::{Draw, Image, Place};

use crate::csx::Key;
use crate::engine::Engine;
use crate::skin::{Node, Skin, image_ref};
use crate::value::*;
use crate::vm::{Done, Thread};

pub type Obj = Rc<RefCell<Native>>;

/// A decoded picture and its hot spot.
pub struct Pic {
    pub img: Rc<Image>,
    pub hot: (i32, i32),
}

/// A skin item's look: its picture for each button state (normal, focus, pushed, pushed and
/// focused, disabled), or its text style.
#[derive(Default)]
pub struct Item {
    pub kind: String,
    pub w: i32,
    pub h: i32,
    pub states: [Option<(Rc<Pic>, Option<[i32; 4]>)>; 5],
    pub state: usize,
    /// the parameter of status notifications
    pub notify: Option<i32>,
    pub check: bool,
    pub checked: bool,
    pub style: Option<Node>,
}

/// A running BeginActivation.
pub struct Act {
    t0: f64,
    dur: f64,
    kind: i32,
    trans: Option<Vec<f64>>,
    curve: Option<Vec<(f64, f64)>>,
    rev: Option<Vec<f64>>,
    mag: Option<Vec<(f64, f64)>>,
}

/// Text written into a MessageSprite: the characters with the time each shows.
#[derive(Default)]
pub struct Msg {
    pub w: i32,
    pub h: i32,
    pub view: [i32; 4],
    pub chars: Vec<(char, f64, i32, [u8; 3])>,
    pub size: i32,
    pub line_h: i32,
    pub color: [u8; 3],
    pub shadow: Option<[u8; 3]>,
    pub speed: f64,
    pub fade: f64,
    pub ruby: bool,
    pub cache: Option<(usize, Rc<Image>)>,
}

#[derive(Default)]
pub struct Native {
    pub class: String,
    // Resource
    pub pic: Option<Rc<Pic>>,
    pub clip: Option<[i32; 4]>,
    pub sound: Option<Rc<Vec<u8>>>,
    pub sound_name: String,
    pub sound_ms: f64,
    pub channel: String,
    pub play_end: Option<f64>,
    pub volume: f64,
    // Sprite
    pub buffer: Option<(u32, u32, bool)>,
    pub back: Option<[u8; 4]>,
    pub visible: bool,
    pub enabled: bool,
    pub trans: f64,
    pub pos: (f64, f64),
    pub center: (f64, f64),
    pub scale: (f64, f64),
    pub angle: f64,
    pub flags: i32,
    pub children: Vec<(i32, Obj)>,
    pub parent: Option<Weak<RefCell<Native>>>,
    pub sprite_id: String,
    pub item: Option<Item>,
    pub text: String,
    pub text_pic: Option<(String, Rc<Image>)>,
    pub pend_trans: Option<Vec<f64>>,
    pub pend_curve: Option<Vec<(f64, f64)>>,
    pub pend_rev: Option<Vec<f64>>,
    pub pend_mag: Option<Vec<(f64, f64)>>,
    pub act: Option<Act>,
    pub skin: Option<Rc<Skin>>,
    pub msg: Option<Msg>,
    // ThreadEvent
    pub signaled: bool,
    // Thread
    pub thread: Option<usize>,
    // ToneFilter: the tone of each channel
    pub tone: Option<[i32; 4]>,
    /// the elements of an object without a skin (the joints of a ModelJoint, ...), by key
    pub elems: Vec<(String, Var)>,
}

impl Native {
    pub fn new(class: &str) -> Native {
        Native {
            class: class.to_string(),
            enabled: true,
            scale: (1.0, 1.0),
            volume: 1.0,
            ..Native::default()
        }
    }

    /// `a := b` between engine objects: filters copy their settings; the others are shared.
    pub fn copy_from(&mut self, o: &Native) -> bool {
        match self.class.as_str() {
            "ToneFilter" => {
                self.tone = o.tone;
                true
            }
            "InputFilter" => true,
            _ => false,
        }
    }
}

/// Sprite classes; the 3D, particle and movie ones only take part as plain sprites.
const SPRITES: [&str; 8] = [
    "Sprite",
    "Window",
    "MessageSprite",
    "SuperSprite",
    "RenderSprite",
    "ParticleSprite",
    "MovieSprite",
    "PolygonModel",
];

/// A 2D affine map: (x, y) → (a x + c y + tx, b x + d y + ty).
#[derive(Clone, Copy)]
pub struct Aff {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Aff {
    pub const ID: Aff = Aff {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    fn then(&self, o: &Aff) -> Aff {
        // self ∘ o
        Aff {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            tx: self.a * o.tx + self.c * o.ty + self.tx,
            ty: self.b * o.tx + self.d * o.ty + self.ty,
        }
    }

    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }

    fn plain(&self) -> bool {
        self.a == 1.0 && self.d == 1.0 && self.b == 0.0 && self.c == 0.0
    }
}

fn local(n: &Native) -> Aff {
    let (s, c) = n.angle.to_radians().sin_cos();
    let (sx, sy) = n.scale;
    let m = Aff {
        a: c * sx,
        b: s * sx,
        c: -s * sy,
        d: c * sy,
        tx: 0.0,
        ty: 0.0,
    };
    let (ox, oy) = m.apply(-n.center.0, -n.center.1);
    Aff {
        tx: n.pos.0 + ox,
        ty: n.pos.1 + oy,
        ..m
    }
}

/// A point of a Bézier curve of control points (cubic pieces when there are 3n+1 of them).
fn bezier(p: &[f64], t: f64) -> f64 {
    match p.len() {
        0 => 0.0,
        1 => p[0],
        2 => p[0] + (p[1] - p[0]) * t,
        n if n > 4 && (n - 1) % 3 == 0 => {
            let k = (n - 1) / 3;
            let i = ((t * k as f64) as usize).min(k - 1);
            bezier(&p[i * 3..i * 3 + 4], t * k as f64 - i as f64)
        }
        _ => {
            let mut v = p.to_vec();
            while v.len() > 1 {
                for i in 0..v.len() - 1 {
                    v[i] += (v[i + 1] - v[i]) * t;
                }
                v.pop();
            }
            v[0]
        }
    }
}

fn bezier2(p: &[(f64, f64)], t: f64) -> (f64, f64) {
    let xs: Vec<f64> = p.iter().map(|q| q.0).collect();
    let ys: Vec<f64> = p.iter().map(|q| q.1).collect();
    (bezier(&xs, t), bezier(&ys, t))
}

// ---- reading arguments

fn arg(args: &[Var], i: usize) -> Value {
    args.get(i)
        .map(|v| deref(v).borrow().clone())
        .unwrap_or_default()
}

fn ai(args: &[Var], i: usize, def: i32) -> i32 {
    match args.get(i) {
        Some(v) => deref(v).borrow().int(),
        None => def,
    }
}

fn ar(args: &[Var], i: usize, def: f64) -> f64 {
    match args.get(i) {
        Some(v) => deref(v).borrow().real(),
        None => def,
    }
}

fn astr(args: &[Var], i: usize) -> Str {
    match args.get(i) {
        Some(v) => deref(v).borrow().str(),
        None => "".into(),
    }
}

pub fn obj_of(v: &Var) -> Option<Obj> {
    match &*deref(v).borrow() {
        Value::Obj(o) => Some(o.clone()),
        _ => None,
    }
}

fn int(i: i32) -> Result<Done, String> {
    Ok(Done::Val(Value::Int(i)))
}

fn ok() -> Result<Done, String> {
    int(0)
}

/// The elements of an array (or of a structure based on one, such as Bezier2D).
fn elements(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a.iter().map(|x| deref(x).borrow().clone()).collect(),
        Value::Struct(..) => match v.member("parent") {
            Some(p) => elements(&deref(&p).borrow()),
            None => Vec::new(),
        },
        Value::Ref(Some(r)) => elements(&r.borrow()),
        _ => Vec::new(),
    }
}

fn xy(v: &Value) -> (f64, f64) {
    let m = |n: &str| v.member(n).map_or(0.0, |x| deref(&x).borrow().real());
    (m("x"), m("y"))
}

/// Sets a member of a structure (adds it when missing), keeping the member's type.
pub fn set_member(target: &Var, name: &str, v: Value) {
    let t = deref(target);
    let found = t.borrow().member(name);
    match found {
        Some(m) => assign(&m, &var(v)),
        None => {
            let mut b = t.borrow_mut();
            match &mut *b {
                Value::Struct(_, m) => m.push((name.into(), var(v))),
                Value::Hash(h, _) => hash_set(h, name.into(), var(v)),
                Value::Empty => {
                    *b = Value::Hash(vec![(name.into(), var(v))], None);
                }
                _ => {}
            }
        }
    }
}

fn get_member(v: &Var, name: &str) -> Value {
    deref(v)
        .borrow()
        .member(name)
        .map(|m| deref(&m).borrow().clone())
        .unwrap_or_default()
}

fn structure(class: &str, members: &[(&str, Value)]) -> Value {
    Value::Struct(
        class.into(),
        members
            .iter()
            .map(|(k, v)| (Str::from(*k), var(v.clone())))
            .collect(),
    )
}

fn point(x: i32, y: i32) -> Value {
    structure("Point", &[("x", Value::Int(x)), ("y", Value::Int(y))])
}

// ---- IsMatchUsage

enum Pat {
    Lit(String, bool),
    Num,
    Any,
    Set(Vec<(char, char)>, bool),
    Group(Vec<Pat>),
    Space,
    End,
}

fn parse_pat(p: &[char], i: &mut usize, close: Option<char>) -> Vec<Pat> {
    let mut out = Vec::new();
    while *i < p.len() {
        let c = p[*i];
        if Some(c) == close {
            *i += 1;
            return out;
        }
        *i += 1;
        match c {
            '&' => {
                let nocase = p.get(*i) == Some(&'+') || p.get(*i) == Some(&'!');
                if nocase {
                    *i += 1;
                }
                let mut s = String::new();
                while *i < p.len() && p[*i] != '&' {
                    if p[*i] == '\\' && *i + 1 < p.len() {
                        *i += 1;
                    }
                    s.push(p[*i]);
                    *i += 1;
                }
                *i += 1;
                out.push(Pat::Lit(s, nocase));
            }
            '%' => {
                let k = p.get(*i).copied().unwrap_or('s');
                *i += 1;
                out.push(if k == 'n' { Pat::Num } else { Pat::Any });
            }
            '<' => {
                let neg = p.get(*i) == Some(&'-');
                if neg {
                    *i += 1;
                }
                let mut set = Vec::new();
                while *i < p.len() && p[*i] != '>' {
                    let a = p[*i];
                    if p.get(*i + 1) == Some(&'-') && p.get(*i + 2).is_some_and(|c| *c != '>') {
                        set.push((a, p[*i + 2]));
                        *i += 3;
                    } else {
                        set.push((a, a));
                        *i += 1;
                    }
                }
                *i += 1;
                out.push(Pat::Set(set, neg));
            }
            '(' => out.push(Pat::Group(parse_pat(p, i, Some(')')))),
            ' ' => out.push(Pat::Space),
            '\\' => {
                if *i >= p.len() {
                    out.push(Pat::End);
                } else {
                    out.push(Pat::Lit(p[*i].to_string(), false));
                    *i += 1;
                }
            }
            c => out.push(Pat::Lit(c.to_string(), false)),
        }
    }
    out
}

/// Matches `pats` at `s[at..]`; calls `k` with the end of each possible match.
fn match_pat(
    pats: &[Pat],
    s: &[char],
    at: usize,
    caps: &mut Vec<String>,
    k: &mut dyn FnMut(usize, &mut Vec<String>) -> bool,
) -> bool {
    let Some((p, rest)) = pats.split_first() else {
        return k(at, caps);
    };
    match p {
        Pat::Lit(l, nocase) => {
            let l: Vec<char> = l.chars().collect();
            if at + l.len() > s.len() {
                return false;
            }
            let ok = s[at..at + l.len()].iter().zip(&l).all(|(a, b)| {
                if *nocase {
                    a.eq_ignore_ascii_case(b)
                } else {
                    a == b
                }
            });
            ok && match_pat(rest, s, at + l.len(), caps, k)
        }
        Pat::Num => {
            let mut e = at;
            if e < s.len() && (s[e] == '-' || s[e] == '+') {
                e += 1;
            }
            let d0 = e;
            while e < s.len() && s[e].is_ascii_digit() {
                e += 1;
            }
            e > d0 && match_pat(rest, s, e, caps, k)
        }
        Pat::Any => {
            for e in at..=s.len() {
                if match_pat(rest, s, e, caps, k) {
                    return true;
                }
            }
            false
        }
        Pat::Set(set, neg) => {
            at < s.len()
                && set.iter().any(|(a, b)| (*a..=*b).contains(&s[at])) != *neg
                && match_pat(rest, s, at + 1, caps, k)
        }
        Pat::Space => {
            let mut e = at;
            while e < s.len() && s[e].is_whitespace() {
                e += 1;
            }
            match_pat(rest, s, e, caps, k)
        }
        Pat::End => at == s.len() && match_pat(rest, s, at, caps, k),
        Pat::Group(g) => {
            let n = caps.len();
            caps.push(String::new());
            let r = match_pat(g, s, at, caps, &mut |e, caps: &mut Vec<String>| {
                caps[n] = s[at..e].iter().collect();
                match_pat(rest, s, e, caps, k)
            });
            if !r {
                caps.truncate(n);
            }
            r
        }
    }
}

pub fn is_match_usage(s: &str, usage: &str) -> Option<(Vec<String>, usize)> {
    let p: Vec<char> = usage.chars().collect();
    let pats = parse_pat(&p, &mut 0, None);
    let s: Vec<char> = s.chars().collect();
    let mut caps = Vec::new();
    let mut end = 0;
    let ok = match_pat(&pats, &s, 0, &mut caps, &mut |e, _| {
        end = e;
        true
    });
    ok.then_some((caps, end))
}

/// Formats an integer: radix (0: signed decimal), digits.
fn format_int(v: i32, radix: i32, col: i32) -> String {
    let r = radix.unsigned_abs().clamp(2, 36);
    let mut s = if radix == 0 {
        v.to_string()
    } else if radix < 0 {
        let mut t = String::new();
        let mut n = (v as i64).unsigned_abs();
        loop {
            t.insert(0, std::char::from_digit((n % r as u64) as u32, r).unwrap());
            n /= r as u64;
            if n == 0 {
                break;
            }
        }
        if v < 0 { format!("-{t}") } else { t }
    } else {
        let mut t = String::new();
        let mut n = v as u32 as u64;
        loop {
            t.insert(
                0,
                std::char::from_digit((n % r as u64) as u32, r)
                    .unwrap()
                    .to_ascii_uppercase(),
            );
            n /= r as u64;
            if n == 0 {
                break;
            }
        }
        t
    };
    if col > 0 {
        let col = col as usize;
        let neg = s.starts_with('-');
        let digits = if neg { s[1..].to_string() } else { s.clone() };
        let digits = if digits.len() > col {
            digits[digits.len() - col..].to_string()
        } else {
            format!("{}{digits}", "0".repeat(col - digits.len()))
        };
        s = if neg { format!("-{digits}") } else { digits };
    }
    s
}

impl Engine {
    pub fn new_native(&mut self, class: &str) -> Option<Value> {
        let known = [
            "Resource",
            "Sprite",
            "Window",
            "MessageSprite",
            "SuperSprite",
            "ResourceManager",
            "InputFilter",
            "ToneFilter",
            "File",
            "Thread",
            "ThreadEvent",
            "ThreadMutex",
            "RenderSprite",
            "ParticleSprite",
            "MovieSprite",
            "PolygonModel",
            "ParticleModel",
            "ModelJoint",
        ];
        if class == "FontArray" {
            return Some(Value::Array(Vec::new()));
        }
        known
            .contains(&class)
            .then(|| Value::Obj(Rc::new(RefCell::new(Native::new(class)))))
    }

    pub fn native_element(&mut self, obj: &Var, key: &Key) -> Result<Var, String> {
        let o = obj_of(obj).ok_or("not an object")?;
        let skin = o.borrow().skin.clone();
        if let (Some(skin), Key::Name(id)) = (skin, key) {
            let mut r = Native::new("Resource");
            if let Some(f) = skin.image_file(id) {
                r.pic = self.pic(&f);
            }
            return Ok(var(Value::Obj(Rc::new(RefCell::new(r)))));
        }
        // 3D models and the like are not drawn: their elements are plain variables
        let k = format!("{key:?}");
        if let Some((_, v)) = o.borrow().elems.iter().find(|(n, _)| *n == k) {
            return Ok(v.clone());
        }
        let v = var(Value::Int(0));
        o.borrow_mut().elems.push((k, v.clone()));
        Ok(v)
    }

    pub fn native_store_op(&mut self, d: &Var, o: u8, v: &Value) -> Result<(), String> {
        let _ = v;
        Err(format!("operator {o} on a {}", d.borrow().type_name()))
    }

    pub fn reference_method(&mut self, r: &Var, name: &str, args: &[Var]) -> Result<Done, String> {
        match name {
            "SetReference" => {
                let t = args.first().map(deref);
                *r.borrow_mut() = Value::Ref(t);
                ok()
            }
            "DetachReference" => {
                let t = match &*r.borrow() {
                    Value::Ref(Some(t)) => Some(t.clone()),
                    _ => None,
                };
                *r.borrow_mut() = Value::Ref(None);
                Ok(match t {
                    Some(t) => Done::Var(t),
                    None => Done::Val(Value::Int(0)),
                })
            }
            "IsIdentity" => {
                let same = match (&*r.borrow(), args.first()) {
                    (Value::Ref(Some(t)), Some(a)) => Rc::ptr_eq(&deref(t), &deref(a)),
                    (Value::Ref(None), None) => true,
                    _ => false,
                };
                int(if same { -1 } else { 0 })
            }
            "GetType" => {
                let n = match &*r.borrow() {
                    Value::Ref(Some(t)) => t.borrow().type_name(),
                    _ => String::new(),
                };
                Ok(Done::Val(s(&n)))
            }
            _ => {
                let v = deref(r).borrow().deep();
                Ok(Done::Val(v))
            }
        }
    }

    pub fn builtin_method(
        &mut self,
        target: &Var,
        name: &str,
        args: &[Var],
    ) -> Result<Done, String> {
        let v = target.borrow().clone();
        if name == "GetType" {
            return Ok(Done::Val(s(&v.type_name())));
        }
        if name == "Duplicate" {
            return Ok(Done::Val(v.deep()));
        }
        if name == "IsIdentity" {
            let same = args
                .first()
                .is_some_and(|a| Rc::ptr_eq(&deref(a), target) || same_obj(&deref(a).borrow(), &v));
            return int(if same { -1 } else { 0 });
        }
        match v {
            Value::Int(i) => self.int_method(target, i, name, args),
            Value::Real(r) => real_method(r, name, args),
            Value::Str(st) => self.str_method(target, &st, name, args),
            Value::Array(_) => array_method(target, name, args),
            Value::Hash(..) => hash_method(target, name, args),
            Value::Struct(..) => {
                // a structure based on an array (Bezier2D) or on another structure
                let parent = v.member("parent");
                match parent {
                    Some(p) => self.builtin_method(&deref(&p), name, args),
                    // a data table
                    None if name == "GetLength" => match &v {
                        Value::Struct(_, m) => int(m.len() as i32),
                        _ => ok(),
                    },
                    None => Err(format!("{}::{name}: no such function", v.type_name())),
                }
            }
            Value::Obj(o) => self.object_method(&o, name, args),
            Value::Empty => {
                // a member function on an empty element: treat it as an empty array
                *target.borrow_mut() = Value::Array(Vec::new());
                array_method(target, name, args)
            }
            other => Err(format!("{}::{name}: no such function", other.type_name())),
        }
    }

    fn int_method(&mut self, t: &Var, i: i32, name: &str, args: &[Var]) -> Result<Done, String> {
        let a = ai(args, 0, 0);
        let set = |v: i32| {
            *t.borrow_mut() = Value::Int(v);
            Ok(Done::Val(Value::Int(v)))
        };
        match name {
            "Format" => Ok(Done::Val(s(&format_int(i, a, ai(args, 1, 0))))),
            "Abs" => int(i.wrapping_abs()),
            "Char" => Ok(Done::Val(s(&char::from_u32(i as u32)
                .unwrap_or('?')
                .to_string()))),
            "TestBit" => int(if i & (1 << (a & 31)) != 0 { -1 } else { 0 }),
            "SetBit" => set(i | (1 << (a & 31))),
            "ResetBit" => set(i & !(1 << (a & 31))),
            "RotateLeft" => set(i.rotate_left(a as u32 & 31)),
            "RotateRight" => set(i.rotate_right(a as u32 & 31)),
            "ShiftLeft" => set(i.wrapping_shl(a as u32)),
            "ShiftRight" => set(((i as u32) >> (a as u32 & 31)) as i32),
            _ => Err(format!("Integer::{name}: no such function")),
        }
    }

    fn str_method(&mut self, t: &Var, st: &str, name: &str, args: &[Var]) -> Result<Done, String> {
        let chars: Vec<char> = st.chars().collect();
        let n = chars.len() as i32;
        let sub = |a: i32, b: i32| -> Value {
            let a = a.clamp(0, n) as usize;
            let b = b.clamp(0, n) as usize;
            Value::Str(chars[a..b.max(a)].iter().collect::<String>().into())
        };
        let set = |v: String| {
            *t.borrow_mut() = Value::Str(v.into());
            ok()
        };
        Ok(Done::Val(match name {
            "GetLength" => Value::Int(n),
            "Char" => Value::Int(chars.get(ai(args, 0, 0) as usize).map_or(0, |c| *c as i32)),
            "SetChar" => {
                let i = ai(args, 0, 0);
                if (0..n).contains(&i) {
                    let c = match arg(args, 1) {
                        Value::Str(x) => x.chars().next().unwrap_or(' '),
                        v => char::from_u32(v.int() as u32).unwrap_or(' '),
                    };
                    let mut c2 = chars.clone();
                    c2[i as usize] = c;
                    return set(c2.into_iter().collect());
                }
                Value::Int(0)
            }
            "Left" => sub(0, ai(args, 0, 0)),
            "Right" => sub(n - ai(args, 0, 0), n),
            "Middle" => {
                let i = ai(args, 0, 0);
                let c = ai(args, 1, -1);
                sub(i, if c < 0 { n } else { i + c })
            }
            "MakeUpper" => return set(st.to_uppercase()),
            "MakeLower" => return set(st.to_lowercase()),
            "TrimLeft" => return set(st.trim_start().to_string()),
            "TrimRight" => return set(st.trim_end().to_string()),
            "ParseFileDirectory" => {
                let i = st.rfind(['\\', '/']).map_or(0, |i| i + 1);
                s(&st[..i])
            }
            "ParseFileName" => s(st.rsplit(['\\', '/']).next().unwrap_or(st)),
            "ParseFileTitle" => {
                let f = st.rsplit(['\\', '/']).next().unwrap_or(st);
                s(f.rsplit_once('.').map_or(f, |(a, _)| a))
            }
            "ParseFileExtension" => {
                let f = st.rsplit(['\\', '/']).next().unwrap_or(st);
                s(f.rsplit_once('.').map_or("", |(_, b)| b))
            }
            "Find" => {
                let pat: Vec<char> = astr(args, 0).chars().collect();
                let from = ai(args, 1, 0).max(0) as usize;
                let mut r = -1;
                if pat.is_empty() {
                    r = from.min(chars.len()) as i32;
                } else {
                    for i in from..chars.len() {
                        if chars[i..].starts_with(&pat) {
                            r = i as i32;
                            break;
                        }
                    }
                }
                Value::Int(r)
            }
            "Replace" => {
                let old = astr(args, 0);
                if old.is_empty() {
                    s(st)
                } else {
                    s(&st.replace(&*old, &astr(args, 1)))
                }
            }
            "Separate" => {
                let sep = astr(args, 1);
                let parts: Vec<String> = if sep.is_empty() {
                    st.split_whitespace().map(String::from).collect()
                } else {
                    st.split(&*sep).map(String::from).collect()
                };
                if let Some(a) = args.first() {
                    let a = deref(a);
                    let mut b = a.borrow_mut();
                    if !matches!(*b, Value::Array(_)) {
                        *b = Value::Array(Vec::new());
                    }
                    if let Value::Array(arr) = &mut *b {
                        for p in parts {
                            arr.push(var(s(&p)));
                        }
                    }
                }
                Value::Int(0)
            }
            "IsMatchUsage" => match is_match_usage(st, &astr(args, 0)) {
                Some((caps, end)) => {
                    if let Some(a) = args.get(1) {
                        *deref(a).borrow_mut() =
                            Value::Array(caps.iter().map(|c| var(s(c))).collect());
                    }
                    if let Some(e) = args.get(2) {
                        assign(e, &var(Value::Int(end as i32)));
                    }
                    s("")
                }
                None => s("usage does not match"),
            },
            "Call" => {
                let f = self
                    .script_fn(st)
                    .ok_or_else(|| format!("String::Call: no function {st}"))?;
                let this = if st.contains("::") {
                    args.first().cloned()
                } else {
                    None
                };
                return Ok(Done::Script(f, this, args.to_vec()));
            }
            "Calculate" => {
                let t = st.trim();
                if let Ok(i) = t.parse::<i32>() {
                    Value::Int(i)
                } else if let Ok(r) = t.parse::<f64>() {
                    Value::Real(r)
                } else {
                    s(t.trim_matches('"'))
                }
            }
            _ => return Err(format!("String::{name}: no such function")),
        }))
    }

    pub fn system(&mut self, t: &mut Thread, name: &str, args: &[Var]) -> Result<Done, String> {
        let _ = t;
        match name {
            "GetCurrentTime" => {
                // a script that waits by reading the clock in a loop goes on next frame
                self.time_polls += 1;
                let now = Value::Int(self.now as i64 as i32);
                Ok(if self.time_polls > 100 {
                    Done::Yield(now)
                } else {
                    Done::Val(now)
                })
            }
            "GetLocalTime" => Ok(Done::Val(structure(
                "Time",
                &[
                    ("nYear", Value::Int(2010)),
                    ("nMonth", Value::Int(1)),
                    ("nDay", Value::Int(1)),
                    ("nWeek", Value::Int(5)),
                    ("nHour", Value::Int(12)),
                    ("nMinute", Value::Int((self.now / 60000.0) as i32 % 60)),
                    ("nSecond", Value::Int((self.now / 1000.0) as i32 % 60)),
                ],
            ))),
            "Sleep" => {
                if self.waited(ai(args, 0, 0) as f64) {
                    Ok(Done::Yield(Value::Int(0)))
                } else {
                    Ok(Done::Block)
                }
            }
            "GetSystemPerformance" => int(0),
            "SetSystemPerformance" | "AddModule" | "OpenToAddArchiveFile" | "UpdateWindow" => ok(),
            "Exit" => {
                self.ended = true;
                Ok(Done::Block)
            }
            "GetMemoryStatus" => Ok(Done::Val(structure(
                "MemoryStatus",
                &[
                    ("nTotalPhys", Value::Int(1 << 30)),
                    ("nAvailPhys", Value::Int(1 << 29)),
                    ("nTotalVirtual", Value::Int(1 << 30)),
                    ("nAvailVirtual", Value::Int(1 << 29)),
                ],
            ))),
            "IPoint" => Ok(Done::Val(point(ai(args, 0, 0), ai(args, 1, 0)))),
            "ISize" => Ok(Done::Val(structure(
                "Size",
                &[
                    ("w", Value::Int(ai(args, 0, 0))),
                    ("h", Value::Int(ai(args, 1, 0))),
                ],
            ))),
            "IRect" => Ok(Done::Val(structure(
                "Rect",
                &[
                    ("left", Value::Int(ai(args, 0, 0))),
                    ("top", Value::Int(ai(args, 1, 0))),
                    ("right", Value::Int(ai(args, 2, 0))),
                    ("bottom", Value::Int(ai(args, 3, 0))),
                ],
            ))),
            "fabs" => Ok(Done::Val(Value::Real(ar(args, 0, 0.0).abs()))),
            _ => {
                self.warn(format!("{name}: no such function (returns 0)"));
                ok()
            }
        }
    }

    // ---- engine objects

    fn object_method(&mut self, o: &Obj, name: &str, args: &[Var]) -> Result<Done, String> {
        let class = o.borrow().class.clone();
        let r = match class.as_str() {
            "Thread" => self.thread_method(o, name, args),
            "ThreadEvent" | "ThreadMutex" => self.event_method(o, name, args),
            "ResourceManager" => self.skin_method(o, name, args),
            "InputFilter" => self.input_method(name, args),
            "ToneFilter" => self.tone_method(o, name, args),
            "File" => self.file_method(o, name, args),
            _ => None,
        };
        if let Some(r) = r {
            return r;
        }
        if SPRITES.contains(&class.as_str()) {
            if class == "Window"
                && let Some(r) = self.window_method(o, name, args)
            {
                return r;
            }
            if class == "MessageSprite"
                && let Some(r) = self.message_method(o, name, args)
            {
                return r;
            }
            if let Some(r) = self.sprite_method(o, name, args) {
                return r;
            }
        }
        if let Some(r) = self.resource_method(o, name, args) {
            return r;
        }
        if name == "AddSubJoint" {
            // keeps the joint as the next element; the models are not drawn
            let mut b = o.borrow_mut();
            let i = b.elems.len();
            let j = args.first().map(deref).unwrap_or_else(|| {
                var(Value::Obj(Rc::new(RefCell::new(Native::new("ModelJoint")))))
            });
            b.elems.push((format!("{:?}", Key::Index(i as i32)), j));
            return int(i as i32);
        }
        self.warn(format!("{class}::{name}: no such function (returns 0)"));
        ok()
    }

    fn thread_method(&mut self, o: &Obj, name: &str, args: &[Var]) -> Option<Result<Done, String>> {
        Some(match name {
            "BeginThread" => {
                let f = astr(args, 0);
                match self.script_fn(&f) {
                    Some(fi) => {
                        let rest = args[1.min(args.len())..].to_vec();
                        let this = if f.contains("::") {
                            rest.first().cloned()
                        } else {
                            None
                        };
                        let id = self.spawn(fi, this, rest);
                        o.borrow_mut().thread = Some(id);
                        ok()
                    }
                    None => Err(format!("BeginThread: no function {f}")),
                }
            }
            "IsThreadRunning" => {
                let id = o.borrow().thread;
                int(if id.is_some_and(|i| self.thread_running(i)) {
                    -1
                } else {
                    0
                })
            }
            "GetThreadResult" | "WaitThread" | "EndThread" => {
                let id = o.borrow().thread;
                match id {
                    Some(i) if self.thread_running(i) => {
                        if self.current == i {
                            ok()
                        } else {
                            Ok(Done::Block)
                        }
                    }
                    Some(i) => {
                        let r = self
                            .threads
                            .get(i)
                            .and_then(|t| t.as_ref())
                            .and_then(|t| t.result.clone());
                        Ok(match r {
                            Some(v) => Done::Val(v.borrow().clone()),
                            None => Done::Val(Value::Int(0)),
                        })
                    }
                    None => ok(),
                }
            }
            _ => return None,
        })
    }

    fn event_method(&mut self, o: &Obj, name: &str, args: &[Var]) -> Option<Result<Done, String>> {
        Some(match name {
            "Create" | "Delete" | "Lock" | "Unlock" | "Release" => ok(),
            "Set" | "Pulse" => {
                o.borrow_mut().signaled = true;
                ok()
            }
            "Reset" => {
                o.borrow_mut().signaled = false;
                ok()
            }
            "Wait" => {
                let mutex = o.borrow().class == "ThreadMutex";
                if mutex || o.borrow().signaled {
                    ok()
                } else {
                    let ms = ai(args, 0, -1);
                    if ms >= 0 && self.waited(ms as f64) {
                        Ok(Done::Yield(Value::Int(4)))
                    } else {
                        Ok(Done::Block)
                    }
                }
            }
            "IsSignaled" => int(if o.borrow().signaled { -1 } else { 0 }),
            _ => return None,
        })
    }

    fn tone_method(&mut self, o: &Obj, name: &str, args: &[Var]) -> Option<Result<Done, String>> {
        Some(match name {
            "SetGeneralTone" => {
                o.borrow_mut().tone = Some([
                    ai(args, 0, 0),
                    ai(args, 2, 0),
                    ai(args, 4, 0),
                    ai(args, 6, 0),
                ]);
                ok()
            }
            "MorphingFilter" => {
                let a = args.first().and_then(obj_of).and_then(|f| f.borrow().tone);
                let b = args.get(1).and_then(obj_of).and_then(|f| f.borrow().tone);
                let d = ai(args, 2, 0);
                let a = a.unwrap_or([0; 4]);
                let b = b.unwrap_or([0; 4]);
                let mut t = [0; 4];
                for i in 0..4 {
                    t[i] = a[i] + (b[i] - a[i]) * d / 256;
                }
                o.borrow_mut().tone = Some(t);
                ok()
            }
            "LoadFilterFile" => ok(),
            _ => return None,
        })
    }

    fn file_method(&mut self, o: &Obj, name: &str, args: &[Var]) -> Option<Result<Done, String>> {
        let _ = o;
        Some(match name {
            "IsExisting" => {
                let n = astr(args, 0);
                int(if self.fs.host.read_save(&save_name(&n)).is_some() {
                    -1
                } else {
                    0
                })
            }
            "Open" => int(1),
            "Close" => ok(),
            // saving is not supported yet: every read fails
            "LoadObject" | "LoadContextTitle" | "ReadText" | "Read" => int(1),
            "SaveObject" | "SaveThumbnailImage" | "WriteText" | "Write" | "Rename" | "Delete" => {
                ok()
            }
            _ => return None,
        })
    }

    fn input_method(&mut self, name: &str, args: &[Var]) -> Option<Result<Done, String>> {
        Some(match name {
            "LoadInputFilter" | "OpenFilter" | "CloseFilter" | "DeleteInputFilter"
            | "FlushInputQueue" => ok(),
            "FlushJoyButtonPushed" => {
                let k = ai(args, 1, -1);
                if k < 0 {
                    self.joy_pushed = [0; 16];
                } else if (k as usize) < self.joy_pushed.len() {
                    self.joy_pushed[k as usize] = 0;
                }
                ok()
            }
            "GetJoyButtonPushed" => {
                let k = ai(args, 0, 0) as usize;
                int(self.joy_pushed.get(k).copied().unwrap_or(0))
            }
            "IsJoyButtonPushing" => int(0),
            "GetCursorPos" => Ok(Done::Val(point(self.mouse.0, self.mouse.1))),
            "MoveCursorPos" => {
                self.mouse = (ai(args, 0, 0), ai(args, 1, 0));
                ok()
            }
            "GetInputEvent" => {
                if self.waited(ai(args, 1, -1).max(0) as f64) {
                    Ok(Done::Yield(Value::Int(4)))
                } else {
                    Ok(Done::Block)
                }
            }
            "GetCapturedJoyStick" => int(0),
            _ => return None,
        })
    }

    fn resource_method(
        &mut self,
        o: &Obj,
        name: &str,
        args: &[Var],
    ) -> Option<Result<Done, String>> {
        Some(match name {
            "LoadImage" => {
                let f = astr(args, 0);
                let p = self.pic(&f);
                let found = p.is_some();
                {
                    let mut n = o.borrow_mut();
                    n.pic = p;
                    n.clip = None;
                    n.buffer = None;
                }
                int(if found { 0 } else { 1 })
            }
            "AttachImage" => {
                let src = args.first().and_then(obj_of);
                let p = src.and_then(|s| s.borrow().pic.clone());
                let clip = args.get(2).map(|r| {
                    let r = deref(r).borrow().clone();
                    let m = |k: &str| r.member(k).map_or(0, |v| deref(&v).borrow().int());
                    [m("left"), m("top"), m("right"), m("bottom")]
                });
                let mut n = o.borrow_mut();
                n.pic = p;
                n.clip = clip;
                ok()
            }
            "Release" => {
                let mut n = o.borrow_mut();
                n.pic = None;
                n.clip = None;
                n.sound = None;
                n.buffer = None;
                n.msg = None;
                n.text_pic = None;
                drop(n);
                if o.borrow().play_end.is_some() {
                    self.stop_sound(o);
                }
                ok()
            }
            "GetInfo" => {
                let n = o.borrow();
                if let Some(p) = &n.pic {
                    Ok(Done::Val(structure(
                        "ImageInfo",
                        &[
                            ("nFormatType", Value::Int(0x0400_0001)),
                            ("nImageWidth", Value::Int(p.img.w as i32)),
                            ("nImageHeight", Value::Int(p.img.h as i32)),
                            ("nBitsPerPixel", Value::Int(32)),
                            ("nFrameCount", Value::Int(1)),
                            ("xHotSpot", Value::Int(p.hot.0)),
                            ("yHotSpot", Value::Int(p.hot.1)),
                            ("nResourceBytes", Value::Int((p.img.w * p.img.h * 4) as i32)),
                        ],
                    )))
                } else if let Some((w, h, _)) = n.buffer {
                    Ok(Done::Val(structure(
                        "ImageInfo",
                        &[
                            ("nFormatType", Value::Int(0x0400_0001)),
                            ("nImageWidth", Value::Int(w as i32)),
                            ("nImageHeight", Value::Int(h as i32)),
                            ("nBitsPerPixel", Value::Int(32)),
                            ("nFrameCount", Value::Int(1)),
                            ("xHotSpot", Value::Int(0)),
                            ("yHotSpot", Value::Int(0)),
                            ("nResourceBytes", Value::Int(0)),
                        ],
                    )))
                } else if n.sound.is_some() {
                    let ms = n.sound_ms;
                    Ok(Done::Val(structure(
                        "SoundInfo",
                        &[
                            ("nSampleCount", Value::Int((ms * 44.1) as i32)),
                            ("nSamplesPerSec", Value::Int(44100)),
                            ("nChannelCount", Value::Int(2)),
                            ("nBitsPerSample", Value::Int(16)),
                            ("nRewoundPosition", Value::Int(-1)),
                            ("nResourceBytes", Value::Int(0)),
                        ],
                    )))
                } else if SPRITES.contains(&n.class.as_str()) {
                    // an empty sprite: an image of no size
                    Ok(Done::Val(structure(
                        "ImageInfo",
                        &[
                            ("nFormatType", Value::Int(0)),
                            ("nImageWidth", Value::Int(0)),
                            ("nImageHeight", Value::Int(0)),
                            ("nBitsPerPixel", Value::Int(0)),
                            ("nFrameCount", Value::Int(0)),
                            ("xHotSpot", Value::Int(0)),
                            ("yHotSpot", Value::Int(0)),
                            ("nResourceBytes", Value::Int(0)),
                        ],
                    )))
                } else {
                    Err("GetInfo: no picture or sound".into())
                }
            }
            "LoadSound" => {
                let f = astr(args, 0);
                let b = self.fs.read(&f);
                let found = b.is_some();
                let ms = b.as_deref().map_or(0.0, crate::mio::duration_ms);
                let mut n = o.borrow_mut();
                n.sound = b.map(Rc::new);
                n.sound_name = f.to_string();
                n.sound_ms = ms;
                int(if found { 0 } else { 1 })
            }
            "AttachSound" => {
                let src = args.first().and_then(obj_of);
                if let Some(src) = src {
                    let (s, nm, ms) = {
                        let b = src.borrow();
                        (b.sound.clone(), b.sound_name.clone(), b.sound_ms)
                    };
                    let mut n = o.borrow_mut();
                    n.sound = s;
                    n.sound_name = nm;
                    n.sound_ms = ms;
                }
                ok()
            }
            "Play" | "PlayFrom" => {
                let repeat = if name == "Play" {
                    ai(args, 0, -1) >= 0
                } else {
                    ai(args, 2, 0) != 0
                };
                self.play_sound(o, repeat);
                ok()
            }
            "Stop" => {
                self.stop_sound(o);
                ok()
            }
            "Pause" | "Restart" | "SetRewindingPortion" | "CancelVolumeEnvelope" | "LoadMidi" => {
                ok()
            }
            "IsPlaying" => {
                let e = o.borrow().play_end;
                int(if e.is_some_and(|e| self.now < e) {
                    -1
                } else {
                    0
                })
            }
            "GetPlayingPosition" => {
                let e = o.borrow().play_end;
                let ms = o.borrow().sound_ms;
                let pos = e.map_or(0.0, |e| (self.now - (e - ms)).max(0.0));
                int((pos * 44.1) as i32)
            }
            "SetVolume" => {
                let v = ar(args, 0, 1.0);
                o.borrow_mut().volume = v;
                ok()
            }
            "GetVolume" => {
                let v = o.borrow().volume;
                for a in args.iter().take(2) {
                    assign(a, &var(Value::Real(v)));
                }
                ok()
            }
            "SetVolumeEnvelope" => {
                let pts = elements(&arg(args, 0));
                if let Some(last) = pts.last() {
                    let v = xy(last).0;
                    if v <= 0.0 && o.borrow().play_end.is_some() {
                        self.stop_sound_fade(o, ai(args, 1, 0).max(0) as u32);
                    }
                    o.borrow_mut().volume = v;
                }
                ok()
            }
            "IsPendingEnvelope" => int(0),
            "GetTotalVolume" => Ok(Done::Val(Value::Real(1.0))),
            "SetTotalVolume" => ok(),
            _ => return None,
        })
    }

    fn play_sound(&mut self, o: &Obj, repeat: bool) {
        let (data, ms, vol) = {
            let n = o.borrow();
            (n.sound.clone(), n.sound_ms, n.volume)
        };
        let Some(data) = data else { return };
        if o.borrow().channel.is_empty() {
            self.next_channel += 1;
            o.borrow_mut().channel = format!("r{}", self.next_channel);
        }
        let end = if repeat { f64::MAX } else { self.now + ms };
        o.borrow_mut().play_end = Some(end);
        // MIO is not decoded yet: the sound only takes its time
        if data.starts_with(b"Entis") {
            return;
        }
        self.sounds.push(crate::engine::SoundEvent {
            channel: o.borrow().channel.clone(),
            data: Some(data),
            volume: vol as f32,
            repeat,
            fade_ms: 0,
        });
    }

    fn stop_sound(&mut self, o: &Obj) {
        self.stop_sound_fade(o, 0);
    }

    fn stop_sound_fade(&mut self, o: &Obj, fade_ms: u32) {
        let ch = o.borrow().channel.clone();
        o.borrow_mut().play_end = None;
        if !ch.is_empty() {
            self.sounds.push(crate::engine::SoundEvent {
                channel: ch,
                data: None,
                volume: 0.0,
                repeat: false,
                fade_ms,
            });
        }
    }

    fn skin_method(&mut self, o: &Obj, name: &str, args: &[Var]) -> Option<Result<Done, String>> {
        Some(match name {
            "LoadResource" => {
                let f = astr(args, 0);
                let lower = f.to_lowercase();
                let (prefix, inf) = if lower.ends_with(".noa") {
                    (format!("{lower}/"), format!("{lower}/system.inf"))
                } else {
                    (String::new(), f.to_string())
                };
                match self.fs.read(&inf) {
                    Some(b) => match crate::skin::parse_xml(&crate::skin::decode_text(&b)) {
                        Ok(root) => {
                            o.borrow_mut().skin = Some(Rc::new(Skin::parse(&prefix, &root)));
                            ok()
                        }
                        Err(e) => {
                            self.warn(format!("{inf}: {e}"));
                            int(1)
                        }
                    },
                    None => {
                        self.warn(format!("{inf}: not found"));
                        int(1)
                    }
                }
            }
            "DeleteContents" => {
                o.borrow_mut().skin = None;
                ok()
            }
            "CreateFormPage" => {
                let skin = o.borrow().skin.clone();
                let spr = args.first().and_then(obj_of);
                let id = astr(args, 1);
                match (skin, spr) {
                    (Some(skin), Some(spr)) => match skin.pages.get(&*id) {
                        Some(page) => {
                            self.build_page(&skin, &spr, page);
                            ok()
                        }
                        None => {
                            self.warn(format!("CreateFormPage: no page {id}"));
                            int(1)
                        }
                    },
                    _ => int(1),
                }
            }
            _ => return None,
        })
    }

    /// Builds a skin page on a sprite: its background and an item sprite for each item.
    fn build_page(&mut self, skin: &Rc<Skin>, spr: &Obj, page: &Node) {
        {
            let mut n = spr.borrow_mut();
            for (_, c) in n.children.drain(..) {
                c.borrow_mut().parent = None;
            }
            n.skin = Some(skin.clone());
            n.pos = (page.int("x", 0) as f64, page.int("y", 0) as f64);
            n.pic = None;
            n.clip = None;
        }
        if let Some(bg) = page.attr("bg").filter(|b| !b.is_empty()) {
            let p = skin.image_file(bg).and_then(|f| self.pic(&f));
            spr.borrow_mut().pic = p;
        }
        for (i, it) in page.kids.iter().enumerate() {
            if it.tag == "command" {
                continue;
            }
            let item = self.build_item(skin, it);
            item.borrow_mut().parent = Some(Rc::downgrade(spr));
            spr.borrow_mut().children.push((i as i32, item));
        }
    }

    fn build_item(&mut self, skin: &Rc<Skin>, it: &Node) -> Obj {
        let mut n = Native::new("Sprite");
        n.visible = true;
        n.sprite_id = it.attr("id").unwrap_or("").to_string();
        n.pos = (it.int("x", 0) as f64, it.int("y", 0) as f64);
        n.skin = Some(skin.clone());
        let style = it.attr("style").and_then(|s| skin.styles.get(s)).cloned();
        let mut item = Item {
            kind: it.tag.clone(),
            w: it.int("width", 0),
            h: it.int("height", 0),
            notify: it
                .kid("command")
                .and_then(|c| c.kid("status_notification"))
                .map(|s| s.int("parameter", 0)),
            ..Item::default()
        };
        if let Some(st) = &style {
            let kinds = ["normal", "focus", "pushed", "pushed_focus", "disabled"];
            for (k, tag) in kinds.iter().enumerate() {
                if let Some(src) = st.kid(tag).and_then(|s| s.attr("image")) {
                    let (id, rect) = image_ref(src);
                    if let Some(p) = skin.image_file(&id).and_then(|f| self.pic(&f)) {
                        item.states[k] = Some((p, rect));
                    }
                }
            }
            if let Some(a) = st.kid("arrange")
                && matches!(a.attr("type"), Some("check") | Some("radio"))
            {
                item.check = true;
            }
        }
        match it.tag.as_str() {
            "image" => {
                let src = it.attr("rsrc").or(it.attr("src")).unwrap_or("");
                let (id, rect) = image_ref(src);
                if let Some(p) = skin.image_file(&id).and_then(|f| self.pic(&f)) {
                    n.pic = Some(p);
                    n.clip = rect.map(|r| [r[0], r[1], r[0] + r[2], r[1] + r[3]]);
                }
            }
            "static_text" => {
                n.text = it.attr("text").unwrap_or("").to_string();
            }
            "object" | "page" | "window" => {
                let sub = it
                    .attr("page")
                    .or(it.attr("src"))
                    .and_then(|p| skin.pages.get(p))
                    .cloned();
                if let Some(sub) = sub {
                    let o = Rc::new(RefCell::new(n));
                    self.build_page(skin, &o, &sub);
                    let mut b = o.borrow_mut();
                    b.pos = (it.int("x", 0) as f64, it.int("y", 0) as f64);
                    b.visible = true;
                    drop(b);
                    return o;
                }
            }
            _ => {}
        }
        item.style = style;
        n.item = Some(item);
        Rc::new(RefCell::new(n))
    }

    /// The item sprite at a path of ids ("ID_SAVE_1\ID_DATE") under a sprite.
    pub fn find_item(&self, o: &Obj, path: &str) -> Option<Obj> {
        if path.is_empty() {
            return Some(o.clone());
        }
        let (first, rest) = match path.split_once('\\') {
            Some((a, b)) => (a, b),
            None => (path, ""),
        };
        fn find(o: &Obj, id: &str, depth: u32) -> Option<Obj> {
            for (_, c) in &o.borrow().children {
                if c.borrow().sprite_id == id {
                    return Some(c.clone());
                }
            }
            if depth > 8 {
                return None;
            }
            for (_, c) in &o.borrow().children {
                if let Some(f) = find(c, id, depth + 1) {
                    return Some(f);
                }
            }
            None
        }
        let f = find(o, first, 0)?;
        self.find_item(&f, rest)
    }

    fn target(&mut self, o: &Obj, args: &[Var], i: usize) -> Option<Obj> {
        let id = astr(args, i);
        let r = self.find_item(o, &id);
        if r.is_none() && self.log > 0 {
            self.warn(format!("no skin item {id}"));
        }
        r
    }

    fn sprite_method(&mut self, o: &Obj, name: &str, args: &[Var]) -> Option<Result<Done, String>> {
        Some(match name {
            "CreateSprite" | "CreateImage" => {
                let fmt = ai(args, 0, 1);
                let (w, h) = (ai(args, 1, 0).max(0) as u32, ai(args, 2, 0).max(0) as u32);
                let mut n = o.borrow_mut();
                n.pic = None;
                n.clip = None;
                n.buffer = Some((w, h, fmt & 0x0400_0000 == 0 && fmt & 0xff == 1));
                n.back = None;
                ok()
            }
            "SetAlphaImage" => {
                let src = args
                    .first()
                    .and_then(obj_of)
                    .and_then(|s| s.borrow().pic.clone());
                if let Some(p) = src {
                    o.borrow_mut().buffer = Some((p.img.w, p.img.h, false));
                }
                ok()
            }
            "SetBackColor" => {
                let c = ai(args, 0, 0) as u32;
                let en = ai(args, 1, 1) != 0;
                let rgba = o.borrow().buffer.is_some_and(|b| !b.2);
                let a = if rgba { (c >> 24) as u8 } else { 255 };
                o.borrow_mut().back = en.then_some([(c >> 16) as u8, (c >> 8) as u8, c as u8, a]);
                ok()
            }
            "AddSprite" => {
                let pri = ai(args, 0, 0);
                if let Some(c) = args.get(1).and_then(obj_of) {
                    detach(&c);
                    c.borrow_mut().parent = Some(Rc::downgrade(o));
                    o.borrow_mut().children.push((pri, c));
                }
                ok()
            }
            "DetachSprite" => {
                if let Some(c) = args.first().and_then(obj_of) {
                    let mine = c
                        .borrow()
                        .parent
                        .as_ref()
                        .and_then(|p| p.upgrade())
                        .is_some_and(|p| Rc::ptr_eq(&p, o));
                    if mine {
                        detach(&c);
                    }
                }
                ok()
            }
            "DetachAllSprite" => {
                let kids: Vec<Obj> = o.borrow_mut().children.drain(..).map(|c| c.1).collect();
                for c in kids {
                    c.borrow_mut().parent = None;
                }
                ok()
            }
            "GetParent" => {
                let p = o.borrow().parent.as_ref().and_then(|p| p.upgrade());
                Ok(Done::Val(Value::Ref(p.map(|p| var(Value::Obj(p))))))
            }
            "SetVisible" => {
                o.borrow_mut().visible = ai(args, 0, -1) != 0;
                ok()
            }
            "IsVisible" => int(if o.borrow().visible { -1 } else { 0 }),
            "MovePosition" => {
                o.borrow_mut().pos = (ar(args, 0, 0.0), ar(args, 1, 0.0));
                ok()
            }
            "GetPosition" => {
                let p = o.borrow().pos;
                Ok(Done::Val(point(p.0 as i32, p.1 as i32)))
            }
            "GetRectangle" => {
                let t = if args.is_empty() {
                    Some(o.clone())
                } else {
                    self.target(o, args, 0)
                };
                // in the coordinates of this sprite's parent
                let (x, y, w, h) = t.map_or((0.0, 0.0, 0, 0), |t| {
                    let (w, h) = size_of(&t.borrow());
                    let (mut x, mut y) = (0.0, 0.0);
                    let mut cur = Some(t.clone());
                    while let Some(c) = cur {
                        let n = c.borrow();
                        x += n.pos.0;
                        y += n.pos.1;
                        if Rc::ptr_eq(&c, o) {
                            break;
                        }
                        cur = n.parent.as_ref().and_then(|p| p.upgrade());
                    }
                    (x, y, w, h)
                });
                Ok(Done::Val(structure(
                    "Rect",
                    &[
                        ("left", Value::Int(x as i32)),
                        ("top", Value::Int(y as i32)),
                        ("right", Value::Int(x as i32 + w)),
                        ("bottom", Value::Int(y as i32 + h)),
                    ],
                )))
            }
            "GetTransparency" | "GetBlendDegree" => {
                let t = if args.is_empty() {
                    Some(o.clone())
                } else {
                    self.target(o, args, 0)
                };
                int(t.map_or(0, |t| t.borrow().trans as i32))
            }
            "SetTransparency" | "SetBlendDegree" => {
                let t = if args.len() < 2 {
                    Some(o.clone())
                } else {
                    self.target(o, args, 1)
                };
                if let Some(t) = t {
                    t.borrow_mut().trans = ar(args, 0, 0.0).clamp(0.0, 256.0);
                }
                ok()
            }
            "GetParameter" => {
                if let Some(p) = args.first() {
                    let n = o.borrow();
                    let pt = |x: f64, y: f64| point(x.round() as i32, y.round() as i32);
                    set_member(p, "nFlags", Value::Int(n.flags));
                    set_member(p, "ptDstPos", pt(n.pos.0, n.pos.1));
                    set_member(p, "ptRevCenter", pt(n.center.0, n.center.1));
                    set_member(p, "rHorzUnit", Value::Real(n.scale.0));
                    set_member(p, "rVertUnit", Value::Real(n.scale.1));
                    set_member(p, "rRevAngle", Value::Real(n.angle));
                    set_member(p, "nTransparency", Value::Int(n.trans as i32));
                }
                ok()
            }
            "SetParameter" => {
                if let Some(p) = args.first() {
                    let g = |k: &str| get_member(p, k);
                    let mut n = o.borrow_mut();
                    n.flags = g("nFlags").int();
                    let d = g("ptDstPos");
                    n.pos = xy(&d);
                    n.center = xy(&g("ptRevCenter"));
                    n.scale = (g("rHorzUnit").real(), g("rVertUnit").real());
                    n.angle = g("rRevAngle").real();
                    n.trans = (g("nTransparency").int() as f64).clamp(0.0, 256.0);
                }
                ok()
            }
            "GetPriority" => {
                let id = astr(args, 0);
                if id.is_empty() {
                    let p = o.borrow().parent.as_ref().and_then(|p| p.upgrade());
                    let pri = p.and_then(|p| {
                        p.borrow()
                            .children
                            .iter()
                            .find(|c| Rc::ptr_eq(&c.1, o))
                            .map(|c| c.0)
                    });
                    int(pri.unwrap_or(0))
                } else {
                    let t = self.target(o, args, 0);
                    let pri = t.and_then(|t| {
                        let p = t.borrow().parent.as_ref().and_then(|p| p.upgrade())?;
                        let p = p.borrow();
                        p.children
                            .iter()
                            .find(|c| Rc::ptr_eq(&c.1, &t))
                            .map(|c| c.0)
                    });
                    int(pri.unwrap_or(0))
                }
            }
            "ChangePriority" => {
                let pri = ai(args, 0, 0);
                let t = if args.len() < 2 {
                    Some(o.clone())
                } else {
                    self.target(o, args, 1)
                };
                if let Some(t) = t {
                    let p = t.borrow().parent.as_ref().and_then(|p| p.upgrade());
                    if let Some(p) = p {
                        for c in p.borrow_mut().children.iter_mut() {
                            if Rc::ptr_eq(&c.1, &t) {
                                c.0 = pri;
                            }
                        }
                    }
                }
                ok()
            }
            "Enable" => {
                let en = ai(args, 0, -1) != 0;
                let t = if args.len() < 2 {
                    Some(o.clone())
                } else {
                    self.target(o, args, 1)
                };
                if let Some(t) = t {
                    t.borrow_mut().enabled = en;
                }
                ok()
            }
            "IsEnabled" => {
                let t = if args.is_empty() {
                    Some(o.clone())
                } else {
                    self.target(o, args, 0)
                };
                int(if t.is_some_and(|t| t.borrow().enabled) {
                    -1
                } else {
                    0
                })
            }
            "GetSpriteID" => Ok(Done::Val(s(&o.borrow().sprite_id))),
            "SetSpriteID" => {
                o.borrow_mut().sprite_id = astr(args, 0).to_string();
                ok()
            }
            "GetSpriteText" => {
                let t = self.target(o, args, 0);
                Ok(Done::Val(s(
                    &t.map_or(String::new(), |t| t.borrow().text.clone())
                )))
            }
            "SetSpriteText" => {
                if let Some(t) = self.target(o, args, 0) {
                    t.borrow_mut().text = astr(args, 1).to_string();
                }
                ok()
            }
            "SetSpriteImage" => {
                if let Some(t) = self.target(o, args, 0) {
                    let skin = t.borrow().skin.clone();
                    let (id, rect) = image_ref(&astr(args, 1));
                    let p = skin
                        .and_then(|s| s.image_file(&id))
                        .and_then(|f| self.pic(&f));
                    let mut n = t.borrow_mut();
                    n.pic = p;
                    n.clip = rect.map(|r| [r[0], r[1], r[0] + r[2], r[1] + r[3]]);
                }
                ok()
            }
            "SetSpriteFontFace"
            | "UpdateRect"
            | "Refresh"
            | "EnableDynamicMode"
            | "SetDrawFunctionFlags"
            | "AttachToneFilter"
            | "CreateZBuffer"
            | "DeleteZBuffer"
            | "ModifyAnimationFlags"
            | "SetEffectParameter"
            | "SetFocus"
            | "MoveFocus" => ok(),
            "BeginAnimation" | "EndAnimation" => ok(),
            "IsDuringAnimation" => int(0),
            "IsButtonChecked" => {
                let t = self.target(o, args, 0);
                let c = t.is_some_and(|t| t.borrow().item.as_ref().is_some_and(|i| i.checked));
                int(if c { -1 } else { 0 })
            }
            "CheckButton" => {
                if let Some(t) = self.target(o, args, 0) {
                    let on = ai(args, 1, -1) != 0;
                    if let Some(i) = t.borrow_mut().item.as_mut() {
                        i.checked = on;
                    }
                }
                ok()
            }
            "SendCommand" => int(-1),
            "GetVertScrollPos" | "GetHorzScrollPos" | "GetVertScrollRange"
            | "GetHorzScrollRange" => int(0),
            "SetVertScrollPos" | "SetHorzScrollPos" | "SetVertScrollRange"
            | "SetHorzScrollRange" => ok(),
            "GetFocus" => {
                let f = self.focus.clone();
                Ok(Done::Val(s(
                    &f.map_or(String::new(), |f| f.borrow().sprite_id.clone())
                )))
            }
            "IsHitSprite" | "IsMoviePlaying" => int(0),
            "GetSpriteAtPoint" => Ok(Done::Val(s(""))),
            "SetBlendingEnvelope" => {
                let v = arg(args, 0);
                let pts = match &v {
                    Value::Array(_) | Value::Struct(..) => {
                        elements(&v).iter().map(|x| x.real() * 256.0).collect()
                    }
                    _ => vec![f64::NAN, v.real()],
                };
                o.borrow_mut().pend_trans = Some(pts);
                ok()
            }
            "SetBezierCurve" => {
                let pts = |v: Value| -> Option<Vec<(f64, f64)>> {
                    let e = elements(&v);
                    (!e.is_empty()).then(|| e.iter().map(xy).collect())
                };
                let curve = pts(arg(args, 0));
                let rev = args
                    .get(1)
                    .map(|_| elements(&arg(args, 1)).iter().map(|x| x.real()).collect());
                let mag = args.get(2).and_then(|_| pts(arg(args, 2)));
                let mut n = o.borrow_mut();
                n.pend_curve = curve;
                n.pend_rev = rev.filter(|r: &Vec<f64>| !r.is_empty());
                n.pend_mag = mag;
                ok()
            }
            "BeginActivation" => {
                let d = arg(args, 0);
                let dur = match &d {
                    Value::Array(_) => elements(&d).iter().map(|x| x.real()).sum(),
                    _ => d.real(),
                };
                let kind = ai(args, 1, 0);
                {
                    let mut n = o.borrow_mut();
                    let mut trans = n.pend_trans.take();
                    if let Some(t) = trans.as_mut()
                        && t.first().is_some_and(|f| f.is_nan())
                    {
                        t[0] = n.trans;
                    }
                    n.act = Some(Act {
                        t0: self.now,
                        dur: dur.max(1.0),
                        kind,
                        trans,
                        curve: n.pend_curve.take(),
                        rev: n.pend_rev.take(),
                        mag: n.pend_mag.take(),
                    });
                }
                self.active.push(Rc::downgrade(o));
                ok()
            }
            "IsActivation" => int(if o.borrow().act.is_some() { -1 } else { 0 }),
            "FlushActivation" => {
                finish_act(&mut o.borrow_mut(), true);
                ok()
            }
            "CancelActivation" => {
                finish_act(&mut o.borrow_mut(), false);
                ok()
            }
            "DrawImage" | "FillRect" | "DrawText" => ok(),
            _ => return None,
        })
    }

    fn window_method(&mut self, o: &Obj, name: &str, args: &[Var]) -> Option<Result<Done, String>> {
        Some(match name {
            "CreateDisplay" => {
                let mut n = o.borrow_mut();
                n.visible = true;
                if args.len() >= 4 {
                    self.width = ai(args, 2, 800).max(1) as u32;
                    self.height = ai(args, 3, 600).max(1) as u32;
                }
                n.buffer = Some((self.width, self.height, true));
                drop(n);
                self.screen = Some(o.clone());
                ok()
            }
            "CloseDisplay" => ok(),
            "GetOptionalFuncFlag" => int(4),
            "SetOptionalFuncFlag"
            | "ChangeCooperationLevel"
            | "ChangeDisplaySize"
            | "SetChangeDisplayModeFlag"
            | "UpdateWindow"
            | "ProcessUserInput"
            | "AsyncTimePaint"
            | "ShowCursor" => ok(),
            "SyncTimePaint" => Ok(Done::Yield(Value::Int(0))),
            "Lock" | "Unlock" => ok(),
            "GetDisplaySize" => Ok(Done::Val(structure(
                "Size",
                &[
                    ("w", Value::Int(self.width as i32)),
                    ("h", Value::Int(self.height as i32)),
                ],
            ))),
            "IsWindowActive" | "IsShowCursor" => int(-1),
            "EnableCommandQueue" => {
                self.queue_commands = ai(args, 0, -1) != 0;
                ok()
            }
            "FlushCommandQueue" => {
                self.commands.clear();
                self.queue_commands = ai(args, 0, -1) != 0;
                ok()
            }
            "QueueCommand" => {
                let id = astr(args, 0).to_string();
                self.commands
                    .push_back((id.clone(), id, ai(args, 1, 0), ai(args, 2, 0)));
                ok()
            }
            "GetCommand" => {
                let remove = ai(args, 2, -1) != 0;
                if let Some(c) = self.commands.front().cloned() {
                    if remove {
                        self.commands.pop_front();
                    }
                    if let Some(w) = args.first() {
                        set_member(w, "strID", s(&c.0));
                        set_member(w, "strFullID", s(&c.1));
                        set_member(w, "nNotification", Value::Int(c.2));
                        set_member(w, "nParameter", Value::Int(c.3));
                    }
                    ok()
                } else {
                    let ms = ai(args, 1, -1);
                    if ms >= 0 && self.waited(ms as f64) {
                        Ok(Done::Yield(Value::Int(4)))
                    } else {
                        Ok(Done::Block)
                    }
                }
            }
            "CallMouseMove" => {
                self.update_focus();
                ok()
            }
            _ => return None,
        })
    }

    fn message_method(
        &mut self,
        o: &Obj,
        name: &str,
        args: &[Var],
    ) -> Option<Result<Done, String>> {
        Some(match name {
            "CreateMessage" => {
                let (w, h) = (ai(args, 0, 0), ai(args, 1, 0));
                let view = match args.get(2) {
                    Some(r) => {
                        let r = deref(r).borrow().clone();
                        let m = |k: &str| r.member(k).map_or(0, |v| deref(&v).borrow().int());
                        [m("left"), m("top"), m("right"), m("bottom")]
                    }
                    None => [0, 0, w, h],
                };
                let mut n = o.borrow_mut();
                n.buffer = Some((w.max(0) as u32, h.max(0) as u32, false));
                n.msg = Some(Msg {
                    w,
                    h,
                    view,
                    size: 24,
                    line_h: 30,
                    color: [255, 255, 255],
                    shadow: Some([0, 0, 0]),
                    speed: 40.0,
                    fade: 0.0,
                    ..Msg::default()
                });
                ok()
            }
            "AttachMessageStyle" => {
                let skin = args
                    .first()
                    .and_then(obj_of)
                    .and_then(|r| r.borrow().skin.clone());
                let st = skin.and_then(|s| s.styles.get(&*astr(args, 1)).cloned());
                if let (Some(st), Some(m)) = (st, o.borrow_mut().msg.as_mut()) {
                    apply_style(m, &st);
                }
                ok()
            }
            "SetFontStyle" => ok(),
            "SetDefaultMsgSpeed" => {
                if let Some(m) = o.borrow_mut().msg.as_mut() {
                    m.speed = ar(args, 0, 40.0);
                    m.fade = ar(args, 1, 0.0);
                }
                ok()
            }
            "SetMessageEffect" | "SetShadowTransparency" | "SetFontBordering" => ok(),
            "OutputMessage" => {
                let text = astr(args, 0);
                let now = self.now;
                let fonts = &mut self.fonts;
                let mut n = o.borrow_mut();
                let count = text.chars().count() as i32;
                if let Some(m) = n.msg.as_mut() {
                    layout_message(fonts, m, &text, now);
                }
                int(count)
            }
            "FlushMessage" => {
                if let Some(m) = o.borrow_mut().msg.as_mut() {
                    for c in m.chars.iter_mut() {
                        c.1 = c.1.min(self.now);
                    }
                }
                ok()
            }
            "ClearMessage" => {
                if let Some(m) = o.borrow_mut().msg.as_mut() {
                    m.chars.clear();
                    m.cache = None;
                }
                ok()
            }
            "IsMessagePending" => {
                let p = o
                    .borrow()
                    .msg
                    .as_ref()
                    .is_some_and(|m| m.chars.last().is_some_and(|c| c.1 > self.now));
                int(if p { -1 } else { 0 })
            }
            "GetCharacterCount" => int(o.borrow().msg.as_ref().map_or(0, |m| m.chars.len() as i32)),
            "GetCursorPos" => {
                let p = o.borrow().msg.as_ref().map_or((0, 0), message_cursor);
                Ok(Done::Val(point(p.0, p.1)))
            }
            "MoveCursorPos" => ok(),
            _ => return None,
        })
    }

    // ---- animation, input and drawing

    pub fn animate(&mut self) {
        let now = self.now;
        self.active.retain(|w| {
            let Some(o) = w.upgrade() else { return false };
            let mut n = o.borrow_mut();
            let Some(a) = &n.act else { return false };
            let mut t = (now - a.t0) / a.dur;
            let done = a.kind == 0 && t >= 1.0;
            t = match a.kind {
                1 => t.fract(),
                2 => {
                    let f = t % 2.0;
                    if f > 1.0 { 2.0 - f } else { f }
                }
                _ => t.min(1.0),
            };
            apply_act(&mut n, t);
            if done {
                n.act = None;
            }
            !done
        });
    }

    /// The enabled, visible button under the pointer, front first.
    fn hit(&self, o: &Obj, x: f64, y: f64, aff: Aff) -> Option<Obj> {
        let n = o.borrow();
        if !n.visible || !n.enabled {
            return None;
        }
        let m = aff.then(&local(&n));
        let mut kids: Vec<(i32, usize, Obj)> = n
            .children
            .iter()
            .enumerate()
            .map(|(i, c)| (c.0, i, c.1.clone()))
            .collect();
        kids.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        for (_, _, c) in kids {
            if let Some(h) = self.hit(&c, x, y, m) {
                return Some(h);
            }
        }
        if let Some(it) = &n.item
            && it.kind == "button"
        {
            let (w, h) = size_of(&n);
            // the inverse of a plain move is enough for skin items
            let (lx, ly) = (x - m.tx, y - m.ty);
            if m.plain() && lx >= 0.0 && ly >= 0.0 && lx < w as f64 && ly < h as f64 {
                return Some(o.clone());
            }
        }
        None
    }

    fn button_at(&self) -> Option<Obj> {
        let s = self.screen.clone()?;
        self.hit(&s, self.mouse.0 as f64, self.mouse.1 as f64, Aff::ID)
    }

    fn full_id(o: &Obj) -> String {
        let mut parts = vec![o.borrow().sprite_id.clone()];
        let mut cur = o.borrow().parent.as_ref().and_then(|p| p.upgrade());
        while let Some(p) = cur {
            let id = p.borrow().sprite_id.clone();
            if !id.is_empty() {
                parts.insert(0, id);
            }
            cur = p.borrow().parent.as_ref().and_then(|p| p.upgrade());
        }
        parts.join("\\")
    }

    fn command(&mut self, o: &Obj, notification: i32, parameter: i32) {
        let id = o.borrow().sprite_id.clone();
        let full = Engine::full_id(o);
        if self.log > 0 {
            eprintln!("entis: command {full} {notification} {parameter}");
        }
        self.commands.push_back((id, full, notification, parameter));
    }

    pub fn update_focus(&mut self) {
        let now = self.button_at();
        let same = match (&now, &self.focus) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        if let Some(old) = self.focus.take() {
            let notify = old.borrow_mut().item.as_mut().and_then(|i| {
                i.state = 0;
                i.notify
            });
            if let Some(p) = notify {
                self.command(&old, 0, p);
            }
        }
        if let Some(new) = &now {
            let notify = new.borrow_mut().item.as_mut().and_then(|i| {
                i.state = 1;
                i.notify
            });
            if let Some(p) = notify {
                self.command(new, 1, p);
            }
        }
        self.focus = now;
    }

    /// A click on the focused button; false when there is none.
    pub fn press_focus(&mut self) -> bool {
        let Some(f) = self.focus.clone() else {
            return false;
        };
        if let Some(i) = f.borrow_mut().item.as_mut()
            && i.check
        {
            i.checked = !i.checked;
        }
        self.command(&f, 0, 0);
        true
    }

    pub fn draw_sprite(&mut self, o: &Obj, aff: Aff, alpha: f64, out: &mut Vec<Draw>) {
        let (m, a, kids) = {
            let n = o.borrow();
            if !n.visible {
                return;
            }
            let a = alpha * (1.0 - n.trans / 256.0);
            if a <= 0.0 {
                return;
            }
            let m = aff.then(&local(&n));
            let mut kids: Vec<(i32, usize, Obj)> = n
                .children
                .iter()
                .enumerate()
                .map(|(i, c)| (c.0, i, c.1.clone()))
                .collect();
            // a smaller priority is in front: draw the larger first
            kids.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)));
            (m, a, kids)
        };
        let (w, h) = (self.width as f32, self.height as f32);
        let place = |m: &Aff, alpha: f64| Place {
            x: 0.0,
            y: 0.0,
            sx: (m.a * m.a + m.b * m.b).sqrt() as f32,
            sy: (m.c * m.c + m.d * m.d).sqrt() as f32,
            cx: m.tx as f32,
            cy: m.ty as f32,
            rot: m.b.atan2(m.a) as f32,
            alpha: alpha as f32,
            add: false,
            flip: (false, false),
            clip: [0.0, 0.0, w, h],
        };
        // the background of a picture buffer
        let back = {
            let n = o.borrow();
            match n.buffer {
                Some((bw, bh, opaque)) if bw > 0 && bh > 0 => {
                    let c = n.back.or(opaque.then_some([0, 0, 0, 255]));
                    c.map(|c| (bw, bh, c))
                }
                _ => None,
            }
        };
        if let Some((bw, bh, c)) = back {
            let key = u32::from_be_bytes(c);
            let img = self.solid(key);
            let (sw, sh) = (bw as f64, bh as f64);
            out.push(Draw {
                image: img,
                src: [0, 0, 1, 1],
                dst: [
                    m.tx as f32,
                    m.ty as f32,
                    (m.tx + sw) as f32,
                    (m.ty + sh) as f32,
                ],
                place: place(&m, a),
            });
        }
        // a picture's hot spot is the sprite's position
        let hot = {
            let n = o.borrow();
            match (&n.item, &n.msg, &n.pic) {
                (None, None, Some(p)) => (p.hot.0 as f64, p.hot.1 as f64),
                _ => (0.0, 0.0),
            }
        };
        let own = self.own_picture(o);
        if let Some((img, src)) = own {
            let (dw, dh) = ((src[2] - src[0]) as f64, (src[3] - src[1]) as f64);
            let (x0, y0) = (m.tx - hot.0, m.ty - hot.1);
            out.push(Draw {
                image: img,
                src,
                dst: [x0 as f32, y0 as f32, (x0 + dw) as f32, (y0 + dh) as f32],
                place: place(&m, a),
            });
        }
        for (_, _, c) in kids {
            self.draw_sprite(&c, m, a, out);
        }
    }

    fn solid(&mut self, rgba: u32) -> Rc<Image> {
        let key = format!("\u{1}solid{rgba:08x}");
        if let Some(Some(p)) = self.pics.get(&key) {
            return p.img.clone();
        }
        let mut img = Image::new(1, 1);
        img.fill(rgba.to_be_bytes());
        let img = Rc::new(img);
        self.pics.insert(
            key,
            Some(Rc::new(Pic {
                img: img.clone(),
                hot: (0, 0),
            })),
        );
        img
    }

    /// What a sprite shows itself: its picture, its button state, its text or its message.
    fn own_picture(&mut self, o: &Obj) -> Option<(Rc<Image>, [i32; 4])> {
        let mut n = o.borrow_mut();
        if let Some(it) = &n.item {
            if it.kind == "button" {
                let st = if !n.enabled {
                    4
                } else if it.checked {
                    if it.state == 1 { 3 } else { 2 }
                } else {
                    it.state
                };
                let pick = it.states[st]
                    .as_ref()
                    .or(it.states[if st == 3 { 2 } else { 0 }].as_ref())
                    .or(it.states[0].as_ref());
                if let Some((p, r)) = pick {
                    let src = r.map_or([0, 0, p.img.w as i32, p.img.h as i32], |r| {
                        [r[0], r[1], r[0] + r[2], r[1] + r[3]]
                    });
                    return Some((p.img.clone(), src));
                }
                return None;
            }
            if it.kind == "static_text" && n.pic.is_none() {
                if n.text.is_empty() {
                    return None;
                }
                if let Some((t, img)) = &n.text_pic
                    && *t == n.text
                {
                    return Some((img.clone(), [0, 0, img.w as i32, img.h as i32]));
                }
                let (w, h) = (it.w.max(1), it.h.max(1));
                let style = it.style.clone();
                let mut m = Msg {
                    w,
                    h,
                    view: [0, 0, w, h],
                    size: 16,
                    line_h: 18,
                    color: [255, 255, 255],
                    ..Msg::default()
                };
                if let Some(st) = &style {
                    apply_style(&mut m, st);
                }
                let text = n.text.clone();
                layout_message(&mut self.fonts, &mut m, &text, f64::MIN);
                let img = Rc::new(render_message(&mut self.fonts, &m, f64::MAX));
                n.text_pic = Some((text, img.clone()));
                return Some((img.clone(), [0, 0, img.w as i32, img.h as i32]));
            }
        }
        if n.msg.is_some() {
            let now = self.now;
            let shown = n
                .msg
                .as_ref()
                .unwrap()
                .chars
                .iter()
                .filter(|c| c.1 <= now)
                .count();
            if shown == 0 {
                return None;
            }
            let m = n.msg.as_mut().unwrap();
            if let Some((k, img)) = &m.cache
                && *k == shown
            {
                return Some((img.clone(), [0, 0, img.w as i32, img.h as i32]));
            }
            let img = Rc::new(render_message(&mut self.fonts, m, now));
            m.cache = Some((shown, img.clone()));
            return Some((img, [0, 0, m.w, m.h]));
        }
        let p = n.pic.as_ref()?;
        let src = n.clip.unwrap_or([0, 0, p.img.w as i32, p.img.h as i32]);
        Some((p.img.clone(), src))
    }
}

fn same_obj(a: &Value, b: &Value) -> bool {
    matches!((a, b), (Value::Obj(x), Value::Obj(y)) if Rc::ptr_eq(x, y))
}

fn detach(c: &Obj) {
    let p = c.borrow_mut().parent.take().and_then(|p| p.upgrade());
    if let Some(p) = p {
        p.borrow_mut().children.retain(|k| !Rc::ptr_eq(&k.1, c));
    }
}

fn size_of(n: &Native) -> (i32, i32) {
    if let Some(it) = &n.item {
        if it.w > 0 {
            return (it.w, it.h);
        }
        // a button of size 0 takes the size of its normal picture
        if let Some((p, r)) = &it.states[0] {
            return match r {
                Some(r) => (r[2], r[3]),
                None => (p.img.w as i32, p.img.h as i32),
            };
        }
    }
    if let Some((w, h, _)) = n.buffer {
        return (w as i32, h as i32);
    }
    if let Some(p) = &n.pic {
        return match n.clip {
            Some(c) => (c[2] - c[0], c[3] - c[1]),
            None => (p.img.w as i32, p.img.h as i32),
        };
    }
    (0, 0)
}

fn apply_act(n: &mut Native, t: f64) {
    let Some(a) = &n.act else { return };
    let (trans, curve, rev, mag) = (
        a.trans.as_ref().map(|p| bezier(p, t)),
        a.curve.as_ref().map(|p| bezier2(p, t)),
        a.rev.as_ref().map(|p| bezier(p, t)),
        a.mag.as_ref().map(|p| bezier2(p, t)),
    );
    if let Some(v) = trans {
        n.trans = v.clamp(0.0, 256.0);
    }
    if let Some(p) = curve {
        n.pos = p;
    }
    if let Some(r) = rev {
        n.angle = r;
    }
    if let Some(m) = mag {
        n.scale = m;
    }
}

fn finish_act(n: &mut Native, to_end: bool) {
    if to_end {
        apply_act(n, 1.0);
    }
    n.act = None;
}

fn apply_style(m: &mut Msg, st: &Node) {
    if let Some(f) = st.kid("font") {
        let s = f.int("size", 0);
        if s > 0 {
            m.size = s;
            m.line_h = m.line_h.max(s + s / 4);
        }
    }
    if let Some(a) = st.kid("arrange") {
        let lh = a.int("line_height", 0);
        if lh > 0 {
            m.line_h = lh;
        }
    }
    if let Some(t) = st.kid("text") {
        m.color = crate::text::rgb(t.int("color", 0xffffff) as i64);
    }
    m.shadow = st.kid("shadow").and_then(|s| {
        (s.int("transparency", 0) < 256).then(|| crate::text::rgb(s.int("color", 0) as i64))
    });
}

/// Where the next character goes.
fn message_cursor(m: &Msg) -> (i32, i32) {
    m.chars
        .last()
        .map_or((m.view[0], m.view[1]), |c| (c.2 & 0xffff, c.2 >> 16))
}

/// Lays out message text after what is there: each character with its time and position.
fn layout_message(fonts: &mut crate::text::Fonts, m: &mut Msg, text: &str, now: f64) {
    let (mut x, mut y) = match m.chars.last() {
        Some(c) => {
            let (cx, cy) = (c.2 & 0xffff, c.2 >> 16);
            if c.0 == '\n' {
                (m.view[0], cy)
            } else {
                (cx + fonts.advance(c.0, m.size as u32), cy)
            }
        }
        None => (m.view[0], m.view[1]),
    };
    let mut t = m.chars.last().map_or(now, |c| c.1.max(now));
    let mut color = m.color;
    let mut speed = m.speed;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c == '\\' && i < chars.len() {
            if chars[i] == '\\' {
                i += 1;
                put_char(fonts, m, '\\', &mut x, &mut y, &mut t, speed, color);
                continue;
            }
            // \cmd;params: or \cmd: ; else the flags \b \nb \i \ni
            let letters = chars[i..]
                .iter()
                .take_while(|c| c.is_ascii_alphabetic())
                .count();
            let after = chars.get(i + letters).copied();
            if letters > 0 && matches!(after, Some(';') | Some(':')) {
                let cmd: String = chars[i..i + letters].iter().collect();
                let mut j = i + letters;
                let mut params = String::new();
                if after == Some(';') {
                    j += 1;
                    while j < chars.len() && chars[j] != ':' {
                        params.push(chars[j]);
                        j += 1;
                    }
                }
                i = (j + 1).min(chars.len());
                match cmd.as_str() {
                    "c" => {
                        color =
                            match u32::from_str_radix(params.split(';').next().unwrap_or(""), 16) {
                                Ok(v) => crate::text::rgb(v as i64),
                                Err(_) => m.color,
                            };
                    }
                    "v" => {
                        speed = if params.is_empty() {
                            m.speed
                        } else {
                            parse_int(params.split(';').next().unwrap_or("")) as f64
                        };
                    }
                    "n" => {
                        x = m.view[0];
                        y += m.line_h;
                        m.chars.push(('\n', t, x | (y << 16), color));
                    }
                    "r" => {
                        let word = params.split(';').next().unwrap_or("").to_string();
                        for wc in word.chars() {
                            put_char(fonts, m, wc, &mut x, &mut y, &mut t, speed, color);
                        }
                    }
                    _ => {}
                }
            } else {
                let rest: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                i += if rest == "nb" || rest == "ni" { 2 } else { 1 };
            }
            continue;
        }
        match c {
            '\r' => {}
            '\n' => {
                x = m.view[0];
                y += m.line_h;
                m.chars.push(('\n', t, x | (y << 16), color));
            }
            _ => put_char(fonts, m, c, &mut x, &mut y, &mut t, speed, color),
        }
    }
    m.cache = None;
}

#[allow(clippy::too_many_arguments)]
fn put_char(
    fonts: &mut crate::text::Fonts,
    m: &mut Msg,
    c: char,
    x: &mut i32,
    y: &mut i32,
    t: &mut f64,
    speed: f64,
    color: [u8; 3],
) {
    let adv = fonts.advance(c, m.size as u32);
    if *x + adv > m.view[2] && *x > m.view[0] {
        *x = m.view[0];
        *y += m.line_h;
    }
    *t += speed.max(0.0);
    m.chars.push((c, *t, *x | (*y << 16), color));
    *x += adv;
}

fn render_message(fonts: &mut crate::text::Fonts, m: &Msg, now: f64) -> Image {
    let mut img = Image::new(m.w.max(1) as u32, m.h.max(1) as u32);
    for &(c, t, at, color) in &m.chars {
        if t > now || c == '\n' {
            continue;
        }
        let (x, y) = (at & 0xffff, at >> 16);
        if let Some(sh) = m.shadow {
            fonts.draw(&mut img, x + 1, y + 1, c, m.size as u32, sh, None);
        }
        fonts.draw(&mut img, x, y, c, m.size as u32, color, None);
    }
    img
}

fn save_name(n: &str) -> String {
    n.rsplit(['\\', '/']).next().unwrap_or(n).to_string()
}

fn real_method(r: f64, name: &str, args: &[Var]) -> Result<Done, String> {
    let a = ar(args, 0, 0.0);
    Ok(Done::Val(Value::Real(match name {
        "Pi" => std::f64::consts::PI,
        "Abs" => r.abs(),
        "Log" => {
            if args.is_empty() {
                r.ln()
            } else {
                r.log(a)
            }
        }
        "Power" => r.powf(a),
        "Sqrt" => r.sqrt(),
        "Sin" => r.sin(),
        "Cos" => r.cos(),
        "Tan" => r.tan(),
        "ASin" => r.asin(),
        "ACos" => r.acos(),
        "ATan" => r.atan(),
        "Round" => r.round(),
        _ => return Err(format!("Real::{name}: no such function")),
    })))
}

fn array_method(t: &Var, name: &str, args: &[Var]) -> Result<Done, String> {
    let mut b = t.borrow_mut();
    let Value::Array(a) = &mut *b else {
        return Err("not an array".into());
    };
    let n = a.len() as i32;
    Ok(Done::Val(match name {
        "GetLength" => Value::Int(n),
        "IsEmpty" => {
            let i = ai(args, 0, 0);
            let e = i < 0
                || i >= n
                || matches!(&*a[i as usize].borrow(), Value::Empty | Value::Ref(None));
            Value::Int(if e { -1 } else { 0 })
        }
        "Find" => {
            let x = arg(args, 0);
            let from = ai(args, 1, 0).max(0) as usize;
            let mut r = -1;
            for (i, e) in a.iter().enumerate().skip(from) {
                let e = deref(e).borrow().clone();
                let same = match (&e, &x) {
                    (Value::Obj(p), Value::Obj(q)) => Rc::ptr_eq(p, q),
                    (Value::Struct(..), _) | (Value::Array(_), _) | (Value::Hash(..), _) => args
                        .first()
                        .is_some_and(|v| Rc::ptr_eq(&deref(v), &deref(&a[i]))),
                    _ => {
                        std::mem::discriminant(&e) == std::mem::discriminant(&x)
                            && compare(1, &e, &x)
                    }
                };
                if same {
                    r = i as i32;
                    break;
                }
            }
            Value::Int(r)
        }
        "Swap" => {
            let (i, j) = (ai(args, 0, 0), ai(args, 1, 0));
            if (0..n).contains(&i) && (0..n).contains(&j) {
                a.swap(i as usize, j as usize);
            }
            Value::Int(0)
        }
        "Insert" => {
            let i = ai(args, 0, 0).clamp(0, n) as usize;
            a.insert(i, var(arg(args, 1).deep()));
            Value::Int(0)
        }
        "Remove" => {
            let i = ai(args, 0, 0).clamp(0, n);
            let c = ai(args, 1, -1);
            let e = if c < 0 { n } else { (i + c).min(n) };
            a.drain(i as usize..e.max(i) as usize);
            Value::Int(0)
        }
        "RemoveAll" => {
            a.clear();
            Value::Int(0)
        }
        // FontArray: the fonts there are
        "EnumerateAllFonts" => {
            a.clear();
            a.push(var(s("ＭＳ ゴシック")));
            Value::Int(0)
        }
        "Detach" => {
            let i = ai(args, 0, 0);
            if (0..n).contains(&i) {
                let v = a.remove(i as usize);
                return Ok(Done::Var(v));
            }
            Value::Int(0)
        }
        "Merge" => {
            for e in elements(&arg(args, 0)) {
                a.push(var(e.deep()));
            }
            Value::Int(0)
        }
        _ => return Err(format!("Array::{name}: no such function")),
    }))
}

fn hash_method(t: &Var, name: &str, args: &[Var]) -> Result<Done, String> {
    let mut b = t.borrow_mut();
    let Value::Hash(h, d) = &mut *b else {
        return Err("not a hash".into());
    };
    Ok(Done::Val(match name {
        "GetLength" => Value::Int(h.len() as i32),
        "IsEmpty" => {
            let k = astr(args, 0);
            let e = match hash_find(h, &k) {
                Ok(i) => matches!(&*h[i].1.borrow(), Value::Empty | Value::Ref(None)),
                Err(_) => true,
            };
            Value::Int(if e { -1 } else { 0 })
        }
        "GetTagName" => {
            let i = ai(args, 0, 0);
            s(&h.get(i.max(0) as usize)
                .map_or(String::new(), |e| e.0.to_string()))
        }
        "Remove" => {
            let k = astr(args, 0);
            if let Ok(i) = hash_find(h, &k) {
                h.remove(i);
            }
            Value::Int(0)
        }
        "RemoveAll" => {
            h.clear();
            Value::Int(0)
        }
        "SetDefaultElement" => {
            *d = args.first().map(|v| Box::new(deref(v).borrow().deep()));
            Value::Int(0)
        }
        _ => return Err(format!("Hash::{name}: no such function")),
    }))
}
