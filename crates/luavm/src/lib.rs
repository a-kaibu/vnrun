//! A Lua virtual machine for precompiled chunks of Lua 5.1 and 5.3, written for games that ship
//! their scripts as luac output (AdvHD). There is no compiler: only `load` of binary chunks.
//!
//! Host functions are [`Value::native`] closures; host objects are [`Value::User`] with a
//! metatable. Coroutines are not supported.

mod pattern;
mod stdlib;
pub mod undump;
pub mod value;
mod vm;

use std::cell::RefCell;
use std::rc::Rc;

pub use undump::{Proto, Version};
pub use value::{Closure, Native, Table, TableRef, UpVal, User, Value};

#[derive(Debug, Clone)]
pub struct LuaError {
    /// what error() was given (a string for runtime errors)
    pub value: Value,
    /// where it was raised: "chunk:line" of each Lua frame, innermost first
    pub trace: Vec<String>,
}

impl LuaError {
    pub fn new(msg: impl Into<String>) -> LuaError {
        LuaError {
            value: Value::str(&msg.into()),
            trace: Vec::new(),
        }
    }
}

impl std::fmt::Display for LuaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.value {
            Value::Str(s) => write!(f, "{}", String::from_utf8_lossy(s))?,
            v => write!(f, "{v:?}")?,
        }
        for t in &self.trace {
            write!(f, "\n  at {t}")?;
        }
        Ok(())
    }
}

pub type LuaResult<T> = Result<T, LuaError>;

pub fn err<T>(msg: impl Into<String>) -> LuaResult<T> {
    Err(LuaError::new(msg))
}

pub struct Lua {
    pub globals: TableRef,
    /// the registers of all active Lua frames
    stack: Vec<Value>,
    /// upvalues that still point into `stack`, by stack index
    open: Vec<(usize, Rc<RefCell<UpVal>>)>,
    string_meta: TableRef,
    depth: usize,
    /// numbers print the 5.1 way (no ".0" on integral floats)
    pub v51: bool,
    /// seconds for os.clock (the host advances it)
    pub clock: f64,
    seed: u64,
    /// the host's state, for native functions
    pub host: Option<Box<dyn std::any::Any>>,
}

impl Default for Lua {
    fn default() -> Self {
        Self::new()
    }
}

impl Lua {
    pub fn new() -> Lua {
        let mut lua = Lua {
            globals: Rc::new(RefCell::new(Table::default())),
            stack: Vec::new(),
            open: Vec::new(),
            string_meta: Rc::new(RefCell::new(Table::default())),
            depth: 0,
            v51: false,
            clock: 0.0,
            seed: 0x2545_f491_4f6c_dd1d,
            host: None,
        };
        stdlib::open(&mut lua);
        lua
    }

    pub fn set_global(&mut self, name: &str, v: impl Into<Value>) {
        self.globals.borrow_mut().set_str(name, v);
    }
    pub fn global(&self, name: &str) -> Value {
        self.globals.borrow().get_str(name)
    }

    /// Loads a precompiled chunk as a function.
    pub fn load(&mut self, chunk: &[u8], name: &str) -> LuaResult<Value> {
        let proto = undump::load(chunk, name).map_err(LuaError::new)?;
        if proto.version == Version::V51 {
            self.v51 = true;
        }
        let env = Rc::new(RefCell::new(UpVal::Closed(Value::Table(
            self.globals.clone(),
        ))));
        Ok(Value::Func(Rc::new(Closure {
            upvals: (0..proto.nups.max(1)).map(|_| env.clone()).collect(),
            proto: Rc::new(proto),
        })))
    }

    /// Loads and runs a chunk.
    pub fn exec(&mut self, chunk: &[u8], name: &str) -> LuaResult<Vec<Value>> {
        let f = self.load(chunk, name)?;
        self.call(&f, Vec::new())
    }

