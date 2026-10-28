//! Tokenized files (magic `fa fa`): the editor's object files (.ui) and the scripts (.sk, .scp,
//! .conf) are stored as a token stream plus a table of names.
//!
//! ```text
//! fa fa  body_len:u32be  names:u16be  body  names (NUL-terminated)
//! body: 0x80 u16 name | 0x81 u16 string | 0x82 i32be | 0x83 f32be | 0xfd newline | 0xfe n tabs
//!       | 0x94 null | 0xb7 true | 0xb8 false | ASCII punctuation | other bytes = keywords
//! ```
//!
//! The .ui files hold one object literal, which [`parse`] turns into a [`Value`].

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(m) => m.get(key),
            _ => None,
        }
    }
    pub fn at(&self, i: usize) -> Option<&Value> {
        match self {
            Value::List(l) => l.get(i),
            _ => None,
        }
    }
    pub fn list(&self) -> &[Value] {
        match self {
            Value::List(l) => l,
            _ => &[],
        }
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            Value::Bool(b) => Some(*b as u8 as f64),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(_) | Value::List(_) | Value::Map(_) => true,
        }
    }
}

/// One token of a tokenized file.
#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Name(String),
    Str(String),
    Int(i64),
    Float(f64),
    Null,
    True,
    False,
    Newline,
    Punct(u8),
    Keyword(u8),
}

/// Whether `b` is a tokenized file.
pub fn is_tokenized(b: &[u8]) -> bool {
    b.len() >= 8 && b[0] == 0xfa && b[1] == 0xfa
}

pub fn tokens(b: &[u8]) -> Result<Vec<Tok>, String> {
    if !is_tokenized(b) {
        return Err("not a tokenized file".into());
    }
    let n = u32::from_be_bytes(b[2..6].try_into().unwrap()) as usize;
    let end = 8usize
        .checked_add(n)
        .filter(|&e| e <= b.len())
        .ok_or("token body cut short")?;
    let names: Vec<String> = b[end..]
        .split(|&c| c == 0)
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    let name = |i: usize| names.get(i).cloned().unwrap_or_default();
    let mut out = Vec::new();
    let mut p = 8;
    let need = |p: usize, k: usize| {
        if p + k <= end {
            Ok(())
        } else {
            Err("token cut short")
        }
    };
    while p < end {
        let c = b[p];
        p += 1;
        match c {
            0x80 | 0x81 => {
                need(p, 2)?;
                let i = u16::from_be_bytes([b[p], b[p + 1]]) as usize;
                p += 2;
                out.push(if c == 0x80 {
                    Tok::Name(name(i))
                } else {
                    Tok::Str(name(i))
                });
            }
            0x82 => {
                need(p, 4)?;
                out.push(Tok::Int(
                    i32::from_be_bytes(b[p..p + 4].try_into().unwrap()) as i64,
                ));
                p += 4;
            }
            0x83 => {
                need(p, 4)?;
                out.push(Tok::Float(
                    f32::from_be_bytes(b[p..p + 4].try_into().unwrap()) as f64,
                ));
                p += 4;
            }
            0x94 => out.push(Tok::Null),
            0xb7 => out.push(Tok::True),
            0xb8 => out.push(Tok::False),
            0xfd => out.push(Tok::Newline),
            0xfe => p += 1,
            c if c < 0x80 => {
                if !c.is_ascii_whitespace() {
                    out.push(Tok::Punct(c));
                }
            }
            c => out.push(Tok::Keyword(c)),
        }
    }
    Ok(out)
}

/// Parses a tokenized object literal (a .ui file).
pub fn parse(b: &[u8]) -> Result<Value, String> {
    let toks: Vec<Tok> = tokens(b)?
        .into_iter()
        .filter(|t| *t != Tok::Newline)
        .collect();
    let mut p = 0;
    value(&toks, &mut p)
}

fn value(t: &[Tok], p: &mut usize) -> Result<Value, String> {
    let tok = t.get(*p).ok_or("object cut short")?.clone();
    *p += 1;
    Ok(match tok {
        Tok::Null => Value::Null,
        Tok::True => Value::Bool(true),
        Tok::False => Value::Bool(false),
        Tok::Int(i) => Value::Int(i),
        Tok::Float(f) => Value::Float(f),
        Tok::Str(s) | Tok::Name(s) => Value::Str(s),
        Tok::Punct(b'-') => match value(t, p)? {
            Value::Int(i) => Value::Int(-i),
            Value::Float(f) => Value::Float(-f),
            v => v,
        },
        Tok::Punct(b'[') => {
            let mut l = Vec::new();
            loop {
                match t.get(*p) {
                    Some(Tok::Punct(b']')) => {
                        *p += 1;
                        break;
                    }
                    Some(Tok::Punct(b',')) => *p += 1,
                    Some(_) => l.push(value(t, p)?),
                    None => return Err("list cut short".into()),
                }
            }
            Value::List(l)
        }
        Tok::Punct(b'{') => {
            let mut m = BTreeMap::new();
            loop {
                match t.get(*p) {
                    Some(Tok::Punct(b'}')) => {
                        *p += 1;
                        break;
                    }
                    Some(Tok::Punct(b',')) => *p += 1,
                    Some(_) => {
                        let k = match value(t, p)? {
                            Value::Str(s) => s,
                            v => format!("{v:?}"),
                        };
                        match t.get(*p) {
                            Some(Tok::Punct(b':')) | Some(Tok::Punct(b'=')) => *p += 1,
                            _ => return Err(format!("expected ':' after {k}")),
                        }
                        m.insert(k, value(t, p)?);
                    }
                    None => return Err("object cut short".into()),
                }
            }
            Value::Map(m)
        }
        t => return Err(format!("unexpected token {t:?}")),
    })
}
