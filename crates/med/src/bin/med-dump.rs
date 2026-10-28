//! Looks into the archives of a MED game.
//!
//!     med-dump list <game folder>                 every file, as pack:name and size
//!     med-dump x <game folder> <out> [pack]       unpack every file (or one pack's)
//!     med-dump png <game folder> <out> <name>..   pictures as PNG

use std::path::Path;

use med::arc::Archives;
use nexas::files::{Disk, Files};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err("usage: med-dump list|x|png <game folder> [out] [names]".into());
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
                    let size = arcs.packs[p].read(files, n).map_or(-1, |b| b.len() as i64);
                    println!("{p}:{n}\t{size}");
                }
            }
        }
        "x" => {
            let out = Path::new(args.get(3).ok_or("x: no out folder")?);
            for p in packs {
                if args.get(4).is_some_and(|only| only != p) {
                    continue;
                }
                let d = out.join(p);
                std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
                for n in &arcs.packs[p].names {
                    let Some(b) = arcs.packs[p].read(files, n) else {
                        eprintln!("{p}:{n}: unreadable");
                        continue;
                    };
                    std::fs::write(d.join(n.replace(['\\', '/'], "_")), b)
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        "png" => {
            let out = Path::new(args.get(3).ok_or("png: no out folder")?);
            std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
            for name in &args[4..] {
                let b = ["gra", "gra2", "jpg"]
                    .iter()
                    .find_map(|p| arcs.read(files, p, name))
                    .ok_or(format!("{name}: not found"))?;
                let img = med::engine::decode_picture(&b)?;
                let path = out.join(format!("{name}.png"));
                image::save_buffer(&path, &img.px, img.w, img.h, image::ColorType::Rgba8)
                    .map_err(|e| e.to_string())?;
                println!("{} {}x{}", path.display(), img.w, img.h);
            }
        }
        _ => return Err("med-dump: list, x or png".into()),
    }
    Ok(())
}
