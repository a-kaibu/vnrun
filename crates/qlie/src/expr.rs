//! Expressions of command arguments: numbers (`12`, `$FF`), "strings", variables (`a`,
//! `a[i]`, `#SystemVar`), operators (`+ - * / % == != < > <= >= & | && || !`) and, in `\cal`,
//! assignments (`a=1`, `a++`, `a+=2`). An argument that is not an expression (story text, a
//! bare word) is taken as it is.

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Str(String),
    /// an object, by number
    Obj(usize),
    Null,
}

impl Value {
    pub fn int(&self) -> i64 {
        match self {
            Value::Int(i) => *i,
            Value::Str(s) => str_int(s),
            Value::Obj(o) => *o as i64 + 1,
            Value::Null => 0,
        }
    }
    pub fn str(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Str(s) => s.clone(),
            Value::Obj(_) => String::new(),
            Value::Null => String::new(),
        }
    }
    pub fn truth(&self) -> bool {
        match self {
            Value::Int(i) => *i != 0,
            Value::Str(s) => str_int(s) != 0,
            Value::Obj(_) => true,
            Value::Null => false,
        }
    }
}

/// A number from text, 0 when it is not one (`$` for hex).
pub fn str_int(s: &str) -> i64 {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let v = if let Some(h) = s.strip_prefix('$') {
        i64::from_str_radix(h, 16).unwrap_or(0)
    } else {
        // leading digits only (StrToIntDef would give 0 for "12a"; leading digits is kinder)
        let d: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        if d.len() != s.len() {
            0
        } else {
            d.parse().unwrap_or(0)
        }
    };
    if neg { -v } else { v }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    Str(String),
    /// lower-case name, maybe with an index, and the name as written
    Var(String, Option<Box<Expr>>, String),
    Un(char, Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
    /// target, operator ("=", "+=", "-=", "++", "--"), value
    Set(Box<Expr>, &'static str, Option<Box<Expr>>),
    /// not an expression: the text itself
    Text(String),
    /// nothing (an empty argument)
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Int(i64),
    Str(String),
    Name(String),
    Op(&'static str),
}

const OPS: [&str; 26] = [
    "&&", "||", "==", "!=", "<>", "<=", ">=", "++", "--", "+=", "-=", "*=", "/=", "+", "-", "*",
    "/", "%", "<", ">", "&", "|", "!", "=", "(", ")",
];

fn name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '#' || c == '@' || c == '!' || c as u32 > 0x7f
}

fn lex(s: &str) -> Option<Vec<Tok>> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '"' {
            let j = cs[i + 1..].iter().position(|&c| c == '"')? + i + 1;
            out.push(Tok::Str(cs[i + 1..j].iter().collect()));
            i = j + 1;
            continue;
        }
        if c == '$' {
            let j = cs[i + 1..]
                .iter()
                .position(|c| !c.is_ascii_hexdigit())
                .map_or(cs.len(), |p| p + i + 1);
            let h: String = cs[i + 1..j].iter().collect();
            out.push(Tok::Int(i64::from_str_radix(&h, 16).ok()?));
            i = j;
            continue;
        }
        if c.is_ascii_digit() {
            let j = cs[i..]
                .iter()
                .position(|c| !c.is_ascii_digit())
                .map_or(cs.len(), |p| p + i);
            // "1a" is not a number nor a name
            if cs.get(j).is_some_and(|&c| name_char(c) && c != '!') {
                return None;
            }
            let d: String = cs[i..j].iter().collect();
            out.push(Tok::Int(d.parse().ok()?));
            i = j;
            continue;
        }
        if c == '[' {
            out.push(Tok::Op("["));
            i += 1;
            continue;
        }
        if c == ']' {
            out.push(Tok::Op("]"));
            i += 1;
            continue;
        }
        if name_char(c) && c != '!' {
            // a label (`@@!Name`) keeps its `!`
            let label = c == '@';
            let j = cs[i..]
                .iter()
                .position(|&c| !(name_char(c) && (c != '!' || label)))
                .map_or(cs.len(), |p| p + i);
            out.push(Tok::Name(cs[i..j].iter().collect()));
            i = j;
            continue;
        }
        let rest: String = cs[i..cs.len().min(i + 2)].iter().collect();
        let op = OPS.iter().find(|o| rest.starts_with(**o))?;
        out.push(Tok::Op(op));
        i += op.chars().count();
    }
    Some(out)
}

