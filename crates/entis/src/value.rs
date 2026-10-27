//! Cotopha values. Every variable, array element and member is a [`Var`] (a shared cell), so
//! that loading a variable pushes a reference to it and a later store changes the variable.

use std::cell::RefCell;
use std::rc::Rc;

use crate::natives::Native;

pub type Var = Rc<RefCell<Value>>;
pub type Str = Rc<str>;

#[derive(Clone, Default)]
pub enum Value {
    /// no object (an unset element)
    #[default]
    Empty,
    Int(i32),
    Real(f64),
    Str(Str),
    Array(Vec<Var>),
    /// sorted by tag; the default element for missing tags
    Hash(Vec<(Str, Var)>, Option<Box<Value>>),
    /// a script structure: class and named members in order (also the Data tables)
    Struct(Str, Vec<(Str, Var)>),
    /// a Reference: the object it refers to
    Ref(Option<Var>),
    /// an engine object (Sprite, Window, Resource, ...)
    Obj(Rc<RefCell<Native>>),
    /// `stack`, `this`, `global` or `data` as an object (dynamic variable lookup)
    Scope(u8),
}

pub fn var(v: Value) -> Var {
    Rc::new(RefCell::new(v))
}

pub fn s(t: &str) -> Value {
    Value::Str(t.into())
}

/// The variable a Reference finally refers to (the variable itself for anything else).
pub fn deref(v: &Var) -> Var {
    let mut cur = v.clone();
    for _ in 0..64 {
        let next = match &*cur.borrow() {
            Value::Ref(Some(t)) => t.clone(),
            _ => break,
        };
        cur = next;
    }
    cur
}

impl Value {
    pub fn type_name(&self) -> String {
        match self {
            Value::Empty => "".into(),
            Value::Int(_) => "Integer".into(),
            Value::Real(_) => "Real".into(),
            Value::Str(_) => "String".into(),
            Value::Array(_) => "Array".into(),
            Value::Hash(..) => "Hash".into(),
            Value::Struct(c, _) => c.to_string(),
            Value::Ref(Some(v)) => v.borrow().type_name(),
            Value::Ref(None) => "".into(),
            Value::Obj(o) => o.borrow().class.clone(),
            Value::Scope(_) => "Global".into(),
        }
    }

    pub fn int(&self) -> i32 {
        match self {
            Value::Int(i) => *i,
            Value::Real(r) => r.round() as i64 as i32,
            Value::Str(s) => parse_int(s),
            Value::Ref(Some(v)) => v.borrow().int(),
            _ => 0,
        }
    }

    pub fn real(&self) -> f64 {
        match self {
            Value::Int(i) => *i as f64,
            Value::Real(r) => *r,
            Value::Str(s) => s.trim().parse().unwrap_or(parse_int(s) as f64),
            Value::Ref(Some(v)) => v.borrow().real(),
            _ => 0.0,
        }
    }

    pub fn str(&self) -> Str {
        match self {
            Value::Str(s) => s.clone(),
            Value::Int(i) => i.to_string().into(),
            Value::Real(r) => fmt_real(*r).into(),
            Value::Ref(Some(v)) => v.borrow().str(),
            _ => "".into(),
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Empty | Value::Ref(None) => false,
            Value::Int(i) => *i != 0,
            Value::Real(r) => *r != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Ref(Some(v)) => v.borrow().truthy(),
            _ => true,
        }
    }

    /// A copy that shares no variables with the original (engine objects stay shared).
    pub fn deep(&self) -> Value {
        match self {
            Value::Array(a) => Value::Array(a.iter().map(|v| var(v.borrow().deep())).collect()),
            Value::Hash(h, d) => Value::Hash(
                h.iter()
                    .map(|(k, v)| (k.clone(), var(v.borrow().deep())))
                    .collect(),
                d.clone(),
            ),
            Value::Struct(c, m) => Value::Struct(
                c.clone(),
                m.iter()
                    .map(|(k, v)| (k.clone(), var(v.borrow().deep())))
                    .collect(),
            ),
            Value::Ref(Some(v)) => v.borrow().deep(),
            other => other.clone(),
        }
    }

