//! Script values. Variables are typed (VarNew: 1 integer, 2 float, 3 flag, 4 string) and an
//! assignment converts to the variable's type; temporaries keep what they were given.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Flag(bool),
    Str(String),
}

impl Default for Value {
    fn default() -> Value {
        Value::Str(String::new())
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Flag(b)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Value {
        Value::Int(n)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Str(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::Str(s)
    }
}

/// A number at the start of a string, as Delphi's StrToInt would read it ("" is 0).
fn parse_num(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    if let Some(h) = t.strip_prefix('$') {
        return i64::from_str_radix(h, 16).ok().map(|n| n as f64);
    }
    // Full-width digits count too.
    let t: String = t
        .chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap(),
            'ー' | '－' => '-',
            c => c,
        })
        .collect();
    t.parse::<f64>().ok()
}

impl Value {
    pub fn int(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            Value::Float(x) => *x as i64,
            Value::Flag(b) => *b as i64,
            Value::Str(s) => match s.as_str() {
                "TRUE" | "True" | "true" => 1,
                "FALSE" | "False" | "false" => 0,
                _ => parse_num(s).map_or(0, |x| x as i64),
            },
        }
    }

    pub fn float(&self) -> f64 {
        match self {
            Value::Float(x) => *x,
            Value::Str(s) => parse_num(s).unwrap_or(0.0),
            v => v.int() as f64,
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Flag(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(x) => *x != 0.0,
            Value::Str(s) => !matches!(s.as_str(), "" | "0" | "FALSE" | "False" | "false"),
        }
    }

    pub fn is_str(&self) -> bool {
        matches!(self, Value::Str(_))
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Value::Float(_))
    }

    /// The value as a variable of type `ty` holds it.
    pub fn to_type(&self, ty: u8) -> Value {
        match ty {
            1 => Value::Int(self.int()),
            2 => Value::Float(self.float()),
            3 => Value::Flag(self.truthy()),
            4 => Value::Str(self.to_string()),
            _ => self.clone(),
        }
    }

    /// The empty value of a variable type.
    pub fn zero(ty: u8) -> Value {
        match ty {
            1 => Value::Int(0),
            2 => Value::Float(0.0),
            3 => Value::Flag(false),
            _ => Value::Str(String::new()),
        }
    }

    pub fn type_of(&self) -> u8 {
        match self {
            Value::Int(_) => 1,
            Value::Float(_) => 2,
            Value::Flag(_) => 3,
            Value::Str(_) => 4,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Flag(b) => write!(f, "{}", if *b { "TRUE" } else { "FALSE" }),
            Value::Str(s) => write!(f, "{s}"),
        }
    }
}

/// A variable: its type, its value, and its elements when it was made an array.
#[derive(Debug, Clone, Default)]
pub struct Var {
    pub ty: u8,
    pub v: Value,
    pub arr: Option<Vec<Value>>,
    /// sizes of each dimension (SetArray with more than one size)
    pub dims: Vec<usize>,
}

impl Var {
    pub fn new(ty: u8, v: Value) -> Var {
        let v = if ty == 0 { v } else { v.to_type(ty) };
        Var {
            ty,
            v,
            arr: None,
            dims: Vec::new(),
        }
    }

    pub fn set(&mut self, v: Value) {
        self.v = if self.ty == 0 { v } else { v.to_type(self.ty) };
        self.arr = None;
    }

    pub fn flat(&self, idx: &[i64]) -> usize {
        if idx.len() <= 1 || self.dims.len() < idx.len() {
            return idx.first().copied().unwrap_or(0).max(0) as usize;
        }
        let mut n = 0usize;
        for (k, &i) in idx.iter().enumerate() {
            n = n * self.dims[k] + i.max(0) as usize;
        }
        n
    }

    pub fn get(&self, i: usize) -> Value {
        self.arr
            .as_ref()
            .and_then(|a| a.get(i).cloned())
            .unwrap_or_else(|| Value::zero(self.ty))
    }

    pub fn set_at(&mut self, i: usize, v: Value) {
        let ty = self.ty;
        let a = self.arr.get_or_insert_with(Vec::new);
        if a.len() <= i {
            a.resize(i + 1, Value::zero(ty));
        }
        a[i] = if ty == 0 { v } else { v.to_type(ty) };
    }

    pub fn len(&self) -> usize {
        self.arr.as_ref().map_or(0, |a| a.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
