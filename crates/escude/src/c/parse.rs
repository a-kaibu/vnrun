//! The parser: preprocessed tokens to a `Unit`.

use std::collections::HashMap;
use std::rc::Rc;

use super::ast::*;
use super::lex::{Tok, Token};
use super::pp::Trap;

/// A stand-in for the type a nested declarator wraps, replaced once that type is known.
const HOLE: usize = usize::MAX;

pub struct Parser {
    toks: Vec<Token>,
    at: usize,
    pub types: Types,
    typedefs: HashMap<String, Ty>,
    tags: HashMap<String, usize>,
    files: Vec<String>,
    items: Vec<Item>,
    protos: Vec<(String, Rc<FuncTy>)>,
    enums: HashMap<String, i64>,
}

#[derive(Default, Clone, Copy)]
struct Storage {
    typedef: bool,
    is_extern: bool,
    is_static: bool,
}

fn fill(t: Ty, with: &Ty) -> Ty {
    match t {
        Ty::Struct(HOLE) => with.clone(),
        Ty::Ptr(b) => Ty::Ptr(Box::new(fill(*b, with))),
        Ty::Array(b, n) => Ty::Array(Box::new(fill(*b, with)), n),
        Ty::Func(f) => {
            let mut f = (*f).clone();
            f.ret = fill(f.ret, with);
            Ty::Func(Rc::new(f))
        }
        t => t,
    }
}

