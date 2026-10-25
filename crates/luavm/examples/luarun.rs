//! Runs precompiled chunks in order and lists the globals they define.
//!
//!     luarun <chunk> ...

fn main() {
    let mut lua = luavm::Lua::new();
    let before: Vec<String> = keys(&lua);
    for f in std::env::args().skip(1) {
        let b = std::fs::read(&f).expect("read");
        match lua.exec(&b, &f) {
            Ok(_) => eprintln!("{f}: ok"),
            Err(e) => eprintln!("{f}: {e}"),
        }
    }
    for k in keys(&lua) {
        if !before.contains(&k) {
            println!("{k}: {:?}", lua.global(&k));
        }
    }
}

fn keys(lua: &luavm::Lua) -> Vec<String> {
    let g = lua.globals.borrow();
    let mut k = luavm::Value::Nil;
    let mut out = Vec::new();
    while let Some((n, _)) = g.next(&k) {
        if n.is_nil() {
            break;
        }
        out.push(format!("{n:?}").trim_matches('"').to_string());
        k = n;
    }
    out
}
