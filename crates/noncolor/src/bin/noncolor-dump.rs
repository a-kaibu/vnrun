//! Looks into a NonColor game's archives.
//!
//!     noncolor-dump <game folder> scripts <out folder>
//!     noncolor-dump <game folder> extract <name> <out file>
//!     noncolor-dump <game folder> png <name.tlg> <out.png>

use std::path::Path;

use nexas::files::Disk;
use noncolor::arc::Archives;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(
            "usage: noncolor-dump <game folder> scripts <out> | extract <name> <out> | png <name> <out>"
                .into(),
        );
    }
    let dir = Path::new(&args[1]);
    let mut disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let arcs = Archives::open(&mut disk);
    eprintln!("key {:08x}", arcs.key);
    match args[2].as_str() {
        "scripts" => {
            let out = Path::new(&args[3]);
            std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
            let es: Vec<_> = arcs.entries_of("script.dat").cloned().collect();
            for e in es {
                let d = arcs
                    .read_entry(&mut disk, &e, &[])
                    .ok_or(format!("{:016x}: unreadable", e.hash))?;
                std::fs::write(out.join(format!("{:016x}.txt", e.hash)), d)
                    .map_err(|e| e.to_string())?;
            }
        }
        "exists" => {
            for n in &args[3..] {
                println!("{} {n}", if arcs.exists(n) { "yes" } else { "no " });
            }
        }
        "extract" => {
            let out = args.get(4).ok_or("an output file")?;
            let d = arcs
                .read(&mut disk, &args[3])
                .ok_or(format!("{}: not found", args[3]))?;
            std::fs::write(out, d).map_err(|e| e.to_string())?;
        }
        "png" => {
            let out = args.get(4).ok_or("an output file")?;
            let d = arcs
                .read(&mut disk, &args[3])
                .ok_or(format!("{}: not found", args[3]))?;
            let t = noncolor::tlg::decode(&d)?;
            image::save_buffer(out, &t.img.px, t.img.w, t.img.h, image::ColorType::Rgba8)
                .map_err(|e| e.to_string())?;
            println!("{}x{} {:?}", t.img.w, t.img.h, t.offset());
        }
        c => return Err(format!("{c}: unknown command")),
    }
    Ok(())
}
