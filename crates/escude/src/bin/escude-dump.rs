//! Lists an Escu:de game's archives, or writes one file out (unpacked).
//!
//!     escude-dump <game folder> [name [out]]
//!     escude-dump <game folder> --all <out folder>

use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return Err("usage: escude-dump <game folder> [name [out] | --all <out folder>]".into());
    }
    let dir = std::path::PathBuf::from(&args[1]);
    let mut disk = Disk {
        root: dir.clone(),
        saves: dir.join("vnrun-saves"),
    };
    let arc = escude::archive::Archives::open(&mut disk)?;
    let Some(name) = args.get(2) else {
        for n in &arc.names {
            let e = &arc.entries[&escude::archive::key(n)];
            println!("{:>10} {} {n}", e.size, e.file);
        }
        return Ok(());
    };
    if name == "--parse" {
        // Parses a program: escude-dump <game> --parse verdia\\global.c
        let path = args.get(3).ok_or("--parse <path>")?;
        let base = path.find('\\').map_or("", |n| &path[..n + 1]).to_string();
        let mut load = |p: &str| arc.read(&mut disk, p);
        let unit = escude::c::parse_program(path, &base, &mut load)?;
        println!(
            "{} items, {} structs, {} traps",
            unit.items.len(),
            unit.types.structs.len(),
            unit.traps.len()
        );
        return Ok(());
    }
    if name == "--all" {
        let out = std::path::PathBuf::from(args.get(3).ok_or("--all <out folder>")?);
        for n in &arc.names {
            let Some(data) = arc.read(&mut disk, n) else {
                eprintln!("{n}: cannot read");
                continue;
            };
            let path = out.join(n.replace('\\', "/"));
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&path, data).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let data = arc
        .read(&mut disk, name)
        .ok_or(format!("{name}: not found"))?;
    match args.get(3) {
        Some(out) => std::fs::write(out, &data).map_err(|e| e.to_string()),
        None => {
            println!("{} bytes", data.len());
            Ok(())
        }
    }
}
