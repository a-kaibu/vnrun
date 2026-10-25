//! Lists the files of a game's archive, or writes one out.
//!
//!     cargo run --release --example pacls <game folder> <pack> [name [out]]

use nexas::files::{Archives, Disk};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(&a[1]);
    let mut disk = Disk {
        saves: root.join("vnrun-saves"),
        root,
    };
    let arcs = Archives::open(&mut disk);
    match a.get(3) {
        None => arcs.names(&a[2]).iter().for_each(|n| println!("{n}")),
        Some(name) => {
            let b = arcs.read(&mut disk, &a[2], name).expect("no such file");
            std::fs::write(a.get(4).map_or(name.as_str(), |s| s), b).unwrap();
        }
    }
}
