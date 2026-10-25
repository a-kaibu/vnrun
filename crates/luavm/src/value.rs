//! Lua values and tables.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::undump::Proto;
use crate::{Lua, LuaError};

pub type TableRef = Rc<RefCell<Table>>;
pub type NativeFn = dyn Fn(&mut Lua, Vec<Value>) -> Result<Vec<Value>, LuaError>;

#[derive(Clone, Default)]
pub enum Value {
    #[default]
    Nil,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(Rc<[u8]>),
    Table(TableRef),
    Func(Rc<Closure>),
    Native(Rc<Native>),
    User(Rc<User>),
}

pub struct Native {
    pub name: &'static str,
    pub f: Box<NativeFn>,
}

/// A host object: an id the host gives meaning to, and the metatable Lua sees.
pub struct User {
    pub id: u64,
    pub kind: &'static str,
    pub meta: RefCell<Option<TableRef>>,
}

pub struct Closure {
    pub proto: Rc<Proto>,
    pub upvals: Vec<Rc<RefCell<UpVal>>>,
}

pub enum UpVal {
    /// still a register: an index into the Lua stack
    Open(usize),
    Closed(Value),
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s.as_bytes()))
    }
    pub fn bytes(b: &[u8]) -> Value {
        Value::Str(Rc::from(b))
    }
    pub fn table(t: Table) -> Value {
        Value::Table(Rc::new(RefCell::new(t)))
    }
    pub fn native(
        name: &'static str,
        f: impl Fn(&mut Lua, Vec<Value>) -> Result<Vec<Value>, LuaError> + 'static,
    ) -> Value {
        Value::Native(Rc::new(Native {
            name,
            f: Box::new(f),
        }))
    }
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "boolean",
            Value::Int(_) | Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::Table(_) => "table",
            Value::Func(_) | Value::Native(_) => "function",
            Value::User(_) => "userdata",
        }
    }
    /// A number, from a number or a numeric string.
    pub fn to_num(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Num(n) => Some(*n),
            Value::Str(s) => match parse_number(s)? {
                Value::Int(i) => Some(i as f64),
                Value::Num(n) => Some(n),
                _ => None,
            },
            _ => None,
        }
    }
    /// An integer, from an integer, an integral float or such a string.
    pub fn to_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Num(n) if n.fract() == 0.0 && n.abs() < 9.2e18 => Some(*n as i64),
            Value::Str(s) => parse_number(s)?.to_int(),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_table(&self) -> Option<&TableRef> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }
    /// Raw equality (no __eq).
    pub fn raw_eq(&self, o: &Value) -> bool {
        match (self, o) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Num(a), Value::Num(b)) => a == b,
            (Value::Int(a), Value::Num(b)) | (Value::Num(b), Value::Int(a)) => *a as f64 == *b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Table(a), Value::Table(b)) => Rc::ptr_eq(a, b),
            (Value::Func(a), Value::Func(b)) => Rc::ptr_eq(a, b),
            (Value::Native(a), Value::Native(b)) => Rc::ptr_eq(a, b),
            (Value::User(a), Value::User(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v.into())
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Num(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::str(v)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Num(n) => write!(f, "{n}"),
            Value::Str(s) => write!(f, "{:?}", String::from_utf8_lossy(s)),
            Value::Table(t) => write!(f, "table: {:p}", Rc::as_ptr(t)),
            Value::Func(c) => write!(f, "function: {:p}", Rc::as_ptr(c)),
            Value::Native(n) => write!(f, "builtin: {}", n.name),
            Value::User(u) => write!(f, "{}: {}", u.kind, u.id),
        }
    }
}

/// Lua's tonumber on a string: decimal or hex integers and floats, with spaces around.
pub fn parse_number(s: &[u8]) -> Option<Value> {
    let t = std::str::from_utf8(s).ok()?.trim();
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if let Some(h) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        if h.is_empty() || h.contains('.') || h.contains(['p', 'P']) {
            return None;
        }
        let mut v: i64 = 0;
        for c in h.chars() {
            v = v.wrapping_mul(16).wrapping_add(c.to_digit(16)? as i64);
        }
        return Some(Value::Int(if neg { v.wrapping_neg() } else { v }));
    }
    if body.is_empty() || !body.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return None;
    }
    if body.chars().all(|c| c.is_ascii_digit())
        && let Ok(i) = body.parse::<i64>() {
            return Some(Value::Int(if neg { -i } else { i }));
        }
    let n: f64 = body.parse().ok()?;
    Some(Value::Num(if neg { -n } else { n }))
}

/// A table key: numbers with an integer value are keyed as integers, as Lua does.
#[derive(Clone, Debug)]
pub struct Key(pub Value);

impl PartialEq for Key {
    fn eq(&self, o: &Key) -> bool {
        self.0.raw_eq(&o.0)
    }
}
impl Eq for Key {}
impl Hash for Key {
    fn hash<H: Hasher>(&self, h: &mut H) {
        match &self.0 {
            Value::Nil => 0u8.hash(h),
            Value::Bool(b) => b.hash(h),
            Value::Int(i) => i.hash(h),
            Value::Num(n) => n.to_bits().hash(h),
            Value::Str(s) => s.hash(h),
            Value::Table(t) => (Rc::as_ptr(t) as usize).hash(h),
            Value::Func(c) => (Rc::as_ptr(c) as usize).hash(h),
            Value::Native(n) => (Rc::as_ptr(n) as *const u8 as usize).hash(h),
            Value::User(u) => (Rc::as_ptr(u) as usize).hash(h),
        }
    }
}

