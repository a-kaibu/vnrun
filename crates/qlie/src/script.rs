//! .s scripts: text (Shift_JIS, or UTF-16 with a byte order mark) of lines.
//!
//! - `@@Label`, `@@!Label`: a place to go to (the `!` is part of the name);
//! - `@@@!file`, `@@@file`: the lines of another file go here (`!`: only once);
//! - `\cmd,arg,arg...`: commands, several to a line; each `\` outside quotes starts one;
//! - `^cmd,...`: a story command, which the `rep` object turns into commands;
//! - any other line: story text, handled by the `rep` object's `\et`; an empty line ends a page.
//!
//! `//` starts a comment. `\if`, `\for` and `\case` blocks end with `\end`; an `\if ... \then`
//! with commands after `\then` on its line ends at the end of the line.

use std::collections::HashMap;

use crate::expr::{Expr, parse_arg};

pub fn decode_sjis(b: &[u8]) -> String {
    nexas::sjis::decode(b)
}

/// The text of a script file.
pub fn decode(b: &[u8]) -> String {
    if b.starts_with(&[0xff, 0xfe]) {
        let u: Vec<u16> = b[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u)
    } else if b.starts_with(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8_lossy(&b[3..]).into_owned()
    } else {
        decode_sjis(b)
    }
}

#[derive(Debug, Clone)]
pub struct Arg {
    pub raw: String,
    pub expr: Expr,
}

#[derive(Debug, Clone)]
pub enum Kind {
    /// lower-case name (`?mc` included) and its arguments
    Cmd(String, Vec<Arg>),
    /// a line of story text
    Text(String),
    /// an empty line
    Blank,
    /// a `^` line: the name (lower case, with `^`) and the rest after it
    Story(String, String),
    Label(String),
}

#[derive(Debug, Clone)]
pub struct Op {
    /// the file it comes from (index into `Unit::files`)
    pub file: u16,
    /// from 1
    pub line: u32,
    pub kind: Kind,
}

/// How an op of a block goes on.
#[derive(Debug, Clone, Copy)]
pub enum Block {
    /// `\if`: where to go when false, where its block ends (after it)
    If { otherwise: usize, after: usize },
    /// `\else` of an `\if`, `\ans` of a `\case` reached from the arm above: go after the block
    Skip { after: usize },
    /// `\for`: after the block
    For { after: usize },
    /// `\end` of a `\for`: back to the `\for`
    Loop { head: usize },
    /// `\case`: its arms (the `\ans`/`\of`/`\else` ops) and after the block
    Case { after: usize },
    /// `\brk`: after the innermost loop (or case)
    Break { after: usize },
    /// `\cnt`: to the `\end` of the innermost loop (its step and test)
    Continue { end: usize },
}

#[derive(Debug, Default)]
pub struct Unit {
    /// the main file first, then what it includes (paths as named, `\` separated)
    pub files: Vec<String>,
    pub ops: Vec<Op>,
    /// lower-case label (without `@@`) -> op
    pub labels: HashMap<String, usize>,
    pub blocks: HashMap<usize, Block>,
    /// `\case` op -> its arms
    pub arms: HashMap<usize, Vec<usize>>,
}

/// Splits at `sep` outside quotes, brackets, parentheses and braces.
pub fn split_top(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut quote = false;
    for c in s.chars() {
        if quote {
            cur.push(c);
            if c == '"' {
                quote = false;
            }
            continue;
        }
        match c {
            '"' => {
                quote = true;
                cur.push(c);
            }
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                cur.push(c);
            }
            c if c == sep && depth <= 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// The line without its `//` comment (outside quotes).
fn strip_comment(s: &str) -> &str {
    let mut quote = false;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => quote = !quote,
            b'/' if !quote && b.get(i + 1) == Some(&b'/') => return &s[..i],
            _ => {}
        }
        i += 1;
    }
    s
}

/// The commands of a line of `\` commands.
pub fn split_cmds(s: &str) -> Vec<(String, Vec<String>)> {
    let mut cmds = Vec::new();
    let mut cur: Option<String> = None;
    let mut quote = false;
    let mut brace = 0;
    for c in s.chars() {
        if !quote && brace == 0 && c == '\\' {
            if let Some(t) = cur.take() {
                cmds.push(t);
            }
            cur = Some(String::new());
            continue;
        }
        match c {
            '"' => quote = !quote,
            '{' if !quote => brace += 1,
            '}' if !quote && brace > 0 => brace -= 1,
            _ => {}
        }
        if let Some(t) = cur.as_mut() {
            t.push(c);
        }
    }
    if let Some(t) = cur.take() {
        cmds.push(t);
    }
    cmds.into_iter()
        .map(|t| {
            let t = t.trim_end();
            let mut parts = split_top(t, ',');
            let name = parts.remove(0).trim().to_lowercase();
            let args = if t.contains(',') {
                parts.into_iter().map(|a| a.trim().to_string()).collect()
            } else {
                Vec::new()
            };
            (name, args)
        })
        .collect()
}

