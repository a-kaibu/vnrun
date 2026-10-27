//! Saving: the File object's records and the snapshot of the script context.
//!
//! Cotopha saves a game with `File::SaveContext`, which stores the whole running program (the
//! global variables, every thread with its stack, and the engine objects they reach) so that
//! `File::LoadContext` later resumes each thread where it was, the saving one just after its
//! SaveContext call. The `data` variables are not part of it: they keep their current values
//! (the script sets a "loaded" flag there before it loads), and the engine objects they hold
//! keep their identity, only their child sprites come from the save.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::engine::Engine;
use crate::natives::{Act, Item, Msg, Native, Obj};
use crate::skin::{Node, Skin};
use crate::value::*;
use crate::vm::{Block, Frame, Ret, Start, Thread};

/// An open File: its records, written to the save store after each change.
#[derive(Default)]
pub struct FileState {
    pub name: String,
    pub write: bool,
    pub records: Vec<(String, Vec<u8>)>,
    pub read_at: usize,
}

const FILE_MAGIC: &[u8; 8] = b"VNENTIS1";
const CTX_MAGIC: &[u8; 8] = b"ENTISCTX";
const EXT: u32 = 0x8000_0000;

impl FileState {
    pub fn pack(&self) -> Vec<u8> {
        let mut w = Out::default();
        w.b.extend_from_slice(FILE_MAGIC);
        w.u32(self.records.len() as u32);
        for (k, v) in &self.records {
            w.str(k);
            w.bytes(v);
        }
        w.b
    }

    pub fn unpack(name: &str, b: &[u8]) -> Option<FileState> {
        if !b.starts_with(FILE_MAGIC) {
            return None;
        }
        let mut r = In { b, p: 8 };
        let n = r.u32()?;
        let mut records = Vec::new();
        for _ in 0..n {
            let k = r.str()?;
            let v = r.bytes()?.to_vec();
            records.push((k, v));
        }
        Some(FileState {
            name: name.to_string(),
            write: false,
            records,
            read_at: 0,
        })
    }

    /// The next record with this tag.
    pub fn next(&mut self, tag: &str) -> Option<Vec<u8>> {
        let i = self.records[self.read_at.min(self.records.len())..]
            .iter()
            .position(|(k, _)| k == tag)?
            + self.read_at;
        self.read_at = i + 1;
        Some(self.records[i].1.clone())
    }

    pub fn find(&self, tag: &str) -> Option<&[u8]> {
        self.records
            .iter()
            .find(|(k, _)| k == tag)
            .map(|(_, v)| v.as_slice())
    }
}

#[derive(Default)]
struct Out {
    b: Vec<u8>,
}

