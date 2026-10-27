//! The Cotopha virtual machine: a stack machine whose stack holds the local variables of the
//! running functions and the operands of expressions, as references to variables. Each script
//! thread has its own stack; engine calls that wait (Sleep, GetCommand, ...) suspend a thread
//! until a later frame.

use std::collections::HashMap;
use std::rc::Rc;

use crate::csx::{Image, Imm, Init, Key, Mode, Op};
use crate::engine::Engine;
use crate::value::*;

pub struct Program {
    pub ops: Vec<Op>,
    pub addr: Vec<u32>,
    pub at: HashMap<u32, usize>,
    pub funcs: HashMap<String, usize>,
    /// function names by entry
    pub names: HashMap<usize, Str>,
    pub img: Image,
}

impl Program {
    pub fn load(img: Image) -> Result<Program, String> {
        let mut ops = Vec::new();
        let mut addr = Vec::new();
        let mut at = HashMap::new();
        let mut p = 0u32;
        while (p as usize) < img.code.len() {
            let (op, next) = img.decode(p)?;
            at.insert(p, ops.len());
            addr.push(p);
            ops.push(op);
            p = next;
        }
        let mut funcs = HashMap::new();
        let mut names = HashMap::new();
        for (a, n) in &img.functions {
            if let Some(&i) = at.get(a) {
                funcs.insert(n.clone(), i);
                names.insert(i, Str::from(n.as_str()));
            }
        }
        Ok(Program {
            ops,
            addr,
            at,
            funcs,
            names,
            img,
        })
    }

    pub fn index(&self, a: u32) -> Result<usize, String> {
        self.at
            .get(&a)
            .copied()
            .ok_or_else(|| format!("no instruction at {a:#x}"))
    }
}

/// What a returning function hands to its caller.
#[derive(Clone)]
pub enum Ret {
    Push,
    Discard,
    /// push this variable instead of the result (operator overloads on a copy)
    PushVar(Var),
}

pub struct Block {
    height: usize,
    /// a Try block: where its Catch starts
    catch: Option<usize>,
}

pub struct Frame {
    pub ret_pc: usize,
    base: usize,
    pub this: Option<Var>,
    /// "Class" of a "Class::Function"
    pub class: Option<Str>,
    pub func: Str,
    blocks: Vec<Block>,
    ret: Ret,
    entered: bool,
}

#[derive(Default)]
pub struct Thread {
    pub stack: Vec<Var>,
    pub names: Vec<Option<Str>>,
    pub frames: Vec<Frame>,
    pub pc: usize,
    /// kept while another thread runs: when the waiting engine call gives up
    pub deadline: Option<f64>,
    pub done: bool,
    pub result: Option<Var>,
    /// things to run when the stack is empty (start-up: constructors, prologue, main)
    pub queue: Vec<Start>,
}

pub enum Start {
    /// construct the object of a global or data variable
    Construct(Var, Str),
    Call(usize),
    /// a thread's function: (function, this, arguments)
    Run(usize, Option<Var>, Vec<Var>),
}

/// What an engine call returns.
pub enum Done {
    Val(Value),
    /// a reference to an existing variable
    Var(Var),
    /// not yet: run the call again in a later frame
    Block,
    /// done, and let the other threads and the screen run before this thread goes on
    Yield(Value),
    /// call a script function: (function, this, arguments)
    Script(usize, Option<Var>, Vec<Var>),
}

impl Thread {
    fn push(&mut self, v: Var) {
        self.stack.push(v);
        self.names.push(None);
    }
    fn pop(&mut self) -> Result<Var, String> {
        self.names.pop();
        self.stack
            .pop()
            .ok_or_else(|| "stack underflow".to_string())
    }
    fn top(&self) -> Result<Var, String> {
        self.stack
            .last()
            .cloned()
            .ok_or_else(|| "stack underflow".to_string())
    }
    fn truncate(&mut self, n: usize) {
        self.stack.truncate(n);
        self.names.truncate(n);
    }
    pub fn frame(&self) -> Option<&Frame> {
        self.frames.last()
    }
}