    pub fn call(&mut self, f: &Value, args: Vec<Value>) -> LuaResult<Vec<Value>> {
        match f {
            Value::Func(c) => {
                if self.depth > 200 {
                    return err("stack overflow");
                }
                self.depth += 1;
                let r = vm::run(self, c.clone(), args);
                self.depth -= 1;
                r
            }
            Value::Native(n) => {
                let n = n.clone();
                (n.f)(self, args)
            }
            v => {
                let h = self.meta_of(v, "__call");
                if h.is_nil() {
                    return err(format!("attempt to call a {} value", v.type_name()));
                }
                let mut a = Vec::with_capacity(args.len() + 1);
                a.push(v.clone());
                a.extend(args);
                self.call(&h, a)
            }
        }
    }

    /// Calls global function `name` (if it exists).
    pub fn call_global(&mut self, name: &str, args: Vec<Value>) -> LuaResult<Vec<Value>> {
        let f = self.global(name);
        if f.is_nil() {
            return err(format!("no function {name}"));
        }
        self.call(&f, args)
    }

    pub fn metatable(&self, v: &Value) -> Option<TableRef> {
        match v {
            Value::Table(t) => t.borrow().meta.clone(),
            Value::User(u) => u.meta.borrow().clone(),
            Value::Str(_) => Some(self.string_meta.clone()),
            _ => None,
        }
    }

    fn meta_of(&self, v: &Value, event: &str) -> Value {
        self.metatable(v)
            .map(|m| m.borrow().get_str(event))
            .unwrap_or_default()
    }

    /// v[k], with __index.
    pub fn index(&mut self, v: &Value, k: &Value) -> LuaResult<Value> {
        let mut v = v.clone();
        for _ in 0..100 {
            let h = match &v {
                Value::Table(t) => {
                    let r = t.borrow().get(k);
                    if !r.is_nil() {
                        return Ok(r);
                    }
                    let h = self.meta_of(&v, "__index");
                    if h.is_nil() {
                        return Ok(Value::Nil);
                    }
                    h
                }
                _ => {
                    let h = self.meta_of(&v, "__index");
                    if h.is_nil() {
                        return err(format!(
                            "attempt to index a {} value (key {k:?})",
                            v.type_name()
                        ));
                    }
                    h
                }
            };
            match h {
                Value::Func(_) | Value::Native(_) => {
                    return Ok(self
                        .call(&h, vec![v, k.clone()])?
                        .into_iter()
                        .next()
                        .unwrap_or_default());
                }
                h => v = h,
            }
        }
        err("'__index' chain too long")
    }

    /// v[k] = x, with __newindex.
    pub fn set_index(&mut self, v: &Value, k: Value, x: Value) -> LuaResult<()> {
        let mut v = v.clone();
        for _ in 0..100 {
            let h = match &v {
                Value::Table(t) => {
                    let present = !t.borrow().get(&k).is_nil();
                    let h = if present {
                        Value::Nil
                    } else {
                        self.meta_of(&v, "__newindex")
                    };
                    if h.is_nil() {
                        if k.is_nil() {
                            return err("table index is nil");
                        }
                        if matches!(k, Value::Num(n) if n.is_nan()) {
                            return err("table index is NaN");
                        }
                        t.borrow_mut().set(k, x);
                        return Ok(());
                    }
                    h
                }
                _ => {
                    let h = self.meta_of(&v, "__newindex");
                    if h.is_nil() {
                        return err(format!(
                            "attempt to index a {} value (key {k:?})",
                            v.type_name()
                        ));
                    }
                    h
                }
            };
            match h {
                Value::Func(_) | Value::Native(_) => {
                    self.call(&h, vec![v, k, x])?;
                    return Ok(());
                }
                h => v = h,
            }
        }
        err("'__newindex' chain too long")
    }

    pub fn tostring(&mut self, v: &Value) -> LuaResult<Vec<u8>> {
        let h = self.meta_of(v, "__tostring");
        if !h.is_nil() {
            let r = self.call(&h, vec![v.clone()])?;
            return match r.into_iter().next() {
                Some(Value::Str(s)) => Ok(s.to_vec()),
                _ => err("'__tostring' must return a string"),
            };
        }
        Ok(self.tostring_raw(v))
    }

    pub fn tostring_raw(&self, v: &Value) -> Vec<u8> {
        match v {
            Value::Nil => b"nil".to_vec(),
            Value::Bool(b) => b.to_string().into_bytes(),
            Value::Int(i) => i.to_string().into_bytes(),
            Value::Num(n) => fmt_num(*n, self.v51).into_bytes(),
            Value::Str(s) => s.to_vec(),
            v => format!("{v:?}").into_bytes(),
        }
    }