impl Out {
    fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    fn bool(&mut self, v: bool) {
        self.b.push(v as u8);
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn bytes(&mut self, s: &[u8]) {
        self.u32(s.len() as u32);
        self.b.extend_from_slice(s);
    }
    fn opt_str(&mut self, s: Option<&str>) {
        self.bool(s.is_some());
        if let Some(s) = s {
            self.str(s);
        }
    }
    fn f64s(&mut self, v: &Option<Vec<f64>>) {
        self.bool(v.is_some());
        if let Some(v) = v {
            self.u32(v.len() as u32);
            for x in v {
                self.f64(*x);
            }
        }
    }
    fn pairs(&mut self, v: &Option<Vec<(f64, f64)>>) {
        self.bool(v.is_some());
        if let Some(v) = v {
            self.u32(v.len() as u32);
            for (a, b) in v {
                self.f64(*a);
                self.f64(*b);
            }
        }
    }
    fn rect(&mut self, r: &Option<[i32; 4]>) {
        self.bool(r.is_some());
        if let Some(r) = r {
            for x in r {
                self.i32(*x);
            }
        }
    }
    fn node(&mut self, n: &Node) {
        self.str(&n.tag);
        self.u32(n.attrs.len() as u32);
        for (k, v) in &n.attrs {
            self.str(k);
            self.str(v);
        }
        self.u32(n.kids.len() as u32);
        for k in &n.kids {
            self.node(k);
        }
    }
}

struct In<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> In<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn bool(&mut self) -> Option<bool> {
        Some(self.u8()? != 0)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    fn str(&mut self) -> Option<String> {
        Some(String::from_utf8_lossy(self.bytes()?).into_owned())
    }
    fn opt_str(&mut self) -> Option<Option<String>> {
        Some(if self.bool()? {
            Some(self.str()?)
        } else {
            None
        })
    }
    fn len(&mut self) -> Option<usize> {
        let n = self.u32()? as usize;
        // every element takes at least a byte
        (n <= self.b.len() - self.p).then_some(n)
    }
    fn f64s(&mut self) -> Option<Option<Vec<f64>>> {
        if !self.bool()? {
            return Some(None);
        }
        let n = self.len()?;
        (0..n)
            .map(|_| self.f64())
            .collect::<Option<Vec<_>>>()
            .map(Some)
    }
    fn pairs(&mut self) -> Option<Option<Vec<(f64, f64)>>> {
        if !self.bool()? {
            return Some(None);
        }
        let n = self.len()?;
        (0..n)
            .map(|_| Some((self.f64()?, self.f64()?)))
            .collect::<Option<Vec<_>>>()
            .map(Some)
    }
    fn rect(&mut self) -> Option<Option<[i32; 4]>> {
        if !self.bool()? {
            return Some(None);
        }
        Some(Some([self.i32()?, self.i32()?, self.i32()?, self.i32()?]))
    }
    fn node(&mut self, depth: u32) -> Option<Node> {
        if depth > 64 {
            return None;
        }
        let tag = self.str()?;
        let n = self.len()?;
        let mut attrs = Vec::with_capacity(n);
        for _ in 0..n {
            attrs.push((self.str()?, self.str()?));
        }
        let n = self.len()?;
        let mut kids = Vec::with_capacity(n);
        for _ in 0..n {
            kids.push(self.node(depth + 1)?);
        }
        Some(Node { tag, attrs, kids })
    }
}

enum Pending {
    V(Var),
    O(Obj),
}

/// Writes a graph of variables and engine objects: references are numbers, and each object is
/// written once, after the roots.
struct Writer {
    o: Out,
    vars: HashMap<usize, u32>,
    objs: HashMap<usize, u32>,
    queue: VecDeque<Pending>,
    ext_vars: HashMap<usize, u32>,
    ext_objs: HashMap<usize, u32>,
    pics: HashMap<usize, String>,
    now: f64,
}

impl Writer {
    fn new(e: &Engine) -> Writer {
        let pics = e
            .pics
            .iter()
            .filter_map(|(k, p)| p.as_ref().map(|p| (Rc::as_ptr(p) as usize, k.clone())))
            .collect();
        Writer {
            o: Out::default(),
            vars: HashMap::new(),
            objs: HashMap::new(),
            queue: VecDeque::new(),
            ext_vars: HashMap::new(),
            ext_objs: HashMap::new(),
            pics,
            now: e.now,
        }
    }

    fn var(&mut self, v: &Var) {
        let p = Rc::as_ptr(v) as usize;
        if let Some(&e) = self.ext_vars.get(&p) {
            self.o.u32(EXT | e);
            return;
        }
        let n = self.vars.len() as u32;
        let id = *self.vars.entry(p).or_insert_with(|| {
            self.queue.push_back(Pending::V(v.clone()));
            n
        });
        self.o.u32(id);
    }

    fn opt_var(&mut self, v: Option<&Var>) {
        self.o.bool(v.is_some());
        if let Some(v) = v {
            self.var(v);
        }
    }

    fn obj(&mut self, v: &Obj) {
        let p = Rc::as_ptr(v) as usize;
        if let Some(&e) = self.ext_objs.get(&p) {
            self.o.u32(EXT | e);
            return;
        }
        let n = self.objs.len() as u32;
        let id = *self.objs.entry(p).or_insert_with(|| {
            self.queue.push_back(Pending::O(v.clone()));
            n
        });
        self.o.u32(id);
    }

    fn time(&mut self, t: f64) {
        self.o.f64(if t >= f64::MAX / 2.0 {
            f64::MAX
        } else {
            t - self.now
        });
    }

    fn value(&mut self, v: &Value) {
        match v {
            Value::Empty => self.o.u8(0),
            Value::Int(i) => {
                self.o.u8(1);
                self.o.i32(*i);
            }
            Value::Real(r) => {
                self.o.u8(2);
                self.o.f64(*r);
            }
            Value::Str(s) => {
                self.o.u8(3);
                self.o.str(s);
            }
            Value::Array(a) => {
                self.o.u8(4);
                self.o.u32(a.len() as u32);
                for x in a {
                    self.var(x);
                }
            }
            Value::Hash(h, d) => {
                self.o.u8(5);
                self.o.u32(h.len() as u32);
                for (k, x) in h {
                    self.o.str(k);
                    self.var(x);
                }
                self.o.bool(d.is_some());
                if let Some(d) = d {
                    self.value(d);
                }
            }
            Value::Struct(c, m) => {
                self.o.u8(6);
                self.o.str(c);
                self.o.u32(m.len() as u32);
                for (k, x) in m {
                    self.o.str(k);
                    self.var(x);
                }
            }
            Value::Ref(r) => {
                self.o.u8(7);
                self.opt_var(r.as_ref());
            }
            Value::Obj(o) => {
                self.o.u8(8);
                self.obj(o);
            }
            Value::Scope(k) => {
                self.o.u8(9);
                self.o.u8(*k);
            }
        }
    }

