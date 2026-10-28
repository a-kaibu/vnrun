//! Compiles a parsed program to stack machine code, putting its globals and strings in memory.

use std::collections::HashMap;
use std::rc::Rc;

use super::ast::*;
use super::code::{FN_BASE, Function, NATIVE_BASE, Op};
use super::parse::const_eval;

/// Where compiled code and data go: memory for globals and strings, and the function table.
pub trait Target {
    /// Zeroed static memory.
    fn alloc(&mut self, size: u32, align: u32) -> u32;
    fn write(&mut self, addr: u32, bytes: &[u8]);
    fn new_function(&mut self, name: &str) -> u32;
    fn set_function(&mut self, index: u32, f: Function);
    /// The id of a built-in function by name.
    fn native(&mut self, name: &str) -> u16;
}

/// What a program offers the programs it calls: its globals and functions.
#[derive(Debug, Clone, Default)]
pub struct Exports {
    pub globals: HashMap<String, (u32, Ty)>,
    pub funcs: HashMap<String, (u32, Rc<FuncTy>)>,
    pub types: Types,
}

pub struct Compiled {
    /// runs the initializers, then main; returns main's value
    pub start: u32,
    pub exports: Exports,
}

#[derive(Clone)]
enum Var {
    /// a frame offset
    Local(u32, Ty),
    Global(u32, Ty),
}

struct Unit<'a> {
    t: &'a mut dyn Target,
    types: Types,
    globals: HashMap<String, (u32, Ty)>,
    funcs: HashMap<String, (u32, Rc<FuncTy>)>,
    natives: HashMap<String, Rc<FuncTy>>,
    strings: HashMap<Vec<u8>, u32>,
    root: Option<&'a Exports>,
    /// the initializer code
    init: Vec<(Op, u32)>,
}

struct Fun {
    code: Vec<Op>,
    lines: Vec<u32>,
    scopes: Vec<HashMap<String, Var>>,
    frame: u32,
    line: u32,
    ret: Ty,
    breaks: Vec<Vec<usize>>,
    continues: Vec<Vec<usize>>,
    labels: HashMap<String, u32>,
    gotos: Vec<(usize, String)>,
    /// cases of the switches being compiled: (value, code index), default
    switches: Vec<(Vec<(u32, u32)>, Option<u32>)>,
    statics: u32,
    name: String,
}

type R<T> = Result<T, String>;

/// Ty of a struct in another program's types, found here by its name.
fn translate(t: &Ty, from: &Types, to: &Types) -> Ty {
    match t {
        Ty::Struct(i) => {
            let name = &from.structs[*i].name;
            match to
                .structs
                .iter()
                .position(|s| !name.is_empty() && &s.name == name)
            {
                Some(j) => Ty::Struct(j),
                None => Ty::U8,
            }
        }
        Ty::Ptr(b) => Ty::Ptr(Box::new(translate(b, from, to))),
        Ty::Array(b, n) => Ty::Array(Box::new(translate(b, from, to)), *n),
        Ty::Func(f) => Ty::Func(Rc::new(FuncTy {
            ret: translate(&f.ret, from, to),
            params: f.params.iter().map(|p| translate(p, from, to)).collect(),
            variadic: f.variadic,
            known: f.known,
        })),
        t => t.clone(),
    }
}

pub fn compile(unit: &super::ast::Unit, t: &mut dyn Target, root: Option<&Exports>) -> R<Compiled> {
    let mut u = Unit {
        t,
        types: unit.types.clone(),
        globals: HashMap::new(),
        funcs: HashMap::new(),
        natives: HashMap::new(),
        strings: HashMap::new(),
        root,
        init: Vec::new(),
    };
    for (n, f) in &unit.traps {
        u.natives.insert(n.clone(), f.clone());
    }
    // functions first, so calls can come before definitions
    let mut protos: HashMap<String, Rc<FuncTy>> = HashMap::new();
    for (n, f) in &unit.protos {
        protos.insert(n.clone(), f.clone());
    }
    for item in &unit.items {
        if let Item::Func(f) = item {
            match u.funcs.get(&f.name) {
                Some(_) => {}
                None => {
                    let i = u.t.new_function(&f.name);
                    u.funcs.insert(f.name.clone(), (i, f.ty.clone()));
                }
            }
        }
    }
    // functions declared, not defined here: the root's, or not there at all
    for (n, f) in &protos {
        if u.funcs.contains_key(n) || u.natives.contains_key(n) {
            continue;
        }
        if let Some(r) = u.root
            && let Some((i, _)) = r.funcs.get(n)
        {
            u.funcs.insert(n.clone(), (*i, f.clone()));
        }
    }
    // globals: storage for each definition; extern ones are this program's or the root's
    let mut externs = Vec::new();
    let mut own: Vec<(u32, u32)> = Vec::new();
    for item in &unit.items {
        let Item::Var(d) = item else { continue };
        if d.is_extern && d.init.is_none() {
            externs.push(d.clone());
            continue;
        }
        let ty = match (&d.ty, u.globals.get(&d.name)) {
            (Ty::Array(_, 0), Some((_, t))) => t.clone(),
            (t, _) => t.clone(),
        };
        if let Some((_, old)) = u.globals.get(&d.name)
            && u.types.size(old) >= u.types.size(&ty)
            && d.init.is_none()
        {
            continue;
        }
        let size = u.types.size(&ty).max(1);
        let addr = u.t.alloc(size, u.types.align(&ty).max(4));
        u.globals.insert(d.name.clone(), (addr, ty));
        own.push((addr, size));
    }
    for d in externs {
        if u.globals.contains_key(&d.name) {
            continue;
        }
        match u.root.and_then(|r| r.globals.get(&d.name)) {
            Some((addr, _)) => {
                u.globals.insert(d.name.clone(), (*addr, d.ty.clone()));
            }
            None => {
                // declared but defined nowhere: give it storage
                let size = u.types.size(&d.ty).max(4);
                let addr = u.t.alloc(size, 4);
                u.globals.insert(d.name.clone(), (addr, d.ty.clone()));
            }
        }
    }
    // initializers
    for item in &unit.items {
        let Item::Var(d) = item else { continue };
        if let Some(init) = &d.init {
            let (addr, ty) = u.globals[&d.name].clone();
            let mut f = Fun::new("<init>", Ty::Void);
            u.init_global(&mut f, addr, &ty, init)?;
            for (op, line) in f.code.into_iter().zip(f.lines) {
                u.init.push((op, line));
            }
        }
    }
    // functions
    for item in &unit.items {
        if let Item::Func(func) = item {
            let (index, _) = u.funcs[&func.name];
            let f = u
                .function(func)
                .map_err(|e| format!("{}: {e}", func.name))?;
            u.t.set_function(index, f);
        }
    }
    // the start: initializers, then main
    let start = u.t.new_function("<start>");
    // a program starts with zeroed globals, each time it is called
    let mut code: Vec<Op> = Vec::new();
    for (addr, size) in &own {
        code.extend([Op::Push(*addr), Op::Zero(*size)]);
    }
    let mut lines: Vec<u32> = vec![0; code.len()];
    code.extend(u.init.iter().map(|p| p.0));
    lines.extend(u.init.iter().map(|p| p.1));
    match u.funcs.get("main") {
        Some((i, _))
            if !u
                .root
                .is_some_and(|r| r.funcs.get("main").is_some_and(|m| m.0 == *i)) =>
        {
            code.push(Op::Call(*i, 0));
        }
        _ => code.push(Op::Push(0)),
    }
    code.push(Op::Ret);
    lines.extend([0, 0]);
    u.t.set_function(
        start,
        Function {
            name: "<start>".into(),
            code,
            lines,
            file: String::new(),
            frame: 0,
            params: Vec::new(),
            defined: true,
        },
    );
    let exports = Exports {
        globals: u.globals.clone(),
        funcs: u.funcs.clone(),
        types: u.types.clone(),
    };
    Ok(Compiled { start, exports })
}

