//! Tests run chunks compiled by luac5.3 / luac5.1 when they are installed.

use super::*;

fn compile(luac: &str, src: &str) -> Option<Vec<u8>> {
    let dir = std::env::temp_dir().join(format!("luavm-test-{}-{luac}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let (s, o) = (dir.join("t.lua"), dir.join("t.out"));
    std::fs::write(&s, src).ok()?;
    let ok = std::process::Command::new(luac)
        .arg("-o")
        .arg(&o)
        .arg(&s)
        .status()
        .ok()?
        .success();
    if !ok {
        panic!("{luac} failed");
    }
    std::fs::read(&o).ok()
}

fn run(src: &str) {
    let mut ran = false;
    for luac in ["luac5.3", "luac5.1"] {
        let Some(chunk) = compile(luac, src) else {
            continue;
        };
        ran = true;
        let mut lua = Lua::new();
        let r = lua.exec(&chunk, "t");
        if let Err(e) = r {
            panic!("{luac}: {e}");
        }
    }
    if !ran {
        eprintln!("no luac installed; skipped");
    }
}

#[test]
fn basics() {
    run(include_str!("../tests/basics.lua"));
}

#[test]
fn coroutines() {
    run(include_str!("../tests/coroutine.lua"));
}