    fn pic(&mut self, p: &Option<Rc<crate::natives::Pic>>) {
        let name = p
            .as_ref()
            .and_then(|p| self.pics.get(&(Rc::as_ptr(p) as usize)).cloned());
        self.o.opt_str(name.as_deref());
    }

    fn native(&mut self, n: &Native) {
        self.o.str(&n.class);
        self.pic(&n.pic);
        self.o.rect(&n.clip);
        self.o.bool(n.sound.is_some());
        self.o.str(&n.sound_name);
        self.o.f64(n.sound_ms);
        self.o.str(&n.channel);
        self.o.bool(n.play_end.is_some());
        if let Some(t) = n.play_end {
            self.time(t);
        }
        self.o.f64(n.volume);
        self.o.bool(n.buffer.is_some());
        if let Some((w, h, a)) = n.buffer {
            self.o.u32(w);
            self.o.u32(h);
            self.o.bool(a);
        }
        self.o.bool(n.back.is_some());
        if let Some(b) = n.back {
            self.o.b.extend_from_slice(&b);
        }
        self.o.bool(n.visible);
        self.o.bool(n.enabled);
        self.o.f64(n.trans);
        for (a, b) in [n.pos, n.center, n.scale] {
            self.o.f64(a);
            self.o.f64(b);
        }
        self.o.f64(n.angle);
        self.o.i32(n.flags);
        self.o.u32(n.children.len() as u32);
        for (p, c) in &n.children {
            self.o.i32(*p);
            self.obj(c);
        }
        let parent = n.parent.as_ref().and_then(|p| p.upgrade());
        self.o.bool(parent.is_some());
        if let Some(p) = &parent {
            self.obj(p);
        }
        self.o.str(&n.sprite_id);
        self.o.bool(n.item.is_some());
        if let Some(it) = &n.item {
            self.o.str(&it.kind);
            self.o.i32(it.w);
            self.o.i32(it.h);
            for st in &it.states {
                self.o.bool(st.is_some());
                if let Some((p, r)) = st {
                    self.pic(&Some(p.clone()));
                    self.o.rect(r);
                }
            }
            self.o.u32(it.state as u32);
            self.o.bool(it.notify.is_some());
            self.o.i32(it.notify.unwrap_or(0));
            self.o.bool(it.check);
            self.o.bool(it.checked);
            self.o.bool(it.style.is_some());
            if let Some(s) = &it.style {
                self.o.node(s);
            }
        }
        self.o.str(&n.text);
        self.o.f64s(&n.pend_trans);
        self.o.pairs(&n.pend_curve);
        self.o.f64s(&n.pend_rev);
        self.o.pairs(&n.pend_mag);
        self.o.bool(n.act.is_some());
        if let Some(a) = &n.act {
            self.time(a.t0);
            self.o.f64(a.dur);
            self.o.i32(a.kind);
            self.o.f64s(&a.trans);
            self.o.pairs(&a.curve);
            self.o.f64s(&a.rev);
            self.o.pairs(&a.mag);
        }
        self.o.opt_str(
            n.skin
                .as_ref()
                .map(|s| s.src.as_str())
                .filter(|s| !s.is_empty()),
        );
        self.o.bool(n.msg.is_some());
        if let Some(m) = &n.msg {
            self.o.i32(m.w);
            self.o.i32(m.h);
            for x in m.view {
                self.o.i32(x);
            }
            self.o.u32(m.chars.len() as u32);
            for (c, t, sz, col) in &m.chars {
                self.o.u32(*c as u32);
                self.time(*t);
                self.o.i32(*sz);
                self.o.b.extend_from_slice(col);
            }
            self.o.i32(m.size);
            self.o.i32(m.line_h);
            self.o.b.extend_from_slice(&m.color);
            self.o.bool(m.shadow.is_some());
            self.o.b.extend_from_slice(&m.shadow.unwrap_or_default());
            self.o.f64(m.speed);
            self.o.f64(m.fade);
            self.o.bool(m.ruby);
        }
        self.o.bool(n.signaled);
        self.o.bool(n.thread.is_some());
        self.o.u32(n.thread.unwrap_or(0) as u32);
        self.o.bool(n.tone.is_some());
        for x in n.tone.unwrap_or_default() {
            self.o.i32(x);
        }
        self.o.u32(n.elems.len() as u32);
        for (k, v) in &n.elems {
            self.o.str(k);
            self.var(v);
        }
    }