impl Fun {
    fn new(name: &str, ret: Ty) -> Fun {
        Fun {
            code: Vec::new(),
            lines: Vec::new(),
            scopes: vec![HashMap::new()],
            frame: 0,
            line: 0,
            ret,
            breaks: Vec::new(),
            continues: Vec::new(),
            labels: HashMap::new(),
            gotos: Vec::new(),
            switches: Vec::new(),
            statics: 0,
            name: name.to_string(),
        }
    }

    fn emit(&mut self, op: Op) -> usize {
        self.code.push(op);
        self.lines.push(self.line);
        self.code.len() - 1
    }

    fn here(&self) -> u32 {
        self.code.len() as u32
    }

    fn patch(&mut self, at: usize, to: u32) {
        self.code[at] = match self.code[at] {
            Op::Jmp(_) => Op::Jmp(to),
            Op::Jz(_) => Op::Jz(to),
            Op::Jnz(_) => Op::Jnz(to),
            op => op,
        };
    }

    fn local(&mut self, size: u32, align: u32) -> u32 {
        let a = align.clamp(1, 4);
        let off = self.frame.div_ceil(a) * a;
        self.frame = off + size;
        off
    }

    fn find(&self, name: &str) -> Option<Var> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }
}

fn load_op(t: &Ty) -> Op {
    match t {
        Ty::I8 => Op::Ld8s,
        Ty::U8 => Op::Ld8u,
        Ty::I16 => Op::Ld16s,
        Ty::U16 => Op::Ld16u,
        _ => Op::Ld32,
    }
}

fn store_op(t: &Ty) -> Op {
    match t {
        Ty::I8 | Ty::U8 => Op::St8,
        Ty::I16 | Ty::U16 => Op::St16,
        _ => Op::St32,
    }
}

/// The type of an arithmetic result. Unsigned types stay unsigned when promoted (as in K&R C:
/// Verdia's ISKANJI(x), ((x ^ 0x20) - 0xa1) <= 0x3b, needs a BYTE to promote to unsigned).
fn arith(a: &Ty, b: &Ty) -> Ty {
    let uns = |t: &Ty| matches!(t, Ty::U8 | Ty::U16 | Ty::U32);
    if a.is_float() || b.is_float() {
        Ty::F32
    } else if uns(a) || uns(b) {
        Ty::U32
    } else {
        Ty::I32
    }
}

/// A value's type as an rvalue: arrays and functions become pointers, small ints become int.
fn decay(t: Ty) -> Ty {
    match t {
        Ty::Array(e, _) => Ty::Ptr(e),
        Ty::Func(_) => t.ptr(),
        t => t,
    }
}

impl<'a> Unit<'a> {
    fn err<T>(&self, f: &Fun, what: String) -> R<T> {
        Err(format!("line {}: {what}", f.line))
    }

    fn string(&mut self, s: &[u8]) -> u32 {
        if let Some(&a) = self.strings.get(s) {
            return a;
        }
        let a = self.t.alloc(s.len() as u32 + 1, 1);
        self.t.write(a, s);
        self.strings.insert(s.to_vec(), a);
        a
    }

    fn size(&self, t: &Ty) -> u32 {
        self.types.size(t)
    }

    // ---- functions

