//! Values, variables and the compiled expressions of instruction arguments.
//!
//! An expression is a list of tokens in postfix order, each `kind:u8 len:u16 data[len]`:
//!
//! | kind | meaning |
//! |---|---|
//! | `B` `W` `I` `L` | integer constant of 1, 2, 4, 8 bytes |
//! | `F` | 8-byte float |
//! | `M` | string constant, Shift_JIS in double quotes |
//! | `H` | variable (`@` or `$`, id:u16) |
//! | `V` ... `)` | element of an array variable: `V` starts it, the index expressions follow (with `,` between them), `)` ends it |
//! | `v` | the variable itself (for keywords that take a variable, like VARACT's SET) |
//! | `H`/`V` with `#` or `` ` `` | the address of a string / number variable or element (555: `#$S(0)`), see [`reference`] |
//! | `+ - * / %` | arithmetic (`+` also joins strings) |
//! | `= ! < > S Z` | `== != < > <= >=` |
//! | `& \|` | `&& \|\|` |
//! | `A ^` | bitwise and, xor |
//! | `R` | negation |
//! | `s` `i` | `$(...)` to string, `@(...)` to integer |

use crate::sjis;

/// Strings are Shift_JIS bytes, as in the scripts: they are joined, cut and compared as
/// bytes, and only decoded to be shown.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Flt(f64),
    Str(Vec<u8>),
}

impl Default for Value {
    fn default() -> Self {
        Value::Int(0)
    }
}

impl Value {
    /// A string value from text.
    pub fn text(s: &str) -> Value {
        Value::Str(sjis::encode(s))
    }
    pub fn int(&self) -> i64 {
        match self {
            Value::Int(i) => *i,
            Value::Flt(f) => *f as i64,
            Value::Str(s) => parse_int(&String::from_utf8_lossy(s)),
        }
    }
    pub fn flt(&self) -> f64 {
        match self {
            Value::Int(i) => *i as f64,
            Value::Flt(f) => *f,
            Value::Str(s) => String::from_utf8_lossy(s).trim().parse().unwrap_or(0.0),
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Value::Int(i) => i.to_string().into_bytes(),
            Value::Flt(f) => fmt_flt(*f).into_bytes(),
            Value::Str(s) => s.clone(),
        }
    }
    /// The value as text (strings decoded from Shift_JIS).
    pub fn str(&self) -> String {
        match self {
            Value::Str(s) => sjis::decode(s),
            v => String::from_utf8_lossy(&v.bytes()).into_owned(),
        }
    }
    pub fn truthy(&self) -> bool {
        match self {
            Value::Str(s) => !s.is_empty(),
            v => v.flt() != 0.0,
        }
    }
}

fn parse_int(s: &str) -> i64 {
    let t = s.trim();
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(h, 16).map(|v| v as i64).unwrap_or(0);
    }
    t.parse::<i64>().or_else(|_| t.parse::<f64>().map(|f| f as i64)).unwrap_or(0)
}

fn fmt_flt(f: f64) -> String {
    let s = format!("{f:.6}");
    let s = s.trim_end_matches('0');
    s.strip_suffix('.').unwrap_or(s).to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Int,
    Flt,
    Str,
}