    fn thread(&mut self, t: &Thread) {
        self.o.u32(t.stack.len() as u32);
        for v in &t.stack {
            self.var(v);
        }
        self.o.u32(t.names.len() as u32);
        for n in &t.names {
            self.o.opt_str(n.as_deref());
        }
        self.o.u32(t.frames.len() as u32);
        for f in &t.frames {
            self.frame(f);
        }
        self.o.u32(t.pc as u32);
        self.o.bool(t.deadline.is_some());
        if let Some(d) = t.deadline {
            self.time(d);
        }
        self.o.bool(t.done);
        self.opt_var(t.result.as_ref());
        self.o.u32(t.queue.len() as u32);
        for s in &t.queue {
            match s {
                Start::Construct(v, c) => {
                    self.o.u8(0);
                    self.var(v);
                    self.o.str(c);
                }
                Start::Call(f) => {
                    self.o.u8(1);
                    self.o.u32(*f as u32);
                }
                Start::Run(f, this, args) => {
                    self.o.u8(2);
                    self.o.u32(*f as u32);
                    self.opt_var(this.as_ref());
                    self.o.u32(args.len() as u32);
                    for a in args {
                        self.var(a);
                    }
                }
            }
        }
    }

    fn frame(&mut self, f: &Frame) {
        self.o.u32(f.ret_pc as u32);
        self.o.u32(f.base as u32);
        self.opt_var(f.this.as_ref());
        self.o.opt_str(f.class.as_deref());
        self.o.str(&f.func);
        self.o.u32(f.blocks.len() as u32);
        for b in &f.blocks {
            self.o.u32(b.height as u32);
            self.o.bool(b.catch.is_some());
            self.o.u32(b.catch.unwrap_or(0) as u32);
        }
        match &f.ret {
            Ret::Push => self.o.u8(0),
            Ret::Discard => self.o.u8(1),
            Ret::PushVar(v) => {
                self.o.u8(2);
                self.var(v);
            }
        }
        self.o.bool(f.entered);
    }