    fn function(&mut self, func: &Func) -> R<Function> {
        let mut f = Fun::new(&func.name, func.ty.ret.clone());
        let mut params = Vec::new();
        for (name, ty) in &func.params {
            let size = self.size(ty).max(4);
            let off = f.local(size, 4);
            params.push((off, if matches!(ty, Ty::Struct(_)) { size } else { 0 }));
            f.scopes[0].insert(name.clone(), Var::Local(off, ty.clone()));
        }
        self.stmt(&mut f, &func.body)?;
        // falling off the end returns 0
        f.emit(Op::Push(0));
        f.emit(Op::Ret);
        for (at, label) in std::mem::take(&mut f.gotos) {
            let Some(&to) = f.labels.get(&label) else {
                return self.err(&f, format!("no label {label}"));
            };
            f.patch(at, to);
        }
        Ok(Function {
            name: func.name.clone(),
            code: f.code,
            lines: f.lines,
            file: func.file.clone(),
            frame: f.frame,
            params,
            defined: true,
        })
    }

    // ---- statements

    fn stmt(&mut self, f: &mut Fun, s: &Stmt) -> R<()> {
        match s {
            Stmt::Expr(e, line) => {
                f.line = *line;
                self.expr(f, e)?;
                f.emit(Op::Pop);
            }
            Stmt::Decls(ds, line) => {
                f.line = *line;
                for d in ds {
                    self.local_decl(f, d)?;
                }
            }
            Stmt::Block(v) => {
                f.scopes.push(HashMap::new());
                for s in v {
                    self.stmt(f, s)?;
                }
                f.scopes.pop();
            }
            Stmt::If(c, a, b, line) => {
                f.line = *line;
                self.cond(f, c)?;
                let jz = f.emit(Op::Jz(0));
                self.stmt(f, a)?;
                match b {
                    Some(b) => {
                        let jmp = f.emit(Op::Jmp(0));
                        let h = f.here();
                        f.patch(jz, h);
                        self.stmt(f, b)?;
                        let h = f.here();
                        f.patch(jmp, h);
                    }
                    None => {
                        let h = f.here();
                        f.patch(jz, h);
                    }
                }
            }
            Stmt::While(c, body, line) => {
                f.line = *line;
                let top = f.here();
                self.cond(f, c)?;
                let jz = f.emit(Op::Jz(0));
                f.breaks.push(vec![jz]);
                f.continues.push(Vec::new());
                self.stmt(f, body)?;
                f.emit(Op::Jmp(top));
                self.close_loop(f, top);
            }
            Stmt::Do(body, c, line) => {
                let top = f.here();
                f.breaks.push(Vec::new());
                f.continues.push(Vec::new());
                self.stmt(f, body)?;
                let cont = f.here();
                f.line = *line;
                self.cond(f, c)?;
                f.emit(Op::Jnz(top));
                self.close_loop(f, cont);
            }
            Stmt::For(init, c, step, body, line) => {
                f.line = *line;
                f.scopes.push(HashMap::new());
                if let Some(i) = init {
                    self.stmt(f, i)?;
                }
                let top = f.here();
                let mut breaks = Vec::new();
                if let Some(c) = c {
                    self.cond(f, c)?;
                    breaks.push(f.emit(Op::Jz(0)));
                }
                f.breaks.push(breaks);
                f.continues.push(Vec::new());
                self.stmt(f, body)?;
                let cont = f.here();
                if let Some(s) = step {
                    self.expr(f, s)?;
                    f.emit(Op::Pop);
                }
                f.emit(Op::Jmp(top));
                self.close_loop(f, cont);
                f.scopes.pop();
            }
            Stmt::Switch(e, body, line) => {
                f.line = *line;
                let t = self.expr(f, e)?;
                if t.is_float() {
                    f.emit(Op::F2I);
                }
                let tmp = f.local(4, 4);
                f.emit(Op::Fp(tmp));
                f.emit(Op::Swap);
                f.emit(Op::St32);
                f.emit(Op::Pop);
                let to_dispatch = f.emit(Op::Jmp(0));
                f.breaks.push(Vec::new());
                f.switches.push((Vec::new(), None));
                self.stmt(f, body)?;
                let end_jump = f.emit(Op::Jmp(0));
                let (cases, default) = f.switches.pop().unwrap();
                let h = f.here();
                f.patch(to_dispatch, h);
                for (v, at) in cases {
                    f.emit(Op::Fp(tmp));
                    f.emit(Op::Ld32);
                    f.emit(Op::Push(v));
                    f.emit(Op::Eq);
                    f.emit(Op::Jnz(at));
                }
                let last = f.emit(Op::Jmp(0));
                match default {
                    Some(d) => f.patch(last, d),
                    None => f.breaks.last_mut().unwrap().push(last),
                }
                let end = f.here();
                f.patch(end_jump, end);
                for b in f.breaks.pop().unwrap() {
                    f.patch(b, end);
                }
            }
            Stmt::Case(e, line) => {
                f.line = *line;
                let Some(v) = const_eval(e, &self.types) else {
                    return self.err(f, "case value not constant".into());
                };
                let h = f.here();
                match f.switches.last_mut() {
                    Some(s) => s.0.push((v as u32, h)),
                    None => return self.err(f, "case outside switch".into()),
                }
            }
            Stmt::Default => {
                let h = f.here();
                if let Some(s) = f.switches.last_mut() {
                    s.1 = Some(h);
                }
            }
            Stmt::Break(line) => {
                f.line = *line;
                let j = f.emit(Op::Jmp(0));
                match f.breaks.last_mut() {
                    Some(b) => b.push(j),
                    None => return self.err(f, "break outside a loop".into()),
                }
            }
            Stmt::Continue(line) => {
                f.line = *line;
                let j = f.emit(Op::Jmp(0));
                match f.continues.last_mut() {
                    Some(b) => b.push(j),
                    None => return self.err(f, "continue outside a loop".into()),
                }
            }
            Stmt::Return(e, line) => {
                f.line = *line;
                match e {
                    Some(e) => {
                        let t = self.expr(f, e)?;
                        let ret = f.ret.clone();
                        self.convert(f, &t, &ret);
                    }
                    None => {
                        f.emit(Op::Push(0));
                    }
                }
                f.emit(Op::Ret);
            }
            Stmt::Goto(l, line) => {
                f.line = *line;
                let j = f.emit(Op::Jmp(0));
                f.gotos.push((j, l.clone()));
            }
            Stmt::Label(l) => {
                let h = f.here();
                f.labels.insert(l.clone(), h);
            }
            Stmt::Empty => {}
        }
        Ok(())
    }

