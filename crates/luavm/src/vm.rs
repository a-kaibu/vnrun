//! The interpreter loop for both instruction sets. 5.1 and 5.3 share the instruction layout
//! (op:6 A:8 C:9 B:9, Bx = B:C, sBx = Bx - 131071) and most opcodes; the numbering differs.

use std::cell::RefCell;
use std::rc::Rc;

use crate::undump::Version;
use crate::value::{Closure, Table, UpVal, Value};
use crate::{Arith, Lua, LuaError, LuaResult, err};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Move,
    LoadK,
    LoadKx,
    LoadBool,
    LoadNil,
    GetUpval,
    GetTabUp,
    GetGlobal,
    GetTable,
    SetTabUp,
    SetGlobal,
    SetUpval,
    SetTable,
    NewTable,
    Method,
    Arith(Arith),
    Not,
    Len,
    Concat,
    Jmp,
    Eq,
    Lt,
    Le,
    Test,
    TestSet,
    Call,
    TailCall,
    Return,
    ForLoop,
    ForPrep,
    TForCall,
    TForLoop,
    TForLoop51,
    SetList,
    Close,
    Closure,
    VarArg,
    ExtraArg,
}

const OPS_53: [Op; 47] = [
    Op::Move,
    Op::LoadK,
    Op::LoadKx,
    Op::LoadBool,
    Op::LoadNil,
    Op::GetUpval,
    Op::GetTabUp,
    Op::GetTable,
    Op::SetTabUp,
    Op::SetUpval,
    Op::SetTable,
    Op::NewTable,
    Op::Method,
    Op::Arith(Arith::Add),
    Op::Arith(Arith::Sub),
    Op::Arith(Arith::Mul),
    Op::Arith(Arith::Mod),
    Op::Arith(Arith::Pow),
    Op::Arith(Arith::Div),
    Op::Arith(Arith::IDiv),
    Op::Arith(Arith::BAnd),
    Op::Arith(Arith::BOr),
    Op::Arith(Arith::BXor),
    Op::Arith(Arith::Shl),
    Op::Arith(Arith::Shr),
    Op::Arith(Arith::Unm),
    Op::Arith(Arith::BNot),
    Op::Not,
    Op::Len,
    Op::Concat,
    Op::Jmp,
    Op::Eq,
    Op::Lt,
    Op::Le,
    Op::Test,
    Op::TestSet,
    Op::Call,
    Op::TailCall,
    Op::Return,
    Op::ForLoop,
    Op::ForPrep,
    Op::TForCall,
    Op::TForLoop,
    Op::SetList,
    Op::Closure,
    Op::VarArg,
    Op::ExtraArg,
];

const OPS_51: [Op; 38] = [
    Op::Move,
    Op::LoadK,
    Op::LoadBool,
    Op::LoadNil,
    Op::GetUpval,
    Op::GetGlobal,
    Op::GetTable,
    Op::SetGlobal,
    Op::SetUpval,
    Op::SetTable,
    Op::NewTable,
    Op::Method,
    Op::Arith(Arith::Add),
    Op::Arith(Arith::Sub),
    Op::Arith(Arith::Mul),
    Op::Arith(Arith::Div),
    Op::Arith(Arith::Mod),
    Op::Arith(Arith::Pow),
    Op::Arith(Arith::Unm),
    Op::Not,
    Op::Len,
    Op::Concat,
    Op::Jmp,
    Op::Eq,
    Op::Lt,
    Op::Le,
    Op::Test,
    Op::TestSet,
    Op::Call,
    Op::TailCall,
    Op::Return,
    Op::ForLoop,
    Op::ForPrep,
    Op::TForLoop51,
    Op::SetList,
    Op::Close,
    Op::Closure,
    Op::VarArg,
];

const FIELDS_PER_FLUSH: usize = 50;

fn decode(v: Version, i: u32) -> LuaResult<(Op, usize, usize, usize, i64)> {
    let op = (i & 0x3f) as usize;
    let op = match v {
        Version::V51 => OPS_51.get(op),
        Version::V53 => OPS_53.get(op),
    };
    let Some(&op) = op else {
        return err(format!("bad opcode {}", i & 0x3f));
    };
    let a = ((i >> 6) & 0xff) as usize;
    let c = ((i >> 14) & 0x1ff) as usize;
    let b = ((i >> 23) & 0x1ff) as usize;
    let bx = (i >> 14) as i64;
    Ok((op, a, b, c, bx))
}

