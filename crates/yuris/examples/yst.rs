//! Prints a script's instructions with their expression tokens.
//!
//!     cargo run --example yst <ysbin.ypf> <script number> [from] [to]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let a = std::fs::read(&args[1]).unwrap();
    let want = format!("yst{:05}.ybn", args[2].parse::<u32>().unwrap());
    let from: usize = args.get(3).map_or(0, |s| s.parse().unwrap());
    let to: usize = args.get(4).map_or(usize::MAX, |s| s.parse().unwrap());
    let idx = yuris::ypf::Index::parse(&a).unwrap();
    let e = idx.entries.iter().find(|e| e.name.to_lowercase().ends_with(&want)).expect("no such script");
    let b = e.unpack(&a[e.offset as usize..e.offset as usize + e.packed_size as usize]).unwrap();
    for (i, ins) in yuris::ybn::script(&b, None).unwrap().code.iter().enumerate().skip(from).take(to.saturating_sub(from)) {
        let args: Vec<String> = ins
            .args
            .iter()
            .map(|a| {
                let t: Vec<String> = yuris::expr::tokens(&a.value)
                    .map(|(t, d)| format!("{}{:02x?}", t as char, d))
                    .collect();
                format!("k{}:{:x}[{}]", a.keyword, a.ty, t.join(" "))
            })
            .collect();
        println!("{i} op{} {}", ins.op, args.join(" "));
    }
}