/// The default value of a declared type (objects are made by `construct`).
fn default_of(ty: u8) -> Value {
    match ty {
        1 => Value::Ref(None),
        2 => Value::Array(Vec::new()),
        3 => Value::Hash(Vec::new(), None),
        4 | 7 | 8 => Value::Int(0),
        5 => Value::Real(0.0),
        6 => s(""),
        _ => Value::Empty,
    }
}

fn coerce(ty: u8, v: &Var) -> Var {
    let val = deref(v).borrow().clone();
    var(match ty {
        4 | 7 | 8 => Value::Int(val.int()),
        5 => Value::Real(val.real()),
        6 => Value::Str(val.str()),
        2 => match val {
            Value::Array(_) => val.deep(),
            Value::Empty => Value::Array(Vec::new()),
            other => other.deep(),
        },
        _ => val.deep(),
    })
}

pub fn init_value(i: &Init) -> Value {
    match i {
        Init::Object(_) => Value::Empty,
        Init::Reference => Value::Ref(None),
        Init::Array(a) => Value::Array(a.iter().map(|x| var(init_value(x))).collect()),
        Init::Hash => Value::Hash(Vec::new(), None),
        Init::Integer(n) => Value::Int(*n as i32),
        Init::Real(r) => Value::Real(*r),
        Init::String(t) => s(t),
        Init::Members(m) => Value::Struct(
            "".into(),
            m.iter()
                .map(|(k, v)| (Str::from(k.as_str()), var(init_value(v))))
                .collect(),
        ),
    }
}

const BUDGET: u32 = 2_000_000;

impl Engine {
    /// Makes an object of a class: a script structure (and the constructor to run on it), or an
    /// engine object.
    pub fn construct(&mut self, class: &str) -> (Var, Option<usize>) {
        let ctor = self.prog.funcs.get(&format!("{class}::{class}")).copied();
        if ctor.is_some() {
            return (var(Value::Struct(class.into(), Vec::new())), ctor);
        }
        if let Some(n) = self.new_native(class) {
            return (var(n), None);
        }
        self.warn(format!("unknown class {class}"));
        (var(Value::Struct(class.into(), Vec::new())), None)
    }

    fn call_script(
        &mut self,
        t: &mut Thread,
        f: usize,
        this: Option<Var>,
        args: Vec<Var>,
        ret: Ret,
    ) {
        let func: Str = self
            .prog
            .names
            .get(&f)
            .cloned()
            .unwrap_or_else(|| "?".into());
        let class = func.split_once("::").map(|(c, _)| Str::from(c));
        if self.log > 2 {
            eprintln!("entis: {}{func}", " ".repeat(t.frames.len()));
        }
        t.frames.push(Frame {
            ret_pc: t.pc,
            base: t.stack.len(),
            this,
            class,
            func,
            blocks: Vec::new(),
            ret,
            entered: false,
        });
        for a in args {
            t.push(a);
        }
        t.pc = f;
    }

    /// Runs a thread until it waits, yields or ends. Returns an error when the script fails
    /// outside any Try block.
    pub fn run_thread(&mut self, t: &mut Thread) -> Result<(), String> {
        let mut budget = BUDGET;
        loop {
            if t.frames.is_empty() {
                if t.queue.is_empty() {
                    t.done = true;
                    return Ok(());
                }
                match t.queue.remove(0) {
                    Start::Construct(v, class) => {
                        let (obj, ctor) = self.construct(&class);
                        *v.borrow_mut() = obj.borrow().clone();
                        if let Some(c) = ctor {
                            self.call_script(t, c, Some(v.clone()), vec![v], Ret::Discard);
                        }
                    }
                    Start::Call(f) => self.call_script(t, f, None, Vec::new(), Ret::Discard),
                    Start::Run(f, this, args) => self.call_script(t, f, this, args, Ret::Push),
                }
                continue;
            }
            budget -= 1;
            if budget == 0 {
                self.warn(format!(
                    "thread busy for {BUDGET} instructions in {}",
                    t.frame().map_or("?", |f| &f.func)
                ));
                return Ok(());
            }
            let pc = t.pc;
            let op = self
                .prog
                .ops
                .get(pc)
                .cloned()
                .ok_or_else(|| format!("pc {pc} out of the code"))?;
            t.pc = pc + 1;
            match self.step(t, pc, op) {
                Ok(true) => {}
                Ok(false) => return Ok(()),
                Err(e) => {
                    let at = self.prog.addr.get(pc).copied().unwrap_or(0);
                    let func = t.frame().map_or("?".into(), |f| f.func.clone());
                    let msg = format!("{e} (at {at:#x} in {func})");
                    self.warn(format!("script error: {msg}"));
                    if !self.throw(t, vec![var(Value::Int(i32::MIN)), var(s(&msg))])? {
                        return Err(msg);
                    }
                }
            }
        }
    }