    /// a == b with __eq.
    pub fn equals(&mut self, a: &Value, b: &Value) -> LuaResult<bool> {
        if a.raw_eq(b) {
            return Ok(true);
        }
        match (a, b) {
            (Value::Table(_), Value::Table(_)) | (Value::User(_), Value::User(_)) => {
                let mut h = self.meta_of(a, "__eq");
                if h.is_nil() {
                    h = self.meta_of(b, "__eq");
                }
                if h.is_nil() {
                    return Ok(false);
                }
                let r = self.call(&h, vec![a.clone(), b.clone()])?;
                Ok(r.first().is_some_and(Value::truthy))
            }
            _ => Ok(false),
        }
    }

    /// a < b (or a <= b).
    pub fn less(&mut self, a: &Value, b: &Value, or_equal: bool) -> LuaResult<bool> {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(if or_equal { x <= y } else { x < y }),
            (Value::Int(_) | Value::Num(_), Value::Int(_) | Value::Num(_)) => {
                let (x, y) = (a.to_num().unwrap(), b.to_num().unwrap());
                Ok(if or_equal { x <= y } else { x < y })
            }
            (Value::Str(x), Value::Str(y)) => Ok(if or_equal { x <= y } else { x < y }),
            _ => {
                let ev = if or_equal { "__le" } else { "__lt" };
                let mut h = self.meta_of(a, ev);
                if h.is_nil() {
                    h = self.meta_of(b, ev);
                }
                if h.is_nil() {
                    return err(format!(
                        "attempt to compare {} with {}",
                        a.type_name(),
                        b.type_name()
                    ));
                }
                let r = self.call(&h, vec![a.clone(), b.clone()])?;
                Ok(r.first().is_some_and(Value::truthy))
            }
        }
    }

    /// #v
    pub fn len(&mut self, v: &Value) -> LuaResult<Value> {
        match v {
            Value::Str(s) => Ok(Value::Int(s.len() as i64)),
            _ => {
                let h = self.meta_of(v, "__len");
                if !h.is_nil() {
                    return Ok(self
                        .call(&h, vec![v.clone()])?
                        .into_iter()
                        .next()
                        .unwrap_or_default());
                }
                match v {
                    Value::Table(t) => Ok(Value::Int(t.borrow().len())),
                    v => err(format!(
                        "attempt to get length of a {} value",
                        v.type_name()
                    )),
                }
            }
        }
    }

    pub fn arith(&mut self, op: Arith, a: &Value, b: &Value) -> LuaResult<Value> {
        if let Some(v) = arith_raw(op, a, b) {
            return Ok(v);
        }
        let ev = op.event();
        let mut h = self.meta_of(a, ev);
        if h.is_nil() {
            h = self.meta_of(b, ev);
        }
        if h.is_nil() {
            let bad = if a.to_num().is_none() { a } else { b };
            return err(format!(
                "attempt to perform arithmetic ({ev}) on a {} value",
                bad.type_name()
            ));
        }
        Ok(self
            .call(&h, vec![a.clone(), b.clone()])?
            .into_iter()
            .next()
            .unwrap_or_default())
    }

    pub fn concat(&mut self, a: &Value, b: &Value) -> LuaResult<Value> {
        let s = |v: &Value, lua: &Lua| match v {
            Value::Str(_) | Value::Int(_) | Value::Num(_) => Some(lua.tostring_raw(v)),
            _ => None,
        };
        if let (Some(x), Some(y)) = (s(a, self), s(b, self)) {
            let mut x = x;
            x.extend_from_slice(&y);
            return Ok(Value::bytes(&x));
        }
        let mut h = self.meta_of(a, "__concat");
        if h.is_nil() {
            h = self.meta_of(b, "__concat");
        }
        if h.is_nil() {
            let bad = if s(a, self).is_none() { a } else { b };
            return err(format!(
                "attempt to concatenate a {} value",
                bad.type_name()
            ));
        }
        Ok(self
            .call(&h, vec![a.clone(), b.clone()])?
            .into_iter()
            .next()
            .unwrap_or_default())
    }
}