    /// A member of a structure (or a parent structure's).
    pub fn member(&self, name: &str) -> Option<Var> {
        if let Value::Struct(_, m) = self {
            if let Some((_, v)) = m.iter().find(|(k, _)| &**k == name) {
                return Some(v.clone());
            }
            if let Some((_, p)) = m.iter().find(|(k, _)| &**k == "parent") {
                return p.borrow().member(name);
            }
        }
        None
    }
}

pub fn fmt_real(r: f64) -> String {
    if r == r.trunc() && r.abs() < 1e15 {
        format!("{r:.1}")
    } else {
        format!("{r}")
    }
}

/// Cotopha integers: decimal, or with an H (hex), B (binary) or O (octal) suffix.
pub fn parse_int(s: &str) -> i32 {
    let t = s.trim();
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let up = t.to_ascii_uppercase();
    let v = if let Some(h) = up.strip_suffix('H') {
        i64::from_str_radix(h, 16).ok()
    } else if let Some(h) = up.strip_prefix("0X") {
        i64::from_str_radix(h, 16).ok()
    } else {
        let digits: String = up.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            let f: f64 = t.parse().unwrap_or(0.0);
            Some(f.round() as i64)
        } else {
            digits.parse::<i64>().ok()
        }
    };
    let v = v.unwrap_or(0) as i32;
    if neg { v.wrapping_neg() } else { v }
}

/// Binary operators: add, sub, mul, div, mod, and, or, xor, logical and, logical or.
pub fn operate(op: u8, a: &Value, b: &Value) -> Result<Value, String> {
    let a = match a {
        Value::Ref(Some(v)) => v.borrow().clone(),
        _ => a.clone(),
    };
    let b = match b {
        Value::Ref(Some(v)) => v.borrow().clone(),
        _ => b.clone(),
    };
    if op == 8 {
        return Ok(Value::Int(if a.truthy() && b.truthy() { -1 } else { 0 }));
    }
    if op == 9 {
        return Ok(Value::Int(if a.truthy() || b.truthy() { -1 } else { 0 }));
    }
    Ok(match (&a, &b) {
        (Value::Str(x), _) => match op {
            0 => Value::Str(format!("{x}{}", b.str()).into()),
            2 => {
                let n = b.int();
                if n >= 0 {
                    Value::Str(x.repeat(n as usize).into())
                } else {
                    let r: String = x.chars().rev().collect();
                    Value::Str(r.repeat((-n) as usize).into())
                }
            }
            _ => return Err(format!("string operator {op}")),
        },
        (Value::Array(x), _) if op == 0 => {
            let mut v = x.clone();
            v.push(var(b.deep()));
            Value::Array(v)
        }
        (Value::Hash(x, d), Value::Hash(y, _)) if op == 0 => {
            let mut h = x.clone();
            for (k, v) in y {
                hash_set(&mut h, k.clone(), var(v.borrow().deep()));
            }
            Value::Hash(h, d.clone())
        }
        (Value::Real(_), _) | (_, Value::Real(_)) if op < 5 => {
            let (x, y) = (a.real(), b.real());
            Value::Real(match op {
                0 => x + y,
                1 => x - y,
                2 => x * y,
                3 => x / y,
                _ => x % y,
            })
        }
        _ => int_op(op, a.int(), b.int())?,
    })
}

fn int_op(op: u8, x: i32, y: i32) -> Result<Value, String> {
    Ok(Value::Int(match op {
        0 => x.wrapping_add(y),
        1 => x.wrapping_sub(y),
        2 => x.wrapping_mul(y),
        3 => {
            if y == 0 {
                return Err("division by zero".into());
            }
            x.wrapping_div(y)
        }
        4 => {
            if y == 0 {
                return Err("division by zero".into());
            }
            x.wrapping_rem(y)
        }
        5 => x & y,
        6 => x | y,
        7 => x ^ y,
        _ => return Err(format!("operator {op}")),
    }))
}