    /// Writes every object referred to so far (and those they refer to).
    fn finish(mut self) -> Vec<u8> {
        let mut nv = 0u32;
        let mut no = 0u32;
        while let Some(p) = self.queue.pop_front() {
            match p {
                Pending::V(v) => {
                    self.o.u8(1);
                    self.o.u32(nv);
                    nv += 1;
                    let val = v.borrow().clone();
                    self.value(&val);
                }
                Pending::O(o) => {
                    self.o.u8(2);
                    self.o.u32(no);
                    no += 1;
                    let b = o.borrow();
                    self.native(&b);
                }
            }
        }
        self.o.u8(0);
        self.o.b
    }
}

/// Reads what a Writer wrote; objects are made when first referred to and filled later.
struct Reader<'a> {
    i: In<'a>,
    vars: Vec<Option<Var>>,
    objs: Vec<Option<Obj>>,
    ext_vars: Vec<Var>,
    ext_objs: Vec<Obj>,
    skins: HashMap<String, Option<Rc<Skin>>>,
    now: f64,
    /// objects whose sound loops on, and those with a running activation
    looping: Vec<Obj>,
    active: Vec<Obj>,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8], now: f64) -> Reader<'a> {
        Reader {
            i: In { b, p: 0 },
            vars: Vec::new(),
            objs: Vec::new(),
            ext_vars: Vec::new(),
            ext_objs: Vec::new(),
            skins: HashMap::new(),
            now,
            looping: Vec::new(),
            active: Vec::new(),
        }
    }

    fn var(&mut self) -> Option<Var> {
        let id = self.i.u32()?;
        if id & EXT != 0 {
            let e = (id & !EXT) as usize;
            return Some(
                self.ext_vars
                    .get(e)
                    .cloned()
                    .unwrap_or_else(|| var(Value::Empty)),
            );
        }
        let id = id as usize;
        if id > 0x40_0000 {
            return None;
        }
        if self.vars.len() <= id {
            self.vars.resize(id + 1, None);
        }
        Some(
            self.vars[id]
                .get_or_insert_with(|| var(Value::Empty))
                .clone(),
        )
    }

    fn opt_var(&mut self) -> Option<Option<Var>> {
        Some(if self.i.bool()? {
            Some(self.var()?)
        } else {
            None
        })
    }

    fn obj(&mut self) -> Option<Obj> {
        let id = self.i.u32()?;
        if id & EXT != 0 {
            let e = (id & !EXT) as usize;
            return Some(
                self.ext_objs
                    .get(e)
                    .cloned()
                    .unwrap_or_else(|| Rc::new(RefCell::new(Native::new("Sprite")))),
            );
        }
        let id = id as usize;
        if id > 0x40_0000 {
            return None;
        }
        if self.objs.len() <= id {
            self.objs.resize(id + 1, None);
        }
        Some(
            self.objs[id]
                .get_or_insert_with(|| Rc::new(RefCell::new(Native::default())))
                .clone(),
        )
    }

    fn time(&mut self) -> Option<f64> {
        let t = self.i.f64()?;
        Some(if t >= f64::MAX / 2.0 {
            f64::MAX
        } else {
            t + self.now
        })
    }

    fn value(&mut self, depth: u32) -> Option<Value> {
        Some(match self.i.u8()? {
            0 => Value::Empty,
            1 => Value::Int(self.i.i32()?),
            2 => Value::Real(self.i.f64()?),
            3 => Value::Str(self.i.str()?.into()),
            4 => {
                let n = self.i.len()?;
                Value::Array((0..n).map(|_| self.var()).collect::<Option<_>>()?)
            }
            5 => {
                let n = self.i.len()?;
                let mut h = Vec::with_capacity(n);
                for _ in 0..n {
                    let k: Str = self.i.str()?.into();
                    h.push((k, self.var()?));
                }
                let d = if self.i.bool()? && depth < 32 {
                    Some(Box::new(self.value(depth + 1)?))
                } else {
                    None
                };
                Value::Hash(h, d)
            }
            6 => {
                let c: Str = self.i.str()?.into();
                let n = self.i.len()?;
                let mut m = Vec::with_capacity(n);
                for _ in 0..n {
                    let k: Str = self.i.str()?.into();
                    m.push((k, self.var()?));
                }
                Value::Struct(c, m)
            }
            7 => Value::Ref(self.opt_var()?),
            8 => Value::Obj(self.obj()?),
            9 => Value::Scope(self.i.u8()?),
            _ => return None,
        })
    }

    fn pic(&mut self, e: &mut Engine) -> Option<Option<Rc<crate::natives::Pic>>> {
        Some(match self.i.opt_str()? {
            Some(name) => match e.pics.get(&name) {
                Some(p) => p.clone(),
                None if name.starts_with('\u{1}') => None,
                None => e.pic(&name),
            },
            None => None,
        })
    }

    fn native(&mut self, e: &mut Engine, me: &Obj) -> Option<Native> {
        let mut n = Native::new(&self.i.str()?);
        n.pic = self.pic(e)?;
        n.clip = self.i.rect()?;
        let had_sound = self.i.bool()?;
        n.sound_name = self.i.str()?;
        n.sound_ms = self.i.f64()?;
        if had_sound && !n.sound_name.is_empty() {
            n.sound = e.fs.read(&n.sound_name).map(Rc::new);
        }
        n.channel = self.i.str()?;
        if self.i.bool()? {
            let t = self.time()?;
            n.play_end = Some(t);
            if t == f64::MAX {
                self.looping.push(me.clone());
            }
        }
        n.volume = self.i.f64()?;
        if self.i.bool()? {
            n.buffer = Some((self.i.u32()?, self.i.u32()?, self.i.bool()?));
        }
        if self.i.bool()? {
            n.back = Some(self.i.take(4)?.try_into().ok()?);
        }
        n.visible = self.i.bool()?;
        n.enabled = self.i.bool()?;
        n.trans = self.i.f64()?;
        n.pos = (self.i.f64()?, self.i.f64()?);
        n.center = (self.i.f64()?, self.i.f64()?);
        n.scale = (self.i.f64()?, self.i.f64()?);
        n.angle = self.i.f64()?;
        n.flags = self.i.i32()?;
        let k = self.i.len()?;
        for _ in 0..k {
            let p = self.i.i32()?;
            n.children.push((p, self.obj()?));
        }
        if self.i.bool()? {
            n.parent = Some(Rc::downgrade(&self.obj()?));
        }
        n.sprite_id = self.i.str()?;
        if self.i.bool()? {
            let mut it = Item {
                kind: self.i.str()?,
                w: self.i.i32()?,
                h: self.i.i32()?,
                ..Item::default()
            };
            for k in 0..5 {
                if self.i.bool()? {
                    let p = self.pic(e)?;
                    let r = self.i.rect()?;
                    it.states[k] = p.map(|p| (p, r));
                }
            }
            it.state = self.i.u32()? as usize;
            let has = self.i.bool()?;
            let v = self.i.i32()?;
            it.notify = has.then_some(v);
            it.check = self.i.bool()?;
            it.checked = self.i.bool()?;
            if self.i.bool()? {
                it.style = Some(self.i.node(0)?);
            }
            n.item = Some(it);
        }
        n.text = self.i.str()?;
        n.pend_trans = self.i.f64s()?;
        n.pend_curve = self.i.pairs()?;
        n.pend_rev = self.i.f64s()?;
        n.pend_mag = self.i.pairs()?;
        if self.i.bool()? {
            n.act = Some(Act {
                t0: self.time()?,
                dur: self.i.f64()?,
                kind: self.i.i32()?,
                trans: self.i.f64s()?,
                curve: self.i.pairs()?,
                rev: self.i.f64s()?,
                mag: self.i.pairs()?,
            });
            self.active.push(me.clone());
        }
        if let Some(src) = self.i.opt_str()? {
            let skin = match self.skins.get(&src) {
                Some(s) => s.clone(),
                None => {
                    let s = e.load_skin(&src);
                    self.skins.insert(src, s.clone());
                    s
                }
            };
            n.skin = skin;
        }
        if self.i.bool()? {
            let mut m = Msg {
                w: self.i.i32()?,
                h: self.i.i32()?,
                ..Msg::default()
            };
            for x in m.view.iter_mut() {
                *x = self.i.i32()?;
            }
            let k = self.i.len()?;
            for _ in 0..k {
                let c = char::from_u32(self.i.u32()?).unwrap_or(' ');
                let t = self.time()?;
                let sz = self.i.i32()?;
                let col: [u8; 3] = self.i.take(3)?.try_into().ok()?;
                m.chars.push((c, t, sz, col));
            }
            m.size = self.i.i32()?;
            m.line_h = self.i.i32()?;
            m.color = self.i.take(3)?.try_into().ok()?;
            let has = self.i.bool()?;
            let sh: [u8; 3] = self.i.take(3)?.try_into().ok()?;
            m.shadow = has.then_some(sh);
            m.speed = self.i.f64()?;
            m.fade = self.i.f64()?;
            m.ruby = self.i.bool()?;
            n.msg = Some(m);
        }
        n.signaled = self.i.bool()?;
        let has = self.i.bool()?;
        let t = self.i.u32()? as usize;
        n.thread = has.then_some(t);
        let has = self.i.bool()?;
        let tone = [self.i.i32()?, self.i.i32()?, self.i.i32()?, self.i.i32()?];
        n.tone = has.then_some(tone);
        let k = self.i.len()?;
        for _ in 0..k {
            let key = self.i.str()?;
            n.elems.push((key, self.var()?));
        }
        Some(n)
    }

    fn thread(&mut self) -> Option<Thread> {
        let mut t = Thread::default();
        let n = self.i.len()?;
        for _ in 0..n {
            t.stack.push(self.var()?);
        }
        let n = self.i.len()?;
        for _ in 0..n {
            t.names
                .push(self.i.opt_str()?.map(|s| Str::from(s.as_str())));
        }
        let n = self.i.len()?;
        for _ in 0..n {
            t.frames.push(self.frame()?);
        }
        t.pc = self.i.u32()? as usize;
        if self.i.bool()? {
            t.deadline = Some(self.time()?);
        }
        t.done = self.i.bool()?;
        t.result = self.opt_var()?;
        let n = self.i.len()?;
        for _ in 0..n {
            t.queue.push(match self.i.u8()? {
                0 => {
                    let v = self.var()?;
                    Start::Construct(v, self.i.str()?.into())
                }
                1 => Start::Call(self.i.u32()? as usize),
                _ => {
                    let f = self.i.u32()? as usize;
                    let this = self.opt_var()?;
                    let k = self.i.len()?;
                    let args = (0..k).map(|_| self.var()).collect::<Option<_>>()?;
                    Start::Run(f, this, args)
                }
            });
        }
        Some(t)
    }

    fn frame(&mut self) -> Option<Frame> {
        let ret_pc = self.i.u32()? as usize;
        let base = self.i.u32()? as usize;
        let this = self.opt_var()?;
        let class = self.i.opt_str()?.map(|s| Str::from(s.as_str()));
        let func: Str = self.i.str()?.into();
        let n = self.i.len()?;
        let mut blocks = Vec::new();
        for _ in 0..n {
            let height = self.i.u32()? as usize;
            let has = self.i.bool()?;
            let c = self.i.u32()? as usize;
            blocks.push(Block {
                height,
                catch: has.then_some(c),
            });
        }
        let ret = match self.i.u8()? {
            0 => Ret::Push,
            1 => Ret::Discard,
            _ => Ret::PushVar(self.var()?),
        };
        let entered = self.i.bool()?;
        Some(Frame {
            ret_pc,
            base,
            this,
            class,
            func,
            blocks,
            ret,
            entered,
        })
    }

    /// Reads the objects after the roots.
    fn finish(&mut self, e: &mut Engine) -> Option<()> {
        loop {
            match self.i.u8()? {
                0 => return Some(()),
                1 => {
                    let id = self.i.u32()? as usize;
                    let v = self.value(0)?;
                    let target = self.vars.get(id).cloned().flatten()?;
                    *target.borrow_mut() = v;
                }
                2 => {
                    let id = self.i.u32()? as usize;
                    let target = self.objs.get(id).cloned().flatten()?;
                    let n = self.native(e, &target)?;
                    *target.borrow_mut() = n;
                }
                _ => return None,
            }
        }
    }
}