pub fn fmt_num(n: f64, v51: bool) -> String {
    if n.is_nan() {
        return if n.is_sign_negative() { "-nan" } else { "nan" }.into();
    }
    if n.is_infinite() {
        return if n < 0.0 { "-inf" } else { "inf" }.into();
    }
    let s = stdlib::fmt_g(n, 14, false);
    if !v51 && n.fract() == 0.0 && !s.contains(['e', '.', 'n', 'i']) {
        format!("{s}.0")
    } else {
        s
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Mod,
    Pow,
    Div,
    IDiv,
    BAnd,
    BOr,
    BXor,
    Shl,
    Shr,
    Unm,
    BNot,
}

impl Arith {
    fn event(self) -> &'static str {
        match self {
            Arith::Add => "__add",
            Arith::Sub => "__sub",
            Arith::Mul => "__mul",
            Arith::Mod => "__mod",
            Arith::Pow => "__pow",
            Arith::Div => "__div",
            Arith::IDiv => "__idiv",
            Arith::BAnd => "__band",
            Arith::BOr => "__bor",
            Arith::BXor => "__bxor",
            Arith::Shl => "__shl",
            Arith::Shr => "__shr",
            Arith::Unm => "__unm",
            Arith::BNot => "__bnot",
        }
    }
}

fn num_or_int(v: &Value) -> Option<Value> {
    match v {
        Value::Int(_) | Value::Num(_) => Some(v.clone()),
        Value::Str(s) => value::parse_number(s),
        _ => None,
    }
}

fn shift_left(x: i64, n: i64) -> i64 {
    if n <= -64 || n >= 64 {
        0
    } else if n >= 0 {
        ((x as u64) << n) as i64
    } else {
        ((x as u64) >> -n) as i64
    }
}

/// Arithmetic on numbers (and numeric strings); None when a metamethod is needed.
pub fn arith_raw(op: Arith, a: &Value, b: &Value) -> Option<Value> {
    let (a, b) = (num_or_int(a)?, num_or_int(b)?);
    use Arith::*;
    if matches!(op, BAnd | BOr | BXor | Shl | Shr | BNot) {
        let (x, y) = (a.to_int()?, b.to_int()?);
        return Some(Value::Int(match op {
            BAnd => x & y,
            BOr => x | y,
            BXor => x ^ y,
            Shl => shift_left(x, y),
            Shr => shift_left(x, y.wrapping_neg()),
            _ => !x,
        }));
    }
    if let (Value::Int(x), Value::Int(y)) = (&a, &b) {
        let (x, y) = (*x, *y);
        match op {
            Add => return Some(Value::Int(x.wrapping_add(y))),
            Sub => return Some(Value::Int(x.wrapping_sub(y))),
            Mul => return Some(Value::Int(x.wrapping_mul(y))),
            Unm => return Some(Value::Int(x.wrapping_neg())),
            Mod if y != 0 => {
                let r = x.wrapping_rem(y);
                return Some(Value::Int(if r != 0 && (r ^ y) < 0 { r + y } else { r }));
            }
            IDiv if y != 0 => {
                let q = x.wrapping_div(y);
                return Some(Value::Int(if (x % y != 0) && ((x ^ y) < 0) {
                    q - 1
                } else {
                    q
                }));
            }
            Mod | IDiv => return None,
            _ => {}
        }
    }
    let (x, y) = (a.to_num()?, b.to_num()?);
    Some(Value::Num(match op {
        Add => x + y,
        Sub => x - y,
        Mul => x * y,
        Div => x / y,
        Pow => x.powf(y),
        Unm => -x,
        IDiv => (x / y).floor(),
        Mod => {
            if y.is_infinite() && x.is_finite() {
                if (x >= 0.0) == (y > 0.0) { x } else { y }
            } else {
                let r = x % y;
                if r != 0.0 && (r < 0.0) != (y < 0.0) {
                    r + y
                } else {
                    r
                }
            }
        }
        _ => unreachable!(),
    }))
}

#[cfg(test)]
mod tests;
