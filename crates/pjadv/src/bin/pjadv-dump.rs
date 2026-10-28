//! Looks into a PJADV game's GAMEDAT PAC archives.
//!
//!     pjadv-dump <game folder> list
//!     pjadv-dump <game folder> extract <name> <out file>
//!     pjadv-dump <game folder> png <name.epa> <out.png>

use std::path::Path;

use nexas::files::Disk;
use pjadv::pac::Pac;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err(
            "usage: pjadv-dump <game folder> list | extract <name> <out> | png <name> <out>".into(),
        );
    }
    let dir = Path::new(&args[1]);
    let mut disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let pac = Pac::open(&mut disk)?;
    let read = |disk: &mut Disk, n: &str| pac.read(disk, n).ok_or(format!("{n}: not found"));
    match args[2].as_str() {
        "list" => {
            for n in pac.names() {
                println!("{n}");
            }
        }
        "extract" => {
            let name = args.get(3).ok_or("a name")?;
            let out = args.get(4).ok_or("an output file")?;
            std::fs::write(out, read(&mut disk, name)?).map_err(|e| e.to_string())?;
        }
        "png" => {
            let name = args.get(3).ok_or("a name")?;
            let out = args.get(4).ok_or("an output file")?;
            let e = pjadv::epa::decode(&read(&mut disk, name)?)?;
            image::save_buffer(out, &e.img.px, e.img.w, e.img.h, image::ColorType::Rgba8)
                .map_err(|e| e.to_string())?;
            println!("{}x{} offset {} {}", e.img.w, e.img.h, e.ox, e.oy);
        }
        c => return Err(format!("{c}: unknown command")),
    }
    Ok(())
}
