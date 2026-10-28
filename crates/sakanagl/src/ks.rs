//! The scenario files (.ks): KAG-style text, `*label|title` lines, `[tag a=b]` and `@tag a=b`
//! commands, `;` comments and `{ ... }` script blocks (the TalkCompiler in system/talk). Text
//! lines run together until a tag; line breaks in the source are not kept.

/// An attribute value.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Str(String),
    Num(f64),
    Bool(bool),
    /// `{ ... }`: an expression, evaluated by the engine.
    Expr(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Label {
        name: String,
        title: String,
    },
    Tag {
        name: String,
        args: Vec<(String, Arg)>,
        vargs: Vec<Arg>,
    },
    Text(String),
    /// A `{ ... }` block between commands.
    Script(String),
}

impl Op {
    pub fn arg(&self, key: &str) -> Option<&Arg> {
        match self {
            Op::Tag { args, .. } => args.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct Script {
    pub ops: Vec<Op>,
}

impl Script {
    pub fn label(&self, name: &str) -> Option<usize> {
        let name = name.trim_start_matches('*');
        self.ops
            .iter()
            .position(|o| matches!(o, Op::Label { name: n, .. } if n == name))
    }
}

pub fn parse(src: &str) -> Script {
    let c: Vec<char> = src.chars().filter(|&c| c != '\r').collect();
    let mut ops = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    let mut line_start = true;
    let flush = |text: &mut String, ops: &mut Vec<Op>| {
        if !text.is_empty() {
            ops.push(Op::Text(std::mem::take(text)));
        }
    };
    while i < c.len() {
        let ch = c[i];
        if line_start {
            // Leading tabs and spaces before a command.
            let mut j = i;
            while j < c.len() && (c[j] == '\t' || c[j] == ' ') {
                j += 1;
            }
            match c.get(j) {
                Some(';') => {
                    while i < c.len() && c[i] != '\n' {
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                Some('*') => {
                    flush(&mut text, &mut ops);
                    let end = c[j..]
                        .iter()
                        .position(|&x| x == '\n')
                        .map_or(c.len(), |p| j + p);
                    let line: String = c[j + 1..end].iter().collect();
                    let (name, title) = line.split_once('|').unwrap_or((&line, ""));
                    ops.push(Op::Label {
                        name: name.trim().to_string(),
                        title: title.trim().trim_matches('|').to_string(),
                    });
                    i = end + 1;
                    continue;
                }
                Some('@') => {
                    flush(&mut text, &mut ops);
                    let end = c[j..]
                        .iter()
                        .position(|&x| x == '\n')
                        .map_or(c.len(), |p| j + p);
                    let body: Vec<char> = c[j + 1..end].to_vec();
                    let mut k = 0;
                    ops.push(tag(&body, &mut k));
                    i = end + 1;
                    continue;
                }
                Some('[') | Some('{') => i = j,
                Some('\n') => {
                    i = j + 1;
                    continue;
                }
                _ => {}
            }
            line_start = false;
            continue;
        }
        match ch {
            '\n' => {
                line_start = true;
                i += 1;
            }
            '[' => {
                flush(&mut text, &mut ops);
                let mut k = i + 1;
                // A tag runs to its `]`, past `{...}` and quotes.
                let mut depth = 0;
                let mut quote = None;
                while k < c.len() {
                    let x = c[k];
                    match quote {
                        Some(q) if x == q => quote = None,
                        Some(_) => {}
                        None => match x {
                            '"' | '\'' => quote = Some(x),
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            ']' if depth <= 0 => break,
                            _ => {}
                        },
                    }
                    k += 1;
                }
                let body: Vec<char> = c[i + 1..k.min(c.len())].to_vec();
                let mut p = 0;
                ops.push(tag(&body, &mut p));
                i = k + 1;
            }
            '{' => {
                flush(&mut text, &mut ops);
                let (code, end) = block(&c, i);
                ops.push(Op::Script(code));
                i = end;
            }
            '\t' => i += 1,
            _ => {
                text.push(ch);
                i += 1;
            }
        }
    }
    flush(&mut text, &mut ops);
    Script { ops }
}

/// A `{ ... }` block from `c[at]`; returns its inside and the index after it.
fn block(c: &[char], at: usize) -> (String, usize) {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut k = at;
    while k < c.len() {
        let x = c[k];
        match quote {
            Some('/') => {
                if x == '\n' {
                    quote = None;
                }
            }
            Some(q) => {
                if x == '\\' {
                    k += 1;
                } else if x == q {
                    quote = None;
                }
            }
            None => match x {
                '"' | '\'' => quote = Some(x),
                '/' if c.get(k + 1) == Some(&'/') => quote = Some('/'),
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return (c[at + 1..k].iter().collect(), k + 1);
                    }
                }
                _ => {}
            },
        }
        k += 1;
    }
    (c[(at + 1).min(c.len())..].iter().collect(), c.len())
}

/// One command's inside: `name a=b c="d" e={f} "varg"`.
fn tag(c: &[char], p: &mut usize) -> Op {
    let skip = |p: &mut usize| {
        while *p < c.len() && c[*p].is_whitespace() {
            *p += 1;
        }
    };
    skip(p);
    let start = *p;
    while *p < c.len() && !c[*p].is_whitespace() && c[*p] != '=' {
        *p += 1;
    }
    let name: String = c[start..*p].iter().collect();
    let mut args = Vec::new();
    let mut vargs = Vec::new();
    loop {
        skip(p);
        if *p >= c.len() {
            break;
        }
        if c[*p] == '"' || c[*p] == '\'' || c[*p] == '{' {
            vargs.push(value(c, p));
            continue;
        }
        let k0 = *p;
        while *p < c.len() && !c[*p].is_whitespace() && c[*p] != '=' {
            *p += 1;
        }
        let key: String = c[k0..*p].iter().collect();
        skip(p);
        if *p < c.len() && c[*p] == '=' {
            *p += 1;
            skip(p);
            let v = value(c, p);
            args.push((key, v));
        } else if !key.is_empty() {
            // A bare attribute is true.
            args.push((key, Arg::Bool(true)));
        }
    }
    Op::Tag { name, args, vargs }
}

fn value(c: &[char], p: &mut usize) -> Arg {
    match c.get(*p) {
        Some(&q @ ('"' | '\'')) => {
            let s = *p + 1;
            let mut k = s;
            while k < c.len() && c[k] != q {
                k += 1;
            }
            *p = (k + 1).min(c.len());
            Arg::Str(c[s..k.min(c.len())].iter().collect())
        }
        Some('{') => {
            let (code, end) = block(c, *p);
            *p = end;
            literal(code.trim()).unwrap_or(Arg::Expr(code.trim().to_string()))
        }
        _ => {
            let s = *p;
            // A bare value ends at a space, or where the next `key=` or `{` starts.
            while *p < c.len() && !c[*p].is_whitespace() && c[*p] != '{' {
                *p += 1;
            }
            let v: String = c[s..*p].iter().collect();
            literal(&v).unwrap_or(Arg::Str(v))
        }
    }
}

/// Numbers, booleans and quoted strings, inside `{}` or bare.
fn literal(s: &str) -> Option<Arg> {
    match s {
        "true" => return Some(Arg::Bool(true)),
        "false" => return Some(Arg::Bool(false)),
        _ => {}
    }
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(h, 16).ok().map(|v| Arg::Num(v as f64));
    }
    if let Ok(v) = s.parse::<f64>() {
        return Some(Arg::Num(v));
    }
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') && !s[1..s.len() - 1].contains('"') {
        return Some(Arg::Str(s[1..s.len() - 1].to_string()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tags_text_and_labels() {
        let s = parse(
            "*start|体験版\n[BG f=\"b.ui\" time=1]\n[transition target={true} color={0x000000ff}]\n\
             ;c\n「あ」\nい\n[cpg cn=true]\n[case \"選ぶ\"]\n{ tf[\"x\"] <- 1; }\n@jump target=*a\n",
        );
        assert_eq!(
            s.ops[0],
            Op::Label {
                name: "start".into(),
                title: "体験版".into()
            }
        );
        assert_eq!(s.ops[1].arg("f"), Some(&Arg::Str("b.ui".into())));
        assert_eq!(s.ops[2].arg("color"), Some(&Arg::Num(255.0)));
        assert_eq!(s.ops[2].arg("target"), Some(&Arg::Bool(true)));
        assert_eq!(s.ops[3], Op::Text("「あ」い".into()));
        assert_eq!(s.ops[4].arg("cn"), Some(&Arg::Bool(true)));
        match &s.ops[5] {
            Op::Tag { vargs, .. } => assert_eq!(vargs[0], Arg::Str("選ぶ".into())),
            o => panic!("{o:?}"),
        }
        assert!(matches!(&s.ops[6], Op::Script(_)));
        assert_eq!(s.ops[7].arg("target"), Some(&Arg::Str("*a".into())));
    }
}