impl Lua {
    fn close_upvals(&mut self, from: usize) {
        while let Some((i, _)) = self.open.last() {
            if *i < from {
                break;
            }
            let (i, u) = self.open.pop().unwrap();
            *u.borrow_mut() = UpVal::Closed(self.stack[i].clone());
        }
    }

    fn find_upval(&mut self, at: usize) -> Rc<RefCell<UpVal>> {
        let pos = self.open.partition_point(|(i, _)| *i < at);
        if let Some((i, u)) = self.open.get(pos)
            && *i == at
        {
            return u.clone();
        }
        let u = Rc::new(RefCell::new(UpVal::Open(self.cur, at)));
        self.open.insert(pos, (at, u.clone()));
        u
    }

    /// A register of another thread (one parked under the running one, or a suspended one).
    fn foreign<R>(&mut self, tid: u32, i: usize, f: impl FnOnce(&mut Value) -> R) -> R {
        if let Some(p) = self.parked.iter_mut().rev().find(|p| p.id == tid) {
            return f(&mut p.stack[i]);
        }
        let co = self.threads.get(&tid).and_then(|w| w.upgrade());
        match co {
            Some(co) => f(&mut co.borrow_mut().stack[i]),
            None => f(&mut Value::Nil),
        }
    }

    fn upval_get(&mut self, u: &Rc<RefCell<UpVal>>) -> Value {
        let (tid, i) = match &*u.borrow() {
            UpVal::Open(t, i) => (*t, *i),
            UpVal::Closed(v) => return v.clone(),
        };
        if tid == self.cur {
            self.stack[i].clone()
        } else {
            self.foreign(tid, i, |v| v.clone())
        }
    }

    fn upval_set(&mut self, u: &Rc<RefCell<UpVal>>, x: Value) {
        let (tid, i) = match &mut *u.borrow_mut() {
            UpVal::Open(t, i) => (*t, *i),
            UpVal::Closed(v) => {
                *v = x;
                return;
            }
        };
        if tid == self.cur {
            self.stack[i] = x;
        } else {
            self.foreign(tid, i, |v| *v = x);
        }
    }
}

/// A Lua function being run: its closure, where its registers start, the next instruction and
/// the end of its multiple results / varargs.
pub(crate) struct Frame {
    cl: Rc<Closure>,
    base: usize,
    pc: usize,
    top: usize,
    varargs: Vec<Value>,
    /// it yielded through a tail call: the values it is resumed with are its results
    tail: bool,
}

pub(crate) enum Exit {
    Return(Vec<Value>),
    Yield(Vec<Value>),
}

/// Sets up the registers of a call to `cl` at the top of the stack.
pub(crate) fn enter(lua: &mut Lua, cl: Rc<Closure>, mut args: Vec<Value>) -> Frame {
    if lua.trace {
        eprintln!("{}:{}", cl.proto.source, cl.proto.line);
    }
    let p = &cl.proto;
    let np = p.params as usize;
    let varargs = if p.vararg && args.len() > np {
        args.split_off(np)
    } else {
        args.truncate(np);
        Vec::new()
    };
    let base = lua.stack.len();
    let size = p.max_stack as usize + 1;
    lua.stack.resize(base + size, Value::Nil);
    for (i, a) in args.into_iter().enumerate() {
        lua.stack[base + i] = a;
    }
    Frame {
        cl,
        base,
        pc: 0,
        top: base + size,
        varargs,
        tail: false,
    }
}

pub fn run(lua: &mut Lua, cl: Rc<Closure>, args: Vec<Value>) -> LuaResult<Vec<Value>> {
    let mut frames = vec![enter(lua, cl, args)];
    match exec(lua, &mut frames, None, false)? {
        Exit::Return(v) => Ok(v),
        Exit::Yield(_) => err("attempt to yield across a C-call boundary"),
    }
}

/// Runs `frames` (the innermost last) until the bottom one returns, or (when `yieldable`) until
/// coroutine.yield is called; the frames are then left as they are for a later `resume` with
/// the values yield returns.
pub(crate) fn exec(
    lua: &mut Lua,
    frames: &mut Vec<Frame>,
    resume: Option<Vec<Value>>,
    yieldable: bool,
) -> LuaResult<Exit> {
    let bottom = frames.first().map_or(lua.stack.len(), |f| f.base);
    let r = step(lua, frames, resume, yieldable);
    match r {
        Err(mut e) => {
            for f in frames.iter().rev() {
                let p = &f.cl.proto;
                e.trace.push(match p.lines.get(f.pc.saturating_sub(1)) {
                    Some(l) => format!("{}:{}", p.source, l),
                    None => format!("{} (function at line {})", p.source, p.line),
                });
            }
            frames.clear();
            lua.close_upvals(bottom);
            lua.stack.truncate(bottom);
            Err(e)
        }
        ok => ok,
    }
}

