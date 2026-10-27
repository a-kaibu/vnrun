//! Looks inside a CatSystem2 game.
//!
//!     cs2-dump <game folder> <archive.int>                 lists an archive
//!     cs2-dump <game folder> <archive.int> <name.cst>      prints a scene
//!     cs2-dump <game folder> <archive.int> <name.hg3> <out>  saves the frames as <out>_<id>.png
//!     cs2-dump <game folder> <archive.int> <name> <out>    saves a file

use catsystem2::int::Archive;
use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err("usage: cs2-dump <game folder> <archive.int> [name out]".into());
    }
    let dir = std::path::PathBuf::from(&args[1]);
    let mut disk = Disk {
        root: dir.clone(),
        saves: dir.join("vnrun-saves"),
    };
    let key = catsystem2::game::passphrase(&mut disk).map(|p| {
        eprintln!("passphrase {:?}", catsystem2::sjis(&p));
        catsystem2::int::game_key(&p)
    });
    let arc = Archive::open(&mut disk, &args[2], key)?;
    let Some(name) = args.get(3) else {
        for n in &arc.names {
            let e = arc.entries[&catsystem2::int::key(n)];
            println!("{:>10} {:>10} {n}", e.offset, e.size);
        }
        return Ok(());
    };
    let data = arc
        .read(&mut disk, name)
        .ok_or(format!("{name}: not found"))?;
    if args.len() == 4 && name.to_lowercase().ends_with(".cst") {
        let s = catsystem2::cst::Scene::parse(&data)?;
        for l in &s.lines {
            println!("{:?}\t{}", l.kind, l.text);
        }
        return Ok(());
    }
    if name.to_lowercase().ends_with(".hg3") {
        let out = args.get(4).ok_or("out prefix?")?;
        for f in catsystem2::hg3::decode(&data)? {
            println!(
                "frame {} {}x{} at {},{} canvas {}x{} base {},{}",
                f.id, f.image.w, f.image.h, f.x, f.y, f.canvas_w, f.canvas_h, f.base_x, f.base_y
            );
            image::save_buffer(
                format!("{out}_{}.png", f.id),
                &f.image.px,
                f.image.w,
                f.image.h,
                image::ColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    std::fs::write(args.get(4).ok_or("out?")?, &data).map_err(|e| e.to_string())
}
