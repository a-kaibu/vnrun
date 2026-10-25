//! The standard library parts games use: base, string, table, math, os (time only), utf8, bit32.

use std::cell::RefCell;
use std::rc::Rc;

use crate::pattern::{Capture, Matcher, is_plain};
use crate::value::{Table, Value, parse_number};
use crate::{Arith, Lua, LuaError, LuaResult, err};

type Args = Vec<Value>;
type Ret = LuaResult<Vec<Value>>;

fn arg(a: &Args, i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

pub fn check_int(a: &Args, i: usize, f: &str) -> LuaResult<i64> {
    arg(a, i)
        .to_int()
        .or_else(|| arg(a, i).to_num().map(|n| n.floor() as i64))
        .ok_or_else(|| {
            LuaError::new(format!(
                "bad argument #{} to '{f}' (number expected)",
                i + 1
            ))
        })
}

fn opt_int(a: &Args, i: usize, d: i64, f: &str) -> LuaResult<i64> {
    if arg(a, i).is_nil() {
        Ok(d)
    } else {
        check_int(a, i, f)
    }
}

pub fn check_num(a: &Args, i: usize, f: &str) -> LuaResult<f64> {
    arg(a, i).to_num().ok_or_else(|| {
        LuaError::new(format!(
            "bad argument #{} to '{f}' (number expected)",
            i + 1
        ))
    })
}

/// A string argument; numbers are converted.
fn check_str(lua: &Lua, a: &Args, i: usize, f: &str) -> LuaResult<Vec<u8>> {
    match arg(a, i) {
        Value::Str(s) => Ok(s.to_vec()),
        v @ (Value::Int(_) | Value::Num(_)) => Ok(lua.tostring_raw(&v)),
        v => err(format!(
            "bad argument #{} to '{f}' (string expected, got {})",
            i + 1,
            v.type_name()
        )),
    }
}

fn check_table(a: &Args, i: usize, f: &str) -> LuaResult<crate::TableRef> {
    match arg(a, i) {
        Value::Table(t) => Ok(t),
        v => err(format!(
            "bad argument #{} to '{f}' (table expected, got {})",
            i + 1,
            v.type_name()
        )),
    }
}

fn reg(t: &mut Table, name: &'static str, f: impl Fn(&mut Lua, Args) -> Ret + 'static) {
    t.set_str(name, Value::native(name, f));
}

pub fn open(lua: &mut Lua) {
    let g = lua.globals.clone();
    let mut g = g.borrow_mut();
    g.set_str("_G", Value::Table(lua.globals.clone()));
    g.set_str("_VERSION", "Lua 5.3");
    reg(&mut g, "print", |lua, a| {
        let mut out = Vec::new();
        for (i, v) in a.iter().enumerate() {
            if i > 0 {
                out.push(b'\t');
            }
            out.extend(lua.tostring(v)?);
        }
        eprintln!("lua: {}", String::from_utf8_lossy(&out));
        Ok(vec![])
    });
    reg(&mut g, "type", |_, a| {
        if a.is_empty() {
            return err("bad argument #1 to 'type' (value expected)");
        }
        Ok(vec![Value::str(arg(&a, 0).type_name())])
    });
    reg(&mut g, "tostring", |lua, a| {
        Ok(vec![Value::bytes(&lua.tostring(&arg(&a, 0))?)])
    });
    reg(&mut g, "tonumber", |_, a| {
        let v = arg(&a, 0);
        if arg(&a, 1).is_nil() {
            return Ok(vec![match v {
                Value::Int(_) | Value::Num(_) => v,
                Value::Str(s) => parse_number(&s).unwrap_or_default(),
                _ => Value::Nil,
            }]);
        }
        let base = check_int(&a, 1, "tonumber")? as u32;
        let s = match &v {
            Value::Str(s) => String::from_utf8_lossy(s).trim().to_lowercase(),
            v => v.to_int().map(|i| i.to_string()).unwrap_or_default(),
        };
        let (neg, s) = match s.strip_prefix('-') {
            Some(r) => (true, r.to_string()),
            None => (false, s),
        };
        Ok(vec![match i64::from_str_radix(&s, base) {
            Ok(n) => Value::Int(if neg { -n } else { n }),
            Err(_) => Value::Nil,
        }])
    });
    reg(&mut g, "rawget", |_, a| {
        Ok(vec![
            check_table(&a, 0, "rawget")?.borrow().get(&arg(&a, 1)),
        ])
    });
    reg(&mut g, "rawset", |_, a| {
        check_table(&a, 0, "rawset")?
            .borrow_mut()
            .set(arg(&a, 1), arg(&a, 2));
        Ok(vec![arg(&a, 0)])
    });
    reg(&mut g, "rawequal", |_, a| {
        Ok(vec![Value::Bool(arg(&a, 0).raw_eq(&arg(&a, 1)))])
    });
    reg(&mut g, "rawlen", |_, a| match arg(&a, 0) {
        Value::Table(t) => Ok(vec![Value::Int(t.borrow().len())]),
        Value::Str(s) => Ok(vec![Value::Int(s.len() as i64)]),
        _ => err("table or string expected"),
    });
    reg(&mut g, "setmetatable", |_, a| {
        let t = check_table(&a, 0, "setmetatable")?;
        t.borrow_mut().meta = match arg(&a, 1) {
            Value::Table(m) => Some(m),
            Value::Nil => None,
            _ => return err("bad argument #2 to 'setmetatable' (nil or table expected)"),
        };
        Ok(vec![arg(&a, 0)])
    });
    reg(&mut g, "getmetatable", |lua, a| {
        Ok(vec![match lua.metatable(&arg(&a, 0)) {
            Some(m) => {
                let protected = m.borrow().get_str("__metatable");
                if protected.is_nil() {
                    Value::Table(m)
                } else {
                    protected
                }
            }
            None => Value::Nil,
        }])
    });
    reg(&mut g, "next", |_, a| {
        let t = check_table(&a, 0, "next")?;
        let r = t.borrow().next(&arg(&a, 1));
        match r {
            Some((k, v)) if !k.is_nil() => Ok(vec![k, v]),
            Some(_) => Ok(vec![Value::Nil]),
            None => err("invalid key to 'next'"),
        }
    });
    let next = g.get_str("next");
    g.set_str(
        "pairs",
        Value::native("pairs", move |lua, a| {
            let t = arg(&a, 0);
            let h = lua
                .metatable(&t)
                .map(|m| m.borrow().get_str("__pairs"))
                .unwrap_or_default();
            if !h.is_nil() {
                let mut r = lua.call(&h, vec![t])?;
                r.resize(3, Value::Nil);
                return Ok(r);
            }
            check_table(&a, 0, "pairs")?;
            Ok(vec![next.clone(), t, Value::Nil])
        }),
    );
    let inext = Value::native("ipairs_iter", |lua, a| {
        let i = check_int(&a, 1, "ipairs")? + 1;
        let v = lua.index(&arg(&a, 0), &Value::Int(i))?;
        Ok(if v.is_nil() {
            vec![Value::Nil]
        } else {
            vec![Value::Int(i), v]
        })
    });
    g.set_str(
        "ipairs",
        Value::native("ipairs", move |_, a| {
            Ok(vec![inext.clone(), arg(&a, 0), Value::Int(0)])
        }),
    );
    reg(&mut g, "select", |_, mut a| {
        let n = a.len() as i64 - 1;
        if let Value::Str(s) = arg(&a, 0)
            && &*s == b"#" {
                return Ok(vec![Value::Int(n)]);
            }
        let i = check_int(&a, 0, "select")?;
        let i = if i < 0 { n + i } else { i - 1 };
        if i < 0 {
            return err("bad argument #1 to 'select' (index out of range)");
        }
        Ok(a.split_off((i as usize + 1).min(a.len())))
    });
    reg(&mut g, "error", |lua, a| {
        let mut v = arg(&a, 0);
        if let Value::Str(_) = v {
            // no position prefix: the trace has the frames
            v = Value::bytes(&lua.tostring_raw(&v));
        }
        Err(LuaError {
            value: v,
            trace: Vec::new(),
        })
    });
    reg(&mut g, "assert", |lua, a| {
        if arg(&a, 0).truthy() {
            return Ok(a);
        }
        match arg(&a, 1) {
            Value::Nil => err("assertion failed!"),
            v => Err(LuaError {
                value: Value::bytes(&lua.tostring_raw(&v)),
                trace: Vec::new(),
            }),
        }
    });
    reg(&mut g, "pcall", |lua, mut a| {
        if a.is_empty() {
            return err("bad argument #1 to 'pcall' (value expected)");
        }
        let f = a.remove(0);
        match lua.call(&f, a) {
            Ok(mut r) => {
                r.insert(0, Value::Bool(true));
                Ok(r)
            }
            Err(e) => Ok(vec![Value::Bool(false), error_value(&e)]),
        }
    });
    reg(&mut g, "xpcall", |lua, mut a| {
        if a.len() < 2 {
            return err("bad argument #2 to 'xpcall' (value expected)");
        }
        let f = a.remove(0);
        let h = a.remove(0);
        match lua.call(&f, a) {
            Ok(mut r) => {
                r.insert(0, Value::Bool(true));
                Ok(r)
            }
            Err(e) => {
                let mut r = lua.call(&h, vec![error_value(&e)])?;
                r.insert(0, Value::Bool(false));
                Ok(r)
            }
        }
    });
    let unpack = Value::native("unpack", |lua, a| {
        let t = arg(&a, 0);
        let i = opt_int(&a, 1, 1, "unpack")?;
        let j = match arg(&a, 2) {
            Value::Nil => lua.len(&t)?.to_int().unwrap_or(0),
            _ => check_int(&a, 2, "unpack")?,
        };
        if j - i >= 1 << 20 {
            return err("too many results to unpack");
        }
        let mut out = Vec::new();
        for k in i..=j {
            out.push(lua.index(&t, &Value::Int(k))?);
        }
        Ok(out)
    });
    g.set_str("unpack", unpack.clone());
    reg(&mut g, "collectgarbage", |_, _| Ok(vec![Value::Int(0)]));

    let mut string = Table::default();
    open_string(&mut string);
    let string = Rc::new(RefCell::new(string));
    lua.string_meta
        .borrow_mut()
        .set_str("__index", Value::Table(string.clone()));
    g.set_str("string", Value::Table(string));

    let mut table = Table::default();
    open_table(&mut table);
    table.set_str("unpack", unpack);
    g.set_str("table", Value::table(table));

    let mut math = Table::default();
    open_math(&mut math);
    g.set_str("math", Value::table(math));

    let mut os = Table::default();
    reg(&mut os, "time", |lua, _| Ok(vec![Value::Int((lua.epoch + lua.clock) as i64)]));
    reg(&mut os, "clock", |lua, _| Ok(vec![Value::Num(lua.clock)]));
    reg(&mut os, "date", |_, _| {
        Ok(vec![Value::str("2000/01/01 00:00:00")])
    });
    g.set_str("os", Value::table(os));

    let mut utf8 = Table::default();
    open_utf8(&mut utf8);
    g.set_str("utf8", Value::table(utf8));

    let mut bit = Table::default();
    open_bit32(&mut bit);
    g.set_str("bit32", Value::table(bit));
}

fn error_value(e: &LuaError) -> Value {
    match &e.value {
        Value::Str(s) => {
            let mut m = s.to_vec();
            if let Some(t) = e.trace.first() {
                m = format!("{t}: {}", String::from_utf8_lossy(&m)).into_bytes();
            }
            Value::bytes(&m)
        }
        v => v.clone(),
    }
}

/// Lua's string index: 1-based, negative from the end, clamped.
fn str_range(len: usize, i: i64, j: i64) -> (usize, usize) {
    let len = len as i64;
    let i = if i < 0 {
        (len + i + 1).max(1)
    } else {
        i.max(1)
    };
    let j = if j < 0 { len + j + 1 } else { j.min(len) };
    if i > j {
        (0, 0)
    } else {
        (i as usize - 1, j as usize)
    }
}

fn open_string(t: &mut Table) {
    reg(t, "len", |lua, a| {
        Ok(vec![Value::Int(check_str(lua, &a, 0, "len")?.len() as i64)])
    });
    reg(t, "sub", |lua, a| {
        let s = check_str(lua, &a, 0, "sub")?;
        let (i, j) = str_range(
            s.len(),
            opt_int(&a, 1, 1, "sub")?,
            opt_int(&a, 2, -1, "sub")?,
        );
        Ok(vec![Value::bytes(&s[i..j])])
    });
    reg(t, "upper", |lua, a| {
        Ok(vec![Value::bytes(
            &check_str(lua, &a, 0, "upper")?.to_ascii_uppercase(),
        )])
    });
    reg(t, "lower", |lua, a| {
        Ok(vec![Value::bytes(
            &check_str(lua, &a, 0, "lower")?.to_ascii_lowercase(),
        )])
    });
    reg(t, "rep", |lua, a| {
        let s = check_str(lua, &a, 0, "rep")?;
        let n = check_int(&a, 1, "rep")?.max(0) as usize;
        let sep = if arg(&a, 2).is_nil() {
            Vec::new()
        } else {
            check_str(lua, &a, 2, "rep")?
        };
        if (s.len() + sep.len()) * n > 1 << 26 {
            return err("resulting string too large");
        }
        let mut out = Vec::new();
        for k in 0..n {
            if k > 0 {
                out.extend_from_slice(&sep);
            }
            out.extend_from_slice(&s);
        }
        Ok(vec![Value::bytes(&out)])
    });
    reg(t, "reverse", |lua, a| {
        let mut s = check_str(lua, &a, 0, "reverse")?;
        s.reverse();
        Ok(vec![Value::bytes(&s)])
    });
    reg(t, "byte", |lua, a| {
        let s = check_str(lua, &a, 0, "byte")?;
        let i = opt_int(&a, 1, 1, "byte")?;
        let (i, j) = str_range(s.len(), i, opt_int(&a, 2, i, "byte")?);
        Ok(s[i..j].iter().map(|&c| Value::Int(c as i64)).collect())
    });
    reg(t, "char", |_, a| {
        let mut out = Vec::new();
        for i in 0..a.len() {
            out.push(check_int(&a, i, "char")? as u8);
        }
        Ok(vec![Value::bytes(&out)])
    });
    reg(t, "format", |lua, a| {
        Ok(vec![Value::bytes(&format(lua, &a)?)])
    });
    reg(t, "find", |lua, a| find(lua, a, true));
    reg(t, "match", |lua, a| find(lua, a, false));
    reg(t, "gmatch", |lua, a| {
        let s = check_str(lua, &a, 0, "gmatch")?;
        let p = check_str(lua, &a, 1, "gmatch")?;
        let pos = Rc::new(RefCell::new(0usize));
        Ok(vec![Value::native("gmatch_iter", move |lua, _| {
            let mut at = *pos.borrow();
            while at <= s.len() {
                let mut m = Matcher::new(&s, &p);
                if let Some(e) = m.do_match(at, 0).map_err(LuaError::new)? {
                    *pos.borrow_mut() = if e == at { e + 1 } else { e };
                    return Ok(captures(lua, &m, &s, at, e, true));
                }
                at += 1;
            }
            *pos.borrow_mut() = s.len() + 1;
            Ok(vec![Value::Nil])
        })])
    });
    reg(t, "gsub", gsub);
}

fn captures(_lua: &Lua, m: &Matcher, s: &[u8], st: usize, e: usize, whole: bool) -> Vec<Value> {
    let n = if m.level == 0 && whole { 1 } else { m.level };
    (0..n)
        .map(|i| match m.get(i, st, e) {
            Capture::Str(a, b) => Value::bytes(&s[a..b]),
            Capture::Pos(p) => Value::Int(p as i64),
        })
        .collect()
}

fn find(lua: &mut Lua, a: Args, find: bool) -> Ret {
    let f = if find { "find" } else { "match" };
    let s = check_str(lua, &a, 0, f)?;
    let p = check_str(lua, &a, 1, f)?;
    let init = opt_int(&a, 2, 1, f)?;
    let init = if init < 0 {
        (s.len() as i64 + init + 1).max(1)
    } else {
        init.max(1)
    } as usize
        - 1;
    if init > s.len() {
        return Ok(vec![Value::Nil]);
    }
    if find && (arg(&a, 3).truthy() || is_plain(&p)) {
        let at = s[init..].windows(p.len().max(1)).position(|w| w == &p[..]);
        let at = if p.is_empty() { Some(0) } else { at };
        return Ok(match at {
            Some(i) => vec![
                Value::Int((init + i + 1) as i64),
                Value::Int((init + i + p.len()) as i64),
            ],
            None => vec![Value::Nil],
        });
    }
    let (anchor, pstart) = if p.first() == Some(&b'^') {
        (true, 1)
    } else {
        (false, 0)
    };
    let pat = &p[pstart..];
    let mut at = init;
    loop {
        let mut m = Matcher::new(&s, pat);
        if let Some(e) = m.do_match(at, 0).map_err(LuaError::new)? {
            if find {
                let mut r = vec![Value::Int(at as i64 + 1), Value::Int(e as i64)];
                r.extend(captures(lua, &m, &s, at, e, false));
                return Ok(r);
            }
            return Ok(captures(lua, &m, &s, at, e, true));
        }
        at += 1;
        if anchor || at > s.len() {
            return Ok(vec![Value::Nil]);
        }
    }
}

fn gsub(lua: &mut Lua, a: Args) -> Ret {
    let s = check_str(lua, &a, 0, "gsub")?;
    let p = check_str(lua, &a, 1, "gsub")?;
    let repl = arg(&a, 2);
    let max = if arg(&a, 3).is_nil() {
        usize::MAX
    } else {
        check_int(&a, 3, "gsub")?.max(0) as usize
    };
    let (anchor, pstart) = if p.first() == Some(&b'^') {
        (true, 1)
    } else {
        (false, 0)
    };
    let pat = &p[pstart..];
    let mut out = Vec::new();
    let mut at = 0;
    let mut n = 0;
    while n < max {
        let mut m = Matcher::new(&s, pat);
        let e = m.do_match(at, 0).map_err(LuaError::new)?;
        if let Some(e) = e {
            n += 1;
            let caps = captures(lua, &m, &s, at, e, true);
            let whole = Value::bytes(&s[at..e]);
            let r = match &repl {
                Value::Str(r) => {
                    let mut o = Vec::new();
                    let mut i = 0;
                    while i < r.len() {
                        if r[i] == b'%' && i + 1 < r.len() {
                            i += 1;
                            let c = r[i];
                            if c == b'0' {
                                o.extend(lua.tostring_raw(&whole));
                            } else if c.is_ascii_digit() {
                                let v = caps.get((c - b'1') as usize).cloned().unwrap_or_default();
                                o.extend(lua.tostring_raw(&v));
                            } else {
                                o.push(c);
                            }
                        } else {
                            o.push(r[i]);
                        }
                        i += 1;
                    }
                    Value::bytes(&o)
                }
                Value::Int(_) | Value::Num(_) => Value::bytes(&lua.tostring_raw(&repl)),
                Value::Table(_) => lua.index(&repl, &caps[0])?,
                Value::Func(_) | Value::Native(_) => lua
                    .call(&repl, caps.clone())?
                    .into_iter()
                    .next()
                    .unwrap_or_default(),
                _ => return err("bad argument #3 to 'gsub' (string/function/table expected)"),
            };
            match r {
                Value::Nil | Value::Bool(false) => out.extend_from_slice(&s[at..e]),
                Value::Str(x) => out.extend_from_slice(&x),
                v @ (Value::Int(_) | Value::Num(_)) => out.extend(lua.tostring_raw(&v)),
                v => return err(format!("invalid replacement value (a {})", v.type_name())),
            }
            if e > at {
                at = e;
            } else {
                if at < s.len() {
                    out.push(s[at]);
                }
                at += 1;
            }
        } else {
            if at < s.len() {
                out.push(s[at]);
            }
            at += 1;
        }
        if at > s.len() || anchor {
            break;
        }
    }
    if at < s.len() {
        out.extend_from_slice(&s[at..]);
    }
    Ok(vec![Value::bytes(&out), Value::Int(n as i64)])
}

/// printf's %g with `prec` significant digits.
pub fn fmt_g(n: f64, prec: usize, upper: bool) -> String {
    let prec = prec.max(1);
    if n == 0.0 {
        return if n.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let e = format!("{:.*e}", prec - 1, n);
    let exp: i32 = e
        .split('e')
        .nth(1)
        .and_then(|x| x.parse().ok())
        .unwrap_or(0);
    let s = if exp < -4 || exp >= prec as i32 {
        let (m, _) = e.split_once('e').unwrap();
        let m = if m.contains('.') {
            m.trim_end_matches('0').trim_end_matches('.')
        } else {
            m
        };
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let f = format!("{:.*}", (prec as i32 - 1 - exp).max(0) as usize, n);
        if f.contains('.') {
            f.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            f
        }
    };
    if upper { s.to_uppercase() } else { s }
}

fn pad(s: String, flags: &str, width: usize, numeric: bool) -> String {
    if s.len() >= width {
        return s;
    }
    let fill = width - s.len();
    if flags.contains('-') {
        format!("{s}{}", " ".repeat(fill))
    } else if flags.contains('0') && numeric {
        let (sign, rest) = match s.strip_prefix('-') {
            Some(r) => ("-", r.to_string()),
            None => ("", s),
        };
        format!("{sign}{}{rest}", "0".repeat(fill))
    } else {
        format!("{}{s}", " ".repeat(fill))
    }
}

fn format(lua: &mut Lua, a: &Args) -> LuaResult<Vec<u8>> {
    let f = check_str(lua, a, 0, "format")?;
    let mut out = Vec::new();
    let mut i = 0;
    let mut n = 1;
    while i < f.len() {
        if f[i] != b'%' {
            out.push(f[i]);
            i += 1;
            continue;
        }
        i += 1;
        if f.get(i) == Some(&b'%') {
            out.push(b'%');
            i += 1;
            continue;
        }
        let start = i;
        while i < f.len() && b"-+ #0".contains(&f[i]) {
            i += 1;
        }
        let flags = String::from_utf8_lossy(&f[start..i]).into_owned();
        let ws = i;
        while i < f.len() && f[i].is_ascii_digit() {
            i += 1;
        }
        let width: usize = std::str::from_utf8(&f[ws..i]).unwrap().parse().unwrap_or(0);
        let mut prec = None;
        if f.get(i) == Some(&b'.') {
            i += 1;
            let ps = i;
            while i < f.len() && f[i].is_ascii_digit() {
                i += 1;
            }
            prec = Some(
                std::str::from_utf8(&f[ps..i])
                    .unwrap()
                    .parse()
                    .unwrap_or(0usize),
            );
        }
        let Some(&conv) = f.get(i) else {
            return err("invalid conversion to format string");
        };
        i += 1;
        let plus = |s: String, x: f64| {
            if x >= 0.0 && flags.contains('+') {
                format!("+{s}")
            } else if x >= 0.0 && flags.contains(' ') {
                format!(" {s}")
            } else {
                s
            }
        };
        let s = match conv {
            b'd' | b'i' => {
                let v = check_int(a, n, "format")?;
                let mut s = v.abs().to_string();
                if let Some(p) = prec {
                    while s.len() < p {
                        s.insert(0, '0');
                    }
                }
                if v < 0 {
                    s.insert(0, '-');
                }
                pad(plus(s, v as f64), &flags, width, true)
            }
            b'u' => pad(
                (check_int(a, n, "format")? as u64).to_string(),
                &flags,
                width,
                true,
            ),
            b'c' => {
                out.push(check_int(a, n, "format")? as u8);
                n += 1;
                continue;
            }
            b'x' | b'X' | b'o' => {
                let v = check_int(a, n, "format")?;
                let mut s = match conv {
                    b'x' => format!("{v:x}"),
                    b'X' => format!("{v:X}"),
                    _ => format!("{v:o}"),
                };
                if let Some(p) = prec {
                    while s.len() < p {
                        s.insert(0, '0');
                    }
                }
                if flags.contains('#') && v != 0 {
                    s.insert_str(
                        0,
                        if conv == b'x' {
                            "0x"
                        } else if conv == b'X' {
                            "0X"
                        } else {
                            "0"
                        },
                    );
                }
                pad(s, &flags, width, true)
            }
            b'f' | b'F' | b'e' | b'E' | b'g' | b'G' => {
                let x = check_num(a, n, "format")?;
                let p = prec.unwrap_or(6);
                let s = match conv {
                    b'f' | b'F' => format!("{x:.p$}"),
                    b'e' | b'E' => {
                        let e = format!("{x:.p$e}");
                        let (m, ex) = e.split_once('e').unwrap();
                        let ex: i32 = ex.parse().unwrap();
                        let s = format!("{m}e{}{:02}", if ex < 0 { '-' } else { '+' }, ex.abs());
                        if conv == b'E' { s.to_uppercase() } else { s }
                    }
                    _ => fmt_g(x, if p == 0 { 1 } else { p }, conv == b'G'),
                };
                pad(plus(s, x), &flags, width, true)
            }
            b'a' | b'A' => format!("{}", check_num(a, n, "format")?),
            b's' => {
                let mut s = lua.tostring(&arg(a, n))?;
                if let Some(p) = prec {
                    s.truncate(p);
                }
                let s = if s.len() < width {
                    let fill = vec![b' '; width - s.len()];
                    if flags.contains('-') {
                        [s, fill].concat()
                    } else {
                        [fill, s].concat()
                    }
                } else {
                    s
                };
                out.extend(s);
                n += 1;
                continue;
            }
            b'q' => {
                let s = lua.tostring_raw(&arg(a, n));
                out.push(b'"');
                for c in s {
                    match c {
                        b'"' | b'\\' | b'\n' => {
                            out.push(b'\\');
                            out.push(c);
                        }
                        b'\r' => out.extend(b"\\r"),
                        0 => out.extend(b"\\0"),
                        c => out.push(c),
                    }
                }
                out.push(b'"');
                n += 1;
                continue;
            }
            c => return err(format!("invalid option '%{}' to 'format'", c as char)),
        };
        out.extend(s.into_bytes());
        n += 1;
    }
    Ok(out)
}

fn open_table(t: &mut Table) {
    reg(t, "insert", |lua, a| {
        let tb = check_table(&a, 0, "insert")?;
        let n = tb.borrow().len();
        match a.len() {
            2 => tb.borrow_mut().set(Value::Int(n + 1), arg(&a, 1)),
            3 => {
                let pos = check_int(&a, 1, "insert")?;
                if pos < 1 || pos > n + 1 {
                    return err("bad argument #2 to 'insert' (position out of bounds)");
                }
                let mut t = tb.borrow_mut();
                for i in (pos..=n).rev() {
                    let v = t.get(&Value::Int(i));
                    t.set(Value::Int(i + 1), v);
                }
                t.set(Value::Int(pos), arg(&a, 2));
            }
            _ => return err("wrong number of arguments to 'insert'"),
        }
        let _ = lua;
        Ok(vec![])
    });
    reg(t, "remove", |_, a| {
        let tb = check_table(&a, 0, "remove")?;
        let n = tb.borrow().len();
        let pos = opt_int(&a, 1, n, "remove")?;
        if a.len() > 1 && n + 1 != pos && (pos < 1 || pos > n + 1) {
            return err("bad argument #2 to 'remove' (position out of bounds)");
        }
        let mut t = tb.borrow_mut();
        let v = t.get(&Value::Int(pos));
        for i in pos..n {
            let x = t.get(&Value::Int(i + 1));
            t.set(Value::Int(i), x);
        }
        if pos <= n {
            t.set(Value::Int(n), Value::Nil);
        }
        Ok(vec![v])
    });
    reg(t, "concat", |lua, a| {
        let tb = check_table(&a, 0, "concat")?;
        let sep = if arg(&a, 1).is_nil() {
            Vec::new()
        } else {
            check_str(lua, &a, 1, "concat")?
        };
        let i = opt_int(&a, 2, 1, "concat")?;
        let j = if arg(&a, 3).is_nil() {
            tb.borrow().len()
        } else {
            check_int(&a, 3, "concat")?
        };
        let mut out = Vec::new();
        for k in i..=j {
            let v = tb.borrow().get(&Value::Int(k));
            match v {
                Value::Str(_) | Value::Int(_) | Value::Num(_) => out.extend(lua.tostring_raw(&v)),
                v => {
                    return err(format!(
                        "invalid value (at index {k}) in table for 'concat' ({})",
                        v.type_name()
                    ));
                }
            }
            if k < j {
                out.extend_from_slice(&sep);
            }
        }
        Ok(vec![Value::bytes(&out)])
    });
    reg(t, "sort", |lua, a| {
        let tb = check_table(&a, 0, "sort")?;
        let cmp = arg(&a, 1);
        let n = tb.borrow().len();
        let mut v: Vec<Value> = (1..=n).map(|i| tb.borrow().get(&Value::Int(i))).collect();
        // merge sort through a fallible comparison
        let mut error = None;
        let mut less = |x: &Value, y: &Value| -> bool {
            if error.is_some() {
                return false;
            }
            let r = if cmp.is_nil() {
                lua.less(x, y, false)
            } else {
                lua.call(&cmp, vec![x.clone(), y.clone()])
                    .map(|r| r.first().is_some_and(Value::truthy))
            };
            r.unwrap_or_else(|e| {
                error = Some(e);
                false
            })
        };
        merge_sort(&mut v, &mut less);
        if let Some(e) = error {
            return Err(e);
        }
        let mut t = tb.borrow_mut();
        for (i, x) in v.into_iter().enumerate() {
            t.set(Value::Int(i as i64 + 1), x);
        }
        Ok(vec![])
    });
    reg(t, "pack", |_, a| {
        let mut t = Table::default();
        let n = a.len();
        for (i, v) in a.into_iter().enumerate() {
            t.set(Value::Int(i as i64 + 1), v);
        }
        t.set_str("n", Value::Int(n as i64));
        Ok(vec![Value::table(t)])
    });
    reg(t, "getn", |_, a| {
        Ok(vec![Value::Int(check_table(&a, 0, "getn")?.borrow().len())])
    });
}

fn merge_sort(v: &mut Vec<Value>, less: &mut impl FnMut(&Value, &Value) -> bool) {
    if v.len() <= 1 {
        return;
    }
    let mut right = v.split_off(v.len() / 2);
    merge_sort(v, less);
    merge_sort(&mut right, less);
    let left = std::mem::take(v);
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if less(&right[j], &left[i]) {
            v.push(right[j].clone());
            j += 1;
        } else {
            v.push(left[i].clone());
            i += 1;
        }
    }
    v.extend_from_slice(&left[i..]);
    v.extend_from_slice(&right[j..]);
}

fn open_math(t: &mut Table) {
    t.set_str("pi", Value::Num(std::f64::consts::PI));
    t.set_str("huge", Value::Num(f64::INFINITY));
    t.set_str("maxinteger", Value::Int(i64::MAX));
    t.set_str("mininteger", Value::Int(i64::MIN));
    fn f1(t: &mut Table, name: &'static str, f: fn(f64) -> f64) {
        reg(t, name, move |_, a| {
            Ok(vec![Value::Num(f(check_num(&a, 0, name)?))])
        });
    }
    f1(t, "sqrt", f64::sqrt);
    f1(t, "sin", f64::sin);
    f1(t, "cos", f64::cos);
    f1(t, "tan", f64::tan);
    f1(t, "asin", f64::asin);
    f1(t, "acos", f64::acos);
    f1(t, "exp", f64::exp);
    f1(t, "deg", f64::to_degrees);
    f1(t, "rad", f64::to_radians);
    let int_or = |x: f64| -> Value {
        if x.abs() < 9.2e18 {
            Value::Int(x as i64)
        } else {
            Value::Num(x)
        }
    };
    reg(t, "floor", move |_, a| match arg(&a, 0) {
        Value::Int(i) => Ok(vec![Value::Int(i)]),
        _ => Ok(vec![int_or(check_num(&a, 0, "floor")?.floor())]),
    });
    reg(t, "ceil", move |_, a| match arg(&a, 0) {
        Value::Int(i) => Ok(vec![Value::Int(i)]),
        _ => Ok(vec![int_or(check_num(&a, 0, "ceil")?.ceil())]),
    });
    reg(t, "abs", |_, a| match arg(&a, 0) {
        Value::Int(i) => Ok(vec![Value::Int(i.wrapping_abs())]),
        _ => Ok(vec![Value::Num(check_num(&a, 0, "abs")?.abs())]),
    });
    reg(t, "atan", |_, a| {
        let y = check_num(&a, 0, "atan")?;
        let x = if arg(&a, 1).is_nil() {
            1.0
        } else {
            check_num(&a, 1, "atan")?
        };
        Ok(vec![Value::Num(y.atan2(x))])
    });
    reg(t, "atan2", |_, a| {
        Ok(vec![Value::Num(
            check_num(&a, 0, "atan2")?.atan2(check_num(&a, 1, "atan2")?),
        )])
    });
    reg(t, "log", |_, a| {
        let x = check_num(&a, 0, "log")?;
        Ok(vec![Value::Num(match arg(&a, 1) {
            Value::Nil => x.ln(),
            _ => {
                let b = check_num(&a, 1, "log")?;
                if b == 2.0 {
                    x.log2()
                } else if b == 10.0 {
                    x.log10()
                } else {
                    x.ln() / b.ln()
                }
            }
        })])
    });
    reg(t, "log10", |_, a| {
        Ok(vec![Value::Num(check_num(&a, 0, "log10")?.log10())])
    });
    reg(t, "pow", |_, a| {
        Ok(vec![Value::Num(
            check_num(&a, 0, "pow")?.powf(check_num(&a, 1, "pow")?),
        )])
    });
    reg(t, "fmod", |_, a| match (arg(&a, 0), arg(&a, 1)) {
        (Value::Int(x), Value::Int(y)) if y != 0 => Ok(vec![Value::Int(x.wrapping_rem(y))]),
        _ => Ok(vec![Value::Num(
            check_num(&a, 0, "fmod")? % check_num(&a, 1, "fmod")?,
        )]),
    });
    reg(t, "modf", |_, a| {
        let x = check_num(&a, 0, "modf")?;
        let i = x.trunc();
        Ok(vec![
            Value::Num(i),
            Value::Num(if x.is_infinite() { 0.0 } else { x - i }),
        ])
    });
    fn minmax(a: &Args, lua: &mut Lua, max: bool) -> Ret {
        let mut best = arg(a, 0);
        if best.to_num().is_none() {
            return err("bad argument #1 (number expected)");
        }
        for v in a.iter().skip(1) {
            if v.to_num().is_none() {
                return err("number expected");
            }
            let better = if max {
                lua.less(&best, v, false)?
            } else {
                lua.less(v, &best, false)?
            };
            if better {
                best = v.clone();
            }
        }
        Ok(vec![best])
    }
    reg(t, "max", |lua, a| minmax(&a, lua, true));
    reg(t, "min", |lua, a| minmax(&a, lua, false));
    reg(t, "tointeger", |_, a| {
        Ok(vec![
            arg(&a, 0).to_int().map(Value::Int).unwrap_or_default(),
        ])
    });
    reg(t, "type", |_, a| {
        Ok(vec![match arg(&a, 0) {
            Value::Int(_) => Value::str("integer"),
            Value::Num(_) => Value::str("float"),
            _ => Value::Nil,
        }])
    });
    reg(t, "random", |lua, a| {
        // xorshift: games only need variety
        lua.seed ^= lua.seed << 13;
        lua.seed ^= lua.seed >> 7;
        lua.seed ^= lua.seed << 17;
        let r = (lua.seed >> 11) as f64 / (1u64 << 53) as f64;
        match a.len() {
            0 => Ok(vec![Value::Num(r)]),
            1 => {
                let m = check_int(&a, 0, "random")?;
                Ok(vec![Value::Int(1 + (r * m as f64) as i64)])
            }
            _ => {
                let (lo, hi) = (check_int(&a, 0, "random")?, check_int(&a, 1, "random")?);
                if lo > hi {
                    return err("bad argument #2 to 'random' (interval is empty)");
                }
                Ok(vec![Value::Int(lo + (r * (hi - lo + 1) as f64) as i64)])
            }
        }
    });
    reg(t, "randomseed", |lua, a| {
        lua.seed = (check_num(&a, 0, "randomseed").unwrap_or(1.0) as i64 as u64) | 1;
        Ok(vec![])
    });
    let _ = Arith::Add;
}

fn open_utf8(t: &mut Table) {
    t.set_str(
        "charpattern",
        Value::bytes(b"[\x00-\x7F\xC2-\xFD][\x80-\xBF]*"),
    );
    reg(t, "char", |_, a| {
        let mut s = String::new();
        for i in 0..a.len() {
            s.push(char::from_u32(check_int(&a, i, "char")? as u32).unwrap_or('\u{fffd}'));
        }
        Ok(vec![Value::str(&s)])
    });
    reg(t, "len", |lua, a| {
        let s = check_str(lua, &a, 0, "len")?;
        Ok(vec![match std::str::from_utf8(&s) {
            Ok(s) => Value::Int(s.chars().count() as i64),
            Err(e) => {
                return Ok(vec![Value::Nil, Value::Int(e.valid_up_to() as i64 + 1)]);
            }
        }])
    });
    reg(t, "codepoint", |lua, a| {
        let s = check_str(lua, &a, 0, "codepoint")?;
        let i = opt_int(&a, 1, 1, "codepoint")?;
        let (i, j) = str_range(s.len(), i, opt_int(&a, 2, i, "codepoint")?);
        let text = String::from_utf8_lossy(&s[i..]);
        let mut out = Vec::new();
        let mut at = i;
        for c in text.chars() {
            if at >= j.max(i + 1) {
                break;
            }
            out.push(Value::Int(c as i64));
            at += c.len_utf8();
        }
        Ok(out)
    });
    reg(t, "offset", |lua, a| {
        let s = check_str(lua, &a, 0, "offset")?;
        let n = check_int(&a, 1, "offset")?;
        let starts: Vec<usize> = (0..s.len()).filter(|&k| s[k] & 0xc0 != 0x80).collect();
        let def = if n >= 0 { 1 } else { s.len() as i64 + 1 };
        let i = opt_int(&a, 2, def, "offset")? as usize - 1;
        let idx = starts.partition_point(|&k| k < i) as i64;
        let k = if n > 0 {
            idx + n - 1
        } else if n == 0 {
            idx
        } else {
            idx + n
        };
        Ok(vec![match k {
            k if k >= 0 && (k as usize) < starts.len() => Value::Int(starts[k as usize] as i64 + 1),
            k if k as usize == starts.len() => Value::Int(s.len() as i64 + 1),
            _ => Value::Nil,
        }])
    });
    reg(t, "codes", |lua, a| {
        let s = check_str(lua, &a, 0, "codes")?;
        let iter = Value::native("codes_iter", move |_, b| {
            let i = check_int(&b, 1, "codes")? as usize;
            let mut k = i;
            if k > 0 {
                k -= 1;
                k += 1;
                while k < s.len() && s[k] & 0xc0 == 0x80 {
                    k += 1;
                }
            }
            if k >= s.len() {
                return Ok(vec![Value::Nil]);
            }
            let c = String::from_utf8_lossy(&s[k..])
                .chars()
                .next()
                .unwrap_or('\u{fffd}');
            Ok(vec![Value::Int(k as i64 + 1), Value::Int(c as i64)])
        });
        Ok(vec![iter, arg(&a, 0), Value::Int(0)])
    });
}

fn open_bit32(t: &mut Table) {
    fn u(a: &Args, i: usize) -> LuaResult<u32> {
        Ok(check_int(a, i, "bit32")? as u32)
    }
    reg(t, "band", |_, a| {
        Ok(vec![Value::Int(
            (0..a.len()).try_fold(u32::MAX, |x, i| u(&a, i).map(|y| x & y))? as i64,
        )])
    });
    reg(t, "bor", |_, a| {
        Ok(vec![Value::Int(
            (0..a.len()).try_fold(0, |x, i| u(&a, i).map(|y| x | y))? as i64,
        )])
    });
    reg(t, "bxor", |_, a| {
        Ok(vec![Value::Int(
            (0..a.len()).try_fold(0, |x, i| u(&a, i).map(|y| x ^ y))? as i64,
        )])
    });
    reg(t, "bnot", |_, a| Ok(vec![Value::Int(!u(&a, 0)? as i64)]));
    reg(t, "btest", |_, a| {
        Ok(vec![Value::Bool(
            (0..a.len()).try_fold(u32::MAX, |x, i| u(&a, i).map(|y| x & y))? != 0,
        )])
    });
    reg(t, "lshift", |_, a| {
        let n = check_int(&a, 1, "lshift")?;
        let x = u(&a, 0)?;
        Ok(vec![Value::Int(if n.abs() >= 32 {
            0
        } else if n >= 0 {
            (x << n) as i64
        } else {
            (x >> -n) as i64
        })])
    });
    reg(t, "rshift", |_, a| {
        let n = check_int(&a, 1, "rshift")?;
        let x = u(&a, 0)?;
        Ok(vec![Value::Int(if n.abs() >= 32 {
            0
        } else if n >= 0 {
            (x >> n) as i64
        } else {
            (x << -n) as i64
        })])
    });
    reg(t, "arshift", |_, a| {
        let n = check_int(&a, 1, "arshift")?.clamp(-31, 31);
        let x = u(&a, 0)? as i32;
        Ok(vec![Value::Int(
            (if n >= 0 { x >> n } else { x << -n }) as u32 as i64,
        )])
    });
    reg(t, "extract", |_, a| {
        let (x, f) = (u(&a, 0)?, check_int(&a, 1, "extract")? as u32);
        let w = opt_int(&a, 2, 1, "extract")? as u32;
        let mask = if w >= 32 { u32::MAX } else { (1 << w) - 1 };
        Ok(vec![Value::Int(((x >> f) & mask) as i64)])
    });
}
