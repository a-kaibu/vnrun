//! Saves. The scripts mark the place to come back to with `$save_point(text)` once a message
//! is on show; `$do_save(n)` writes that place and `$do_load(n)` goes back to it. A place is the
//! thread that ran the message as it was when the message's first text started (its frames,
//! stack and thread variables), with the save-scope variables of the save point. Loading puts
//! that thread back and sends HOT_RESET(1, "", 0), on which the scripts (pic.mjo, console.mjo)
//! draw the scene again from those variables; the message then runs again.
//!
//! ```text
//! file:  "MJSAVE1\0" str text, u32 n + n x (u32 hash, value)  save variables
//!        u32 thread, i32 line, u32 n + n values  stack, u32 n + n x (u32, value)  thread variables,
//!        u32 n + n frames
//! frame: str script, u32 pc, u32 base, u32 argc, value case, 3 x skip, opt u32 dtor,
//!        opt value ret, u8 cont, u32 func
//! value: u8 0 int i32 | 1 float f32 | 2 str | 3 array: u8 kind, u32 dims n + n x u32, values
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::engine::Engine;
use crate::value::{Array, Value};
use crate::vm::{Cont, Frame, Skip, Thread};

const MAGIC: &[u8; 8] = b"MJSAVE1\0";

/// A thread as saved, its frames still by script name.
pub struct Place {
    /// the thread's id
    pub thread: usize,
    pub line: i32,
    pub stack: Vec<Value>,
    pub vars: HashMap<u32, Value>,
    pub frames: Vec<SavedFrame>,
}

pub struct SavedFrame {
    pub script: String,
    pub pc: usize,
    pub base: usize,
    pub argc: usize,
    pub case: Value,
    pub skip: [Option<Skip>; 3],
    pub dtor: Option<usize>,
    pub ret: Option<Value>,
    pub cont: Cont,
    pub func: u32,
}

/// A save: what the slot shows, the save variables and the place.
pub struct Save {
    pub text: String,
    pub save: HashMap<u32, Value>,
    pub place: Place,
}

struct W(Vec<u8>);

impl W {
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend(s.as_bytes());
    }
    fn value(&mut self, v: &Value) {
        match v {
            Value::Int(i) => {
                self.0.push(0);
                self.u32(*i as u32);
            }
            Value::Float(x) => {
                self.0.push(1);
                self.u32(x.to_bits());
            }
            Value::Str(s) => {
                self.0.push(2);
                self.str(s);
            }
            Value::Arr(a) => {
                let a = a.borrow();
                self.0.push(3);
                self.0.push(a.kind);
                self.u32(a.dims.len() as u32);
                for d in &a.dims {
                    self.u32(*d as u32);
                }
                for v in &a.data {
                    self.value(v);
                }
            }
        }
    }
    fn vars(&mut self, vars: &HashMap<u32, Value>) {
        let mut keys: Vec<_> = vars.keys().copied().collect();
        keys.sort();
        self.u32(keys.len() as u32);
        for k in keys {
            self.u32(k);
            self.value(&vars[&k]);
        }
    }
    fn opt(&mut self, v: Option<usize>) {
        match v {
            Some(v) => {
                self.0.push(1);
                self.u32(v as u32);
            }
            None => self.0.push(0),
        }
    }
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}

impl R<'_> {
    fn u8(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.p).ok_or("save: cut short")?;
        self.p += 1;
        Ok(v)
    }
    fn u32(&mut self) -> Result<u32, String> {
        let v = self.b.get(self.p..self.p + 4).ok_or("save: cut short")?;
        self.p += 4;
        Ok(u32::from_le_bytes(v.try_into().unwrap()))
    }
    fn len(&mut self) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > self.b.len() {
            return Err("save: bad length".into());
        }
        Ok(n)
    }
    fn str(&mut self) -> Result<String, String> {
        let n = self.len()?;
        let s = self.b.get(self.p..self.p + n).ok_or("save: cut short")?;
        self.p += n;
        Ok(String::from_utf8_lossy(s).into_owned())
    }
    fn value(&mut self) -> Result<Value, String> {
        Ok(match self.u8()? {
            0 => Value::Int(self.u32()? as i32),
            1 => Value::Float(f32::from_bits(self.u32()?)),
            2 => Value::string(self.str()?),
            3 => {
                let kind = self.u8()?;
                let n = self.len()?;
                let dims = (0..n)
                    .map(|_| self.u32().map(|d| d as usize))
                    .collect::<Result<Vec<_>, _>>()?;
                let count = dims.iter().product::<usize>();
                if count > self.b.len() {
                    return Err("save: bad array".into());
                }
                let data = (0..count)
                    .map(|_| self.value())
                    .collect::<Result<Vec<_>, _>>()?;
                Value::Arr(Rc::new(RefCell::new(Array { dims, data, kind })))
            }
            t => return Err(format!("save: value kind {t}")),
        })
    }
    fn vars(&mut self) -> Result<HashMap<u32, Value>, String> {
        let n = self.len()?;
        let mut out = HashMap::new();
        for _ in 0..n {
            let k = self.u32()?;
            out.insert(k, self.value()?);
        }
        Ok(out)
    }
    fn opt(&mut self) -> Result<Option<usize>, String> {
        Ok(match self.u8()? {
            0 => None,
            _ => Some(self.u32()? as usize),
        })
    }
}

