//! Lists the files of a YPF archive: packed size, size and name.
//!
//!     cargo run --example ypfls <archive.ypf>

fn main() {
    let path = std::env::args().nth(1).expect("usage: ypfls <archive.ypf>");
    let a = std::fs::read(path).unwrap();
    for e in yuris::ypf::Index::parse(&a).unwrap().entries {
        println!("{} {} {}", e.packed_size, e.size, e.name);
    }
}
