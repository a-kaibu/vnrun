//! Integer expressions of the scripts: `$540==0||$541==1`, `\301-\300`, `#200 == 0`,
//! `200+@`, `_EXITCODE_>=0`, `0xff`, `100%`.

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(i64),
    /// a name: `$540`, `\0`, `#200`, `_EXITCODE_`, `btn[0]`, `@`
    Name(String),
    Op(&'static str),
    Open,
    Close,
}

const OPS: [&str; 18] = [
    "==", "!=", ">=", "<=", "&&", "||", "<<", ">>", "<", ">", "+", "-", "*", "/", "%", "&", "|",
    "!",
];

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'(' {
            out.push(Tok::Open);
            i += 1;
        } else if c == b')' {
            out.push(Tok::Close);
            i += 1;
        } else if c.is_ascii_digit() {
            let st = i;
            if c == b'0' && b.get(i + 1).is_some_and(|&x| x == b'x' || x == b'X') {
                i += 2;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let v = u64::from_str_radix(&s[st + 2..i], 16).map_err(|e| e.to_string())?;
                out.push(Tok::Num(v as u32 as i32 as i64));
            } else {
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                out.push(Tok::Num(
                    s[st..i].parse().map_err(|_| format!("number in {s}"))?,
                ));
                // 100% is 100
                if b.get(i) == Some(&b'%') {
                    i += 1;
                }
            }
        } else if let Some(op) = OPS.iter().find(|op| s[i..].starts_with(**op)) {
            out.push(Tok::Op(op));
            i += op.len();
        } else {
            // a name runs to the next operator, space or parenthesis (brackets included)
            let st = i;
            let mut depth = 0;
            while i < b.len() {
                let c = b[i];
                if c == b'[' {
                    depth += 1;
                } else if c == b']' {
                    depth -= 1;
                } else if depth == 0
                    && (c.is_ascii_whitespace()
                        || c == b'('
                        || c == b')'
                        || (i > st && b"=!<>&|+-*/%".contains(&c)))
                {
                    break;
                }
                i += 1;
            }
            if i == st {
                return Err(format!("bad expression {s:?}"));
            }
            out.push(Tok::Name(s[st..i].to_string()));
        }
    }
    Ok(out)
}

/// Where names get their values.
pub trait Env {
    fn get(&mut self, name: &str) -> i64;
}

struct P<'a, E: Env + ?Sized> {
    t: Vec<Tok>,
    i: usize,
    env: &'a mut E,
}

fn prec(op: &str) -> u8 {
    match op {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "&" => 4,
        "==" | "!=" => 5,
        "<" | ">" | "<=" | ">=" => 6,
        "<<" | ">>" => 7,
        "+" | "-" => 8,
        _ => 9,
    }
}

impl<E: Env + ?Sized> P<'_, E> {
    fn unary(&mut self) -> Result<i64, String> {
        match self.t.get(self.i).cloned() {
            Some(Tok::Num(n)) => {
                self.i += 1;
                Ok(n)
            }
            Some(Tok::Name(n)) => {
                self.i += 1;
                Ok(self.env.get(&n))
            }
            Some(Tok::Op("-")) => {
                self.i += 1;
                Ok(self.unary()?.wrapping_neg())
            }
            Some(Tok::Op("+")) => {
                self.i += 1;
                self.unary()
            }
            Some(Tok::Op("!")) => {
                self.i += 1;
                Ok((self.unary()? == 0) as i64)
            }
            Some(Tok::Open) => {
                self.i += 1;
                let v = self.binary(0)?;
                if self.t.get(self.i) == Some(&Tok::Close) {
                    self.i += 1;
                }
                Ok(v)
            }
            t => Err(format!("unexpected {t:?}")),
        }
    }

    fn binary(&mut self, min: u8) -> Result<i64, String> {
        let mut l = self.unary()?;
        while let Some(Tok::Op(op)) = self.t.get(self.i).cloned() {
            let p = prec(op);
            if p <= min {
                break;
            }
            self.i += 1;
            let r = self.binary(p)?;
            l = match op {
                "||" => (l != 0 || r != 0) as i64,
                "&&" => (l != 0 && r != 0) as i64,
                "|" => l | r,
                "&" => l & r,
                "==" => (l == r) as i64,
                "!=" => (l != r) as i64,
                "<" => (l < r) as i64,
                ">" => (l > r) as i64,
                "<=" => (l <= r) as i64,
                ">=" => (l >= r) as i64,
                "<<" => l.wrapping_shl(r as u32),
                ">>" => l.wrapping_shr(r as u32),
                "+" => l.wrapping_add(r),
                "-" => l.wrapping_sub(r),
                "*" => l.wrapping_mul(r),
                "/" => {
                    if r == 0 {
                        0
                    } else {
                        l / r
                    }
                }
                "%" => {
                    if r == 0 {
                        0
                    } else {
                        l % r
                    }
                }
                _ => return Err(format!("operator {op}")),
            };
        }
        Ok(l)
    }
}

pub fn eval(s: &str, env: &mut (impl Env + ?Sized)) -> Result<i64, String> {
    let t = lex(s)?;
    if t.is_empty() {
        return Ok(0);
    }
    let mut p = P { t, i: 0, env };
    p.binary(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct V;
    impl Env for V {
        fn get(&mut self, n: &str) -> i64 {
            match n {
                "$540" => 3,
                "\\301" => 50,
                "\\300" => 20,
                "@" => 2,
                _ => -1,
            }
        }
    }

    #[test]
    fn exprs() {
        assert_eq!(eval("$540==0||$540==3||$541==1", &mut V).unwrap(), 1);
        assert_eq!(eval("\\301-\\300", &mut V).unwrap(), 30);
        assert_eq!(eval("200+@", &mut V).unwrap(), 202);
        assert_eq!(eval("1+2*3-4", &mut V).unwrap(), 3);
        assert_eq!(eval("( 10 != -1 && 0 == 0 )", &mut V).unwrap(), 1);
        assert_eq!(eval("-$540", &mut V).unwrap(), -3);
        assert_eq!(eval("0xffffffff", &mut V).unwrap(), -1);
        assert_eq!(eval("8-2-1", &mut V).unwrap(), 5);
    }
}
