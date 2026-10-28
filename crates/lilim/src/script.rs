//! The `.scr` scripts: Shift-JIS text, a statement per line.
//!
//! - `# ...` a comment, `::START` / `::END` the body's ends, `:NAME` a label
//! - `^word(args)` control: `gsb` / `go` (call / go to a script), `jp` (go to a label), `ret`,
//!   `if` / `elif` / `else` / `endi`, `sw` / `case` / `df` / `ends`, `macro` ... `endm`
//! - `%n = expr`, `$s = expr`: numbers and strings
//! - `word(args)` or `word`: a command (`bgon("bg001a1")`, `ud(1, 200)`, `wndoff`, ...)
//! - `[name]text` a line someone says, anything else a line of narration
//! - `/option` a choice's option; the last one ends with `//` (the answer, from 0, in `%reg`)

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Label(String),
    Ctl(String, Vec<String>),
    Assign(String, String),
    Cmd(String, Vec<String>),
    Text(Option<String>, String),
    /// an option, and whether it is the last
    Choice(String, bool),
    End,
}

pub struct Script {
    pub lines: Vec<Line>,
    /// upper-case label -> line
    pub labels: HashMap<String, usize>,
}

/// Cuts a `# comment` off, outside quotes.
fn strip_comment(s: &str) -> &str {
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => return &s[..i],
            _ => {}
        }
    }
    s
}

/// `name(a, b, "c")` -> ("name", ["a", "b", "\"c\""]); `name` -> ("name", []).
fn call(s: &str) -> (String, Vec<String>) {
    let s = s.trim();
    let Some(open) = s.find('(') else {
        let mut it = s.split_whitespace();
        let name = it.next().unwrap_or("").to_string();
        return (name, it.map(|x| x.to_string()).collect());
    };
    let name = s[..open].trim().to_string();
    let inner = s[open + 1..].trim_end();
    let inner = inner.strip_suffix(')').unwrap_or(inner);
    (name, split_args(inner))
}

/// Splits at commas outside quotes and brackets.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            '(' if !quoted => {
                depth += 1;
                cur.push(c);
            }
            ')' if !quoted => {
                depth -= 1;
                cur.push(c);
            }
            ',' if !quoted && depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

