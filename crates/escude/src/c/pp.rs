//! The preprocessor: #include, #define (with arguments, # and ##), #undef, #ifdef/#ifndef/
//! #else/#endif, and ACPX's #trap lines, which declare the engine's built-in functions.

use std::collections::{HashMap, HashSet};

use super::lex::{Tok, Token, lex};

#[derive(Debug, Clone)]
struct Macro {
    params: Option<Vec<String>>,
    body: Vec<Token>,
}

/// A built-in function: its return type (tokens) and its parameters (tokens each).
#[derive(Debug, Clone)]
pub struct Trap {
    pub ret: Vec<Tok>,
    pub params: Vec<Vec<Tok>>,
}

pub struct Pre<'a> {
    load: &'a mut dyn FnMut(&str) -> Option<Vec<u8>>,
    macros: HashMap<String, Macro>,
    pub traps: HashMap<String, Trap>,
    /// file names by id
    pub files: Vec<String>,
}

enum Item {
    Tok(Token),
    End(String),
}

fn dir_of(path: &str) -> &str {
    path.rfind('\\').map_or("", |n| &path[..n + 1])
}

impl<'a> Pre<'a> {
    pub fn new(load: &'a mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Pre<'a> {
        Pre {
            load,
            macros: HashMap::new(),
            traps: HashMap::new(),
            files: Vec::new(),
        }
    }

    /// The tokens of a file and what it includes, macros expanded. `base` is the folder that
    /// include names may also be taken from.
    pub fn run(&mut self, path: &str, base: &str) -> Result<Vec<Token>, String> {
        let mut out = Vec::new();
        self.file(path, base, &mut out, 0)?;
        Ok(out)
    }

    fn file(
        &mut self,
        path: &str,
        base: &str,
        out: &mut Vec<Token>,
        depth: u32,
    ) -> Result<(), String> {
        if depth > 32 {
            return Err(format!("{path}: includes nest too deep"));
        }
        let data = (self.load)(path).ok_or(format!("{path}: not found"))?;
        let id = self.files.len() as u32;
        self.files.push(path.to_string());
        let toks = lex(&data, id).map_err(|e| format!("{path}: {e}"))?;
        // lines of directives and runs of other tokens
        let mut i = 0;
        let mut text: Vec<Token> = Vec::new();
        // #if nesting: (taking this part, took a part already)
        let mut conds: Vec<(bool, bool)> = Vec::new();
        let active = |conds: &Vec<(bool, bool)>| conds.iter().all(|c| c.0);
        while i < toks.len() {
            let t = &toks[i];
            if t.bol && t.tok == Tok::Punct("#") {
                let mut j = i + 1;
                while j < toks.len() && !toks[j].bol {
                    j += 1;
                }
                let line: Vec<Token> = toks[i + 1..j].to_vec();
                i = j;
                let name = match line.first().map(|t| &t.tok) {
                    Some(Tok::Ident(n)) => n.clone(),
                    _ => continue,
                };
                match name.as_str() {
                    "ifdef" | "ifndef" => {
                        let def = matches!(line.get(1).map(|t| &t.tok), Some(Tok::Ident(n)) if self.macros.contains_key(n));
                        let take = (name == "ifdef") == def;
                        conds.push((take && active(&conds), take));
                        continue;
                    }
                    "if" => {
                        // only #if 0 / #if 1 and the like
                        let take =
                            matches!(line.get(1).map(|t| &t.tok), Some(Tok::Int(v, _)) if *v != 0);
                        conds.push((take && active(&conds), take));
                        continue;
                    }
                    "else" => {
                        if let Some(c) = conds.pop() {
                            let take = !c.1;
                            conds.push((take && active(&conds), true));
                        }
                        continue;
                    }
                    "endif" => {
                        conds.pop();
                        continue;
                    }
                    _ => {}
                }
                if !active(&conds) {
                    continue;
                }
                // what came before the directive is expanded with the macros as they were
                if !text.is_empty() {
                    let run = std::mem::take(&mut text);
                    self.expand(run, out)?;
                }
                match name.as_str() {
                    "include" => {
                        let Some(Tok::Str(s)) = line.get(1).map(|t| &t.tok) else {
                            return Err(format!("{path}:{}: #include needs a name", t.line));
                        };
                        let inc = nexas::sjis::decode(s);
                        let tries = [
                            format!("{}{inc}", dir_of(path)),
                            format!("{base}{inc}"),
                            inc.clone(),
                        ];
                        let found = tries.iter().find(|p| (self.load)(p).is_some()).cloned();
                        let Some(p) = found else {
                            return Err(format!("{path}:{}: {inc} not found", t.line));
                        };
                        self.file(&p, base, out, depth + 1)?;
                    }
                    "define" => self.define(&line[1..], path)?,
                    "undef" => {
                        if let Some(Tok::Ident(n)) = line.get(1).map(|t| &t.tok) {
                            self.macros.remove(n);
                        }
                    }
                    "trap" => self.trap(&line[1..]),
                    _ => {}
                }
                continue;
            }
            if active(&conds) {
                text.push(t.clone());
            }
            i += 1;
        }
        if !text.is_empty() {
            self.expand(text, out)?;
        }
        Ok(())
    }

    fn define(&mut self, line: &[Token], path: &str) -> Result<(), String> {
        let Some(Tok::Ident(name)) = line.first().map(|t| &t.tok) else {
            return Err(format!("{path}: #define needs a name"));
        };
        let mut at = 1;
        let mut params = None;
        if line
            .get(1)
            .is_some_and(|t| t.tok == Tok::Punct("(") && !t.space)
        {
            let mut ps = Vec::new();
            at = 2;
            while at < line.len() && line[at].tok != Tok::Punct(")") {
                match &line[at].tok {
                    Tok::Ident(p) => ps.push(p.clone()),
                    Tok::Punct("...") => ps.push("__VA_ARGS__".into()),
                    _ => {}
                }
                at += 1;
            }
            at += 1;
            params = Some(ps);
        }
        self.macros.insert(
            name.clone(),
            Macro {
                params,
                body: line.get(at..).unwrap_or(&[]).to_vec(),
            },
        );
        Ok(())
    }

    fn trap(&mut self, line: &[Token]) {
        // ret... name ( params ) ;
        let Some(open) = line.iter().position(|t| t.tok == Tok::Punct("(")) else {
            return;
        };
        if open == 0 {
            return;
        }
        let Tok::Ident(name) = &line[open - 1].tok else {
            return;
        };
        let ret: Vec<Tok> = line[..open - 1].iter().map(|t| t.tok.clone()).collect();
        let mut params = Vec::new();
        let mut cur = Vec::new();
        for t in &line[open + 1..] {
            match &t.tok {
                Tok::Punct(")") => break,
                Tok::Punct(",") => params.push(std::mem::take(&mut cur)),
                tok => cur.push(tok.clone()),
            }
        }
        if !cur.is_empty() {
            params.push(cur);
        }
        self.traps.insert(name.clone(), Trap { ret, params });
    }

    fn expand(&mut self, toks: Vec<Token>, out: &mut Vec<Token>) -> Result<(), String> {
        let mut input: Vec<Item> = toks.into_iter().rev().map(Item::Tok).collect();
        let mut off: HashMap<String, u32> = HashMap::new();
        self.expand_items(&mut input, &mut off, out)
    }

    fn expand_items(
        &mut self,
        input: &mut Vec<Item>,
        off: &mut HashMap<String, u32>,
        out: &mut Vec<Token>,
    ) -> Result<(), String> {
        while let Some(item) = input.pop() {
            let t = match item {
                Item::End(n) => {
                    if let Some(c) = off.get_mut(&n) {
                        *c -= 1;
                        if *c == 0 {
                            off.remove(&n);
                        }
                    }
                    continue;
                }
                Item::Tok(t) => t,
            };
            let Tok::Ident(name) = &t.tok else {
                out.push(t);
                continue;
            };
            let Some(m) = self.macros.get(name).cloned() else {
                out.push(t);
                continue;
            };
            if off.contains_key(name) {
                out.push(t);
                continue;
            }
            let body = match &m.params {
                None => m.body.clone(),
                Some(params) => {
                    // the next real token must be '('
                    let mut k = input.len();
                    let mut ends = Vec::new();
                    loop {
                        if k == 0 {
                            break;
                        }
                        match &input[k - 1] {
                            Item::End(n) => {
                                ends.push(n.clone());
                                k -= 1;
                            }
                            Item::Tok(_) => break,
                        }
                    }
                    let is_call =
                        k > 0 && matches!(&input[k - 1], Item::Tok(x) if x.tok == Tok::Punct("("));
                    if !is_call {
                        out.push(t);
                        continue;
                    }
                    // drop the markers before '(' (their macros end here) and the '('
                    for _ in 0..(input.len() - k) {
                        if let Some(Item::End(n)) = input.pop()
                            && let Some(c) = off.get_mut(&n)
                        {
                            *c -= 1;
                            if *c == 0 {
                                off.remove(&n);
                            }
                        }
                    }
                    input.pop();
                    let mut args: Vec<Vec<Token>> = vec![Vec::new()];
                    let mut depth = 0;
                    loop {
                        let Some(it) = input.pop() else {
                            return Err(format!("macro {name}: arguments not closed"));
                        };
                        let a = match it {
                            Item::End(n) => {
                                if let Some(c) = off.get_mut(&n) {
                                    *c -= 1;
                                    if *c == 0 {
                                        off.remove(&n);
                                    }
                                }
                                continue;
                            }
                            Item::Tok(a) => a,
                        };
                        match a.tok {
                            Tok::Punct("(") | Tok::Punct("[") | Tok::Punct("{") => depth += 1,
                            Tok::Punct(")") if depth == 0 => break,
                            Tok::Punct(")") | Tok::Punct("]") | Tok::Punct("}") => depth -= 1,
                            Tok::Punct(",") if depth == 0 && args.len() < params.len() => {
                                args.push(Vec::new());
                                continue;
                            }
                            _ => {}
                        }
                        args.last_mut().unwrap().push(a);
                    }
                    if params.is_empty() && args.len() == 1 && args[0].is_empty() {
                        args.clear();
                    }
                    self.substitute(&m.body, params, &args, off)?
                }
            };
            // rescan the body with this macro off
            *off.entry(name.clone()).or_insert(0) += 1;
            input.push(Item::End(name.clone()));
            for mut b in body.into_iter().rev() {
                b.line = t.line;
                b.file = t.file;
                b.bol = false;
                input.push(Item::Tok(b));
            }
        }
        Ok(())
    }

    fn substitute(
        &mut self,
        body: &[Token],
        params: &[String],
        args: &[Vec<Token>],
        off: &HashMap<String, u32>,
    ) -> Result<Vec<Token>, String> {
        let index = |t: &Token| match &t.tok {
            Tok::Ident(n) => params.iter().position(|p| p == n),
            _ => None,
        };
        let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
        let mut out: Vec<Token> = Vec::new();
        let mut i = 0;
        while i < body.len() {
            let t = &body[i];
            if t.tok == Tok::Punct("#")
                && let Some(p) = body.get(i + 1).and_then(index)
            {
                let mut s = Vec::new();
                for (k, a) in arg(p).iter().enumerate() {
                    if k > 0 && a.space {
                        s.push(b' ');
                    }
                    s.extend(spell(&a.tok));
                }
                out.push(Token {
                    tok: Tok::Str(s),
                    ..t.clone()
                });
                i += 2;
                continue;
            }
            if t.tok == Tok::Punct("##") {
                i += 1;
                continue;
            }
            if body.get(i + 1).is_some_and(|n| n.tok == Tok::Punct("##"))
                || (i > 0 && body[i - 1].tok == Tok::Punct("##"))
            {
                // an operand of ## is not expanded
                let piece = match index(t) {
                    Some(p) => arg(p),
                    None => vec![t.clone()],
                };
                if i > 0 && body[i - 1].tok == Tok::Punct("##") {
                    // paste onto the last token
                    let mut piece = piece.into_iter();
                    if let (Some(last), Some(first)) = (out.pop(), piece.next()) {
                        let mut s = spell(&last.tok);
                        s.extend(spell(&first.tok));
                        let mut joined = lex(&s, last.file).unwrap_or_default();
                        for j in joined.iter_mut() {
                            j.line = last.line;
                            j.bol = false;
                            j.space = false;
                        }
                        out.extend(joined);
                    }
                    out.extend(piece);
                } else {
                    out.extend(piece);
                }
                i += 1;
                continue;
            }
            match index(t) {
                Some(p) => {
                    // arguments are expanded on their own first
                    let mut items: Vec<Item> = arg(p).into_iter().rev().map(Item::Tok).collect();
                    let mut off2 = off.clone();
                    let mut exp = Vec::new();
                    self.expand_items(&mut items, &mut off2, &mut exp)?;
                    if let Some(f) = exp.first_mut() {
                        f.space = t.space;
                    }
                    out.extend(exp);
                }
                None => out.push(t.clone()),
            }
            i += 1;
        }
        Ok(out)
    }
}

/// A token as source text.
pub fn spell(t: &Tok) -> Vec<u8> {
    match t {
        Tok::Ident(s) => s.as_bytes().to_vec(),
        Tok::Int(v, u) => format!("{}{}", *v as i64, if *u { "u" } else { "" }).into_bytes(),
        Tok::Float(f) => format!("{f:?}").into_bytes(),
        Tok::Str(s) => {
            let mut o = vec![b'"'];
            for &c in s {
                match c {
                    b'"' => o.extend(b"\\\""),
                    b'\\' => o.extend(b"\\\\"),
                    b'\n' => o.extend(b"\\n"),
                    _ => o.push(c),
                }
            }
            o.push(b'"');
            o
        }
        Tok::Punct(p) => p.as_bytes().to_vec(),
    }
}

/// Names a set of tokens uses, for tests.
#[allow(dead_code)]
pub fn idents(toks: &[Token]) -> HashSet<String> {
    toks.iter()
        .filter_map(|t| match &t.tok {
            Tok::Ident(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> String {
        let src = src.to_string();
        let mut load = move |p: &str| (p == "a.c").then(|| src.clone().into_bytes());
        let mut pre = Pre::new(&mut load);
        let toks = pre.run("a.c", "").unwrap();
        let v: Vec<String> = toks
            .iter()
            .map(|t| String::from_utf8(spell(&t.tok)).unwrap())
            .collect();
        v.join(" ")
    }

    #[test]
    fn expands_macros() {
        assert_eq!(run("#define A 1+B\n#define B 2\nx = A;"), "x = 1 + 2 ;");
        assert_eq!(run("#define F(a,b) (a*b)\nF(1+2, 3)"), "( 1 + 2 * 3 )");
        assert_eq!(run("#define S(x) #x\nS(a b)"), "\"a b\"");
        assert_eq!(run("#define C(a,b) a##b\nC(x,1)"), "x1");
        assert_eq!(run("#define G F\n#define F(x) x+1\nG(2)"), "2 + 1");
        assert_eq!(run("#define R R+1\nR"), "R + 1");
        assert_eq!(
            run(
                "#define BEGIN switch(m){\n#define CASE(a,b) case a: f(b); break;\nBEGIN CASE(1, \"x\") }"
            ),
            "switch ( m ) { case 1 : f ( \"x\" ) ; break ; }"
        );
    }

    #[test]
    fn reads_traps() {
        let src = "#trap BOOL gload(nPlane, pszFile);\nint x;".to_string();
        let mut load = move |p: &str| (p == "a.c").then(|| src.clone().into_bytes());
        let mut pre = Pre::new(&mut load);
        let toks = pre.run("a.c", "").unwrap();
        assert_eq!(toks.len(), 3);
        assert_eq!(pre.traps["gload"].params.len(), 2);
    }
}