impl Kind {
    pub fn zero(self) -> Value {
        match self {
            Kind::Int => Value::Int(0),
            Kind::Flt => Value::Flt(0.0),
            Kind::Str => Value::Str(Vec::new()),
        }
    }
    pub fn coerce(self, v: Value) -> Value {
        match (self, v) {
            (Kind::Int, v @ Value::Int(_)) => v,
            (Kind::Int, v) => Value::Int(v.int()),
            (Kind::Flt, v @ Value::Flt(_)) => v,
            (Kind::Flt, v) => Value::Flt(v.flt()),
            (Kind::Str, v) => Value::Str(v.bytes()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Var {
    pub kind: Kind,
    /// sizes of the dimensions; empty for a plain variable
    pub dims: Vec<usize>,
    pub vals: Vec<Value>,
}

impl Var {
    pub fn new(kind: Kind, dims: Vec<usize>, init: Value) -> Var {
        let n = dims.iter().product::<usize>().max(1);
        Var { kind, dims, vals: vec![kind.coerce(init); n] }
    }
    /// Flat position of an element; a plain variable ignores indices.
    pub fn slot(&self, idx: &[i64]) -> Option<usize> {
        if self.dims.is_empty() || idx.is_empty() {
            return Some(0);
        }
        let mut at = 0usize;
        for (k, &d) in self.dims.iter().enumerate() {
            let i = *idx.get(k).unwrap_or(&0);
            if i < 0 || i as usize >= d {
                return None;
            }
            at = at * d + i as usize;
        }
        Some(at)
    }
}

/// A variable id and the `@`/`$` it was written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarId {
    pub id: u16,
    pub str: bool,
}

/// Where a value can be stored: an element of a variable, or the whole variable.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub var: VarId,
    pub index: Vec<i64>,
    /// written as `v` (the variable, not one element)
    pub whole: bool,
}

pub trait Vars {
    fn get(&self, place: &Place) -> Value;
    /// Flat position of an element of a variable (for addresses).
    fn slot(&self, var: VarId, index: &[i64]) -> i64 {
        let _ = var;
        index.first().copied().unwrap_or(0)
    }
}

/// An address (`#$S(n)`, `` `@A(n) ``) as a number: the variable in the high 32 bits and the
/// flat element in the low ones, so that adding n moves n elements on.
pub fn reference(id: u16, slot: i64) -> i64 {
    (id as i64) << 32 | (slot & 0xffff_ffff)
}

/// The variable and element of an address.
pub fn dereference(r: i64) -> (u16, i64) {
    ((r >> 32) as u16, r & 0xffff_ffff)
}

#[derive(Debug, Clone)]
enum Item {
    Val(Value),
    Place(Place),
    Open(VarId),
    /// `V` with `#` or `` ` ``: the address of the element the indices name
    OpenRef(VarId),
}

fn is_ref(d: &[u8]) -> bool {
    matches!(d.first(), Some(b'#' | b'`'))
}

fn val(item: Item, vars: &dyn Vars) -> Value {
    match item {
        Item::Val(v) => v,
        Item::Place(p) => vars.get(&p),
        Item::Open(id) => vars.get(&Place { var: id, index: Vec::new(), whole: false }),
        Item::OpenRef(id) => Value::Int(reference(id.id, 0)),
    }
}

fn var_id(d: &[u8]) -> VarId {
    VarId { id: d.get(1..3).map_or(0, |b| u16::from_le_bytes([b[0], b[1]])), str: matches!(d.first(), Some(b'$' | b'#')) }
}

/// Splits an expression into its tokens.
pub fn tokens(e: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    let mut k = 0;
    std::iter::from_fn(move || {
        if k + 3 > e.len() {
            return None;
        }
        let t = e[k];
        let n = u16::from_le_bytes([e[k + 1], e[k + 2]]) as usize;
        let d = &e[(k + 3).min(e.len())..(k + 3 + n).min(e.len())];
        k += 3 + n;
        Some((t, d))
    })
}

fn run(e: &[u8], vars: &dyn Vars) -> Result<Item, String> {
    let mut st: Vec<Item> = Vec::new();
    let pop = |st: &mut Vec<Item>| st.pop().ok_or_else(|| "expression stack is empty".to_string());
    for (t, d) in tokens(e) {
        // Numbers cut short read as zero bytes.
        let mut n = [0u8; 8];
        n[..d.len().min(8)].copy_from_slice(&d[..d.len().min(8)]);
        let item = match t {
            b'B' => Item::Val(Value::Int(n[0] as i8 as i64)),
            b'W' => Item::Val(Value::Int(i16::from_le_bytes([n[0], n[1]]) as i64)),
            b'I' => Item::Val(Value::Int(i32::from_le_bytes(n[..4].try_into().unwrap()) as i64)),
            b'L' => Item::Val(Value::Int(i64::from_le_bytes(n))),
            b'F' => Item::Val(Value::Flt(f64::from_le_bytes(n))),
            b'M' => Item::Val(Value::Str(unquote(d))),
            b'H' if is_ref(d) => Item::Val(Value::Int(reference(var_id(d).id, 0))),
            b'V' if is_ref(d) => Item::OpenRef(var_id(d)),
            b'H' => Item::Place(Place { var: var_id(d), index: Vec::new(), whole: false }),
            b'v' => Item::Place(Place { var: var_id(d), index: Vec::new(), whole: true }),
            b'V' => Item::Open(var_id(d)),
            b',' => continue,
            b')' => {
                let mut index = Vec::new();
                loop {
                    match pop(&mut st)? {
                        Item::Open(var) => {
                            index.reverse();
                            break Item::Place(Place { var, index, whole: false });
                        }
                        Item::OpenRef(var) => {
                            index.reverse();
                            break Item::Val(Value::Int(reference(var.id, vars.slot(var, &index))));
                        }
                        i => index.push(val(i, vars).int()),
                    }
                }
            }
            b'R' => match val(pop(&mut st)?, vars) {
                Value::Int(i) => Item::Val(Value::Int(i.wrapping_neg())),
                v => Item::Val(Value::Flt(-v.flt())),
            },
            b's' => Item::Val(Value::Str(val(pop(&mut st)?, vars).bytes())),
            b'i' => Item::Val(Value::Int(val(pop(&mut st)?, vars).int())),
            op => {
                let b = val(pop(&mut st)?, vars);
                let a = val(pop(&mut st)?, vars);
                Item::Val(binary(op, a, b)?)
            }
        };
        st.push(item);
    }
    match st.len() {
        1 => Ok(st.pop().unwrap()),
        0 => Ok(Item::Val(Value::Int(0))),
        n => Err(format!("expression left {n} values")),
    }
}

fn unquote(d: &[u8]) -> Vec<u8> {
    for q in [b'"', b'\''] {
        if d.len() >= 2 && d[0] == q && d[d.len() - 1] == q {
            return d[1..d.len() - 1].to_vec();
        }
    }
    d.to_vec()
}

pub(crate) fn binary(op: u8, a: Value, b: Value) -> Result<Value, String> {
    use Value::*;
    let flt = matches!(a, Flt(_)) || matches!(b, Flt(_));
    let strs = matches!(a, Str(_)) || matches!(b, Str(_));
    let cmp = |o: std::cmp::Ordering| -> Value {
        Int(match op {
            b'=' => o.is_eq(),
            b'!' => o.is_ne(),
            b'<' => o.is_lt(),
            b'>' => o.is_gt(),
            b'S' => o.is_le(),
            _ => o.is_ge(),
        } as i64)
    };
    Ok(match op {
        b'+' if strs => Str([a.bytes(), b.bytes()].concat()),
        b'=' | b'!' | b'<' | b'>' | b'S' | b'Z' => {
            if strs {
                cmp(a.bytes().cmp(&b.bytes()))
            } else if flt {
                cmp(a.flt().partial_cmp(&b.flt()).unwrap_or(std::cmp::Ordering::Equal))
            } else {
                cmp(a.int().cmp(&b.int()))
            }
        }
        b'&' => Int((a.truthy() && b.truthy()) as i64),
        b'|' => Int((a.truthy() || b.truthy()) as i64),
        b'A' => Int(a.int() & b.int()),
        b'^' => Int(a.int() ^ b.int()),
        b'O' => Int(a.int() | b.int()),
        b'+' | b'-' | b'*' | b'/' | b'%' if flt => {
            let (x, y) = (a.flt(), b.flt());
            Flt(match op {
                b'+' => x + y,
                b'-' => x - y,
                b'*' => x * y,
                b'/' => {
                    if y == 0.0 {
                        0.0
                    } else {
                        x / y
                    }
                }
                _ => x % y,
            })
        }
        b'+' | b'-' | b'*' | b'/' | b'%' => {
            let (x, y) = (a.int(), b.int());
            Int(match op {
                b'+' => x.wrapping_add(y),
                b'-' => x.wrapping_sub(y),
                b'*' => x.wrapping_mul(y),
                b'/' => x.checked_div(y).unwrap_or(0),
                _ => x.checked_rem(y).unwrap_or(0),
            })
        }
        _ => return Err(format!("unknown operator {:?}", op as char)),
    })
}

/// The value of an expression.
pub fn eval(e: &[u8], vars: &dyn Vars) -> Result<Value, String> {
    Ok(val(run(e, vars)?, vars))
}

/// The place an expression names (`@A`, `$S(1,2)`), for assignments.
pub fn place(e: &[u8], vars: &dyn Vars) -> Result<Place, String> {
    match run(e, vars)? {
        Item::Place(p) => Ok(p),
        Item::Open(var) => Ok(Place { var, index: Vec::new(), whole: false }),
        Item::OpenRef(_) => Err("not a variable: an address".into()),
        Item::Val(v) => Err(format!("not a variable: {v:?}")),
    }
}

/// For declarations (`INT[@A(10,5)]`): the variable and its dimension sizes.
pub fn declared(e: &[u8], vars: &dyn Vars) -> Result<(VarId, Vec<usize>), String> {
    let p = place(e, vars)?;
    Ok((p.var, p.index.iter().map(|&i| i.max(0) as usize).collect()))
}
