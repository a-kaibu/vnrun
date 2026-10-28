//! Looks into the archives of a Liar-soft game.
//!
//!     liar-dump list <game folder>                    every file, as pack:name and size
//!     liar-dump png <game folder> <out> <pack:name>..  WCG pictures (and each of an LWG) as PNG
//!     liar-dump gsc <game folder> [n..]               the ops of scripts (all: check them)

use std::path::Path;

use liar::xfl::Archives;
use nexas::files::{Disk, Files};

fn save(path: &Path, img: &nexas::gfx::Image) -> Result<(), String> {
    image::save_buffer(path, &img.px, img.w, img.h, image::ColorType::Rgba8)
        .map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err("usage: liar-dump list|png|gsc <game folder> [out] [names]".into());
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
                    println!("{p}:{n}\t{}", arcs.packs[p].entries[n].size);
                }
            }
        }
        "png" => {
            let out = Path::new(args.get(3).ok_or("png: no out folder")?);
            std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
            for item in &args[4..] {
                let (p, n) = item.split_once(':').ok_or("png: pack:name")?;
                let b = arcs.read(files, p, n).ok_or(format!("{item}: not found"))?;
                if let Some(l) = liar::wcg::Lwg::parse(&b) {
                    println!("{item}: LWG {}x{}", l.width, l.height);
                    for part in &l.parts {
                        let img = liar::wcg::decode(&b[part.offset..part.offset + part.size])?;
                        let path = out.join(format!("{n}_{}.png", part.name));
                        save(&path, &img)?;
                        println!(
                            "  {} at {},{} {}x{}",
                            part.name, part.x, part.y, img.w, img.h
                        );
                    }
                } else {
                    let img = liar::wcg::decode(&b)?;
                    let path = out.join(format!("{p}_{n}.png"));
                    save(&path, &img)?;
                    println!("{} {}x{}", path.display(), img.w, img.h);
                }
            }
        }
        "gsc" => {
            let names: Vec<String> = if args.len() > 3 {
                args[3..].iter().map(|n| format!("{n}.gsc")).collect()
            } else {
                arcs.packs
                    .get("scr")
                    .map(|p| p.names.clone())
                    .unwrap_or_default()
            };
            for n in names {
                let b = arcs
                    .read(files, "scr", &n)
                    .ok_or(format!("{n}: not found"))?;
                let s = liar::gsc::Script::parse(&b).map_err(|e| format!("{n}: {e}"))?;
                if args.len() > 3 {
                    for r in &s.recs {
                        println!("{:#x} {:#x} {:?}", r.off, r.op, r.args);
                    }
                    for (i, t) in s.strs.iter().enumerate() {
                        println!("  s{i} {t}");
                    }
                } else {
                    println!("{n}: {} ops, {} strings", s.recs.len(), s.strs.len());
                }
            }
        }
        _ => return Err("liar-dump: list, png or gsc".into()),
    }
    Ok(())
}