pub fn make_args(args: Vec<String>) -> Vec<Arg> {
    args.into_iter()
        .map(|raw| Arg {
            expr: parse_arg(&raw),
            raw,
        })
        .collect()
}

/// Finds an included file: (the file that includes it, its name) -> its path and text.
pub type Loader<'a> = dyn FnMut(&str, &str) -> Option<(String, String)> + 'a;

/// Reads one file's lines into `unit`; `load` gives the text of an included file.
fn add_file(unit: &mut Unit, path: &str, text: &str, load: &mut Loader, depth: u32) {
    let file = unit.files.len() as u16;
    unit.files.push(path.to_string());
    for (n, raw) in text.lines().enumerate() {
        let line = n as u32 + 1;
        let s = raw.trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace());
        let push = |unit: &mut Unit, kind| unit.ops.push(Op { file, line, kind });
        if s.is_empty() {
            push(unit, Kind::Blank);
            continue;
        }
        if let Some(inc) = s.strip_prefix("@@@") {
            let once = inc.starts_with('!');
            let inc = strip_comment(inc.trim_start_matches('!')).trim();
            if depth > 16 {
                continue;
            }
            if let Some((found, t)) = load(path, inc) {
                let key = crate::game::norm(&found);
                if once && unit.files.iter().any(|f| crate::game::norm(f) == key) {
                    continue;
                }
                add_file(unit, &found, &t, load, depth + 1);
            } else {
                eprintln!("qlie: {path}:{line}: no {inc} to include");
            }
            continue;
        }
        if let Some(l) = s.strip_prefix("@@") {
            let l = strip_comment(l).trim();
            let l = l.split_whitespace().next().unwrap_or("").to_lowercase();
            let at = unit.ops.len();
            unit.labels.entry(l.clone()).or_insert(at);
            push(unit, Kind::Label(l));
            continue;
        }
        let code = strip_comment(s).trim();
        if code.is_empty() {
            // a comment alone is not an empty line
            continue;
        }
        if code.starts_with('\\') {
            for (name, args) in split_cmds(code) {
                push(unit, Kind::Cmd(name, make_args(args)));
            }
        } else if let Some(rest) = code.strip_prefix('^') {
            let (name, rest) = match rest.find(',') {
                Some(i) => (&rest[..i], &rest[i + 1..]),
                None => (rest, ""),
            };
            push(
                unit,
                Kind::Story(format!("^{}", name.trim().to_lowercase()), rest.to_string()),
            );
        } else {
            // story text keeps its `//`? Text has no comments in practice.
            push(unit, Kind::Text(code.to_string()));
        }
    }
}

impl Unit {
    pub fn load(path: &str, text: &str, load: &mut Loader) -> Unit {
        let mut u = Unit::default();
        add_file(&mut u, path, text, load, 0);
        u.match_blocks();
        u
    }