/// The thread now, `pc` the place its innermost frame goes on from.
pub fn place_of(th: &Thread, pc: usize) -> Place {
    let n = th.frames.len();
    Place {
        thread: th.id,
        line: th.line,
        stack: th.stack.iter().map(deep).collect(),
        vars: th.vars.iter().map(|(k, v)| (*k, deep(v))).collect(),
        frames: th
            .frames
            .iter()
            .enumerate()
            .map(|(i, f)| SavedFrame {
                script: f.script.name.clone(),
                pc: if i + 1 == n { pc } else { f.pc },
                base: f.base,
                argc: f.argc,
                case: deep(&f.case),
                skip: f.skip,
                dtor: f.dtor,
                ret: f.ret.as_ref().map(deep),
                cont: f.cont,
                func: f.func,
            })
            .collect(),
    }
}

/// A copy that shares no array with the running game.
pub fn deep(v: &Value) -> Value {
    match v {
        Value::Arr(a) => {
            let a = a.borrow();
            Value::Arr(Rc::new(RefCell::new(Array {
                dims: a.dims.clone(),
                data: a.data.iter().map(deep).collect(),
                kind: a.kind,
            })))
        }
        v => v.clone(),
    }
}

pub fn deep_vars(vars: &HashMap<u32, Value>) -> HashMap<u32, Value> {
    vars.iter().map(|(k, v)| (*k, deep(v))).collect()
}

pub fn write(s: &Save) -> Vec<u8> {
    let mut w = W(MAGIC.to_vec());
    w.str(&s.text);
    w.vars(&s.save);
    let p = &s.place;
    w.u32(p.thread as u32);
    w.u32(p.line as u32);
    w.u32(p.stack.len() as u32);
    for v in &p.stack {
        w.value(v);
    }
    w.vars(&p.vars);
    w.u32(p.frames.len() as u32);
    for f in &p.frames {
        w.str(&f.script);
        w.u32(f.pc as u32);
        w.u32(f.base as u32);
        w.u32(f.argc as u32);
        w.value(&f.case);
        for s in &f.skip {
            match s {
                Some(s) => {
                    w.0.push(1);
                    w.u32(s.target as u32);
                    w.u32(s.height as u32);
                }
                None => w.0.push(0),
            }
        }
        w.opt(f.dtor);
        match &f.ret {
            Some(v) => {
                w.0.push(1);
                w.value(v);
            }
            None => w.0.push(0),
        }
        w.0.push((f.cont == Cont::Push) as u8);
        w.u32(f.func);
    }
    w.0
}

pub fn read(b: &[u8]) -> Result<Save, String> {
    if !b.starts_with(MAGIC) {
        return Err("not a save".into());
    }
    let mut r = R { b, p: MAGIC.len() };
    let text = r.str()?;
    let save = r.vars()?;
    let thread = r.u32()? as usize;
    let line = r.u32()? as i32;
    let n = r.len()?;
    let stack = (0..n).map(|_| r.value()).collect::<Result<Vec<_>, _>>()?;
    let vars = r.vars()?;
    let n = r.len()?;
    let mut frames = Vec::new();
    for _ in 0..n {
        let script = r.str()?;
        let pc = r.u32()? as usize;
        let base = r.u32()? as usize;
        let argc = r.u32()? as usize;
        let case = r.value()?;
        let mut skip = [None; 3];
        for s in skip.iter_mut() {
            if r.u8()? != 0 {
                *s = Some(Skip {
                    target: r.u32()? as usize,
                    height: r.u32()? as usize,
                });
            }
        }
        let dtor = r.opt()?;
        let ret = match r.u8()? {
            0 => None,
            _ => Some(r.value()?),
        };
        let cont = if r.u8()? != 0 { Cont::Push } else { Cont::Drop };
        let func = r.u32()?;
        frames.push(SavedFrame {
            script,
            pc,
            base,
            argc,
            case,
            skip,
            dtor,
            ret,
            cont,
            func,
        });
    }
    Ok(Save {
        text,
        save,
        place: Place {
            thread,
            line,
            stack,
            vars,
            frames,
        },
    })
}

/// Puts a saved place back on `th` (its frames, stack, variables).
pub fn restore(e: &mut Engine, th: &mut Thread, p: Place) -> Result<(), String> {
    let mut frames = Vec::new();
    for f in p.frames {
        let stem = f.script.trim_end_matches(".mjo").trim_end_matches(".MJO");
        let script = e.load_script(stem)?;
        frames.push(Frame {
            script,
            pc: f.pc,
            base: f.base,
            argc: f.argc,
            case: f.case,
            skip: f.skip,
            dtor: f.dtor,
            ret: f.ret,
            cont: f.cont,
            func: f.func,
            saved_wait: None,
        });
    }
    th.frames = frames;
    th.stack = p.stack;
    th.vars = p.vars;
    th.line = p.line;
    th.wait = None;
    th.pending.clear();
    th.unwinding = false;
    Ok(())
}

pub fn slot_name(n: i32) -> String {
    format!("save{n:04}.bin")
}