struct P {
    t: Vec<Tok>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn op(&mut self, o: &str) -> bool {
        if self.peek() == Some(&Tok::Op(leak(o))) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn bin(&mut self, level: usize) -> Option<Expr> {
        const LEVELS: [&[&str]; 5] = [
            &["||", "|"],
            &["&&", "&"],
            &["==", "!=", "<>", "<=", ">=", "<", ">"],
            &["+", "-"],
            &["*", "/", "%"],
        ];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut l = self.bin(level + 1)?;
        while let Some(Tok::Op(o)) = self.peek().cloned() {
            if !LEVELS[level].contains(&o) {
                break;
            }
            self.i += 1;
            let r = self.bin(level + 1)?;
            l = Expr::Bin(o, Box::new(l), Box::new(r));
        }
        Some(l)
    }
    fn unary(&mut self) -> Option<Expr> {
        if self.op("-") {
            return Some(Expr::Un('-', Box::new(self.unary()?)));
        }
        if self.op("!") {
            return Some(Expr::Un('!', Box::new(self.unary()?)));
        }
        if self.op("+") {
            return self.unary();
        }
        self.atom()
    }
    fn atom(&mut self) -> Option<Expr> {
        match self.peek()?.clone() {
            Tok::Int(v) => {
                self.i += 1;
                Some(Expr::Int(v))
            }
            Tok::Str(s) => {
                self.i += 1;
                Some(Expr::Str(s))
            }
            Tok::Name(n) => {
                self.i += 1;
                if n.starts_with("@@") {
                    return Some(Expr::Str(n));
                }
                let idx = if self.op("[") {
                    let e = self.bin(0)?;
                    if !self.op("]") {
                        return None;
                    }
                    Some(Box::new(e))
                } else {
                    None
                };
                Some(Expr::Var(n.to_lowercase(), idx, n))
            }
            Tok::Op("(") => {
                self.i += 1;
                let e = self.bin(0)?;
                if !self.op(")") {
                    return None;
                }
                Some(e)
            }
            _ => None,
        }
    }
}

fn leak(o: &str) -> &'static str {
    OPS.iter()
        .chain(["[", "]"].iter())
        .find(|x| **x == o)
        .copied()
        .unwrap_or("")
}

/// An argument: an expression, an assignment, or else the text.
pub fn parse_arg(raw: &str) -> Expr {
    let s = raw.trim();
    if s.is_empty() {
        return Expr::Empty;
    }
    let Some(t) = lex(s) else {
        return Expr::Text(s.to_string());
    };
    let mut p = P { t, i: 0 };
    // assignment: target, then = += -= ++ --
    let start = p.i;
    if let Some(target @ Expr::Var(..)) = p.atom() {
        for op in ["=", "+=", "-=", "*=", "/=", "++", "--"] {
            if p.op(op) {
                let op = leak(op);
                if op == "++" || op == "--" {
                    if p.i == p.t.len() {
                        return Expr::Set(Box::new(target), op, None);
                    }
                    break;
                }
                if p.i == p.t.len() {
                    // `a=` sets an empty value
                    return Expr::Set(
                        Box::new(target),
                        op,
                        Some(Box::new(Expr::Str(String::new()))),
                    );
                }
                if let Some(v) = p.bin(0)
                    && p.i == p.t.len()
                {
                    return Expr::Set(Box::new(target), op, Some(Box::new(v)));
                }
                return Expr::Text(s.to_string());
            }
        }
    }
    p.i = start;
    match p.bin(0) {
        Some(e) if p.i == p.t.len() => e,
        _ => Expr::Text(s.to_string()),
    }
}

/// Where variables live.
pub trait Env {
    /// The value of a variable (None: there is no such variable).
    fn get(&mut self, name: &str, idx: Option<i64>) -> Option<Value>;
    fn set(&mut self, name: &str, idx: Option<i64>, v: Value);
}

fn str_eq(a: &str, b: &str) -> bool {
    a == b || a.to_lowercase() == b.to_lowercase()
}