/// A loaded context, applied between frames.
pub struct Context {
    globals: Vec<(Str, Var)>,
    threads: Vec<Option<Thread>>,
    screen: Option<Obj>,
    rand: u64,
    next_channel: u32,
    patches: Vec<(Obj, Vec<(i32, Obj)>)>,
    looping: Vec<Obj>,
    active: Vec<Obj>,
}

/// The children of a variable's value, in order (for paths into the data variables).
fn kids(v: &Value) -> Vec<Var> {
    match v {
        Value::Array(a) => a.clone(),
        Value::Struct(_, m) => m.iter().map(|(_, x)| x.clone()).collect(),
        Value::Hash(h, _) => h.iter().map(|(_, x)| x.clone()).collect(),
        Value::Ref(Some(r)) => vec![r.clone()],
        _ => Vec::new(),
    }
}

impl Engine {
    /// The engine objects the data variables hold, with their paths (data index, then child
    /// positions).
    fn data_objects(&self) -> Vec<(Vec<u32>, Obj)> {
        let mut out = Vec::new();
        let mut seen_v = std::collections::HashSet::new();
        let mut seen_o = std::collections::HashSet::new();
        let mut stack: Vec<(Var, Vec<u32>)> = self
            .data
            .iter()
            .enumerate()
            .rev()
            .map(|(i, (_, v))| (v.clone(), vec![i as u32]))
            .collect();
        while let Some((v, path)) = stack.pop() {
            if !seen_v.insert(Rc::as_ptr(&v) as usize) || seen_v.len() > 200_000 {
                continue;
            }
            let val = v.borrow();
            if let Value::Obj(o) = &*val {
                if seen_o.insert(Rc::as_ptr(o) as usize) {
                    out.push((path.clone(), o.clone()));
                }
                continue;
            }
            for (k, c) in kids(&val).into_iter().enumerate().rev() {
                let mut p = path.clone();
                p.push(k as u32);
                stack.push((c, p));
            }
        }
        out
    }

