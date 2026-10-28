//! Prints the instructions of precompiled chunks (5.3 opcode names; 5.1 chunks print raw numbers).
//!
//!     luadis <chunk> [function name filter]

use luavm::undump::{self, Proto, Version};

const NAMES: [&str; 47] = [
    "MOVE", "LOADK", "LOADKX", "LOADBOOL", "LOADNIL", "GETUPVAL", "GETTABUP", "GETTABLE",
    "SETTABUP", "SETUPVAL", "SETTABLE", "NEWTABLE", "SELF", "ADD", "SUB", "MUL", "MOD", "POW",
    "DIV", "IDIV", "BAND", "BOR", "BXOR", "SHL", "SHR", "UNM", "BNOT", "NOT", "LEN", "CONCAT",
    "JMP", "EQ", "LT", "LE", "TEST", "TESTSET", "CALL", "TAILCALL", "RETURN", "FORLOOP", "FORPREP",
    "TFORCALL", "TFORLOOP", "SETLIST", "CLOSURE", "VARARG", "EXTRAARG",
];
const NAMES51: [&str; 38] = [
    "MOVE",
    "LOADK",
    "LOADBOOL",
    "LOADNIL",
    "GETUPVAL",
    "GETGLOBAL",
    "GETTABLE",
    "SETGLOBAL",
    "SETUPVAL",
    "SETTABLE",
    "NEWTABLE",
    "SELF",
    "ADD",
    "SUB",
    "MUL",
    "DIV",
    "MOD",
    "POW",
    "UNM",
    "NOT",
    "LEN",
    "CONCAT",
    "JMP",
    "EQ",
    "LT",
    "LE",
    "TEST",
    "TESTSET",
    "CALL",
    "TAILCALL",
    "RETURN",
    "FORLOOP",
    "FORPREP",
    "TFORLOOP",
    "SETLIST",
    "CLOSE",
    "CLOSURE",
    "VARARG",
];

fn dump(p: &Proto, path: &str) {
    println!(
        "function {path} (line {}, {} params{})",
        p.line,
        p.params,
        if p.vararg { ", vararg" } else { "" }
    );
    for (pc, &i) in p.code.iter().enumerate() {
        let op = (i & 0x3f) as usize;
        let a = (i >> 6) & 0xff;
        let c = (i >> 14) & 0x1ff;
        let b = (i >> 23) & 0x1ff;
        let bx = i >> 14;
        let sbx = bx as i64 - 131071;
        let name = match p.version {
            Version::V53 => NAMES.get(op),
            Version::V51 => NAMES51.get(op),
        }
        .copied()
        .unwrap_or("?");
        let k = |x: u32| {
            if x & 0x100 != 0 {
                format!(
                    "{:?}",
                    p.consts
                        .get((x & 0xff) as usize)
                        .cloned()
                        .unwrap_or_default()
                )
            } else {
                format!("r{x}")
            }
        };
        let text = match name {
            "LOADK" | "GETGLOBAL" | "SETGLOBAL" => format!(
                "r{a} {:?}",
                p.consts.get(bx as usize).cloned().unwrap_or_default()
            ),
            "JMP" | "FORLOOP" | "FORPREP" | "TFORLOOP" => format!("{a} -> {}", pc as i64 + 1 + sbx),
            "CLOSURE" => format!("r{a} f{bx}"),
            "GETTABUP" => format!("r{a} u{b}[{}]", k(c)),
            "SETTABUP" => format!("u{a}[{}] = {}", k(b), k(c)),
            "GETTABLE" | "SELF" => format!("r{a} r{b}[{}]", k(c)),
            "SETTABLE" => format!("r{a}[{}] = {}", k(b), k(c)),
            "EQ" | "LT" | "LE" | "ADD" | "SUB" | "MUL" | "DIV" | "MOD" | "POW" | "IDIV" => {
                format!("{a} {} {}", k(b), k(c))
            }
            _ => format!("{a} {b} {c}"),
        };
        println!("  {pc:4} {name:9} {text}");
    }
    for (n, q) in p.protos.iter().enumerate() {
        dump(q, &format!("{path}/f{n}"));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let b = std::fs::read(&args[1]).expect("read");
    let p = undump::load(&b, &args[1]).expect("load");
    dump(&p, "main");
}
