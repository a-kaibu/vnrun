//! The text scripts (NScripter-like, Shift-JIS): `*label` lines, commands such as
//! `BG name, 500` with `@`-joined fields, `【name,voice】「line」` speech, and plain text lines.
//! Every script file shares one label space.

use std::collections::HashMap;

pub struct Scripts {
    pub lines: Vec<String>,
    pub labels: HashMap<String, usize>,
}

impl Scripts {
    pub fn parse(files: &[Vec<u8>]) -> Scripts {
        let mut s = Scripts {
            lines: Vec::new(),
            labels: HashMap::new(),
        };
        for f in files {
            for l in nexas::sjis::decode(f).lines() {
                let l = l.trim();
                if let Some(name) = l.strip_prefix('*') {
                    s.labels
                        .entry(name.trim().to_string())
                        .or_insert(s.lines.len());
                }
                s.lines.push(l.to_string());
            }
            // a file never runs on into the next one
            s.lines.push("return".into());
        }
        s
    }
}

/// Splits at `sep` outside quotes and parentheses.
pub fn split_top(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let (mut cur, mut quote, mut depth) = (String::new(), false, 0);
    for c in s.chars() {
        match c {
            '"' => quote = !quote,
            '(' if !quote => depth += 1,
            ')' if !quote => depth -= 1,
            c if c == sep && !quote && depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

/// A command line: its name and its arguments (trimmed).
pub fn command(s: &str) -> (String, Vec<String>) {
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .unwrap_or(s.len());
    let (name, rest) = s.split_at(end);
    let rest = rest.trim();
    let args = if rest.is_empty() {
        Vec::new()
    } else {
        split_top(rest, ',')
            .into_iter()
            .map(|a| a.trim().to_string())
            .collect()
    };
    (name.to_string(), args)
}

/// Removes the quotes around a string argument.
pub fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}

pub type Vars = HashMap<String, i64>;

/// An integer expression: numbers, `%name` variables and C-like operators (`\` divides too).
pub fn eval(s: &str, vars: &Vars) -> Option<i64> {
    let toks = tokens(s)?;
    let mut p = Parser {
        t: &toks,
        i: 0,
        vars,
    };
    let v = p.binary(0)?;
    (p.i == toks.len()).then_some(v)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(i64),
    Var(String),
    Op(&'static str),
}

const OPS: [&str; 22] = [
    "||", "&&", "==", "!=", "<=", ">=", "<<", ">>", "<", ">", "+", "-", "*", "/", "\\", "%", "&",
    "|", "^", "!", "(", ")",
];

fn tokens(s: &str) -> Option<Vec<Tok>> {
    let b: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let t: String = b[start..i].iter().collect();
            let v = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                Some(h) => i64::from_str_radix(h, 16).ok()?,
                None => t.parse().ok()?,
            };
            out.push(Tok::Num(v));
        } else if c == '%'
            && b.get(i + 1)
                .is_some_and(|c| c.is_alphanumeric() || *c == '_')
        {
            let start = i + 1;
            i += 1;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            out.push(Tok::Var(b[start..i].iter().collect()));
        } else {
            let rest: String = b[i..(i + 2).min(b.len())].iter().collect();
            let op = OPS.iter().find(|o| rest.starts_with(**o))?;
            out.push(Tok::Op(op));
            i += op.chars().count();
        }
    }
    Some(out)
}

struct Parser<'a> {
    t: &'a [Tok],
    i: usize,
    vars: &'a Vars,
}

const LEVELS: [&[&str]; 10] = [
    &["||"],
    &["&&"],
    &["|"],
    &["^"],
    &["&"],
    &["==", "!="],
    &["<", "<=", ">", ">="],
    &["<<", ">>"],
    &["+", "-"],
    &["*", "/", "\\", "%"],
];

impl Parser<'_> {
    fn binary(&mut self, level: usize) -> Option<i64> {
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut v = self.binary(level + 1)?;
        while let Some(Tok::Op(op)) = self.t.get(self.i) {
            if !LEVELS[level].contains(op) {
                break;
            }
            self.i += 1;
            let r = self.binary(level + 1)?;
            v = match *op {
                "||" => (v != 0 || r != 0) as i64,
                "&&" => (v != 0 && r != 0) as i64,
                "|" => v | r,
                "^" => v ^ r,
                "&" => v & r,
                "==" => (v == r) as i64,
                "!=" => (v != r) as i64,
                "<" => (v < r) as i64,
                "<=" => (v <= r) as i64,
                ">" => (v > r) as i64,
                ">=" => (v >= r) as i64,
                "<<" => v.wrapping_shl(r as u32),
                ">>" => v.wrapping_shr(r as u32),
                "+" => v.wrapping_add(r),
                "-" => v.wrapping_sub(r),
                "*" => v.wrapping_mul(r),
                "%" => v.checked_rem(r).unwrap_or(0),
                _ => v.checked_div(r).unwrap_or(0),
            };
        }
        Some(v)
    }

    fn unary(&mut self) -> Option<i64> {
        let t = self.t.get(self.i)?.clone();
        self.i += 1;
        match t {
            Tok::Num(v) => Some(v),
            Tok::Var(n) => Some(self.vars.get(&n).copied().unwrap_or(0)),
            Tok::Op("-") => Some(-self.unary()?),
            Tok::Op("!") => Some((self.unary()? == 0) as i64),
            Tok::Op("(") => {
                let v = self.binary(0)?;
                (self.t.get(self.i) == Some(&Tok::Op(")"))).then(|| self.i += 1)?;
                Some(v)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expressions() {
        let mut v = Vars::new();
        v.insert("Height".into(), 540);
        v.insert("StH".into(), 800);
        assert_eq!(eval("%Height-%StH/2+100", &v), Some(240));
        assert_eq!(eval("%0<2 && !(3==4)", &v), Some(1));
        assert_eq!(eval("0x10>>1", &v), Some(8));
        assert_eq!(eval("-5", &v), Some(-5));
    }

    #[test]
    fn commands() {
        let (n, a) = command(r#"SELECT "a,*x", "b,*y","#);
        assert_eq!(n, "SELECT");
        assert_eq!(a, vec![r#""a,*x""#, r#""b,*y""#, ""]);
        let (n, a) = command("ST.XY 004, 500@@2, 200, 0");
        assert_eq!((n.as_str(), a.len()), ("ST.XY", 4));
    }
}