    fn data_path(&self, path: &[u32]) -> Option<Obj> {
        let mut v = self.data.get(*path.first()? as usize)?.1.clone();
        for &k in &path[1..] {
            let next = kids(&v.borrow()).get(k as usize)?.clone();
            v = next;
        }
        let b = v.borrow();
        match &*b {
            Value::Obj(o) => Some(o.clone()),
            _ => None,
        }
    }

    /// The running program, `cur` being the thread that saves (taken out of the list).
    pub fn save_context(&self, cur: &Thread) -> Vec<u8> {
        let mut w = Writer::new(self);
        w.o.b.extend_from_slice(CTX_MAGIC);
        for (i, (_, v)) in self.data.iter().enumerate() {
            w.ext_vars.insert(Rc::as_ptr(v) as usize, i as u32);
        }
        w.o.u32(self.data.len() as u32);
        let objs = self.data_objects();
        w.o.u32(objs.len() as u32);
        for (i, (path, o)) in objs.iter().enumerate() {
            w.ext_objs.insert(Rc::as_ptr(o) as usize, i as u32);
            w.o.u32(path.len() as u32);
            for p in path {
                w.o.u32(*p);
            }
        }
        w.o.u64(self.rand);
        w.o.u32(self.next_channel);
        w.o.u32(self.globals.len() as u32);
        for (k, v) in &self.globals {
            w.o.str(k);
            w.var(v);
        }
        w.o.bool(self.screen.is_some());
        if let Some(s) = &self.screen {
            w.obj(s);
        }
        w.o.u32(self.threads.len() as u32);
        for (i, t) in self.threads.iter().enumerate() {
            let t = if i == self.current {
                Some(cur)
            } else {
                t.as_ref()
            };
            w.o.bool(t.is_some());
            if let Some(t) = t {
                w.thread(t);
            }
        }
        // the child sprites of the data objects (the screen's scene)
        w.o.u32(objs.len() as u32);
        for (i, (_, o)) in objs.iter().enumerate() {
            w.o.u32(i as u32);
            let kids = o.borrow().children.clone();
            w.o.u32(kids.len() as u32);
            for (p, c) in &kids {
                w.o.i32(*p);
                w.obj(c);
            }
        }
        w.finish()
    }

