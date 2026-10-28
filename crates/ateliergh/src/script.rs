//! The scripts: Shift-JIS text. `//` starts a comment, `[name]` on its own line is a label,
//! `<Name a,b,c>` is a command, and everything else is text for the message window (one
//! window line per script line). Commands may sit inside text (`<WinRubi 地上,こっち>`).

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Label(String),
    Tag(String, Vec<String>),
    Text(String),
    /// the end of a script line that had text
    Break,
}

#[derive(Debug, Default)]
pub struct Script {
    pub name: String,
    pub items: Vec<Item>,
    /// lower-case label -> index of the item after it
    pub labels: HashMap<String, usize>,
}

impl Script {
    pub fn parse(name: &str, src: &str) -> Script {
        let mut s = Script {
            name: name.to_string(),
            ..Default::default()
        };
        for line in src.lines() {
            let line = match line.find("//") {
                Some(i) => &line[..i],
                None => line,
            };
            let t = line.trim();
            if t.starts_with('[') && t.ends_with(']') && t.len() > 2 {
                let l = t[1..t.len() - 1].trim().to_string();
                s.labels.insert(l.to_lowercase(), s.items.len());
                s.items.push(Item::Label(l));
                continue;
            }
            let mut rest = line;
            let mut text = false;
            while !rest.is_empty() {
                match rest.find('<') {
                    Some(i) => {
                        let before = &rest[..i];
                        if !before.trim().is_empty() {
                            s.items.push(Item::Text(before.trim_end().to_string()));
                            text = true;
                        }
                        let after = &rest[i + 1..];
                        let Some(j) = after.find('>') else {
                            s.items.push(Item::Text(after.to_string()));
                            text = true;
                            break;
                        };
                        let body = after[..j].trim();
                        let (cmd, args) = match body.find([' ', '\t']) {
                            Some(k) => (&body[..k], body[k + 1..].trim()),
                            None => (body, ""),
                        };
                        let args = if args.is_empty() {
                            Vec::new()
                        } else {
                            args.split(',').map(|a| a.trim().to_string()).collect()
                        };
                        s.items.push(Item::Tag(cmd.to_string(), args));
                        rest = &after[j + 1..];
                    }
                    None => {
                        if !rest.trim().is_empty() {
                            s.items.push(Item::Text(rest.trim_end().to_string()));
                            text = true;
                        }
                        break;
                    }
                }
            }
            if text {
                s.items.push(Item::Break);
            }
        }
        s
    }
}

/// `(a==1)`, `(a!=0&&b>=2)`: variables (missing ones are 0) against numbers.
pub fn cond(expr: &str, var: &dyn Fn(&str) -> i32) -> bool {
    let e = expr.trim().trim_start_matches('(').trim_end_matches(')');
    if let Some((a, b)) = e.split_once("||") {
        return cond(a, var) || cond(b, var);
    }
    if let Some((a, b)) = e.split_once("&&") {
        return cond(a, var) && cond(b, var);
    }
    let val = |s: &str| {
        let s = s.trim().trim_matches(|c| c == '(' || c == ')');
        s.parse::<i32>().unwrap_or_else(|_| var(s))
    };
    for op in ["==", "!=", ">=", "<=", ">", "<", "="] {
        if let Some((a, b)) = e.split_once(op) {
            let (a, b) = (val(a), val(b));
            return match op {
                "==" | "=" => a == b,
                "!=" => a != b,
                ">=" => a >= b,
                "<=" => a <= b,
                ">" => a > b,
                _ => a < b,
            };
        }
    }
    val(e) != 0
}
