//! Looks into an AZ System game's archive.
//!
//!     azsys-dump <game folder> script <name.asb>      (the records, one a line)
//!     azsys-dump <game folder> extract <name> <out file>
//!     azsys-dump <game folder> png <name.cpb> <out.png>

use std::path::Path;

use azsys::arc::Arc;
use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(
            "usage: azsys-dump <game folder> script <name> | extract <name> <out> | png <name> <out>"
                .into(),
        );
    }
    let dir = Path::new(&args[1]);
    let mut disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let arc = Arc::open(&mut disk)?;
    match args[2].as_str() {
        "script" => {
            let d = arc.asb(&mut disk, &args[3])?;
            for (i, r) in azsys::script::parse(&d).iter().enumerate() {
                println!("{i} {:02x} {:?} {}", r.op, r.a, r.s);
            }
        }
        "extract" => {
            let out = args.get(4).ok_or("an output file")?;
            let d = arc
                .read(&mut disk, &args[3])
                .ok_or(format!("{}: not found", args[3]))?;
            std::fs::write(out, d).map_err(|e| e.to_string())?;
        }
        "png" => {
            let out = args.get(4).ok_or("an output file")?;
            let d = arc
                .read(&mut disk, &args[3])
                .ok_or(format!("{}: not found", args[3]))?;
            let img = azsys::cpb::decode(&d)?;
            image::save_buffer(out, &img.px, img.w, img.h, image::ColorType::Rgba8)
                .map_err(|e| e.to_string())?;
            println!("{}x{}", img.w, img.h);
        }
        c => return Err(format!("{c}: unknown command")),
    }
    Ok(())
}
