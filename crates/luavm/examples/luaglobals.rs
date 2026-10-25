//! Lists the global names chunks read, with the fields read from them (`name.field`).
//!
//!     luaglobals <chunk> ...

use std::collections::BTreeSet;

use luavm::undump::{self, Proto, Version};
use luavm::Value;

fn name(v: &Value) -> Option<String> {
    v.as_bytes().map(|b| String::from_utf8_lossy(b).into_owned())
}

fn walk(p: &Proto, out: &mut BTreeSet<String>) {
    let (gettabup, getglobal, gettable, selfop) = match p.version {
        Version::V53 => (6, 99, 7, 12),
        Version::V51 => (99, 5, 6, 11),
    };
    // register -> global it holds (within straight-line code; good enough for a listing)
    let mut held: Vec<Option<String>> = vec![None; 256];
    for &i in &p.code {
        let op = i & 0x3f;
        let a = ((i >> 6) & 0xff) as usize;
        let c = ((i >> 14) & 0x1ff) as usize;
        let b = ((i >> 23) & 0x1ff) as usize;
        let bx = (i >> 14) as usize;
        let k = |x: usize| if x & 0x100 != 0 { p.consts.get(x & 0xff).and_then(name) } else { None };
        if op == gettabup && b == 0 {
            if let Some(n) = k(c) {
                out.insert(n.clone());
                held[a] = Some(n);
                continue;
            }
        } else if op == getglobal {
            if let Some(n) = p.consts.get(bx).and_then(name) {
                out.insert(n.clone());
                held[a] = Some(n);
                continue;
            }
        } else if op == gettable || op == selfop {
            if let (Some(g), Some(f)) = (held[b].clone(), k(c)) {
                out.insert(format!("{g}.{f}"));
            } else if op == selfop {
                if let Some(f) = k(c) {
                    out.insert(format!(":{f}"));
                }
            }
        }
        held[a] = None;
    }
    for q in &p.protos {
        walk(q, out);
    }
}

fn main() {
    let mut out = BTreeSet::new();
    for f in std::env::args().skip(1) {
        let b = std::fs::read(&f).expect("read");
        match undump::load(&b, &f) {
            Ok(p) => walk(&p, &mut out),
            Err(e) => eprintln!("{e}"),
        }
    }
    for n in out {
        println!("{n}");
    }
}
