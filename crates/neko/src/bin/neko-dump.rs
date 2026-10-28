//! Looks into the NEKOPACK archives of a NekoSDK game.
//!
//!     neko-dump list <game folder>                    every file, as pack\name and size
//!     neko-dump x <game folder> <out> [pack]          unpack every file (or one pack's)
//!     neko-dump png <game folder> <out> <pack\name>.. pictures (BMP + its .alp) as PNG

use std::path::Path;

use neko::arc::Archives;
use nexas::files::{Disk, Files};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err("usage: neko-dump list|x|png <game folder> [out] [names]".into());
    }
    let dir = Path::new(&args[2]);
    let mut disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let files: &mut dyn Files = &mut disk;
    let arcs = Archives::open(files);
    let mut packs: Vec<&String> = arcs.packs.keys().collect();
    packs.sort();
    match args[1].as_str() {
        "list" => {
            for p in packs {
                for n in &arcs.packs[p].names {
                    let size = arcs.packs[p].read(files, n).map_or(0, |b| b.len());
                    println!("{p}\\{n}\t{size}");
                }
            }
        }
        "x" => {
            let out = Path::new(args.get(3).ok_or("x: no out folder")?);
            for p in packs {
                if args.get(4).is_some_and(|only| only != p) {
                    continue;
                }
                for n in &arcs.packs[p].names {
                    let Some(b) = arcs.packs[p].read(files, n) else {
                        eprintln!("{p}\\{n}: unreadable");
                        continue;
                    };
                    let mut path = out.join(p);
                    for part in n.split('\\') {
                        path.push(part);
                    }
                    if let Some(d) = path.parent() {
                        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&path, b).map_err(|e| e.to_string())?;
                }
            }
        }
        "png" => {
            let out = Path::new(args.get(3).ok_or("png: no out folder")?);
            std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
            for name in &args[4..] {
                let bmp = arcs.read(files, name).ok_or(format!("{name}: not found"))?;
                let alp_name = match name.rsplit_once('.') {
                    Some((stem, _)) => format!("{stem}.alp"),
                    None => format!("{name}.alp"),
                };
                let alp = arcs.read(files, &alp_name);
                let img = neko::pic::decode(&bmp, alp.as_deref())?;
                let file = name.replace(['\\', '/'], "_").replace(".bmp", ".png");
                let path = out.join(file);
                image::save_buffer(&path, &img.px, img.w, img.h, image::ColorType::Rgba8)
                    .map_err(|e| e.to_string())?;
                println!("{} {}x{}", path.display(), img.w, img.h);
            }
        }
        _ => return Err("neko-dump: list, x or png".into()),
    }
    Ok(())
}
