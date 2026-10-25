//! Writes one unpacked file of a YPF archive to stdout.
//!
//!     cargo run --example ypfcat <archive.ypf> <name suffix>

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let a = std::fs::read(&args[1]).unwrap();
    let idx = yuris::ypf::Index::parse(&a).unwrap();
    let e = idx.entries.iter().find(|e| e.name.to_lowercase().ends_with(&args[2].to_lowercase())).expect("no such file");
    let b = e.unpack(&a[e.offset as usize..e.offset as usize + e.packed_size as usize]).unwrap();
    std::io::stdout().write_all(&b).unwrap();
}