/// The hash part: insertion-ordered with tombstones, so `next` keeps working while fields are
/// cleared during a traversal (which Lua allows).
#[derive(Default)]
pub struct Map {
    keys: Vec<Key>,
    vals: Vec<Value>,
    idx: HashMap<Key, usize>,
    dead: usize,
}

impl Map {
    pub fn get(&self, k: &Key) -> Option<&Value> {
        self.idx
            .get(k)
            .map(|&i| &self.vals[i])
            .filter(|v| !v.is_nil())
    }
    pub fn contains_key(&self, k: &Key) -> bool {
        self.get(k).is_some()
    }
    pub fn insert(&mut self, k: Key, v: Value) {
        if let Some(&i) = self.idx.get(&k) {
            if self.vals[i].is_nil() {
                self.dead -= 1;
            }
            self.vals[i] = v;
            return;
        }
        if self.dead > 16 && self.dead * 2 > self.keys.len() {
            self.compact();
        }
        self.idx.insert(k.clone(), self.keys.len());
        self.keys.push(k);
        self.vals.push(v);
    }
    pub fn remove(&mut self, k: &Key) -> Option<Value> {
        let &i = self.idx.get(k)?;
        if self.vals[i].is_nil() {
            return None;
        }
        self.dead += 1;
        Some(std::mem::take(&mut self.vals[i]))
    }
    fn compact(&mut self) {
        let (keys, vals) = (
            std::mem::take(&mut self.keys),
            std::mem::take(&mut self.vals),
        );
        self.idx.clear();
        self.dead = 0;
        for (k, v) in keys.into_iter().zip(vals) {
            if !v.is_nil() {
                self.idx.insert(k.clone(), self.keys.len());
                self.keys.push(k);
                self.vals.push(v);
            }
        }
    }
    /// The first live entry at or after position `i`.
    fn from(&self, i: usize) -> Option<(Value, Value)> {
        (i..self.keys.len())
            .find(|&j| !self.vals[j].is_nil())
            .map(|j| (self.keys[j].0.clone(), self.vals[j].clone()))
    }
    pub fn iter(&self) -> impl Iterator<Item = (&Key, &Value)> {
        self.keys
            .iter()
            .zip(&self.vals)
            .filter(|(_, v)| !v.is_nil())
    }
}

#[derive(Default)]
pub struct Table {
    /// values of keys 1..=arr.len()
    pub arr: Vec<Value>,
    pub map: Map,
    pub meta: Option<TableRef>,
}

fn norm(k: &Value) -> Value {
    match k {
        Value::Num(n) if n.fract() == 0.0 && n.abs() < 9.2e18 => Value::Int(*n as i64),
        k => k.clone(),
    }
}

impl Table {
    pub fn get(&self, k: &Value) -> Value {
        let k = norm(k);
        if let Value::Int(i) = k
            && i >= 1 && (i as usize) <= self.arr.len() {
                return self.arr[i as usize - 1].clone();
            }
        if k.is_nil() {
            return Value::Nil;
        }
        self.map.get(&Key(k)).cloned().unwrap_or_default()
    }
    pub fn get_str(&self, k: &str) -> Value {
        self.get(&Value::str(k))
    }
    pub fn set(&mut self, k: Value, v: Value) {
        let k = norm(&k);
        if let Value::Int(i) = k {
            let n = self.arr.len();
            if i >= 1 && (i as usize) <= n {
                self.arr[i as usize - 1] = v;
                return;
            }
            if i as usize == n + 1 && !v.is_nil() {
                self.arr.push(v);
                self.map.remove(&Key(k));
                // pull the following keys out of the hash part
                loop {
                    let next = Key(Value::Int(self.arr.len() as i64 + 1));
                    match self.map.remove(&next) {
                        Some(x) => self.arr.push(x),
                        None => break,
                    }
                }
                return;
            }
        }
        if v.is_nil() {
            self.map.remove(&Key(k));
        } else {
            self.map.insert(Key(k), v);
        }
    }
    pub fn set_str(&mut self, k: &str, v: impl Into<Value>) {
        self.set(Value::str(k), v.into());
    }
    /// A border, as Lua's # gives.
    pub fn len(&self) -> i64 {
        let mut n = self.arr.len();
        while n > 0 && self.arr[n - 1].is_nil() {
            n -= 1;
        }
        let mut n = n as i64;
        if n as usize == self.arr.len() {
            while self.map.contains_key(&Key(Value::Int(n + 1))) {
                n += 1;
            }
        }
        n
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The entry after key `k` (nil: the first); (nil, nil) at the end; None when `k` is not a
    /// key of the table.
    pub fn next(&self, k: &Value) -> Option<(Value, Value)> {
        let k = norm(k);
        let from_arr = match &k {
            Value::Nil => Some(0),
            Value::Int(i) if *i >= 1 && (*i as usize) <= self.arr.len() => Some(*i as usize),
            _ => None,
        };
        let map_at = match from_arr {
            Some(from) => {
                if let Some(j) = self.arr[from..].iter().position(|v| !v.is_nil()) {
                    return Some((
                        Value::Int((from + j) as i64 + 1),
                        self.arr[from + j].clone(),
                    ));
                }
                0
            }
            None => self.map.idx.get(&Key(k)).copied()? + 1,
        };
        Some(self.map.from(map_at).unwrap_or((Value::Nil, Value::Nil)))
    }
}
