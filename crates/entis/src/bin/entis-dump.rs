//! Lists and extracts Entis GLS files.
//!
//!     entis-dump keys <exe>
//!     entis-dump list <game folder>
//!     entis-dump get <game folder> <name> <out file>

use std::path::Path;

use entis::game::Fs;
use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: entis-dump keys <exe> | dis <csx> | list <game> | get <game> <name> <out>";
    match args.get(1).map(String::as_str) {
        Some("keys") => {
            let exe = std::fs::read(args.get(2).ok_or(usage)?).map_err(|e| e.to_string())?;
            for (n, k) in entis::noa::exe_keys(&exe) {
                println!("{n}\t{k}");
            }
        }
        Some("png") => {
            for f in &args[2..] {
                let b = std::fs::read(f).map_err(|e| e.to_string())?;
                let e = entis::eri::decode(&b).map_err(|e| format!("{f}: {e}"))?;
                println!("{f}: {}x{} hot {:?}", e.w, e.h, e.hot);
                image::save_buffer(format!("{f}.png"), &e.px, e.w, e.h, image::ColorType::Rgba8)
                    .map_err(|e| e.to_string())?;
            }
        }
        Some("dis") => {
            let b = std::fs::read(args.get(2).ok_or(usage)?).map_err(|e| e.to_string())?;
            let img = entis::csx::Image::parse(&b)?;
            println!("; prologue {:x?}", img.prologue);
            println!("; epilogue {:x?}", img.epilogue);
            for (n, o) in &img.globals {
                println!("; global {n} = {o:?}");
            }
            for (n, o) in &img.data {
                println!("; data {n} = {o:?}");
            }
            let names: std::collections::HashMap<u32, &String> =
                img.functions.iter().map(|(a, n)| (*a, n)).collect();
            let mut at = 0u32;
            while (at as usize) < img.code.len() {
                if let Some(n) = names.get(&at) {
                    println!("\n{n}:");
                }
                match img.decode(at) {
                    Ok((op, next)) => {
                        println!("{at:08x}  {op:?}");
                        at = next;
                    }
                    Err(e) => {
                        println!("{e}");
                        break;
                    }
                }
            }
        }
        Some("list") | Some("get") => {
            let dir = Path::new(args.get(2).ok_or(usage)?);
            let disk = Disk {
                root: dir.to_path_buf(),
                saves: dir.join("vnrun-saves"),
            };
            let mut fs = Fs::open(Box::new(disk), &entis::game::walk_dir(dir))?;
            if args[1] == "list" {
                for (a, e) in fs.entries() {
                    println!("{a}\t{}\t{}\t{:x}", e.name, e.size, e.enc);
                }
            } else {
                let name = args.get(3).ok_or(usage)?;
                let out = args.get(4).ok_or(usage)?;
                let b = fs.read(name).ok_or_else(|| format!("{name}: not found"))?;
                std::fs::write(out, b).map_err(|e| e.to_string())?;
            }
        }
        _ => return Err(usage.into()),
    }
    Ok(())
}