pub fn eval(e: &Expr, env: &mut dyn Env) -> Value {
    match e {
        Expr::Int(v) => Value::Int(*v),
        Expr::Str(s) => Value::Str(s.clone()),
        Expr::Text(s) => Value::Str(s.clone()),
        Expr::Empty => Value::Str(String::new()),
        Expr::Var(n, idx, _) => {
            let i = idx.as_ref().map(|e| eval(e, env).int());
            match env.get(n, i) {
                Some(v) => v,
                None if idx.is_none() => match n.as_str() {
                    "null" | "nil" => Value::Null,
                    "true" => Value::Int(1),
                    "false" => Value::Int(0),
                    // a bare word is its text
                    _ => Value::Str(original_case(e)),
                },
                None => Value::Int(0),
            }
        }
        Expr::Un(op, a) => {
            let v = eval(a, env);
            match op {
                '-' => Value::Int(-v.int()),
                _ => Value::Int((!v.truth()) as i64),
            }
        }
        Expr::Bin(op, a, b) => {
            let (x, y) = (eval(a, env), eval(b, env));
            let strs = matches!(x, Value::Str(_)) || matches!(y, Value::Str(_));
            let nulls = matches!(x, Value::Null | Value::Obj(_))
                || matches!(y, Value::Null | Value::Obj(_));
            let b = |c: bool| Value::Int(c as i64);
            match *op {
                "+" if strs => Value::Str(x.str() + &y.str()),
                "+" => Value::Int(x.int().wrapping_add(y.int())),
                "-" => Value::Int(x.int().wrapping_sub(y.int())),
                "*" => Value::Int(x.int().wrapping_mul(y.int())),
                "/" => {
                    let d = y.int();
                    Value::Int(if d == 0 { 0 } else { x.int() / d })
                }
                "%" => {
                    let d = y.int();
                    Value::Int(if d == 0 { 0 } else { x.int() % d })
                }
                "==" | "!=" | "<>" => {
                    let eq = if nulls {
                        x == y
                    } else if strs {
                        str_eq(&x.str(), &y.str())
                    } else {
                        x.int() == y.int()
                    };
                    b(if *op == "==" { eq } else { !eq })
                }
                "<" | ">" | "<=" | ">=" => {
                    let o = if strs && !(is_num(&x) && is_num(&y)) {
                        x.str().cmp(&y.str())
                    } else {
                        x.int().cmp(&y.int())
                    };
                    b(match *op {
                        "<" => o.is_lt(),
                        ">" => o.is_gt(),
                        "<=" => o.is_le(),
                        _ => o.is_ge(),
                    })
                }
                "&&" => b(x.truth() && y.truth()),
                "||" => b(x.truth() || y.truth()),
                "&" => Value::Int(x.int() & y.int()),
                "|" => Value::Int(x.int() | y.int()),
                _ => Value::Null,
            }
        }
        Expr::Set(t, op, v) => {
            let Expr::Var(n, idx, _) = t.as_ref() else {
                return Value::Null;
            };
            let i = idx.as_ref().map(|e| eval(e, env).int());
            let new = match (*op, v) {
                ("=", Some(v)) => eval(v, env),
                ("++", _) => Value::Int(env.get(n, i).map_or(0, |v| v.int()) + 1),
                ("--", _) => Value::Int(env.get(n, i).map_or(0, |v| v.int()) - 1),
                (op, Some(v)) => {
                    let old = env.get(n, i).unwrap_or(Value::Int(0));
                    let r = eval(v, env);
                    match op {
                        "+=" if matches!(old, Value::Str(_)) => Value::Str(old.str() + &r.str()),
                        "+=" => Value::Int(old.int() + r.int()),
                        "-=" => Value::Int(old.int() - r.int()),
                        "*=" => Value::Int(old.int() * r.int()),
                        _ => {
                            let d = r.int();
                            Value::Int(if d == 0 { 0 } else { old.int() / d })
                        }
                    }
                }
                _ => return Value::Null,
            };
            env.set(n, i, new.clone());
            new
        }
    }
}

fn is_num(v: &Value) -> bool {
    match v {
        Value::Int(_) => true,
        Value::Str(s) => {
            let s = s.trim().trim_start_matches('-');
            !s.is_empty() && (s.chars().all(|c| c.is_ascii_digit()) || s.starts_with('$'))
        }
        _ => false,
    }
}

/// A bare word as it was written (names are kept in lower case).
fn original_case(e: &Expr) -> String {
    match e {
        Expr::Var(_, _, n) => n.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct M(HashMap<String, Value>);
    impl Env for M {
        fn get(&mut self, name: &str, idx: Option<i64>) -> Option<Value> {
            self.0.get(&format!("{name}{idx:?}")).cloned()
        }
        fn set(&mut self, name: &str, idx: Option<i64>, v: Value) {
            self.0.insert(format!("{name}{idx:?}"), v);
        }
    }

    #[test]
    fn exprs() {
        let mut m = M(HashMap::new());
        let mut ev = |s: &str| eval(&parse_arg(s), &mut m);
        assert_eq!(ev("1+2*3"), Value::Int(7));
        assert_eq!(ev("$FFFF-1"), Value::Int(0xfffe));
        assert_eq!(ev(r#""a\"+1"#), Value::Str("a\\1".into()));
        assert_eq!(ev("x=5"), Value::Int(5));
        assert_eq!(ev("x++"), Value::Int(6));
        assert_eq!(ev("(x==6)&(1!=2)"), Value::Int(1));
        assert_eq!(ev("ResultInt[0]>0"), Value::Int(0));
        assert_eq!(ev("@@!Start"), Value::Str("@@!Start".into()));
        assert_eq!(ev("「ねえ」"), Value::Str("「ねえ」".into()));
        assert_eq!(ev(r#""Stop"=="stop""#), Value::Int(1));
    }
}