    pub fn read_context(&mut self, b: &[u8]) -> Result<Context, String> {
        let bad = || "the save data is broken".to_string();
        if !b.starts_with(CTX_MAGIC) {
            return Err(bad());
        }
        let mut r = Reader::new(b, self.now);
        r.i.p = 8;
        let ndata = r.i.u32().ok_or_else(bad)? as usize;
        r.ext_vars = (0..ndata)
            .map(|i| {
                self.data
                    .get(i)
                    .map_or_else(|| var(Value::Empty), |(_, v)| v.clone())
            })
            .collect();
        let nobj = r.i.len().ok_or_else(bad)?;
        for _ in 0..nobj {
            let k = r.i.len().ok_or_else(bad)?;
            let path = (0..k)
                .map(|_| r.i.u32())
                .collect::<Option<Vec<_>>>()
                .ok_or_else(bad)?;
            let o = self
                .data_path(&path)
                .unwrap_or_else(|| Rc::new(RefCell::new(Native::new("Sprite"))));
            r.ext_objs.push(o);
        }
        let rand = r.i.u64().ok_or_else(bad)?;
        let next_channel = r.i.u32().ok_or_else(bad)?;
        let n = r.i.len().ok_or_else(bad)?;
        let mut globals = Vec::new();
        for _ in 0..n {
            let k: Str = r.i.str().ok_or_else(bad)?.into();
            globals.push((k, r.var().ok_or_else(bad)?));
        }
        let screen = if r.i.bool().ok_or_else(bad)? {
            Some(r.obj().ok_or_else(bad)?)
        } else {
            None
        };
        let n = r.i.len().ok_or_else(bad)?;
        let mut threads = Vec::new();
        for _ in 0..n {
            threads.push(if r.i.bool().ok_or_else(bad)? {
                Some(r.thread().ok_or_else(bad)?)
            } else {
                None
            });
        }
        let n = r.i.len().ok_or_else(bad)?;
        let mut patches = Vec::new();
        for _ in 0..n {
            let i = r.i.u32().ok_or_else(bad)? as usize;
            let k = r.i.len().ok_or_else(bad)?;
            let mut kids = Vec::new();
            for _ in 0..k {
                let p = r.i.i32().ok_or_else(bad)?;
                kids.push((p, r.obj().ok_or_else(bad)?));
            }
            if let Some(o) = r.ext_objs.get(i) {
                patches.push((o.clone(), kids));
            }
        }
        r.finish(self).ok_or_else(bad)?;
        if globals.len() != self.globals.len() {
            return Err("the save data is from another program".into());
        }
        Ok(Context {
            globals,
            threads,
            screen,
            rand,
            next_channel,
            patches,
            looping: std::mem::take(&mut r.looping),
            active: std::mem::take(&mut r.active),
        })
    }

    /// Puts a loaded context in place of the running program.
    pub fn apply_context(&mut self, c: Context) {
        for ch in 1..=self.next_channel {
            self.sounds.push(crate::engine::SoundEvent {
                channel: format!("r{ch}"),
                data: None,
                volume: 0.0,
                repeat: false,
                fade_ms: 0,
            });
        }
        self.globals = c.globals;
        self.threads = c.threads;
        self.screen = c.screen;
        self.rand = c.rand;
        self.next_channel = self.next_channel.max(c.next_channel);
        for (o, kids) in c.patches {
            for (_, k) in &kids {
                k.borrow_mut().parent = Some(Rc::downgrade(&o));
            }
            o.borrow_mut().children = kids;
        }
        self.active = c.active.iter().map(Rc::downgrade).collect();
        self.focus = None;
        self.commands.clear();
        self.joy_pushed = [0; 16];
        self.deadline = None;
        for o in c.looping {
            o.borrow_mut().play_end = None;
            self.play_sound(&o, true);
        }
    }

    /// A variable's value with everything it refers to (File::SaveObject, the save title).
    pub fn save_value(&self, v: &Var) -> Vec<u8> {
        let mut w = Writer::new(self);
        w.var(v);
        w.finish()
    }

    pub fn load_value(&mut self, b: &[u8]) -> Option<Value> {
        let mut r = Reader::new(b, self.now);
        let v = r.var()?;
        r.finish(self)?;
        let val = v.borrow().clone();
        Some(val)
    }
}
