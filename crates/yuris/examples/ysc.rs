//! Prints the commands (ysc.ybn) or labels (ysl.ybn) of a game's ysbin.ypf.
//!
//!     cargo run --example ysc <ysbin.ypf> [ysl]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let a = std::fs::read(&args[1]).unwrap();
    let want = if args.get(2).is_some_and(|s| s == "ysl") { "ysl.ybn" } else { "ysc.ybn" };
    let idx = yuris::ypf::Index::parse(&a).unwrap();
    for e in &idx.entries {
        if e.name.to_lowercase().ends_with(want) {
            let raw = &a[e.offset as usize..e.offset as usize + e.packed_size as usize];
            let b = e.unpack(raw).unwrap();
            if want == "ysl.ybn" {
                for l in yuris::ybn::labels(&b).unwrap() {
                    println!("{} {} {}", l.script, l.ip, l.name);
                }
            } else {
                for (i, c) in yuris::ybn::commands(&b).unwrap().iter().enumerate() {
                    println!("{i} {} {:?}", c.name, c.keywords);
                }
            }
        }
    }
}