pub fn unary(op: u8, a: &Value) -> Value {
    match (op, a) {
        (0, _) => a.clone(),
        (1, Value::Real(r)) => Value::Real(-r),
        (1, _) => Value::Int(a.int().wrapping_neg()),
        (2, _) => Value::Int(!a.int()),
        _ => Value::Int(if a.truthy() { 0 } else { -1 }),
    }
}

/// Comparisons: not equal, equal, less, less or equal, greater, greater or equal.
pub fn compare(op: u8, a: &Value, b: &Value) -> bool {
    use std::cmp::Ordering;
    let a = match a {
        Value::Ref(Some(v)) => v.borrow().clone(),
        _ => a.clone(),
    };
    let b = match b {
        Value::Ref(Some(v)) => v.borrow().clone(),
        _ => b.clone(),
    };
    let ord = match (&a, &b) {
        (Value::Str(_), _) | (_, Value::Str(_)) => a.str().cmp(&b.str()),
        (Value::Real(_), _) | (_, Value::Real(_)) => {
            a.real().partial_cmp(&b.real()).unwrap_or(Ordering::Equal)
        }
        (Value::Obj(x), Value::Obj(y)) => {
            if Rc::ptr_eq(x, y) {
                Ordering::Equal
            } else {
                Ordering::Less
            }
        }
        _ => a.int().cmp(&b.int()),
    };
    match op {
        0 => ord != Ordering::Equal,
        1 => ord == Ordering::Equal,
        2 => ord == Ordering::Less,
        3 => ord != Ordering::Greater,
        4 => ord == Ordering::Greater,
        _ => ord != Ordering::Less,
    }
}

pub fn hash_find(h: &[(Str, Var)], k: &str) -> Result<usize, usize> {
    h.binary_search_by(|(t, _)| (**t).cmp(k))
}

pub fn hash_set(h: &mut Vec<(Str, Var)>, k: Str, v: Var) {
    match hash_find(h, &k) {
        Ok(i) => h[i].1 = v,
        Err(i) => h.insert(i, (k, v)),
    }
}

/// Assigns `src` to the variable `dst` the way `:=` does: into the type `dst` has.
pub fn assign(dst: &Var, src: &Var) {
    // an unset Reference takes the source itself
    {
        let is_null_ref = matches!(&*dst.borrow(), Value::Ref(None));
        if is_null_ref {
            let target = deref(src);
            if !Rc::ptr_eq(&target, dst) {
                *dst.borrow_mut() = Value::Ref(Some(target));
            }
            return;
        }
    }
    let dst = deref(dst);
    let src = deref(src);
    if Rc::ptr_eq(&dst, &src) {
        return;
    }
    let sv = src.borrow().clone();
    let nv = {
        let d = dst.borrow();
        match (&*d, &sv) {
            (Value::Int(_), Value::Int(_) | Value::Real(_) | Value::Str(_)) => Value::Int(sv.int()),
            (Value::Real(_), Value::Int(_) | Value::Real(_) | Value::Str(_)) => {
                Value::Real(sv.real())
            }
            (Value::Str(_), Value::Int(_) | Value::Real(_) | Value::Str(_)) => Value::Str(sv.str()),
            (Value::Int(_), Value::Empty) => Value::Int(0),
            (Value::Str(_), Value::Empty) => s(""),
            (Value::Obj(o), Value::Obj(p)) if !Rc::ptr_eq(o, p) => {
                // filters and input filters copy their settings; other objects are shared
                let copy = o.borrow_mut().copy_from(&p.borrow());
                if copy {
                    return;
                }
                sv.clone()
            }
            _ => sv.deep(),
        }
    };
    *dst.borrow_mut() = nv;
}
