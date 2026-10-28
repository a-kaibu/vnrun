//! Looks into a LiLiM game's archives.
//!
//!     lilim-dump <game folder> list
//!     lilim-dump <game folder> extract <name> <out file>   (unpacked)
//!     lilim-dump <game folder> png <name> <out.png>

use std::path::Path;

use lilim::arc::Archives;
use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err(
            "usage: lilim-dump <game folder> list | extract <name> <out> | png <name> <out.png>"
                .into(),
        );
    }
    let dir = Path::new(&args[1]);
    let mut disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let arcs = Archives::open(&mut disk)?;
    match args[2].as_str() {
        "list" => {
            let mut v: Vec<_> = arcs.names().collect();
            v.sort_by(|a, b| (&a.arc, &a.name).cmp(&(&b.arc, &b.name)));
            for e in v {
                println!("{}\t{}\t{}\t{}", e.arc, e.name, e.offset, e.size);
            }
        }
        "extract" | "png" => {
            let name = args.get(3).ok_or("a name")?;
            let out = args.get(4).ok_or("an output file")?;
            let e = arcs
                .entry(name)
                .ok_or(format!("{name}: not found"))?
                .clone();
            let b = arcs.read(&mut disk, &e).ok_or("unreadable")?;
            if args[2] == "extract" {
                std::fs::write(out, &b).map_err(|e| e.to_string())?;
            } else {
                let img = lilim::pic::decode(&b)?;
                image::save_buffer(out, &img.px, img.w, img.h, image::ColorType::Rgba8)
                    .map_err(|e| e.to_string())?;
                println!("{}x{}", img.w, img.h);
            }
        }
        _ => return Err("list, extract or png".into()),
    }
    Ok(())
}