    /// Sends an exception to the innermost Try block; false when there is none.
    fn throw(&mut self, t: &mut Thread, args: Vec<Var>) -> Result<bool, String> {
        while let Some(f) = t.frames.last_mut() {
            while let Some(b) = f.blocks.last() {
                if let Some(c) = b.catch {
                    let h = b.height;
                    // keep the Try block: the handler's Leave closes it
                    f.blocks.last_mut().unwrap().catch = None;
                    t.truncate(h);
                    let n = match &self.prog.ops[c] {
                        Op::Enter(_, Ok(p)) => p.len(),
                        _ => 0,
                    };
                    for i in 0..n {
                        t.push(args.get(i).cloned().unwrap_or_else(|| var(Value::Int(0))));
                    }
                    t.pc = c;
                    return Ok(true);
                }
                f.blocks.pop();
            }
            let f = t.frames.pop().unwrap();
            t.truncate(f.base);
            t.pc = f.ret_pc;
        }
        Ok(false)
    }

    fn lookup_name(&self, t: &Thread, n: &str) -> Option<Var> {
        if let Some(f) = t.frame() {
            for i in (f.base..t.stack.len()).rev() {
                if t.names[i].as_deref() == Some(n) {
                    return Some(t.stack[i].clone());
                }
            }
            if let Some(this) = &f.this {
                if let Some(m) = deref(this).borrow().member(n) {
                    return Some(m);
                }
            }
        }
        self.scope_lookup(3, n).or_else(|| self.scope_lookup(4, n))
    }

    fn scope_lookup(&self, scope: u8, n: &str) -> Option<Var> {
        let table = if scope == 3 {
            &self.globals
        } else {
            &self.data
        };
        table
            .iter()
            .find(|(k, _)| &**k == n)
            .map(|(_, v)| v.clone())
    }

