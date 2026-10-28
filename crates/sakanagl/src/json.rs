//! Plain JSON (the sprite sheets' .ssg, sakana.proj) into [`Value`].

use std::collections::BTreeMap;

use crate::obj::Value;

pub fn parse(s: &str) -> Result<Value, String> {
    let c: Vec<char> = s.chars().collect();
    let mut p = 0;
    value(&c, &mut p)
}

fn ws(c: &[char], p: &mut usize) {
    while *p < c.len() && c[*p].is_whitespace() {
        *p += 1;
    }
}

fn value(c: &[char], p: &mut usize) -> Result<Value, String> {
    ws(c, p);
    let Some(&ch) = c.get(*p) else {
        return Err("JSON cut short".into());
    };
    match ch {
        '{' => {
            *p += 1;
            let mut m = BTreeMap::new();
            loop {
                ws(c, p);
                match c.get(*p) {
                    Some('}') => {
                        *p += 1;
                        return Ok(Value::Map(m));
                    }
                    Some(',') => *p += 1,
                    Some('"') => {
                        let k = string(c, p)?;
                        ws(c, p);
                        if c.get(*p) != Some(&':') {
                            return Err("expected ':'".into());
                        }
                        *p += 1;
                        m.insert(k, value(c, p)?);
                    }
                    _ => return Err(format!("bad object at {p}")),
                }
            }
        }
        '[' => {
            *p += 1;
            let mut l = Vec::new();
            loop {
                ws(c, p);
                match c.get(*p) {
                    Some(']') => {
                        *p += 1;
                        return Ok(Value::List(l));
                    }
                    Some(',') => *p += 1,
                    Some(_) => l.push(value(c, p)?),
                    None => return Err("list cut short".into()),
                }
            }
        }
        '"' => Ok(Value::Str(string(c, p)?)),
        _ => {
            let s0 = *p;
            while *p < c.len() && !matches!(c[*p], ',' | ']' | '}') && !c[*p].is_whitespace() {
                *p += 1;
            }
            let t: String = c[s0..*p].iter().collect();
            Ok(match t.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "null" => Value::Null,
                _ => {
                    if let Ok(i) = t.parse::<i64>() {
                        Value::Int(i)
                    } else {
                        Value::Float(t.parse().map_err(|_| format!("bad JSON value {t}"))?)
                    }
                }
            })
        }
    }
}

fn string(c: &[char], p: &mut usize) -> Result<String, String> {
    *p += 1;
    let mut s = String::new();
    while let Some(&ch) = c.get(*p) {
        *p += 1;
        match ch {
            '"' => return Ok(s),
            '\\' => {
                let e = c.get(*p).copied().unwrap_or('\\');
                *p += 1;
                match e {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    'u' => {
                        let h: String = c[*p..(*p + 4).min(c.len())].iter().collect();
                        *p += 4;
                        if let Some(ch) = u32::from_str_radix(&h, 16).ok().and_then(char::from_u32)
                        {
                            s.push(ch);
                        }
                    }
                    e => s.push(e),
                }
            }
            ch => s.push(ch),
        }
    }
    Err("string cut short".into())
}