impl Parser {
    pub fn new(toks: Vec<Token>, files: Vec<String>) -> Parser {
        Parser {
            toks,
            at: 0,
            types: Types::default(),
            typedefs: HashMap::new(),
            tags: HashMap::new(),
            files,
            items: Vec::new(),
            protos: Vec::new(),
            enums: HashMap::new(),
        }
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at).map(|t| &t.tok)
    }

    fn peek_at(&self, k: usize) -> Option<&Tok> {
        self.toks.get(self.at + k).map(|t| &t.tok)
    }

    fn line(&self) -> u32 {
        self.toks
            .get(self.at)
            .or(self.toks.last())
            .map_or(0, |t| t.line)
    }

    fn here(&self) -> String {
        match self.toks.get(self.at).or(self.toks.last()) {
            Some(t) => format!(
                "{}:{}",
                self.files.get(t.file as usize).map_or("?", |s| s.as_str()),
                t.line
            ),
            None => "end".into(),
        }
    }

    fn err<T>(&self, what: &str) -> Result<T, String> {
        let near: Vec<String> = self.toks[self.at.min(self.toks.len())..]
            .iter()
            .take(6)
            .map(|t| String::from_utf8_lossy(&super::pp::spell(&t.tok)).into_owned())
            .collect();
        Err(format!(
            "{}: {what} (near: {})",
            self.here(),
            near.join(" ")
        ))
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if *q == p)
    }

    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, p: &str) -> Result<(), String> {
        if self.eat(p) {
            Ok(())
        } else {
            self.err(&format!("expected {p}"))
        }
    }

    fn ident(&mut self) -> Result<String, String> {
        match self.peek() {
            Some(Tok::Ident(n)) => {
                let n = n.clone();
                self.at += 1;
                Ok(n)
            }
            _ => self.err("expected a name"),
        }
    }

    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(n)) if n == k)
    }

    /// Does a type start here?
    fn type_start(&self, k: usize) -> bool {
        match self.peek_at(k) {
            Some(Tok::Ident(n)) => {
                matches!(
                    n.as_str(),
                    "void"
                        | "char"
                        | "short"
                        | "int"
                        | "long"
                        | "float"
                        | "double"
                        | "signed"
                        | "unsigned"
                        | "struct"
                        | "union"
                        | "const"
                        | "volatile"
                        | "register"
                        | "static"
                        | "extern"
                        | "typedef"
                        | "inline"
                        | "enum"
                ) || self.typedefs.contains_key(n)
            }
            _ => false,
        }
    }

    // ---- types

    fn specifiers(&mut self) -> Result<(Ty, Storage), String> {
        let mut st = Storage::default();
        let (
            mut unsigned,
            mut signed,
            mut short,
            mut long,
            mut char_,
            mut int,
            mut float,
            mut void,
        ) = (false, false, false, 0, false, false, false, false);
        let mut named: Option<Ty> = None;
        loop {
            let Some(Tok::Ident(n)) = self.peek() else {
                break;
            };
            let n = n.clone();
            match n.as_str() {
                "typedef" => st.typedef = true,
                "extern" => st.is_extern = true,
                "static" => st.is_static = true,
                "const" | "volatile" | "register" | "inline" | "__inline" => {}
                "unsigned" => unsigned = true,
                "signed" => signed = true,
                "short" => short = true,
                "long" => long += 1,
                "char" => char_ = true,
                "int" => int = true,
                "float" | "double" => float = true,
                "void" => void = true,
                "struct" | "union" => {
                    self.at += 1;
                    named = Some(self.struct_spec(n == "union")?);
                    continue;
                }
                "enum" => {
                    self.at += 1;
                    self.enum_spec()?;
                    named = Some(Ty::I32);
                    continue;
                }
                _ => {
                    let any = unsigned
                        || signed
                        || short
                        || long > 0
                        || char_
                        || int
                        || float
                        || void
                        || named.is_some();
                    match self.typedefs.get(&n) {
                        Some(t) if !any => named = Some(t.clone()),
                        _ => break,
                    }
                }
            }
            self.at += 1;
        }
        let _ = signed;
        let ty = if let Some(t) = named {
            t
        } else if void {
            Ty::Void
        } else if float {
            Ty::F32
        } else if char_ {
            if unsigned { Ty::U8 } else { Ty::I8 }
        } else if short {
            if unsigned { Ty::U16 } else { Ty::I16 }
        } else if unsigned {
            Ty::U32
        } else {
            Ty::I32
        };
        Ok((ty, st))
    }

    fn enum_spec(&mut self) -> Result<(), String> {
        if let Some(Tok::Ident(_)) = self.peek() {
            self.at += 1;
        }
        if self.eat("{") {
            // enumerators become constants
            let mut v: i64 = 0;
            while !self.eat("}") {
                let name = self.ident()?;
                if self.eat("=") {
                    let e = self.conditional()?;
                    v = self
                        .const_eval(&e)
                        .ok_or_else(|| format!("{}: enum value not constant", self.here()))?;
                }
                self.enums.insert(name, v);
                v += 1;
                if !self.eat(",") {
                    self.expect("}")?;
                    break;
                }
            }
        }
        Ok(())
    }

    fn struct_spec(&mut self, union: bool) -> Result<Ty, String> {
        let tag = match self.peek() {
            Some(Tok::Ident(n)) => {
                let n = n.clone();
                self.at += 1;
                Some(n)
            }
            _ => None,
        };
        let id = match tag.as_ref().and_then(|t| self.tags.get(t)) {
            Some(&id) if !(self.is("{") && self.types.structs[id].complete) => id,
            _ => {
                let id = self.types.structs.len();
                self.types.structs.push(StructDef {
                    name: tag.clone().unwrap_or_default(),
                    ..StructDef::default()
                });
                if let Some(t) = &tag {
                    self.tags.insert(t.clone(), id);
                }
                id
            }
        };
        if self.eat("{") {
            let mut fields = Vec::new();
            while !self.eat("}") {
                let (base, _) = self.specifiers()?;
                if self.eat(";") {
                    // an unnamed struct inside: its fields are this struct's
                    if let Ty::Struct(s) = base {
                        let inner = self.types.structs[s].fields.clone();
                        for f in inner {
                            fields.push((f.name, f.ty));
                        }
                    }
                    continue;
                }
                loop {
                    let (name, ty, _) = self.declarator(base.clone())?;
                    if self.eat(":") {
                        // bit fields are not used; take the width and keep the whole type
                        self.conditional()?;
                    }
                    fields.push((name.unwrap_or_default(), ty));
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect(";")?;
            }
            self.types.layout(id, fields, union);
        }
        Ok(Ty::Struct(id))
    }

    /// pointers, a name (or a nested declarator), then arrays and parameter lists.
    #[allow(clippy::type_complexity)]
    fn declarator(
        &mut self,
        base: Ty,
    ) -> Result<(Option<String>, Ty, Option<Vec<(String, Ty)>>), String> {
        let mut t = base;
        loop {
            if self.eat("*") {
                t = t.ptr();
            } else if self.is_kw("const") || self.is_kw("volatile") {
                self.at += 1;
            } else {
                break;
            }
        }
        let mut name = None;
        let mut inner: Option<Ty> = None;
        let mut inner_params = None;
        if self.is("(")
            && (matches!(
                self.peek_at(1),
                Some(Tok::Punct("*")) | Some(Tok::Punct("("))
            ) || (matches!(self.peek_at(1), Some(Tok::Ident(_))) && !self.type_start(1)))
        {
            self.at += 1;
            let (n, it, ps) = self.declarator(Ty::Struct(HOLE))?;
            self.expect(")")?;
            name = n;
            inner = Some(it);
            inner_params = ps;
        } else if let Some(Tok::Ident(n)) = self.peek()
            && !matches!(n.as_str(), "const" | "volatile")
        {
            name = Some(n.clone());
            self.at += 1;
        }
        // suffixes, applied inside out
        enum Suf {
            Arr(u32),
            Fun(FuncTy, Vec<(String, Ty)>),
        }
        let mut sufs = Vec::new();
        loop {
            if self.eat("[") {
                let n = if self.is("]") {
                    0
                } else {
                    let e = self.conditional()?;
                    self.const_eval(&e)
                        .ok_or_else(|| format!("{}: array size not constant", self.here()))?
                        as u32
                };
                self.expect("]")?;
                sufs.push(Suf::Arr(n));
            } else if self.eat("(") {
                let (f, names) = self.params()?;
                sufs.push(Suf::Fun(f, names));
            } else {
                break;
            }
        }
        let mut params = None;
        for s in sufs.into_iter().rev() {
            t = match s {
                Suf::Arr(n) => Ty::Array(Box::new(t), n),
                Suf::Fun(mut f, names) => {
                    f.ret = t;
                    params = Some(names);
                    Ty::Func(Rc::new(f))
                }
            };
        }
        if let Some(it) = inner {
            t = fill(it, &t);
            if inner_params.is_some() {
                params = inner_params;
            }
        }
        Ok((name, t, params))
    }

    fn params(&mut self) -> Result<(FuncTy, Vec<(String, Ty)>), String> {
        let mut f = FuncTy {
            ret: Ty::Void,
            params: Vec::new(),
            variadic: false,
            known: true,
        };
        let mut names = Vec::new();
        if self.eat(")") {
            f.known = false;
            return Ok((f, names));
        }
        if self.is_kw("void") && matches!(self.peek_at(1), Some(Tok::Punct(")"))) {
            self.at += 2;
            return Ok((f, names));
        }
        loop {
            if self.eat("...") {
                f.variadic = true;
            } else if self.type_start(0) {
                let (base, _) = self.specifiers()?;
                let (n, t, _) = self.declarator(base)?;
                // arrays and functions as parameters are pointers
                let t = match t {
                    Ty::Array(e, _) => Ty::Ptr(e),
                    Ty::Func(_) => t.ptr(),
                    t => t,
                };
                f.params.push(t.clone());
                names.push((n.unwrap_or_default(), t));
            } else {
                // an untyped name (old style): int
                let n = self.ident()?;
                f.params.push(Ty::I32);
                names.push((n, Ty::I32));
            }
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        Ok((f, names))
    }

    fn type_name(&mut self) -> Result<Ty, String> {
        let (base, _) = self.specifiers()?;
        let (_, t, _) = self.declarator(base)?;
        Ok(t)
    }

    /// A built-in function's type from its #trap line.
    pub fn trap_type(&mut self, name: &str, trap: &Trap) -> Rc<FuncTy> {
        let mut ret = Ty::I32;
        let toks: Vec<Token> = trap
            .ret
            .iter()
            .map(|t| Token {
                tok: t.clone(),
                bol: false,
                space: true,
                file: 0,
                line: 0,
            })
            .collect();
        if !toks.is_empty() {
            let saved = (std::mem::replace(&mut self.toks, toks), self.at);
            self.at = 0;
            if let Ok((base, _)) = self.specifiers() {
                ret = base;
                while self.eat("*") {
                    ret = ret.ptr();
                }
            }
            self.toks = saved.0;
            self.at = saved.1;
        }
        let mut params = Vec::new();
        for (k, p) in trap.params.iter().enumerate() {
            let pname = p.iter().rev().find_map(|t| match t {
                Tok::Ident(n) => Some(n.as_str()),
                _ => None,
            });
            let is_ptr = p.contains(&Tok::Punct("*")) || pname.is_some_and(|n| n.starts_with('p'));
            let float = float_param(name, k, pname.unwrap_or(""));
            params.push(if float {
                Ty::F32
            } else if is_ptr {
                Ty::U8.ptr()
            } else {
                Ty::I32
            });
        }
        let variadic = trap
            .params
            .last()
            .is_some_and(|p| p.iter().any(|t| matches!(t, Tok::Ident(n) if n == "nArgs")));
        if variadic {
            params.pop();
        }
        Rc::new(FuncTy {
            ret,
            params,
            variadic,
            known: true,
        })
    }

    // ---- top level

    pub fn unit(mut self, traps: &HashMap<String, Trap>) -> Result<Unit, String> {
        let mut trap_types = Vec::new();
        let mut names: Vec<&String> = traps.keys().collect();
        names.sort();
        for n in names {
            let t = self.trap_type(n, &traps[n]);
            trap_types.push((n.clone(), t));
        }
        while self.at < self.toks.len() {
            if self.eat(";") {
                continue;
            }
            self.external()?;
        }
        Ok(Unit {
            items: self.items,
            types: self.types,
            traps: trap_types,
            protos: self.protos,
        })
    }

    fn external(&mut self) -> Result<(), String> {
        let line = self.line();
        let (base, st) = self.specifiers()?;
        if self.eat(";") {
            return Ok(());
        }
        loop {
            let (name, ty, params) = self.declarator(base.clone())?;
            let Some(name) = name else {
                return self.err("a declaration needs a name");
            };
            if st.typedef {
                self.typedefs.insert(name, ty);
            } else if let Ty::Func(f) = &ty {
                if self.is("{") {
                    let file = self.here();
                    let body = self.block()?;
                    self.items.push(Item::Func(Func {
                        name,
                        ty: f.clone(),
                        params: params.unwrap_or_default(),
                        body,
                        file,
                    }));
                    return Ok(());
                }
                self.protos.push((name, f.clone()));
            } else {
                let init = if self.eat("=") {
                    Some(self.initializer()?)
                } else {
                    None
                };
                let ty = self.size_from_init(ty, &init);
                self.items.push(Item::Var(Decl {
                    name,
                    ty,
                    init,
                    is_static: st.is_static,
                    is_extern: st.is_extern,
                }));
            }
            if !self.eat(",") {
                break;
            }
        }
        if !self.eat(";") {
            return self.err(&format!("expected ; after a declaration (line {line})"));
        }
        Ok(())
    }

    /// `x[] = {...}` and `s[] = "..."` take their size from the initializer.
    fn size_from_init(&self, ty: Ty, init: &Option<Init>) -> Ty {
        match (ty, init) {
            (Ty::Array(e, 0), Some(Init::List(l))) => Ty::Array(e, l.len() as u32),
            (Ty::Array(e, 0), Some(Init::Expr(Expr::Str(s)))) => Ty::Array(e, s.len() as u32 + 1),
            (t, _) => t,
        }
    }

    fn initializer(&mut self) -> Result<Init, String> {
        if self.eat("{") {
            let mut l = Vec::new();
            while !self.eat("}") {
                l.push(self.initializer()?);
                if !self.eat(",") {
                    self.expect("}")?;
                    break;
                }
            }
            Ok(Init::List(l))
        } else {
            Ok(Init::Expr(self.assign()?))
        }
    }

    // ---- statements

    fn block(&mut self) -> Result<Stmt, String> {
        self.expect("{")?;
        let mut v = Vec::new();
        while !self.eat("}") {
            if self.at >= self.toks.len() {
                return self.err("block not closed");
            }
            v.push(self.statement()?);
        }
        Ok(Stmt::Block(v))
    }

    fn local_decls(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        let (base, st) = self.specifiers()?;
        let mut v = Vec::new();
        if self.eat(";") {
            return Ok(Stmt::Empty);
        }
        loop {
            let (name, ty, _) = self.declarator(base.clone())?;
            let name = name.unwrap_or_default();
            if st.typedef {
                self.typedefs.insert(name, ty);
            } else if let Ty::Func(f) = &ty {
                self.protos.push((name, f.clone()));
            } else {
                let init = if self.eat("=") {
                    Some(self.initializer()?)
                } else {
                    None
                };
                let ty = self.size_from_init(ty, &init);
                v.push(Decl {
                    name,
                    ty,
                    init,
                    is_static: st.is_static,
                    is_extern: st.is_extern,
                });
            }
            if !self.eat(",") {
                break;
            }
        }
        self.expect(";")?;
        Ok(Stmt::Decls(v, line))
    }

    fn statement(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        if self.is("{") {
            return self.block();
        }
        if self.eat(";") {
            return Ok(Stmt::Empty);
        }
        if let Some(Tok::Ident(n)) = self.peek() {
            let n = n.clone();
            if matches!(self.peek_at(1), Some(Tok::Punct(":")))
                && !self.typedefs.contains_key(&n)
                && n != "default"
            {
                self.at += 2;
                return Ok(Stmt::Label(n));
            }
            match n.as_str() {
                "if" => {
                    self.at += 1;
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    let a = self.statement()?;
                    let b = if self.is_kw("else") {
                        self.at += 1;
                        Some(Box::new(self.statement()?))
                    } else {
                        None
                    };
                    return Ok(Stmt::If(c, Box::new(a), b, line));
                }
                "while" => {
                    self.at += 1;
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    let body = self.statement()?;
                    return Ok(Stmt::While(c, Box::new(body), line));
                }
                "do" => {
                    self.at += 1;
                    let body = self.statement()?;
                    if !self.is_kw("while") {
                        return self.err("expected while");
                    }
                    self.at += 1;
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    self.expect(";")?;
                    return Ok(Stmt::Do(Box::new(body), c, line));
                }
                "for" => {
                    self.at += 1;
                    self.expect("(")?;
                    let init = if self.eat(";") {
                        None
                    } else if self.type_start(0) {
                        Some(Box::new(self.local_decls()?))
                    } else {
                        let e = self.expr()?;
                        self.expect(";")?;
                        Some(Box::new(Stmt::Expr(e, line)))
                    };
                    let cond = if self.is(";") {
                        None
                    } else {
                        Some(self.expr()?)
                    };
                    self.expect(";")?;
                    let step = if self.is(")") {
                        None
                    } else {
                        Some(self.expr()?)
                    };
                    self.expect(")")?;
                    let body = self.statement()?;
                    return Ok(Stmt::For(init, cond, step, Box::new(body), line));
                }
                "switch" => {
                    self.at += 1;
                    self.expect("(")?;
                    let c = self.expr()?;
                    self.expect(")")?;
                    let body = self.statement()?;
                    return Ok(Stmt::Switch(c, Box::new(body), line));
                }
                "case" => {
                    self.at += 1;
                    let e = self.conditional()?;
                    self.expect(":")?;
                    return Ok(Stmt::Case(e, line));
                }
                "default" => {
                    self.at += 1;
                    self.expect(":")?;
                    return Ok(Stmt::Default);
                }
                "break" => {
                    self.at += 1;
                    self.expect(";")?;
                    return Ok(Stmt::Break(line));
                }
                "continue" => {
                    self.at += 1;
                    self.expect(";")?;
                    return Ok(Stmt::Continue(line));
                }
                "return" => {
                    self.at += 1;
                    let e = if self.is(";") {
                        None
                    } else {
                        Some(self.expr()?)
                    };
                    self.expect(";")?;
                    return Ok(Stmt::Return(e, line));
                }
                "goto" => {
                    self.at += 1;
                    let l = self.ident()?;
                    self.expect(";")?;
                    return Ok(Stmt::Goto(l, line));
                }
                _ => {}
            }
        }
        if self.type_start(0)
            && !(matches!(self.peek(), Some(Tok::Ident(n)) if self.typedefs.contains_key(n))
                && matches!(self.peek_at(1), Some(Tok::Punct(p)) if ["=", "(", ".", "->", "[", "+=", "-=", "++", "--"].contains(p)))
        {
            return self.local_decls();
        }
        let e = self.expr()?;
        self.expect(";")?;
        Ok(Stmt::Expr(e, line))
    }

    // ---- expressions

    pub fn expr(&mut self) -> Result<Expr, String> {
        let mut e = self.assign()?;
        while self.eat(",") {
            let r = self.assign()?;
            e = Expr::Comma(Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn assign(&mut self) -> Result<Expr, String> {
        let l = self.conditional()?;
        let op = match self.peek() {
            Some(Tok::Punct(p)) => match *p {
                "=" => Some(None),
                "+=" => Some(Some(BOp::Add)),
                "-=" => Some(Some(BOp::Sub)),
                "*=" => Some(Some(BOp::Mul)),
                "/=" => Some(Some(BOp::Div)),
                "%=" => Some(Some(BOp::Mod)),
                "<<=" => Some(Some(BOp::Shl)),
                ">>=" => Some(Some(BOp::Shr)),
                "&=" => Some(Some(BOp::And)),
                "|=" => Some(Some(BOp::Or)),
                "^=" => Some(Some(BOp::Xor)),
                _ => None,
            },
            _ => None,
        };
        if let Some(op) = op {
            self.at += 1;
            let r = self.assign()?;
            return Ok(Expr::Assign(op, Box::new(l), Box::new(r)));
        }
        Ok(l)
    }

    fn conditional(&mut self) -> Result<Expr, String> {
        let c = self.binary(0)?;
        if self.eat("?") {
            let a = self.expr()?;
            self.expect(":")?;
            let b = self.conditional()?;
            return Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    fn binary(&mut self, min: u8) -> Result<Expr, String> {
        let mut l = self.cast()?;
        loop {
            let (op, prec) = match self.peek() {
                Some(Tok::Punct(p)) => match *p {
                    "||" => (BOp::LOr, 1),
                    "&&" => (BOp::LAnd, 2),
                    "|" => (BOp::Or, 3),
                    "^" => (BOp::Xor, 4),
                    "&" => (BOp::And, 5),
                    "==" => (BOp::Eq, 6),
                    "!=" => (BOp::Ne, 6),
                    "<" => (BOp::Lt, 7),
                    "<=" => (BOp::Le, 7),
                    ">" => (BOp::Gt, 7),
                    ">=" => (BOp::Ge, 7),
                    "<<" => (BOp::Shl, 8),
                    ">>" => (BOp::Shr, 8),
                    "+" => (BOp::Add, 9),
                    "-" => (BOp::Sub, 9),
                    "*" => (BOp::Mul, 10),
                    "/" => (BOp::Div, 10),
                    "%" => (BOp::Mod, 10),
                    _ => break,
                },
                _ => break,
            };
            if prec <= min {
                break;
            }
            self.at += 1;
            let r = self.binary(prec)?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn cast(&mut self) -> Result<Expr, String> {
        if self.is("(") && self.type_start(1) {
            self.at += 1;
            let t = self.type_name()?;
            self.expect(")")?;
            let e = self.cast()?;
            return Ok(Expr::Cast(t, Box::new(e)));
        }
        self.unary()
    }

    fn unary(&mut self) -> Result<Expr, String> {
        let op = match self.peek() {
            Some(Tok::Punct(p)) => match *p {
                "-" => Some(UOp::Neg),
                "+" => Some(UOp::Plus),
                "~" => Some(UOp::BitNot),
                "!" => Some(UOp::Not),
                "*" => Some(UOp::Deref),
                "&" => Some(UOp::Addr),
                "++" => Some(UOp::PreInc),
                "--" => Some(UOp::PreDec),
                _ => None,
            },
            Some(Tok::Ident(n)) if n == "sizeof" => {
                self.at += 1;
                if self.is("(") && self.type_start(1) {
                    self.at += 1;
                    let t = self.type_name()?;
                    self.expect(")")?;
                    return Ok(Expr::SizeofTy(t));
                }
                let e = self.unary()?;
                return Ok(Expr::SizeofE(Box::new(e)));
            }
            _ => None,
        };
        if let Some(op) = op {
            self.at += 1;
            let e = if matches!(op, UOp::PreInc | UOp::PreDec) {
                self.unary()?
            } else {
                self.cast()?
            };
            return Ok(Expr::Unary(op, Box::new(e)));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            if self.eat("(") {
                let mut args = Vec::new();
                if !self.eat(")") {
                    loop {
                        args.push(self.assign()?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                    self.expect(")")?;
                }
                e = Expr::Call(Box::new(e), args);
            } else if self.eat("[") {
                let i = self.expr()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(i));
            } else if self.eat(".") {
                let f = self.ident()?;
                e = Expr::Member(Box::new(e), f, false);
            } else if self.eat("->") {
                let f = self.ident()?;
                e = Expr::Member(Box::new(e), f, true);
            } else if self.eat("++") {
                e = Expr::Unary(UOp::PostInc, Box::new(e));
            } else if self.eat("--") {
                e = Expr::Unary(UOp::PostDec, Box::new(e));
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let Some(t) = self.peek().cloned() else {
            return self.err("unexpected end");
        };
        self.at += 1;
        match t {
            Tok::Int(v, u) => Ok(Expr::Int(v, if u { Ty::U32 } else { Ty::I32 })),
            Tok::Float(f) => Ok(Expr::Float(f)),
            Tok::Str(mut s) => {
                while let Some(Tok::Str(more)) = self.peek() {
                    s.extend(more.clone());
                    self.at += 1;
                }
                Ok(Expr::Str(s))
            }
            Tok::Ident(n) => match self.enums.get(&n) {
                Some(&v) => Ok(Expr::Int(v as u32, Ty::I32)),
                None => Ok(Expr::Name(n)),
            },
            Tok::Punct("(") => {
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            _ => {
                self.at -= 1;
                self.err("unexpected token")
            }
        }
    }

    /// The value of a constant expression (array sizes, case labels).
    pub fn const_eval(&self, e: &Expr) -> Option<i64> {
        const_eval(e, &self.types)
    }
}

/// The value of a constant expression.
pub fn const_eval(e: &Expr, types: &Types) -> Option<i64> {
    let c = |e: &Expr| const_eval(e, types);
    Some(match e {
        Expr::Int(v, t) => {
            if *t == Ty::U32 {
                *v as i64
            } else {
                *v as i32 as i64
            }
        }
        Expr::Float(f) => *f as i64,
        Expr::SizeofTy(t) => types.size(t) as i64,
        Expr::Cast(t, e) => {
            let v = c(e)?;
            match t {
                Ty::I8 => v as i8 as i64,
                Ty::U8 => v as u8 as i64,
                Ty::I16 => v as i16 as i64,
                Ty::U16 => v as u16 as i64,
                Ty::I32 => v as i32 as i64,
                _ => v as u32 as i64,
            }
        }
        Expr::Unary(op, e) => {
            let v = c(e)?;
            match op {
                UOp::Neg => -v,
                UOp::Plus => v,
                UOp::BitNot => !v,
                UOp::Not => (v == 0) as i64,
                _ => return None,
            }
        }
        Expr::Bin(op, a, b) => {
            let (x, y) = (c(a)?, c(b)?);
            match op {
                BOp::Add => x + y,
                BOp::Sub => x - y,
                BOp::Mul => x * y,
                BOp::Div => x.checked_div(y)?,
                BOp::Mod => x.checked_rem(y)?,
                BOp::Shl => x << (y & 63),
                BOp::Shr => x >> (y & 63),
                BOp::And => x & y,
                BOp::Or => x | y,
                BOp::Xor => x ^ y,
                BOp::Eq => (x == y) as i64,
                BOp::Ne => (x != y) as i64,
                BOp::Lt => (x < y) as i64,
                BOp::Le => (x <= y) as i64,
                BOp::Gt => (x > y) as i64,
                BOp::Ge => (x >= y) as i64,
                BOp::LAnd => (x != 0 && y != 0) as i64,
                BOp::LOr => (x != 0 || y != 0) as i64,
            }
        }
        Expr::Cond(a, b, d) => {
            if c(a)? != 0 {
                c(b)?
            } else {
                c(d)?
            }
        }
        _ => return None,
    })
}

/// Built-in functions that take floats, by name and parameter.
fn float_param(func: &str, k: usize, pname: &str) -> bool {
    match func {
        "fabs" | "sqrt" | "pow" | "log" | "exp" | "floor" | "ceil" | "sin" | "cos" | "tan"
        | "asin" | "acos" | "atan" | "bezier" | "neville" | "linear" | "p3d_env" => true,
        "ftoa" => k == 1,
        "kf_set" => k >= 1,
        "kf_getkey" => k == 2,
        "p3d_init" | "p3d_light" => k >= 1 && pname.starts_with('f'),
        _ => false,
    }
}