    fn close_loop(&mut self, f: &mut Fun, cont: u32) {
        let end = f.here();
        for b in f.breaks.pop().unwrap() {
            f.patch(b, end);
        }
        for c in f.continues.pop().unwrap() {
            f.patch(c, cont);
        }
    }

    fn local_decl(&mut self, f: &mut Fun, d: &Decl) -> R<()> {
        if d.is_extern {
            // a global seen from here
            if let Some((addr, _)) = self
                .globals
                .get(&d.name)
                .cloned()
                .or_else(|| self.root.and_then(|r| r.globals.get(&d.name).cloned()))
            {
                f.scopes
                    .last_mut()
                    .unwrap()
                    .insert(d.name.clone(), Var::Global(addr, d.ty.clone()));
            }
            return Ok(());
        }
        if d.is_static {
            // kept between calls: global storage, initialized once with the program
            let size = self.size(&d.ty).max(1);
            let addr = self.t.alloc(size, self.types.align(&d.ty).max(4));
            f.statics += 1;
            if let Some(init) = &d.init {
                let mut g = Fun::new("<init>", Ty::Void);
                self.init_global(&mut g, addr, &d.ty, init)?;
                for (op, line) in g.code.into_iter().zip(g.lines) {
                    self.init.push((op, line));
                }
            }
            f.scopes
                .last_mut()
                .unwrap()
                .insert(d.name.clone(), Var::Global(addr, d.ty.clone()));
            return Ok(());
        }
        let size = self.size(&d.ty).max(1);
        let off = f.local(size, self.types.align(&d.ty));
        f.scopes
            .last_mut()
            .unwrap()
            .insert(d.name.clone(), Var::Local(off, d.ty.clone()));
        if let Some(init) = &d.init {
            match (&d.ty, init) {
                (t, Init::Expr(e)) if !matches!(t, Ty::Array(..)) => {
                    // an ordinary assignment
                    f.emit(Op::Fp(off));
                    self.assign_value(f, &d.ty, e)?;
                    f.emit(Op::Pop);
                }
                _ => {
                    f.emit(Op::Fp(off));
                    f.emit(Op::Zero(size));
                    self.init_at(
                        f,
                        &|f: &mut Fun, o| {
                            f.emit(Op::Fp(off + o));
                        },
                        0,
                        &d.ty,
                        init,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn init_global(&mut self, f: &mut Fun, addr: u32, ty: &Ty, init: &Init) -> R<()> {
        self.init_at(
            f,
            &|f: &mut Fun, o| {
                f.emit(Op::Push(addr + o));
            },
            0,
            ty,
            init,
        )
    }

    /// Code storing an initializer into an object at `base` + `off`.
    fn init_at(
        &mut self,
        f: &mut Fun,
        base: &dyn Fn(&mut Fun, u32),
        off: u32,
        ty: &Ty,
        init: &Init,
    ) -> R<()> {
        match init {
            Init::List(items) => {
                let mut it = items.iter().peekable();
                self.init_list(f, base, off, ty, &mut it)?;
            }
            Init::Expr(Expr::Str(s)) if matches!(ty, Ty::Array(e, _) if self.size(e) == 1) => {
                let n = match ty {
                    Ty::Array(_, n) => *n,
                    _ => 0,
                };
                let mut bytes = s.clone();
                bytes.push(0);
                bytes.truncate(n as usize);
                let lit = self.string(&bytes);
                base(f, off);
                f.emit(Op::Push(lit));
                f.emit(Op::Copy(bytes.len() as u32));
                f.emit(Op::Pop);
            }
            Init::Expr(e) => {
                base(f, off);
                self.assign_value(f, ty, e)?;
                f.emit(Op::Pop);
            }
        }
        Ok(())
    }

    fn init_list<'b>(
        &mut self,
        f: &mut Fun,
        base: &dyn Fn(&mut Fun, u32),
        off: u32,
        ty: &Ty,
        it: &mut std::iter::Peekable<std::slice::Iter<'b, Init>>,
    ) -> R<()> {
        match ty {
            Ty::Array(e, n) => {
                let es = self.size(e);
                let mut k = 0;
                while (*n == 0 || k < *n) && it.peek().is_some() {
                    self.init_elem(f, base, off + k * es, e, it)?;
                    k += 1;
                }
            }
            Ty::Struct(s) => {
                let fields = self.types.structs[*s].fields.clone();
                let union = self.types.structs[*s].union;
                for fl in fields {
                    if it.peek().is_none() {
                        break;
                    }
                    self.init_elem(f, base, off + fl.off, &fl.ty, it)?;
                    if union {
                        break;
                    }
                }
            }
            t => {
                if let Some(i) = it.next() {
                    self.init_at(f, base, off, t, i)?;
                }
            }
        }
        Ok(())
    }

    fn init_elem<'b>(
        &mut self,
        f: &mut Fun,
        base: &dyn Fn(&mut Fun, u32),
        off: u32,
        ty: &Ty,
        it: &mut std::iter::Peekable<std::slice::Iter<'b, Init>>,
    ) -> R<()> {
        let aggregate = matches!(ty, Ty::Array(..) | Ty::Struct(_));
        match it.peek() {
            Some(Init::List(_)) => {
                let i = it.next().unwrap();
                self.init_at(f, base, off, ty, i)
            }
            Some(Init::Expr(Expr::Str(_))) if matches!(ty, Ty::Array(e, _) if self.size(e) == 1) => {
                let i = it.next().unwrap();
                self.init_at(f, base, off, ty, i)
            }
            Some(_) if aggregate => self.init_list(f, base, off, ty, it),
            Some(_) => {
                let i = it.next().unwrap();
                self.init_at(f, base, off, ty, i)
            }
            None => Ok(()),
        }
    }

    // ---- expressions

    /// A condition: leaves 0 or not 0.
    fn cond(&mut self, f: &mut Fun, e: &Expr) -> R<()> {
        let t = self.expr(f, e)?;
        if t.is_float() {
            f.emit(Op::FBool);
        }
        Ok(())
    }

    /// Converts the value on the stack from one type to another.
    fn convert(&mut self, f: &mut Fun, from: &Ty, to: &Ty) {
        let from = decay(from.clone());
        match (from.is_float(), to.is_float()) {
            (false, true) => {
                f.emit(if from == Ty::U32 { Op::U2F } else { Op::I2F });
            }
            (true, false) => {
                f.emit(if *to == Ty::U32 { Op::F2U } else { Op::F2I });
                self.narrow(f, to);
            }
            (false, false) => {
                if self.size(&from) > self.size(to)
                    || (from.unsigned() != to.unsigned() && self.size(to) < 4)
                {
                    self.narrow(f, to);
                }
            }
            _ => {}
        }
    }

    fn narrow(&mut self, f: &mut Fun, to: &Ty) {
        match to {
            Ty::I8 => {
                f.emit(Op::Sx8);
            }
            Ty::U8 => {
                f.emit(Op::Zx8);
            }
            Ty::I16 => {
                f.emit(Op::Sx16);
            }
            Ty::U16 => {
                f.emit(Op::Zx16);
            }
            _ => {}
        }
    }

    /// [addr] -> [value]: stores an expression's value (converted) into an object of type `ty`.
    fn assign_value(&mut self, f: &mut Fun, ty: &Ty, e: &Expr) -> R<()> {
        if let Ty::Struct(_) | Ty::Array(..) = ty {
            let n = self.size(ty);
            self.expr(f, e)?;
            f.emit(Op::Copy(n));
            return Ok(());
        }
        let t = self.expr(f, e)?;
        self.convert(f, &t, ty);
        f.emit(store_op(ty));
        Ok(())
    }

    fn var(&self, f: &Fun, name: &str) -> Option<Var> {
        if let Some(v) = f.find(name) {
            return Some(v);
        }
        if let Some((a, t)) = self.globals.get(name) {
            return Some(Var::Global(*a, t.clone()));
        }
        if let Some(r) = self.root
            && let Some((a, t)) = r.globals.get(name)
        {
            return Some(Var::Global(*a, translate(t, &r.types, &self.types)));
        }
        None
    }

    /// Pushes an object's address; returns its type.
    fn addr(&mut self, f: &mut Fun, e: &Expr) -> R<Ty> {
        match e {
            Expr::Name(n) => match self.var(f, n) {
                Some(Var::Local(off, t)) => {
                    f.emit(Op::Fp(off));
                    Ok(t)
                }
                Some(Var::Global(a, t)) => {
                    f.emit(Op::Push(a));
                    Ok(t)
                }
                None => self.err(f, format!("{n} is not known")),
            },
            Expr::Unary(UOp::Deref, p) => {
                let t = self.expr(f, p)?;
                match t {
                    Ty::Ptr(b) => Ok(*b),
                    Ty::Func(_) => Ok(t),
                    _ => Ok(Ty::I32),
                }
            }
            Expr::Index(a, i) => {
                let (pa, pi) = (decay(self.type_of(f, a)?), decay(self.type_of(f, i)?));
                let (a, i) = if pa.is_ptr() || !pi.is_ptr() {
                    (a, i)
                } else {
                    (i, a)
                };
                let t = self.expr(f, a)?;
                let elem = match t {
                    Ty::Ptr(b) => *b,
                    _ => Ty::U8,
                };
                let ti = self.expr(f, i)?;
                if ti.is_float() {
                    f.emit(Op::F2I);
                }
                let es = self.size(&elem);
                if es != 1 {
                    f.emit(Op::Push(es));
                    f.emit(Op::Mul);
                }
                f.emit(Op::Add);
                Ok(elem)
            }
            Expr::Member(o, name, arrow) => {
                let st = if *arrow {
                    match self.expr(f, o)? {
                        Ty::Ptr(b) => *b,
                        t => t,
                    }
                } else {
                    self.addr(f, o)?
                };
                let Ty::Struct(s) = st else {
                    return self.err(f, format!(".{name} of something not a struct"));
                };
                let Some(fl) = self.types.field(s, name).cloned() else {
                    return self.err(
                        f,
                        format!("no field {name} in {}", self.types.structs[s].name),
                    );
                };
                if fl.off != 0 {
                    f.emit(Op::Push(fl.off));
                    f.emit(Op::Add);
                }
                Ok(fl.ty)
            }
            Expr::Str(s) => {
                let a = self.string(s);
                f.emit(Op::Push(a));
                Ok(Ty::Array(Box::new(Ty::I8), s.len() as u32 + 1))
            }
            Expr::Cast(t, e) => {
                // (T)x as an object: take x's address
                self.addr(f, e)?;
                Ok(t.clone())
            }
            Expr::Comma(a, b) => {
                self.expr(f, a)?;
                f.emit(Op::Pop);
                self.addr(f, b)
            }
            Expr::Assign(..) | Expr::Call(..) | Expr::Cond(..) => {
                // a struct value is its address
                let t = self.expr(f, e)?;
                Ok(t)
            }
            _ => self.err(f, "not an object".into()),
        }
    }

    /// The type of an expression, without code.
    fn type_of(&mut self, f: &mut Fun, e: &Expr) -> R<Ty> {
        let (n, l) = (f.code.len(), f.lines.len());
        let saved_line = f.line;
        let t = self.expr(f, e);
        f.code.truncate(n);
        f.lines.truncate(l);
        f.line = saved_line;
        t
    }

    /// Pushes an expression's value; returns its type (arrays as pointers; a struct value
    /// is its address and keeps its struct type).
    fn expr(&mut self, f: &mut Fun, e: &Expr) -> R<Ty> {
        match e {
            Expr::Int(v, t) => {
                f.emit(Op::Push(*v));
                Ok(t.clone())
            }
            Expr::Float(x) => {
                f.emit(Op::Push(x.to_bits()));
                Ok(Ty::F32)
            }
            Expr::Str(s) => {
                let a = self.string(s);
                f.emit(Op::Push(a));
                Ok(Ty::I8.ptr())
            }
            Expr::Name(n) => {
                if self.var(f, n).is_none() {
                    // a function as a value
                    if let Some((i, ft)) = self.funcs.get(n) {
                        f.emit(Op::Push(FN_BASE + i));
                        return Ok(Ty::Func(ft.clone()).ptr());
                    }
                    if let Some(r) = self.root
                        && let Some((i, ft)) = r.funcs.get(n)
                    {
                        f.emit(Op::Push(FN_BASE + i));
                        return Ok(Ty::Func(ft.clone()).ptr());
                    }
                    if self.natives.contains_key(n) {
                        let id = self.t.native(n);
                        f.emit(Op::Push(NATIVE_BASE + id as u32));
                        return Ok(Ty::U8.ptr());
                    }
                    return self.err(f, format!("{n} is not known"));
                }
                let t = self.addr(f, e)?;
                Ok(self.load(f, t))
            }
            Expr::Unary(op, x) => self.unary(f, *op, x),
            Expr::Bin(op, a, b) => self.binary(f, *op, a, b),
            Expr::Assign(op, l, r) => {
                let lt = self.addr(f, l)?;
                match op {
                    None => {
                        self.assign_value(f, &lt, r)?;
                    }
                    Some(op) => {
                        f.emit(Op::Dup);
                        let cur = self.load(f, lt.clone());
                        let rt = self.expr(f, r)?;
                        let res = self.arith_op(f, *op, &cur, &rt)?;
                        self.convert(f, &res, &lt);
                        f.emit(store_op(&lt));
                    }
                }
                Ok(lt)
            }
            Expr::Cond(c, a, b) => {
                self.cond(f, c)?;
                let jz = f.emit(Op::Jz(0));
                let (ta, tb) = (self.type_of(f, a)?, self.type_of(f, b)?);
                let float = ta.is_float() || tb.is_float();
                let t = self.expr(f, a)?;
                if float && !t.is_float() {
                    self.convert(f, &t, &Ty::F32);
                }
                let jmp = f.emit(Op::Jmp(0));
                let h = f.here();
                f.patch(jz, h);
                let t2 = self.expr(f, b)?;
                if float && !t2.is_float() {
                    self.convert(f, &t2, &Ty::F32);
                }
                let h = f.here();
                f.patch(jmp, h);
                Ok(if float {
                    Ty::F32
                } else if ta.is_ptr() {
                    decay(ta)
                } else if tb.is_ptr() {
                    decay(tb)
                } else if matches!(ta, Ty::Struct(_)) {
                    ta
                } else {
                    arith(&ta, &tb)
                })
            }
            Expr::Comma(a, b) => {
                self.expr(f, a)?;
                f.emit(Op::Pop);
                self.expr(f, b)
            }
            Expr::Call(callee, args) => self.call(f, callee, args),
            Expr::Index(..) | Expr::Member(..) => {
                let t = self.addr(f, e)?;
                Ok(self.load(f, t))
            }
            Expr::Cast(t, x) => {
                let from = self.expr(f, x)?;
                if !matches!(t, Ty::Void) {
                    self.convert(f, &from, t);
                }
                Ok(t.clone())
            }
            Expr::SizeofTy(t) => {
                let n = self.size(t);
                f.emit(Op::Push(n));
                Ok(Ty::U32)
            }
            Expr::SizeofE(x) => {
                let t = match &**x {
                    Expr::Str(s) => Ty::Array(Box::new(Ty::I8), s.len() as u32 + 1),
                    _ => self.object_type(f, x)?,
                };
                let n = self.size(&t);
                f.emit(Op::Push(n));
                Ok(Ty::U32)
            }
        }
    }

    /// The type of an expression as an object (arrays not decayed), for sizeof.
    fn object_type(&mut self, f: &mut Fun, e: &Expr) -> R<Ty> {
        let (n, l) = (f.code.len(), f.lines.len());
        let t = match e {
            Expr::Name(_) | Expr::Index(..) | Expr::Member(..) | Expr::Unary(UOp::Deref, _) => {
                self.addr(f, e)
            }
            _ => self.expr(f, e),
        };
        f.code.truncate(n);
        f.lines.truncate(l);
        t
    }

    /// [addr] -> [value] of an object of type t; returns the value's type.
    fn load(&mut self, f: &mut Fun, t: Ty) -> Ty {
        match t {
            Ty::Array(e, _) => Ty::Ptr(e),
            Ty::Struct(_) => t,
            Ty::Func(_) => t.ptr(),
            t => {
                f.emit(load_op(&t));
                t
            }
        }
    }

    fn unary(&mut self, f: &mut Fun, op: UOp, x: &Expr) -> R<Ty> {
        match op {
            UOp::Neg => {
                let t = self.expr(f, x)?;
                f.emit(if t.is_float() { Op::FNeg } else { Op::Neg });
                Ok(if t.is_float() { t } else { arith(&t, &Ty::I32) })
            }
            UOp::Plus => self.expr(f, x),
            UOp::BitNot => {
                let t = self.expr(f, x)?;
                f.emit(Op::BitNot);
                Ok(arith(&t, &Ty::I32))
            }
            UOp::Not => {
                let t = self.expr(f, x)?;
                if t.is_float() {
                    f.emit(Op::FBool);
                }
                f.emit(Op::LNot);
                Ok(Ty::I32)
            }
            UOp::Deref => {
                let t = self.expr(f, x)?;
                match t {
                    Ty::Ptr(b) => {
                        if matches!(*b, Ty::Func(_)) {
                            return Ok(Ty::Ptr(b));
                        }
                        Ok(self.load(f, *b))
                    }
                    _ => {
                        f.emit(Op::Ld32);
                        Ok(Ty::I32)
                    }
                }
            }
            UOp::Addr => {
                if let Expr::Name(n) = x
                    && self.var(f, n).is_none()
                {
                    // &function
                    return self.expr(f, x);
                }
                let t = self.addr(f, x)?;
                Ok(t.ptr())
            }
            UOp::PreInc | UOp::PreDec | UOp::PostInc | UOp::PostDec => {
                let t = self.addr(f, x)?;
                let up = matches!(op, UOp::PreInc | UOp::PostInc);
                let post = matches!(op, UOp::PostInc | UOp::PostDec);
                if t.is_float() {
                    f.emit(Op::FIncDec {
                        delta: if up { 1 } else { -1 },
                        post,
                    });
                } else {
                    let step = match &t {
                        Ty::Ptr(b) => self.size(b).max(1) as i32,
                        _ => 1,
                    };
                    let width = match self.size(&t) {
                        1 => 1,
                        2 => 2,
                        _ => 4,
                    };
                    f.emit(Op::IncDec {
                        width,
                        signed: !t.unsigned(),
                        delta: if up { step } else { -step },
                        post,
                    });
                }
                Ok(t)
            }
        }
    }

    fn binary(&mut self, f: &mut Fun, op: BOp, a: &Expr, b: &Expr) -> R<Ty> {
        match op {
            BOp::LAnd | BOp::LOr => {
                self.cond(f, a)?;
                let j = f.emit(if op == BOp::LAnd {
                    Op::Jz(0)
                } else {
                    Op::Jnz(0)
                });
                self.cond(f, b)?;
                f.emit(Op::LNot);
                f.emit(Op::LNot);
                let end = f.emit(Op::Jmp(0));
                let h = f.here();
                f.patch(j, h);
                f.emit(Op::Push(if op == BOp::LAnd { 0 } else { 1 }));
                let h = f.here();
                f.patch(end, h);
                Ok(Ty::I32)
            }
            _ => {
                let ta = self.expr(f, a)?;
                let tb = self.type_of(f, b)?;
                // convert the left before the right is pushed
                let res = arith(&ta, &tb);
                if res.is_float() && !ta.is_float() && !ta.is_ptr() {
                    self.convert(f, &ta, &Ty::F32);
                }
                let tb = self.expr(f, b)?;
                let ta2 = if res.is_float() && !ta.is_ptr() {
                    Ty::F32
                } else {
                    ta
                };
                self.arith_op(f, op, &ta2, &tb)
            }
        }
    }

    /// [a, b] -> [a op b], `a` already converted for a float result.
    fn arith_op(&mut self, f: &mut Fun, op: BOp, ta: &Ty, tb: &Ty) -> R<Ty> {
        let ta = decay(ta.clone());
        let tb = decay(tb.clone());
        // pointer arithmetic
        if matches!(op, BOp::Add | BOp::Sub) && (ta.is_ptr() || tb.is_ptr()) {
            if ta.is_ptr() && tb.is_ptr() && op == BOp::Sub {
                f.emit(Op::Sub);
                let es = ta.target().map_or(1, |t| self.size(t)).max(1);
                if es != 1 {
                    f.emit(Op::Push(es));
                    f.emit(Op::DivS);
                }
                return Ok(Ty::I32);
            }
            if ta.is_ptr() {
                if tb.is_float() {
                    f.emit(Op::F2I);
                }
                let es = ta.target().map_or(1, |t| self.size(t)).max(1);
                if es != 1 {
                    f.emit(Op::Push(es));
                    f.emit(Op::Mul);
                }
                f.emit(if op == BOp::Add { Op::Add } else { Op::Sub });
                return Ok(ta);
            }
            // int + pointer
            let es = tb.target().map_or(1, |t| self.size(t)).max(1);
            f.emit(Op::Swap);
            if es != 1 {
                f.emit(Op::Push(es));
                f.emit(Op::Mul);
            }
            f.emit(Op::Add);
            return Ok(tb);
        }
        let res = arith(&ta, &tb);
        if res.is_float() {
            if !tb.is_float() {
                self.convert(f, &tb, &Ty::F32);
            }
            if !ta.is_float() {
                // the left was not converted (compound assignment): do it under the right
                f.emit(Op::Swap);
                self.convert(f, &ta, &Ty::F32);
                f.emit(Op::Swap);
            }
            let o = match op {
                BOp::Add => Op::FAdd,
                BOp::Sub => Op::FSub,
                BOp::Mul => Op::FMul,
                BOp::Div => Op::FDiv,
                BOp::Mod => Op::FMod,
                BOp::Eq => Op::FEq,
                BOp::Ne => Op::FNe,
                BOp::Lt => Op::FLt,
                BOp::Le => Op::FLe,
                BOp::Gt => Op::FGt,
                BOp::Ge => Op::FGe,
                _ => return self.err(f, format!("{op:?} on floats")),
            };
            f.emit(o);
            return Ok(
                if matches!(
                    op,
                    BOp::Eq | BOp::Ne | BOp::Lt | BOp::Le | BOp::Gt | BOp::Ge
                ) {
                    Ty::I32
                } else {
                    Ty::F32
                },
            );
        }
        let uns = res == Ty::U32 || ta.is_ptr() || tb.is_ptr();
        let o = match op {
            BOp::Add => Op::Add,
            BOp::Sub => Op::Sub,
            BOp::Mul => Op::Mul,
            BOp::Div => {
                if uns {
                    Op::DivU
                } else {
                    Op::DivS
                }
            }
            BOp::Mod => {
                if uns {
                    Op::ModU
                } else {
                    Op::ModS
                }
            }
            BOp::Shl => Op::Shl,
            BOp::Shr => {
                if ta.unsigned() && self.size(&ta) >= 4 {
                    Op::ShrU
                } else {
                    Op::ShrS
                }
            }
            BOp::And => Op::And,
            BOp::Or => Op::Or,
            BOp::Xor => Op::Xor,
            BOp::Eq => Op::Eq,
            BOp::Ne => Op::Ne,
            BOp::Lt => {
                if uns {
                    Op::LtU
                } else {
                    Op::LtS
                }
            }
            BOp::Le => {
                if uns {
                    Op::LeU
                } else {
                    Op::LeS
                }
            }
            BOp::Gt => {
                if uns {
                    Op::GtU
                } else {
                    Op::GtS
                }
            }
            BOp::Ge => {
                if uns {
                    Op::GeU
                } else {
                    Op::GeS
                }
            }
            BOp::LAnd | BOp::LOr => unreachable!(),
        };
        f.emit(o);
        Ok(match op {
            BOp::Eq | BOp::Ne | BOp::Lt | BOp::Le | BOp::Gt | BOp::Ge => Ty::I32,
            BOp::Shl | BOp::Shr => arith(&ta, &Ty::I32),
            _ => res,
        })
    }

    fn call(&mut self, f: &mut Fun, callee: &Expr, args: &[Expr]) -> R<Ty> {
        // a named function, a built-in, or a pointer
        if let Expr::Name(n) = callee
            && self.var(f, n).is_none()
        {
            if let Some((i, ft)) = self.funcs.get(n).cloned() {
                self.args(f, &ft, args)?;
                f.emit(Op::Call(i, args.len() as u8));
                return Ok(ft.ret.clone());
            }
            if let Some(ft) = self.natives.get(n).cloned() {
                self.args(f, &ft, args)?;
                let id = self.t.native(n);
                f.emit(Op::Native(id, args.len() as u8));
                return Ok(ft.ret.clone());
            }
            if let Some(r) = self.root
                && let Some((i, ft)) = r.funcs.get(n).cloned()
            {
                let ft = match translate(&Ty::Func(ft), &r.types, &self.types) {
                    Ty::Func(ft) => ft,
                    _ => unreachable!(),
                };
                self.args(f, &ft, args)?;
                f.emit(Op::Call(i, args.len() as u8));
                return Ok(ft.ret.clone());
            }
            // not declared: a built-in the header does not list, or a function defined nowhere
            let ft = Rc::new(FuncTy {
                ret: Ty::I32,
                params: Vec::new(),
                variadic: true,
                known: false,
            });
            self.args(f, &ft, args)?;
            let id = self.t.native(n);
            f.emit(Op::Native(id, args.len() as u8));
            return Ok(Ty::I32);
        }
        let t = self.expr(f, callee)?;
        let ft = match &t {
            Ty::Ptr(b) => match &**b {
                Ty::Func(ft) => ft.clone(),
                _ => Rc::new(FuncTy {
                    ret: Ty::I32,
                    params: Vec::new(),
                    variadic: true,
                    known: false,
                }),
            },
            Ty::Func(ft) => ft.clone(),
            _ => Rc::new(FuncTy {
                ret: Ty::I32,
                params: Vec::new(),
                variadic: true,
                known: false,
            }),
        };
        // the pointer goes under the arguments: keep it in a temporary
        let tmp = f.local(4, 4);
        f.emit(Op::Fp(tmp));
        f.emit(Op::Swap);
        f.emit(Op::St32);
        f.emit(Op::Pop);
        self.args(f, &ft, args)?;
        f.emit(Op::Fp(tmp));
        f.emit(Op::Ld32);
        f.emit(Op::CallInd(args.len() as u8));
        Ok(ft.ret.clone())
    }

    fn args(&mut self, f: &mut Fun, ft: &FuncTy, args: &[Expr]) -> R<()> {
        for (k, a) in args.iter().enumerate() {
            let t = self.expr(f, a)?;
            match ft.params.get(k) {
                Some(p) if ft.known => self.convert(f, &t, p),
                _ => {}
            }
        }
        Ok(())
    }
}
