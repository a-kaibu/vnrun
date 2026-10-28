//! Script values: int, float, string and the three array kinds (shared, so that a function
//! handed an array changes the caller's).

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

#[derive(Clone, Debug, Default)]
pub struct Array {
    pub dims: Vec<usize>,
    pub data: Vec<Value>,
    /// the element kind: 0 int, 1 float, 2 string
    pub kind: u8,
}

impl Array {
    pub fn new(kind: u8, dims: Vec<usize>) -> Array {
        let n = dims.iter().product::<usize>();
        Array {
            data: vec![Value::default_of(kind); n],
            dims,
            kind,
        }
    }

    /// The flat position of an element, the first index varying slowest.
    pub fn index(&self, idx: &[i32]) -> Option<usize> {
        if idx.len() > self.dims.len() || idx.is_empty() {
            return None;
        }
        let mut at = 0usize;
        for (k, d) in self.dims.iter().enumerate() {
            let i = *idx.get(k).unwrap_or(&0);
            if i < 0 || i as usize >= *d {
                return None;
            }
            at = at * d + i as usize;
        }
        Some(at)
    }
}

#[derive(Clone)]
pub enum Value {
    Int(i32),
    Float(f32),
    Str(Rc<str>),
    Arr(Rc<RefCell<Array>>),
}

impl Default for Value {
    fn default() -> Value {
        Value::Int(0)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}f"),
            Value::Str(s) => write!(f, "{s:?}"),
            Value::Arr(a) => write!(f, "arr{:?}", a.borrow().dims),
        }
    }
}

impl Value {
    /// The value of a never set variable of type `t` (majiro-py's MjoType numbers).
    pub fn default_of(t: u8) -> Value {
        match t {
            1 => Value::Float(0.0),
            2 => Value::Str("".into()),
            3..=5 => Value::Arr(Rc::new(RefCell::new(Array::new(t - 3, vec![0])))),
            _ => Value::Int(0),
        }
    }

    pub fn int(&self) -> i32 {
        match self {
            Value::Int(i) => *i,
            Value::Float(x) => *x as i32,
            Value::Str(s) => s.trim().parse().unwrap_or(0),
            Value::Arr(_) => 0,
        }
    }

    pub fn float(&self) -> f32 {
        match self {
            Value::Int(i) => *i as f32,
            Value::Float(x) => *x,
            Value::Str(s) => s.trim().parse().unwrap_or(0.0),
            Value::Arr(_) => 0.0,
        }
    }

    pub fn str(&self) -> Rc<str> {
        match self {
            Value::Str(s) => s.clone(),
            Value::Int(i) => i.to_string().into(),
            Value::Float(x) => x.to_string().into(),
            Value::Arr(_) => "".into(),
        }
    }

    pub fn string(s: impl AsRef<str>) -> Value {
        Value::Str(s.as_ref().into())
    }

    /// Converts to the kind of a variable of type `t`.
    pub fn to_kind(self, t: u8) -> Value {
        match (t, &self) {
            (0, Value::Float(_)) => Value::Int(self.int()),
            (1, Value::Int(_)) => Value::Float(self.float()),
            _ => self,
        }
    }
}
