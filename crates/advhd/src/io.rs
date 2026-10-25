//! The part of Lua's `io` the interface uses: files in the save folder (bookmarks, logs).
//! Folders in a path are dropped; every file lives in the save folder.

use crate::host::St;
use luavm::{Lua, LuaError, Table, Value};
use std::cell::RefCell;
use std::rc::Rc;

struct File {
    name: String,
    buf: Vec<u8>,
    pos: usize,
    write: bool,
}

impl File {
    fn read(&mut self, how: &Value) -> Value {
        let rest = &self.buf[self.pos.min(self.buf.len())..];
        if let Some(n) = how.to_int().filter(|_| !matches!(how, Value::Str(_))) {
            if rest.is_empty() && n > 0 {
                return Value::Nil;
            }
            let n = (n.max(0) as usize).min(rest.len());
            self.pos += n;
            return Value::bytes(&rest[..n]);
        }
        let how = how.as_bytes().unwrap_or(b"l");
        match how.strip_prefix(b"*").unwrap_or(how).first() {
            Some(b'n') => {
                let skip = rest.iter().take_while(|c| c.is_ascii_whitespace()).count();
                let len = rest[skip..]
                    .iter()
                    .take_while(|c| {
                        c.is_ascii_digit() || b"+-.eExX".contains(c) || c.is_ascii_hexdigit()
                    })
                    .count();
                self.pos += skip + len;
                luavm::value::parse_number(&rest[skip..skip + len]).unwrap_or(Value::Nil)
            }
            Some(b'a') => {
                self.pos = self.buf.len();
                Value::bytes(rest)
            }
            _ => {
                if rest.is_empty() {
                    return Value::Nil;
                }
                let end = rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len());
                self.pos += (end + 1).min(rest.len());
                let line = rest[..end].strip_suffix(b"\r").unwrap_or(&rest[..end]);
                Value::bytes(line)
            }
        }
    }
}

fn name(path: &[u8]) -> String {
    let base = path
        .rsplit(|&c| c == b'/' || c == b'\\')
        .next()
        .unwrap_or(path);
    encoding_rs::SHIFT_JIS.decode(base).0.into_owned()
}

pub fn install(lua: &mut Lua, st: &St) {
    let mut io = Table::default();
    let st = st.clone();
    io.set_str(
        "open",
        Value::native("io.open", move |_, args| {
            let path = args
                .first()
                .and_then(|v| v.as_bytes())
                .ok_or_else(|| LuaError::new("io.open: no file name"))?;
            let mode = args
                .get(1)
                .and_then(|v| v.as_bytes())
                .unwrap_or(b"r")
                .to_vec();
            let name = name(path);
            let old = st.borrow_mut().files.read_save(&name);
            let write = mode.iter().any(|c| b"wa+".contains(c));
            let buf = match mode.first() {
                Some(b'w') => Vec::new(),
                Some(b'a') => old.unwrap_or_default(),
                _ => match old {
                    Some(b) => b,
                    None => {
                        return Ok(vec![
                            Value::Nil,
                            Value::str(&format!("{name}: No such file or directory")),
                        ]);
                    }
                },
            };
            let pos = if mode.first() == Some(&b'a') {
                buf.len()
            } else {
                0
            };
            Ok(vec![file(
                &st,
                File {
                    name,
                    buf,
                    pos,
                    write,
                },
            )])
        }),
    );
    lua.set_global("io", Value::table(io));
}

fn file(st: &St, f: File) -> Value {
    let f = Rc::new(RefCell::new(f));
    let mut t = Table::default();
    let g = f.clone();
    t.set_str(
        "read",
        Value::native("file:read", move |_, args| {
            let mut f = g.borrow_mut();
            if args.len() <= 1 {
                return Ok(vec![f.read(&Value::str("l"))]);
            }
            Ok(args[1..].iter().map(|h| f.read(h)).collect())
        }),
    );
    let g = f.clone();
    t.set_str(
        "lines",
        Value::native("file:lines", move |_, _| {
            let g = g.clone();
            Ok(vec![Value::native("lines", move |_, _| {
                Ok(vec![g.borrow_mut().read(&Value::str("l"))])
            })])
        }),
    );
    let (g, st) = (f.clone(), st.clone());
    t.set_str(
        "write",
        Value::native("file:write", move |lua, args| {
            let mut f = g.borrow_mut();
            if !f.write {
                return Ok(vec![Value::Nil, Value::str("file is not open for writing")]);
            }
            for v in &args[1.min(args.len())..] {
                let b = lua.tostring_raw(v);
                let at = f.pos.min(f.buf.len());
                f.buf.truncate(at);
                f.buf.extend_from_slice(&b);
                f.pos = f.buf.len();
            }
            let (name, buf) = (f.name.clone(), f.buf.clone());
            st.borrow_mut().files.write_save(&name, &buf);
            Ok(vec![args.first().cloned().unwrap_or(Value::Nil)])
        }),
    );
    t.set_str(
        "close",
        Value::native("file:close", |_, _| Ok(vec![Value::Bool(true)])),
    );
    t.set_str("flush", Value::native("file:flush", |_, _| Ok(vec![])));
    Value::table(t)
}