/// Stores call results the way the CALL at `a` with result count `c` wants them.
fn place(lua: &mut Lua, base: usize, top: &mut usize, a: usize, c: usize, res: Vec<Value>) {
    if c == 0 {
        let need = base + a + res.len();
        if lua.stack.len() < need {
            lua.stack.resize(need, Value::Nil);
        }
        *top = need;
        for (i, x) in res.into_iter().enumerate() {
            lua.stack[base + a + i] = x;
        }
    } else {
        let mut it = res.into_iter();
        for i in 0..c - 1 {
            lua.stack[base + a + i] = it.next().unwrap_or_default();
        }
    }
}

fn step(
    lua: &mut Lua,
    frames: &mut Vec<Frame>,
    resume: Option<Vec<Value>>,
    yieldable: bool,
) -> LuaResult<Exit> {
    let Some(f) = frames.last() else {
        return Ok(Exit::Return(resume.unwrap_or_default()));
    };
    let mut cl = f.cl.clone();
    let mut p = cl.proto.clone();
    let mut v = p.version;
    let mut base = f.base;
    let mut pc = f.pc;
    let mut top = f.top;
    // values to return from the current frame (set by RETURN, a tail call to a host function, or
    // a resume after a yield through a tail call)
    let mut ret: Option<Vec<Value>> = None;
    if let Some(vals) = resume {
        if f.tail {
            ret = Some(vals);
        } else {
            let (_, a, _, c, _) = decode(v, p.code[pc - 1])?;
            place(lua, base, &mut top, a, c, vals);
        }
    }
    macro_rules! save {
        () => {{
            let f = frames.last_mut().unwrap();
            f.pc = pc;
            f.top = top;
        }};
    }
    macro_rules! load {
        () => {{
            let f = frames.last().unwrap();
            cl = f.cl.clone();
            p = cl.proto.clone();
            v = p.version;
            base = f.base;
            pc = f.pc;
            top = f.top;
        }};
    }
    macro_rules! r {
        ($i:expr) => {
            lua.stack[base + $i]
        };
    }
    macro_rules! rk {
        ($x:expr) => {
            if $x & 0x100 != 0 {
                p.consts[$x & 0xff].clone()
            } else {
                lua.stack[base + $x].clone()
            }
        };
    }
    macro_rules! k {
        ($x:expr) => {
            p.consts
                .get($x as usize)
                .cloned()
                .ok_or_else(|| LuaError::new("bad constant"))?
        };
    }
    loop {
        if let Some(res) = ret.take() {
            lua.close_upvals(base);
            lua.stack.truncate(base);
            frames.pop();
            if frames.is_empty() {
                return Ok(Exit::Return(res));
            }
            load!();
            let (_, a, _, c, _) = decode(v, p.code[pc - 1])?;
            place(lua, base, &mut top, a, c, res);
        }
        let Some(&ins) = p.code.get(pc) else {
            return err("ran past the end of a function");
        };
        pc += 1;
        let (op, a, b, c, bx) = decode(v, ins)?;
        let sbx = bx - 131071;
        match op {
            Op::Move => r!(a) = r!(b).clone(),
            Op::LoadK => r!(a) = k!(bx),
            Op::LoadKx => {
                let ax = p.code.get(pc).copied().unwrap_or(0) >> 6;
                pc += 1;
                r!(a) = k!(ax);
            }
            Op::LoadBool => {
                r!(a) = Value::Bool(b != 0);
                if c != 0 {
                    pc += 1;
                }
            }
            Op::LoadNil => {
                let last = if v == Version::V51 { b } else { a + b };
                for i in a..=last {
                    r!(i) = Value::Nil;
                }
            }
            Op::GetUpval => r!(a) = lua.upval_get(&cl.upvals[b]),
            Op::SetUpval => {
                let x = r!(a).clone();
                lua.upval_set(&cl.upvals[b], x);
            }
            Op::GetTabUp => {
                let t = lua.upval_get(&cl.upvals[b]);
                let key = rk!(c);
                r!(a) = lua.index(&t, &key)?;
            }
            Op::SetTabUp => {
                let t = lua.upval_get(&cl.upvals[a]);
                let (key, x) = (rk!(b), rk!(c));
                lua.set_index(&t, key, x)?;
            }
            Op::GetGlobal => {
                let key = k!(bx);
                let hit = lua.globals.borrow().get(&key);
                r!(a) = if hit.is_nil() {
                    let t = Value::Table(lua.globals.clone());
                    lua.index(&t, &key)?
                } else {
                    hit
                };
            }
            Op::SetGlobal => {
                let t = Value::Table(lua.globals.clone());
                let (key, x) = (k!(bx), r!(a).clone());
                lua.set_index(&t, key, x)?;
            }
            Op::GetTable => {
                let (t, key) = (r!(b).clone(), rk!(c));
                r!(a) = lua.index(&t, &key)?;
            }
            Op::SetTable => {
                let (t, key, x) = (r!(a).clone(), rk!(b), rk!(c));
                lua.set_index(&t, key, x)?;
            }
            Op::NewTable => r!(a) = Value::table(Table::default()),
            Op::Method => {
                let (t, key) = (r!(b).clone(), rk!(c));
                r!(a + 1) = t.clone();
                r!(a) = lua.index(&t, &key)?;
            }
            Op::Arith(o) => {
                let (x, y) = if matches!(o, Arith::Unm | Arith::BNot) {
                    (r!(b).clone(), r!(b).clone())
                } else {
                    (rk!(b), rk!(c))
                };
                r!(a) = lua.arith(o, &x, &y)?;
            }
            Op::Not => r!(a) = Value::Bool(!r!(b).truthy()),
            Op::Len => {
                let x = r!(b).clone();
                r!(a) = lua.len(&x)?;
            }
            Op::Concat => {
                let mut acc = r!(c).clone();
                for i in (b..c).rev() {
                    let x = r!(i).clone();
                    acc = lua.concat(&x, &acc)?;
                }
                r!(a) = acc;
            }
            Op::Jmp => {
                if v == Version::V53 && a > 0 {
                    lua.close_upvals(base + a - 1);
                }
                pc = (pc as i64 + sbx) as usize;
            }
            Op::Eq | Op::Lt | Op::Le => {
                let (x, y) = (rk!(b), rk!(c));
                let t = match op {
                    Op::Eq => lua.equals(&x, &y)?,
                    Op::Lt => lua.less(&x, &y, false)?,
                    _ => lua.less(&x, &y, true)?,
                };
                if t != (a != 0) {
                    pc += 1;
                }
            }
            Op::Test => {
                if r!(a).truthy() != (c != 0) {
                    pc += 1;
                }
            }
            Op::TestSet => {
                if r!(b).truthy() == (c != 0) {
                    r!(a) = r!(b).clone();
                } else {
                    pc += 1;
                }
            }
            Op::Call | Op::TailCall => {
                let f = r!(a).clone();
                let end = if b == 0 { top } else { base + a + b };
                let args: Vec<Value> = lua.stack[base + a + 1..end].to_vec();
                if let Value::Func(callee) = f {
                    if frames.len() > 7000 {
                        return err("stack overflow");
                    }
                    save!();
                    if op == Op::TailCall {
                        lua.close_upvals(base);
                        lua.stack.truncate(base);
                        frames.pop();
                    }
                    let fr = enter(lua, callee, args);
                    frames.push(fr);
                    load!();
                    continue;
                }
                save!();
                let res = lua.call(&f, args)?;
                if let Some(y) = lua.yielded.take() {
                    if !yieldable {
                        return err("attempt to yield across a C-call boundary");
                    }
                    frames.last_mut().unwrap().tail = op == Op::TailCall;
                    return Ok(Exit::Yield(y));
                }
                if op == Op::TailCall {
                    ret = Some(res);
                } else {
                    place(lua, base, &mut top, a, c, res);
                }
            }
            Op::Return => {
                let end = if b == 0 { top } else { base + a + b - 1 };
                ret = Some(lua.stack[base + a..end.max(base + a)].to_vec());
            }
            Op::ForPrep => {
                let (init, limit, step) = (r!(a).clone(), r!(a + 1).clone(), r!(a + 2).clone());
                let ints = v == Version::V53
                    && matches!((&init, &step), (Value::Int(_), Value::Int(_)))
                    && limit.to_num().is_some();
                if ints {
                    let (i, s) = (init.to_int().unwrap(), step.to_int().unwrap());
                    if s == 0 {
                        return err("'for' step is zero");
                    }
                    let l = limit.to_num().unwrap();
                    let l = if s > 0 { l.floor() } else { l.ceil() };
                    let l = l.clamp(i64::MIN as f64, i64::MAX as f64) as i64;
                    r!(a + 1) = Value::Int(l);
                    r!(a) = Value::Int(i.wrapping_sub(s));
                } else {
                    let (Some(i), Some(l), Some(s)) =
                        (init.to_num(), limit.to_num(), step.to_num())
                    else {
                        return err("'for' initial value, limit and step must be numbers");
                    };
                    r!(a) = Value::Num(i - s);
                    r!(a + 1) = Value::Num(l);
                    r!(a + 2) = Value::Num(s);
                }
                pc = (pc as i64 + sbx) as usize;
            }
            Op::ForLoop => {
                let go = match (&r!(a), &r!(a + 1), &r!(a + 2)) {
                    (Value::Int(i), Value::Int(l), Value::Int(s)) => {
                        let (i, l, s) = (*i, *l, *s);
                        match i.checked_add(s) {
                            Some(n) if if s > 0 { n <= l } else { n >= l } => {
                                r!(a) = Value::Int(n);
                                Some(Value::Int(n))
                            }
                            _ => None,
                        }
                    }
                    (i, l, s) => {
                        let (i, l, s) = (
                            i.to_num().unwrap_or(0.0),
                            l.to_num().unwrap_or(0.0),
                            s.to_num().unwrap_or(0.0),
                        );
                        let n = i + s;
                        r!(a) = Value::Num(n);
                        if if s > 0.0 { n <= l } else { n >= l } {
                            Some(Value::Num(n))
                        } else {
                            None
                        }
                    }
                };
                if let Some(n) = go {
                    r!(a + 3) = n;
                    pc = (pc as i64 + sbx) as usize;
                }
            }
            Op::TForCall | Op::TForLoop51 => {
                let f = r!(a).clone();
                let args = vec![r!(a + 1).clone(), r!(a + 2).clone()];
                save!();
                let res = lua.call(&f, args)?;
                let mut it = res.into_iter();
                for i in 0..c {
                    r!(a + 3 + i) = it.next().unwrap_or_default();
                }
                if op == Op::TForLoop51 {
                    if r!(a + 3).is_nil() {
                        pc += 1;
                    } else {
                        r!(a + 2) = r!(a + 3).clone();
                    }
                }
            }
            Op::TForLoop => {
                if !r!(a + 1).is_nil() {
                    r!(a) = r!(a + 1).clone();
                    pc = (pc as i64 + sbx) as usize;
                }
            }
            Op::SetList => {
                let n = if b == 0 { top - (base + a) - 1 } else { b };
                let block = if c == 0 {
                    let x = p.code.get(pc).copied().unwrap_or(0);
                    pc += 1;
                    if v == Version::V53 {
                        (x >> 6) as usize
                    } else {
                        x as usize
                    }
                } else {
                    c
                };
                let t = r!(a).clone();
                let Value::Table(t) = t else {
                    return err("SETLIST on a non-table");
                };
                let mut t = t.borrow_mut();
                for i in 1..=n {
                    let k = ((block - 1) * FIELDS_PER_FLUSH + i) as i64;
                    t.set(Value::Int(k), r!(a + i).clone());
                }
            }
            Op::Close => lua.close_upvals(base + a),
            Op::Closure => {
                let proto = p
                    .protos
                    .get(bx as usize)
                    .cloned()
                    .ok_or_else(|| LuaError::new("bad prototype"))?;
                let mut upvals = Vec::with_capacity(proto.nups as usize);
                match v {
                    Version::V53 => {
                        for &(instack, idx) in &proto.upvals {
                            upvals.push(if instack {
                                lua.find_upval(base + idx as usize)
                            } else {
                                cl.upvals[idx as usize].clone()
                            });
                        }
                    }
                    Version::V51 => {
                        for _ in 0..proto.nups {
                            let pseudo = p.code.get(pc).copied().unwrap_or(0);
                            pc += 1;
                            let (pop, _, pb, _, _) = decode(v, pseudo)?;
                            upvals.push(if pop == Op::Move {
                                lua.find_upval(base + pb)
                            } else {
                                cl.upvals[pb].clone()
                            });
                        }
                    }
                }
                r!(a) = Value::Func(Rc::new(Closure { proto, upvals }));
            }
            Op::VarArg => {
                let varargs = &frames.last().unwrap().varargs;
                if b == 0 {
                    let need = base + a + varargs.len();
                    if lua.stack.len() < need {
                        lua.stack.resize(need, Value::Nil);
                    }
                    top = need;
                    for (i, x) in varargs.iter().enumerate() {
                        lua.stack[base + a + i] = x.clone();
                    }
                } else {
                    for i in 0..b - 1 {
                        lua.stack[base + a + i] = varargs.get(i).cloned().unwrap_or_default();
                    }
                }
            }
            Op::ExtraArg => return err("unexpected EXTRAARG"),
        }
    }
}
