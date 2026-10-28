//! Looks into an Ivory game's archives.
//!
//!     ivory-dump <game folder> list
//!     ivory-dump <game folder> extract <name> <out file>
//!     ivory-dump <game folder> extract-all <pack> <out folder>
//!     ivory-dump <game folder> png <name> <out.png>

use std::path::Path;

use ivory::pk::Packs;
use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err(
            "usage: ivory-dump <game folder> list | extract <name> <out> | png <name> <out>".into(),
        );
    }
    let dir = Path::new(&args[1]);
    let mut disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let packs = Packs::open(&mut disk);
    match args[2].as_str() {
        "list" => {
            let mut v: Vec<_> = packs.entries().collect();
            v.sort_by(|a, b| (&a.pack, &a.name).cmp(&(&b.pack, &b.name)));
            for e in v {
                println!("{}\t{}\t{}", e.pack, e.name, e.size);
            }
        }
        "extract" => {
            let name = args.get(3).ok_or("a name")?;
            let out = args.get(4).ok_or("an output file")?;
            let b = packs
                .read(&mut disk, name)
                .ok_or(format!("{name}: not found"))?;
            std::fs::write(out, &b).map_err(|e| e.to_string())?;
        }
        "extract-all" => {
            let pack = args.get(3).ok_or("a pack")?;
            let out = Path::new(args.get(4).ok_or("an output folder")?);
            std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
            let names: Vec<String> = packs
                .entries()
                .filter(|e| &e.pack == pack)
                .map(|e| e.name.clone())
                .collect();
            for n in names {
                let b = packs.read(&mut disk, &n).ok_or(format!("{n}: not found"))?;
                std::fs::write(out.join(&n), &b).map_err(|e| e.to_string())?;
            }
        }
        "png" => {
            let name = args.get(3).ok_or("a name")?;
            let out = args.get(4).ok_or("an output file")?;
            let b = packs
                .read(&mut disk, name)
                .ok_or(format!("{name}: not found"))?;
            let sg = ivory::sg::decode(&b)?;
            image::save_buffer(out, &sg.img.px, sg.img.w, sg.img.h, image::ColorType::Rgba8)
                .map_err(|e| e.to_string())?;
            println!("{}x{} at {} {}", sg.img.w, sg.img.h, sg.x, sg.y);
        }
        c => return Err(format!("{c}: unknown command")),
    }
    Ok(())
}