    fn match_blocks(&mut self) {
        #[derive(Clone, Copy)]
        enum Open {
            If {
                at: usize,
                // the line it ends with, for one-line ifs
                line: Option<(u16, u32)>,
                otherwise: Option<usize>,
                elses: usize,
            },
            For(usize),
            Case(usize),
        }
        let mut stack: Vec<Open> = Vec::new();
        // \else ops and \ans ops waiting for their block's end
        let mut skips: Vec<Vec<usize>> = Vec::new();
        let mut breaks: Vec<Vec<usize>> = Vec::new();
        let n = self.ops.len();
        // \cnt ops and the \for they go on with
        let mut conts: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        // closes the top block, which ends just before op `end` (`after` goes after it)
        fn close(
            u: &mut Unit,
            o: Open,
            after: usize,
            skips: Vec<usize>,
            brks: Vec<usize>,
            end_op: Option<usize>,
        ) {
            match o {
                Open::If { at, otherwise, .. } => {
                    u.blocks.insert(
                        at,
                        Block::If {
                            otherwise: otherwise.unwrap_or(after),
                            after,
                        },
                    );
                }
                Open::For(at) => {
                    u.blocks.insert(at, Block::For { after });
                    if let Some(e) = end_op {
                        u.blocks.insert(e, Block::Loop { head: at });
                    }
                }
                Open::Case(at) => {
                    u.blocks.insert(at, Block::Case { after });
                }
            }
            for s in skips {
                u.blocks.insert(s, Block::Skip { after });
            }
            for b in brks {
                u.blocks.insert(b, Block::Break { after });
            }
        }
        while i < n {
            let (file, line) = (self.ops[i].file, self.ops[i].line);
            // one-line ifs end at the end of their line
            while let Some(&Open::If {
                line: Some(l), at, ..
            }) = stack.last()
            {
                if l == (file, line) && i > at {
                    break;
                }
                let o = stack.pop().unwrap();
                let s = skips.pop().unwrap();
                let b = breaks.pop().unwrap();
                close(self, o, i, s, b, None);
            }
            let name = match &self.ops[i].kind {
                Kind::Cmd(name, _) => name.as_str(),
                _ => {
                    i += 1;
                    continue;
                }
            };
            match name {
                "if" => {
                    // `\then` and more commands on the same line: a one-line if
                    let then = self.ops[i + 1..]
                        .iter()
                        .take_while(|o| o.file == file && o.line == line)
                        .position(|o| matches!(&o.kind, Kind::Cmd(c, _) if c == "then"));
                    let one = then.is_some_and(|t| {
                        self.ops
                            .get(i + 2 + t)
                            .is_some_and(|o| o.file == file && o.line == line)
                    });
                    stack.push(Open::If {
                        at: i,
                        line: one.then_some((file, line)),
                        otherwise: None,
                        elses: 0,
                    });
                    skips.push(Vec::new());
                    breaks.push(Vec::new());
                }
                "for" => {
                    stack.push(Open::For(i));
                    skips.push(Vec::new());
                    breaks.push(Vec::new());
                }
                "case" => {
                    stack.push(Open::Case(i));
                    skips.push(Vec::new());
                    breaks.push(Vec::new());
                    self.arms.insert(i, Vec::new());
                }
                "else" => match stack.last_mut() {
                    Some(Open::If {
                        otherwise, elses, ..
                    }) => {
                        if *elses == 0 {
                            *otherwise = Some(i + 1);
                        }
                        *elses += 1;
                        skips.last_mut().unwrap().push(i);
                    }
                    Some(Open::Case(c)) => {
                        let c = *c;
                        self.arms.get_mut(&c).unwrap().push(i);
                        skips.last_mut().unwrap().push(i);
                    }
                    _ => {}
                },
                "ans" | "of" => {
                    if let Some(Open::Case(c)) = stack.last() {
                        let c = *c;
                        self.arms.get_mut(&c).unwrap().push(i);
                        skips.last_mut().unwrap().push(i);
                    }
                }
                "brk" => {
                    // the innermost loop, else the innermost case
                    let k = stack
                        .iter()
                        .rposition(|o| matches!(o, Open::For(_)))
                        .or_else(|| stack.iter().rposition(|o| matches!(o, Open::Case(_))));
                    if let Some(k) = k {
                        breaks[k].push(i);
                    }
                }
                "cnt" => {
                    if let Some(Open::For(at)) =
                        stack.iter().rev().find(|o| matches!(o, Open::For(_)))
                    {
                        conts.push((i, *at));
                    }
                }
                "end" => {
                    if let Some(o) = stack.pop() {
                        let s = skips.pop().unwrap();
                        let b = breaks.pop().unwrap();
                        close(self, o, i + 1, s, b, Some(i));
                    }
                }
                _ => {}
            }
            i += 1;
        }
        while let Some(o) = stack.pop() {
            let s = skips.pop().unwrap();
            let b = breaks.pop().unwrap();
            close(self, o, n, s, b, None);
        }
        for (c, head) in conts {
            let end = self
                .blocks
                .iter()
                .find(|(_, b)| matches!(b, Block::Loop { head: h } if *h == head))
                .map(|(e, _)| *e);
            if let Some(end) = end {
                self.blocks.insert(c, Block::Continue { end });
            }
        }
    }

    pub fn label(&self, name: &str) -> Option<usize> {
        let name = name.trim().trim_start_matches("@@").to_lowercase();
        self.labels.get(&name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks() {
        let src = "@@TOP\n\\if,a==1\\then\n\\cal,b=1\n\\else\n\\cal,b=2\n\\end\n\\if,c\\then \\cal,d=1\n\\cal,e=1\n";
        let u = Unit::load("t.s", src, &mut |_, _| None);
        // ops: label, if, then, cal, else, cal, end, if, then, cal, cal
        assert!(matches!(
            u.blocks[&1],
            Block::If {
                otherwise: 5,
                after: 7
            }
        ));
        assert!(matches!(u.blocks[&4], Block::Skip { after: 7 }));
        assert!(matches!(
            u.blocks[&7],
            Block::If {
                otherwise: 10,
                after: 10
            }
        ));
        assert_eq!(u.label("@@top"), Some(0));
    }

    #[test]
    fn commands() {
        let c = split_cmds(r#"\scp,snd[0]	\l,"音楽\"+f+".ogg"	\v,0,1000"#);
        assert_eq!(c.len(), 3);
        assert_eq!(c[1].1, vec![r#""音楽\"+f+".ogg""#.to_string()]);
        let c = split_cmds(r"\if,(a==1)&(b!=1)\then");
        assert_eq!(c[0].1, vec!["(a==1)&(b!=1)".to_string()]);
        assert_eq!(c[1].0, "then");
    }
}
