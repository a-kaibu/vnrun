//! Prints the banks (entry points) of a compiled script.
//!
//!     cargo run --release --example banks <game folder> <file.bin>

use nexas::files::{Archives, Disk};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(&a[1]);
    let mut disk = Disk {
        saves: root.join("vnrun-saves"),
        root,
    };
    let arcs = Archives::open(&mut disk);
    let b = arcs
        .read(&mut disk, "Script", &a[2])
        .expect("no such script");
    let s = nexas::script::parse(&b).unwrap();
    for bank in &s.main.banks {
        let vars: Vec<String> = (0..8)
            .filter(|&k| bank.vars[k] >= 0)
            .map(|k| {
                let v = bank.vars[k] as usize;
                format!(
                    "{}={}",
                    s.main.int_names.get(v).map_or("?", |n| n.as_str()),
                    bank.defaults[k]
                )
            })
            .collect();
        println!("label {}: {}", bank.label, vars.join(" "));
    }
}