pub fn parse(bytes: &[u8]) -> Script {
    let text = encoding_rs::SHIFT_JIS.decode(bytes).0;
    let mut lines = Vec::new();
    let mut labels = HashMap::new();
    for raw in text.lines() {
        let t = raw
            .trim_start_matches([' ', '\t'])
            .trim_end_matches(['\r', ' ', '\t']);
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(rest) = t.strip_prefix("::") {
            if rest.trim().eq_ignore_ascii_case("END") {
                lines.push(Line::End);
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix(':') {
            let name = strip_comment(rest).trim().to_uppercase();
            labels.insert(name.clone(), lines.len());
            lines.push(Line::Label(name));
            continue;
        }
        if let Some(rest) = t.strip_prefix('^') {
            let (name, args) = call(strip_comment(rest));
            lines.push(Line::Ctl(name.to_lowercase(), args));
            continue;
        }
        if let Some(rest) = t.strip_prefix('/') {
            let o = strip_comment(rest).trim_end();
            match o.strip_suffix("//") {
                Some(o) => lines.push(Line::Choice(o.to_string(), true)),
                None => lines.push(Line::Choice(o.to_string(), false)),
            }
            continue;
        }
        if t.starts_with('%') || t.starts_with('$') {
            let c = strip_comment(t);
            if let Some((l, r)) = c.split_once('=')
                && !r.starts_with('=')
            {
                lines.push(Line::Assign(l.trim().to_string(), r.trim().to_string()));
                continue;
            }
        }
        if let Some(rest) = t.strip_prefix('[')
            && let Some((name, said)) = rest.split_once(']')
        {
            lines.push(Line::Text(Some(name.to_string()), said.to_string()));
            continue;
        }
        if t.starts_with(|c: char| c.is_ascii_alphabetic()) {
            let (name, args) = call(strip_comment(t));
            lines.push(Line::Cmd(name.to_lowercase(), args));
            continue;
        }
        lines.push(Line::Text(None, t.to_string()));
    }
    Script { lines, labels }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Int(i64),
    Str(String),
}

impl Val {
    pub fn int(&self) -> i64 {
        match self {
            Val::Int(n) => *n,
            Val::Str(s) => s.trim().parse().unwrap_or(0),
        }
    }
    pub fn text(&self) -> String {
        match self {
            Val::Int(n) => n.to_string(),
            Val::Str(s) => s.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(i64),
    Str(String),
    Var(String),
    Word(String),
    Op(&'static str),
}

const OPS: [&str; 19] = [
    "==", "!=", "<=", ">=", "&&", "||", "<", ">", "+", "-", "*", "/", "%", "&", "|", "(", ")", "!",
    "=",
];

fn tokens(s: &str) -> Vec<Tok> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch == '"' {
            let start = i + 1;
            i += 1;
            while i < c.len() && c[i] != '"' {
                i += 1;
            }
            out.push(Tok::Str(c[start..i.min(c.len())].iter().collect()));
            i += 1;
        } else if (ch == '%' || ch == '$')
            && c.get(i + 1)
                .is_some_and(|n| n.is_alphanumeric() || *n == '_')
        {
            let start = i;
            i += 1;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(Tok::Var(
                c[start..i].iter().collect::<String>().to_lowercase(),
            ));
        } else if ch.is_ascii_digit() {
            let start = i;
            while i < c.len() && c[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let w: String = c[start..i].iter().collect();
            let n = match w.strip_prefix("0x").or_else(|| w.strip_prefix("0X")) {
                Some(h) => i64::from_str_radix(h, 16).unwrap_or(0),
                None => w.parse().unwrap_or(0),
            };
            out.push(Tok::Num(n));
        } else if ch.is_alphanumeric() || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(Tok::Word(c[start..i].iter().collect()));
        } else {
            let rest: String = c[i..(i + 2).min(c.len())].iter().collect();
            match OPS.iter().find(|o| rest.starts_with(**o)) {
                Some(o) => {
                    out.push(Tok::Op(o));
                    i += o.chars().count();
                }
                None => i += 1,
            }
        }
    }
    out
}

/// Evaluates an argument or condition; `var` reads variables (lower case, with % or $).
pub fn eval(s: &str, var: &dyn Fn(&str) -> Val) -> Val {
    let t = tokens(s);
    let mut p = Parser { t, i: 0, var };
    p.or()
}

struct Parser<'a> {
    t: Vec<Tok>,
    i: usize,
    var: &'a dyn Fn(&str) -> Val,
}

impl Parser<'_> {
    fn peek_op(&self) -> Option<&'static str> {
        match self.t.get(self.i) {
            Some(Tok::Op(o)) => Some(o),
            _ => None,
        }
    }
    fn or(&mut self) -> Val {
        let mut l = self.and();
        while self.peek_op() == Some("||") {
            self.i += 1;
            let r = self.and();
            l = Val::Int((l.int() != 0 || r.int() != 0) as i64);
        }
        l
    }
    fn and(&mut self) -> Val {
        let mut l = self.cmp();
        while self.peek_op() == Some("&&") {
            self.i += 1;
            let r = self.cmp();
            l = Val::Int((l.int() != 0 && r.int() != 0) as i64);
        }
        l
    }
    fn cmp(&mut self) -> Val {
        let mut l = self.add();
        while let Some(op @ ("==" | "!=" | "<" | ">" | "<=" | ">=" | "=")) = self.peek_op() {
            self.i += 1;
            let r = self.add();
            let ord = match (&l, &r) {
                (Val::Str(a), Val::Str(b)) => a.cmp(b),
                _ => l.int().cmp(&r.int()),
            };
            let v = match op {
                "==" | "=" => ord.is_eq(),
                "!=" => !ord.is_eq(),
                "<" => ord.is_lt(),
                ">" => ord.is_gt(),
                "<=" => ord.is_le(),
                _ => ord.is_ge(),
            };
            l = Val::Int(v as i64);
        }
        l
    }
    fn add(&mut self) -> Val {
        let mut l = self.mul();
        while let Some(op @ ("+" | "-" | "&" | "|")) = self.peek_op() {
            self.i += 1;
            let r = self.mul();
            l = match (op, &l, &r) {
                ("+", Val::Str(a), _) => Val::Str(format!("{a}{}", r.text())),
                ("+", _, _) => Val::Int(l.int().wrapping_add(r.int())),
                ("-", _, _) => Val::Int(l.int().wrapping_sub(r.int())),
                ("&", _, _) => Val::Int(l.int() & r.int()),
                _ => Val::Int(l.int() | r.int()),
            };
        }
        l
    }
    fn mul(&mut self) -> Val {
        let mut l = self.unary();
        while let Some(op @ ("*" | "/" | "%")) = self.peek_op() {
            self.i += 1;
            let r = self.unary().int();
            let a = l.int();
            l = Val::Int(match op {
                "*" => a.wrapping_mul(r),
                "/" if r != 0 => a / r,
                "%" if r != 0 => a % r,
                _ => 0,
            });
        }
        l
    }
    fn unary(&mut self) -> Val {
        match self.peek_op() {
            Some("-") => {
                self.i += 1;
                Val::Int(-self.unary().int())
            }
            Some("!") => {
                self.i += 1;
                Val::Int((self.unary().int() == 0) as i64)
            }
            Some("(") => {
                self.i += 1;
                let v = self.or();
                if self.peek_op() == Some(")") {
                    self.i += 1;
                }
                v
            }
            _ => {
                let t = self.t.get(self.i).cloned();
                self.i += 1;
                match t {
                    Some(Tok::Num(n)) => Val::Int(n),
                    Some(Tok::Str(s)) => Val::Str(s),
                    Some(Tok::Var(v)) => (self.var)(&v),
                    Some(Tok::Word(w)) => Val::Str(w),
                    _ => Val::Int(0),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines() {
        let s2 = parse(b"/a\r\n/b//\r\n");
        assert_eq!(
            s2.lines,
            vec![
                Line::Choice("a".into(), false),
                Line::Choice("b".into(), true)
            ]
        );
        let src = "::START\r\n# c\r\nbgon(\"bg001a1\")\t# bg\r\n[ひばり]「a」\r\n　b\r\n^if(%x == 1)\r\n%x = 2\r\n:L1\r\nwndoff\r\n::END\r\n";
        let s = parse(&encoding_rs::SHIFT_JIS.encode(src).0);
        assert_eq!(
            s.lines[0],
            Line::Cmd("bgon".into(), vec!["\"bg001a1\"".into()])
        );
        assert_eq!(
            s.lines[1],
            Line::Text(Some("ひばり".into()), "「a」".into())
        );
        assert_eq!(s.lines[2], Line::Text(None, "　b".into()));
        assert_eq!(s.lines[3], Line::Ctl("if".into(), vec!["%x == 1".into()]));
        assert_eq!(s.lines[4], Line::Assign("%x".into(), "2".into()));
        assert_eq!(s.labels["L1"], 5);
        assert_eq!(s.lines[6], Line::Cmd("wndoff".into(), vec![]));
        assert_eq!(s.lines[7], Line::End);
    }

    #[test]
    fn exprs() {
        let v = |n: &str| if n == "%x" { Val::Int(3) } else { Val::Int(0) };
        assert_eq!(eval("%x == 3", &v), Val::Int(1));
        assert_eq!(eval("%x + 2 * 2", &v), Val::Int(7));
        assert_eq!(eval("0xff", &v), Val::Int(255));
        assert_eq!(eval("\"abc\"", &v), Val::Str("abc".into()));
        assert_eq!(eval("-1", &v), Val::Int(-1));
    }
}
