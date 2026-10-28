//! The few script expressions the scenarios put in `{ }`: literals, the flag tables and the
//! helpers of define.ks (`isSystemFlagsEnabled(...)`, `getFlagValue(k)`), comparisons, `!`,
//! `&&` and `||`.

use std::collections::BTreeMap;

use crate::obj::Value;

pub struct Env<'a> {
    pub f: &'a BTreeMap<String, Value>,
    pub sf: &'a BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
enum T {
    Num(f64),
    Str(String),
    Id(String),
    Op(&'static str),
}

fn lex(s: &str) -> Vec<T> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    const OPS: [&str; 16] = [
        "==", "!=", "<=", ">=", "&&", "||", "::", "(", ")", "[", "]", ",", "<", ">", "!", ".",
    ];
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() || ch == ';' {
            i += 1;
        } else if ch == '"' || ch == '\'' {
            let s0 = i + 1;
            i = s0;
            while i < c.len() && c[i] != ch {
                i += 1;
            }
            out.push(T::Str(c[s0..i.min(c.len())].iter().collect()));
            i += 1;
        } else if ch.is_ascii_digit() {
            let s0 = i;
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '.') {
                i += 1;
            }
            let t: String = c[s0..i].iter().collect();
            let v = t
                .strip_prefix("0x")
                .and_then(|h| i64::from_str_radix(h, 16).ok().map(|v| v as f64))
                .or_else(|| t.parse().ok())
                .unwrap_or(0.0);
            out.push(T::Num(v));
        } else if ch.is_alphabetic() || ch == '_' {
            let s0 = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(T::Id(c[s0..i].iter().collect()));
        } else {
            let two: String = c[i..(i + 2).min(c.len())].iter().collect();
            if let Some(op) = OPS.iter().find(|o| o.len() == 2 && **o == two) {
                out.push(T::Op(op));
                i += 2;
            } else if let Some(op) = OPS.iter().find(|o| o.len() == 1 && o.starts_with(ch)) {
                out.push(T::Op(op));
                i += 1;
            } else {
                i += 1;
            }
        }
    }
    out
}

struct P<'a, 'b> {
    t: Vec<T>,
    i: usize,
    env: &'b Env<'a>,
}

impl P<'_, '_> {
    fn peek(&self) -> Option<&T> {
        self.t.get(self.i)
    }
    fn eat(&mut self, op: &str) -> bool {
        if matches!(self.peek(), Some(T::Op(o)) if *o == op) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn or(&mut self) -> Value {
        let mut l = self.and();
        while self.eat("||") {
            let r = self.and();
            l = if l.truthy() { l } else { r };
        }
        l
    }
    fn and(&mut self) -> Value {
        let mut l = self.cmp();
        while self.eat("&&") {
            let r = self.cmp();
            l = if l.truthy() { r } else { l };
        }
        l
    }
    fn cmp(&mut self) -> Value {
        let l = self.unary();
        for op in ["==", "!=", "<=", ">=", "<", ">"] {
            if self.eat(op) {
                let r = self.unary();
                let b = match (l.num(), r.num()) {
                    (Some(a), Some(b)) => match op {
                        "==" => a == b,
                        "!=" => a != b,
                        "<=" => a <= b,
                        ">=" => a >= b,
                        "<" => a < b,
                        _ => a > b,
                    },
                    _ => match op {
                        "==" => eq(&l, &r),
                        "!=" => !eq(&l, &r),
                        _ => false,
                    },
                };
                return Value::Bool(b);
            }
        }
        l
    }
    fn unary(&mut self) -> Value {
        if self.eat("!") {
            return Value::Bool(!self.unary().truthy());
        }
        self.postfix()
    }
    fn postfix(&mut self) -> Value {
        let mut v = self.primary();
        loop {
            if self.eat("[") {
                let k = self.or();
                self.eat("]");
                v = index(&v, &k);
            } else if self.eat(".") {
                if let Some(T::Id(k)) = self.peek().cloned() {
                    self.i += 1;
                    v = index(&v, &Value::Str(k));
                }
            } else {
                return v;
            }
        }
    }
    fn args(&mut self) -> Vec<Value> {
        let mut a = Vec::new();
        if self.eat(")") {
            return a;
        }
        loop {
            a.push(self.or());
            if self.eat(",") {
                continue;
            }
            self.eat(")");
            return a;
        }
    }
    fn primary(&mut self) -> Value {
        self.eat("::");
        let Some(t) = self.peek().cloned() else {
            return Value::Null;
        };
        self.i += 1;
        match t {
            T::Num(n) => Value::Float(n),
            T::Str(s) => Value::Str(s),
            T::Op("(") => {
                let v = self.or();
                self.eat(")");
                v
            }
            T::Id(id) => match id.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "null" | "undefined" => Value::Null,
                "f" => Value::Map(self.env.f.clone()),
                "sf" => Value::Map(self.env.sf.clone()),
                _ if self.eat("(") => {
                    let a = self.args();
                    match id.as_str() {
                        "isSystemFlagsEnabled" => Value::Bool(a.iter().all(|k| {
                            k.str()
                                .and_then(|k| self.env.sf.get(k))
                                .is_some_and(Value::truthy)
                        })),
                        "getFlagValue" => a
                            .first()
                            .and_then(Value::str)
                            .and_then(|k| self.env.f.get(k))
                            .cloned()
                            .unwrap_or(Value::Null),
                        _ => Value::Null,
                    }
                }
                _ => Value::Null,
            },
            _ => Value::Null,
        }
    }
}

fn eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => false,
    }
}

fn index(v: &Value, k: &Value) -> Value {
    match (v, k) {
        (Value::Map(m), Value::Str(s)) => m.get(s).cloned().unwrap_or(Value::Null),
        (Value::List(l), k) => k
            .num()
            .and_then(|i| l.get(i as usize))
            .cloned()
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

pub fn eval(src: &str, env: &Env) -> Value {
    let mut p = P {
        t: lex(src),
        i: 0,
        env,
    };
    p.or()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags() {
        let mut f = BTreeMap::new();
        f.insert("Cha01flg".to_string(), Value::Int(1));
        let mut sf = BTreeMap::new();
        sf.insert("endtry".to_string(), Value::Bool(true));
        let env = Env { f: &f, sf: &sf };
        assert!(eval(" isSystemFlagsEnabled( \"endtry\") ", &env).truthy());
        assert!(!eval("isSystemFlagsEnabled(\"endtry\", \"x\")", &env).truthy());
        assert!(eval("getFlagValue(\"Cha01flg\") == 1 ", &env).truthy());
        assert!(!eval("getFlagValue(\"Cha02flg\") == 1", &env).truthy());
        assert!(eval("f[\"Cha01flg\"] && !sf.nothing", &env).truthy());
    }
}