    /// The element of an object: a member, array or hash element (made when missing).
    fn element(&mut self, t: &Thread, obj: &Var, key: Key) -> Result<Var, String> {
        let target = deref(obj);
        let mut b = target.borrow_mut();
        if matches!(*b, Value::Empty) {
            *b = match key {
                Key::Name(_) => Value::Hash(Vec::new(), None),
                _ => Value::Array(Vec::new()),
            };
        }
        match (&mut *b, &key) {
            (Value::Struct(_, m), Key::Name(n)) => {
                if let Some((_, v)) = m.iter().find(|(k, _)| k == n) {
                    return Ok(v.clone());
                }
                drop(b);
                let r = target.borrow().member(n);
                match r {
                    Some(v) => Ok(v),
                    None => Err(format!("no member {n}")),
                }
            }
            (Value::Struct(_, m), Key::Index(i)) => {
                // a class made on a parent (Bezier1D: an array) indexes the parent
                if let Some((_, p)) = m.iter().find(|(k, _)| &**k == "parent") {
                    let p = p.clone();
                    drop(b);
                    return self.element(t, &p, key);
                }
                m.get(*i as usize)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| format!("no element {i}"))
            }
            (Value::Hash(h, d), Key::Name(n)) => match hash_find(h, n) {
                Ok(i) => Ok(h[i].1.clone()),
                Err(i) => {
                    let v = var(d.as_deref().cloned().unwrap_or_default().deep());
                    h.insert(i, (n.clone(), v.clone()));
                    Ok(v)
                }
            },
            (Value::Hash(h, _), Key::Index(i)) => h
                .get(*i as usize)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| format!("no element {i}")),
            (Value::Array(a), Key::Index(i)) => {
                if *i < 0 {
                    return Err(format!("array index {i}"));
                }
                let i = *i as usize;
                while a.len() <= i {
                    a.push(var(Value::Empty));
                }
                Ok(a[i].clone())
            }
            (Value::Array(a), Key::Name(n)) => {
                // a number as a string
                let i = parse_int(n).max(0) as usize;
                while a.len() <= i {
                    a.push(var(Value::Empty));
                }
                Ok(a[i].clone())
            }
            (Value::Scope(sc), Key::Name(n)) => {
                let sc = *sc;
                drop(b);
                let r = if sc == 0 {
                    self.lookup_name(t, n)
                } else {
                    self.scope_lookup(sc, n)
                };
                r.ok_or_else(|| format!("no variable {n}"))
            }
            (Value::Obj(_), _) => {
                drop(b);
                self.native_element(&target, &key)
            }
            (other, _) => Err(format!("element {key:?} of a {}", other.type_name())),
        }
    }

    /// Runs one instruction; false: the thread waits.
    fn step(&mut self, t: &mut Thread, pc: usize, op: Op) -> Result<bool, String> {
        match op {
            Op::New(mode, ty, class, name) => {
                let (v, ctor) = match (&class, ty) {
                    (Some(c), 0) => self.construct(c),
                    _ => (var(default_of(ty)), None),
                };
                match mode {
                    Mode::Stack => {
                        t.stack.push(v.clone());
                        t.names.push(Some(name));
                    }
                    _ => {
                        let this = t
                            .frame()
                            .and_then(|f| f.this.clone())
                            .ok_or("New member without this")?;
                        let this = deref(&this);
                        let mut b = this.borrow_mut();
                        match &mut *b {
                            Value::Struct(_, m) => {
                                if let Some(e) = m.iter_mut().find(|(k, _)| *k == name) {
                                    e.1 = v.clone();
                                } else {
                                    m.push((name, v.clone()));
                                }
                            }
                            other => {
                                return Err(format!("member {name} of a {}", other.type_name()));
                            }
                        }
                    }
                }
                if let Some(c) = ctor {
                    self.call_script(t, c, Some(v.clone()), vec![v], Ret::Discard);
                }
            }
            Op::Free => {
                t.pop()?;
            }
            Op::LoadImm(imm) => {
                let v = match imm {
                    Imm::Object(c) => {
                        let (v, ctor) = self.construct(&c);
                        t.push(v.clone());
                        if let Some(c) = ctor {
                            self.call_script(t, c, Some(v.clone()), vec![v], Ret::Discard);
                        }
                        return Ok(true);
                    }
                    Imm::Reference => Value::Ref(None),
                    Imm::Array => Value::Array(Vec::new()),
                    Imm::Hash => Value::Hash(Vec::new(), None),
                    Imm::Integer(i) => Value::Int(i as i32),
                    Imm::Real(r) => Value::Real(r),
                    Imm::String(x) => Value::Str(x),
                };
                t.push(var(v));
            }
            Op::Load(mode, key) => {
                let v = match (mode, key) {
                    (Mode::Stack, Key::Index(i)) => {
                        let base = t.frame().map_or(0, |f| f.base);
                        t.stack
                            .get(base + i as usize)
                            .cloned()
                            .ok_or_else(|| format!("no local {i}"))?
                    }
                    (Mode::Stack, Key::Name(n)) | (Mode::Auto, Key::Name(n)) => self
                        .lookup_name(t, &n)
                        .ok_or_else(|| format!("no variable {n}"))?,
                    (Mode::Stack, Key::Whole) => var(Value::Scope(0)),
                    (Mode::This, Key::Whole) => {
                        t.frame().and_then(|f| f.this.clone()).ok_or("no this")?
                    }
                    (Mode::This, k) => {
                        let this = t.frame().and_then(|f| f.this.clone()).ok_or("no this")?;
                        self.element(t, &this, k)?
                    }
                    (Mode::Global, Key::Index(i)) => self
                        .globals
                        .get(i as usize)
                        .map(|(_, v)| v.clone())
                        .ok_or_else(|| format!("no global {i}"))?,
                    (Mode::Data, Key::Index(i)) => self
                        .data
                        .get(i as usize)
                        .map(|(_, v)| v.clone())
                        .ok_or_else(|| format!("no data {i}"))?,
                    (Mode::Global, Key::Name(n)) => self
                        .scope_lookup(3, &n)
                        .ok_or_else(|| format!("no global {n}"))?,
                    (Mode::Data, Key::Name(n)) => self
                        .scope_lookup(4, &n)
                        .ok_or_else(|| format!("no data {n}"))?,
                    (Mode::Global, Key::Whole) => var(Value::Scope(3)),
                    (Mode::Data, Key::Whole) => var(Value::Scope(4)),
                    (m, k) => return Err(format!("load {m:?} {k:?}")),
                };
                t.push(v);
            }
            Op::Store(op) => {
                let v = t.pop()?;
                let target = t.top()?;
                match op {
                    None => assign(&target, &v),
                    Some(o) => {
                        let d = deref(&target);
                        let class = match &*d.borrow() {
                            Value::Struct(c, _) => Some(c.clone()),
                            _ => None,
                        };
                        if let Some(f) = class
                            .and_then(|c| self.prog.funcs.get(&format!("{c}::@Operate")).copied())
                        {
                            let args = vec![d.clone(), var(Value::Int(o as i32)), v];
                            self.call_script(t, f, Some(d), args, Ret::Discard);
                            return Ok(true);
                        }
                        self.store_op(&d, o, &v)?;
                    }
                }
            }
            Op::Enter(name, params) => {
                let f = t.frames.last_mut().ok_or("Enter outside a function")?;
                if !f.entered {
                    f.entered = true;
                    let base = f.base;
                    let params = params.unwrap_or_default();
                    let n = t.stack.len() - base;
                    for (i, (ty, _class, pname)) in params.iter().enumerate() {
                        if i < n {
                            if *ty != 1 {
                                let c = coerce(*ty, &t.stack[base + i]);
                                t.stack[base + i] = c;
                            }
                            t.names[base + i] = Some(pname.clone());
                        } else {
                            t.stack.push(var(default_of(*ty)));
                            t.names.push(Some(pname.clone()));
                        }
                    }
                    if n > params.len() {
                        t.truncate(base + params.len());
                    }
                } else {
                    match params {
                        Err(c) => {
                            let c = self.prog.index(c)?;
                            let h = t.stack.len();
                            t.frames.last_mut().unwrap().blocks.push(Block {
                                height: h,
                                catch: Some(c),
                            });
                        }
                        Ok(p) => {
                            let k = p.len().min(t.stack.len());
                            let h = t.stack.len() - k;
                            for (i, (ty, _, pname)) in p.iter().enumerate() {
                                let at = h + i;
                                if at < t.stack.len() {
                                    if *ty != 1 {
                                        t.stack[at] = coerce(*ty, &t.stack[at]);
                                    }
                                    t.names[at] = Some(pname.clone());
                                }
                            }
                            let _ = name;
                            t.frames.last_mut().unwrap().blocks.push(Block {
                                height: h,
                                catch: None,
                            });
                        }
                    }
                }
            }
            Op::Leave => {
                let f = t.frames.last_mut().ok_or("Leave outside a function")?;
                let b = f.blocks.pop().ok_or("Leave without a block")?;
                t.truncate(b.height);
            }
            Op::Jump(a) => t.pc = self.prog.index(a)?,
            Op::CJump(c, a) => {
                let v = t.pop()?;
                let truth = v.borrow().truthy();
                if truth == (c != 0) {
                    t.pc = self.prog.index(a)?;
                }
            }
            Op::Call(mode, n, name) => {
                let n = n.max(0) as usize;
                if t.stack.len() < n {
                    return Err("stack underflow".into());
                }
                let at = t.stack.len() - n;
                let args: Vec<Var> = t.stack.drain(at..).collect();
                t.names.truncate(at);
                if mode == Mode::Immediate {
                    // Throw
                    if !self.throw(t, args)? {
                        return Err(format!(
                            "uncaught exception in {}",
                            t.frame().map_or("?", |f| &f.func)
                        ));
                    }
                    return Ok(true);
                }
                let r = if mode == Mode::This {
                    if args.is_empty() {
                        return Err(format!("{name}: no object"));
                    }
                    self.method(t, &args[0], &name, &args[1..])?
                } else if mode == Mode::Global {
                    match self.prog.funcs.get(&*name) {
                        Some(&f) => Done::Script(f, None, args.clone()),
                        None => self.system(t, &name, &args)?,
                    }
                } else {
                    self.call_auto(t, &name, &args)?
                };
                return self.finish_call(t, pc, args, r);
            }
            Op::Return(_) => {
                let f = t.frames.last().ok_or("Return outside a function")?;
                let result =
                    if t.stack.len() > f.base && t.names.last().is_some_and(|n| n.is_none()) {
                        t.pop().ok()
                    } else {
                        None
                    };
                let f = t.frames.pop().unwrap();
                t.truncate(f.base);
                t.pc = f.ret_pc;
                match f.ret {
                    Ret::Push => t.push(result.unwrap_or_else(|| var(Value::Int(0)))),
                    Ret::Discard => {}
                    Ret::PushVar(v) => t.push(v),
                }
                if t.frames.is_empty() {
                    t.result = Some(
                        t.stack
                            .last()
                            .cloned()
                            .unwrap_or_else(|| var(Value::Int(0))),
                    );
                }
            }
            Op::Element(key) => {
                let obj = t.pop()?;
                let v = self.element(t, &obj, key)?;
                t.push(v);
            }
            Op::ElementIndirect => {
                let idx = t.pop()?;
                let obj = t.pop()?;
                let key = match &*deref(&idx).borrow() {
                    Value::Str(x) => Key::Name(x.clone()),
                    other => Key::Index(other.int()),
                };
                let v = self.element(t, &obj, key)?;
                t.push(v);
            }
            Op::Operate(o) => {
                let b = t.pop()?;
                let a = t.pop()?;
                let ad = deref(&a);
                let class = match &*ad.borrow() {
                    Value::Struct(c, _) => Some(c.clone()),
                    _ => None,
                };
                if let Some(f) =
                    class.and_then(|c| self.prog.funcs.get(&format!("{c}::@Operate")).copied())
                {
                    let copy = var(ad.borrow().deep());
                    let args = vec![copy.clone(), var(Value::Int(o as i32)), b];
                    self.call_script(t, f, Some(copy.clone()), args, Ret::PushVar(copy));
                    return Ok(true);
                }
                let r = {
                    let (x, y) = (ad.borrow(), deref(&b));
                    let y = y.borrow();
                    operate(o, &x, &y)?
                };
                t.push(var(r));
            }
            Op::UniOperate(o) => {
                let a = t.pop()?;
                let r = unary(o, &deref(&a).borrow());
                t.push(var(r));
            }
            Op::Compare(c) => {
                let b = t.pop()?;
                let a = t.pop()?;
                let r = compare(c, &deref(&a).borrow(), &deref(&b).borrow());
                t.push(var(Value::Int(if r { -1 } else { 0 })));
            }
        }
        Ok(true)
    }

    fn store_op(&mut self, d: &Var, o: u8, v: &Var) -> Result<(), String> {
        let vv = deref(v).borrow().clone();
        let mut b = d.borrow_mut();
        match &mut *b {
            Value::Array(a) if o == 0 => {
                a.push(var(vv.deep()));
                return Ok(());
            }
            Value::Hash(h, _) if o == 0 => {
                if let Value::Hash(y, _) = vv {
                    for (k, x) in y {
                        hash_set(h, k, var(x.borrow().deep()));
                    }
                }
                return Ok(());
            }
            Value::Obj(_) => {
                drop(b);
                return self.native_store_op(d, o, &vv);
            }
            _ => {}
        }
        let r = operate(o, &b, &vv)?;
        *b = match &*b {
            Value::Int(_) => Value::Int(r.int()),
            Value::Real(_) => Value::Real(r.real()),
            Value::Str(_) => Value::Str(r.str()),
            _ => r,
        };
        Ok(())
    }

    fn finish_call(
        &mut self,
        t: &mut Thread,
        pc: usize,
        args: Vec<Var>,
        r: Done,
    ) -> Result<bool, String> {
        match r {
            Done::Val(v) => {
                self.deadline = None;
                t.push(var(v));
            }
            Done::Var(v) => {
                self.deadline = None;
                t.push(v);
            }
            Done::Yield(v) => {
                self.deadline = None;
                t.push(var(v));
                return Ok(false);
            }
            Done::Block => {
                for a in args {
                    t.push(a);
                }
                t.pc = pc;
                return Ok(false);
            }
            Done::Script(f, this, a) => {
                self.deadline = None;
                self.call_script(t, f, this, a, Ret::Push);
            }
        }
        Ok(true)
    }

    /// `obj.name(args)`: a script member function, or one of the engine.
    fn method(
        &mut self,
        t: &mut Thread,
        obj: &Var,
        name: &str,
        args: &[Var],
    ) -> Result<Done, String> {
        // Reference's own functions work on the reference itself
        let is_ref = matches!(&*obj.borrow(), Value::Ref(_));
        if is_ref
            && matches!(
                name,
                "SetReference" | "DetachReference" | "IsIdentity" | "GetType" | "Duplicate"
            )
        {
            return self.reference_method(obj, name, args);
        }
        let target = deref(obj);
        let mut cur = target.clone();
        for _ in 0..16 {
            let class = match &*cur.borrow() {
                Value::Struct(c, _) => Some(c.clone()),
                Value::Obj(o) => Some(Str::from(o.borrow().class.as_str())),
                _ => None,
            };
            let Some(class) = class else { break };
            if let Some(&f) = self.prog.funcs.get(&format!("{class}::{name}")) {
                let mut a = vec![cur.clone()];
                a.extend_from_slice(args);
                return Ok(Done::Script(f, Some(cur), a));
            }
            let parent = cur.borrow().member("parent");
            match parent {
                Some(p) => cur = deref(&p),
                None => break,
            }
        }
        let _ = t;
        self.builtin_method(&target, name, args)
    }

    /// A call without an object: a member function of `this`, a script function or an engine
    /// function.
    fn call_auto(&mut self, t: &mut Thread, name: &str, args: &[Var]) -> Result<Done, String> {
        if let Some(this) = t.frame().and_then(|f| f.this.clone()) {
            let mut cur = deref(&this);
            for _ in 0..16 {
                let class = match &*cur.borrow() {
                    Value::Struct(c, _) => Some(c.clone()),
                    _ => None,
                };
                let Some(class) = class else { break };
                if let Some(&f) = self.prog.funcs.get(&format!("{class}::{name}")) {
                    let mut a = vec![cur.clone()];
                    a.extend_from_slice(args);
                    return Ok(Done::Script(f, Some(cur), a));
                }
                let parent = cur.borrow().member("parent");
                match parent {
                    Some(p) => cur = deref(&p),
                    None => break,
                }
            }
        }
        if let Some(&f) = self.prog.funcs.get(name) {
            return Ok(Done::Script(f, None, args.to_vec()));
        }
        self.system(t, name, args)
    }

    /// Calls a script function by name from engine code (String::Call, Thread::BeginThread).
    pub fn script_fn(&self, name: &str) -> Option<usize> {
        self.prog.funcs.get(name).copied()
    }
}

/// The data and global variables of a program, with the objects to construct at start.
pub fn variables(img: &Image) -> (Vec<(Str, Var)>, Vec<(Str, Var)>, Vec<(Var, Str)>) {
    let mut make = Vec::new();
    let mut table = |items: &[(String, Init)]| -> Vec<(Str, Var)> {
        items
            .iter()
            .map(|(n, i)| {
                let v = var(init_value(i));
                if let Init::Object(c) = i {
                    make.push((v.clone(), Str::from(c.as_str())));
                }
                (Str::from(n.as_str()), v)
            })
            .collect()
    };
    let globals = table(&img.globals);
    let data = table(&img.data);
    (globals, data, make)
}

pub fn rc_eq(a: &Var, b: &Var) -> bool {
    Rc::ptr_eq(&deref(a), &deref(b))
}
