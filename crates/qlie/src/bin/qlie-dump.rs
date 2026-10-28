//! Looks inside a QLIE game.
//!
//!     qlie-dump <game folder> <GameData/dataN.pack>               lists an archive
//!     qlie-dump <game folder> <GameData/dataN.pack> <name> <out>  saves an entry
//!     qlie-dump <game folder> <GameData/dataN.pack> - <out dir>   saves every entry
//!     qlie-dump <game folder> --scripts                           parses every script
//!     qlie-dump <game folder> --abmp <name>                       shows a .b file's motions

use nexas::files::Disk;
use qlie::pack::Pack;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err("usage: qlie-dump <game folder> <pack> [name out]".into());
    }
    let dir = std::path::PathBuf::from(&args[1]);
    let mut disk = Disk {
        saves: dir.join("vnrun-saves"),
        root: dir,
    };
    if args[1] == "--unpack" {
        // a "1PC" packed file: qlie-dump --unpack <in> <out>
        let b = std::fs::read(&args[2]).map_err(|e| e.to_string())?;
        let d = qlie::pack::decompress(&b).ok_or("not 1PC")?;
        return std::fs::write(args.get(3).ok_or("no out")?, d).map_err(|e| e.to_string());
    }
    if args[2] == "--scripts" || args[2] == "--abmp" || args[2] == "--cat" {
        return look(disk, &args[2], args.get(3));
    }
    let key = qlie::game::key_file(&mut disk);
    let pack = Pack::open(&mut disk, &args[2], key)?;
    eprintln!("version {:?}, {} entries", pack.version, pack.entries.len());
    match args.get(3).map(String::as_str) {
        None => {
            for e in &pack.entries {
                println!(
                    "{:>10} {:>10} {}{} {}",
                    e.size,
                    e.unpacked,
                    if e.packed { 'p' } else { '-' },
                    e.method,
                    e.name
                );
            }
        }
        Some("-") => {
            let out = std::path::Path::new(&args[4]);
            for e in pack.entries.clone() {
                let data = pack
                    .read_entry(&mut disk, &e)
                    .ok_or(format!("{}: unreadable", e.name))?;
                let p = out.join(&e.name);
                std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(&p, data).map_err(|e| e.to_string())?;
            }
        }
        Some(name) => {
            let e = pack
                .entries
                .iter()
                .find(|e| e.name.eq_ignore_ascii_case(name))
                .ok_or(format!("{name}: not in the pack"))?
                .clone();
            let data = pack.read_entry(&mut disk, &e).ok_or("unreadable")?;
            std::fs::write(&args[4], data).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn look(disk: Disk, what: &str, name: Option<&String>) -> Result<(), String> {
    use qlie::script::{Kind, Unit};
    let mut g = qlie::game::Game::open(Box::new(disk))?;
    if what == "--cat" {
        // a script's text
        let name = name.ok_or("--cat <name>")?;
        let b = g.read(name).ok_or("no such file")?;
        if let Ok(out) = std::env::var("QLIE_RAW") {
            return std::fs::write(out, &b).map_err(|e| e.to_string());
        }
        print!("{}", qlie::script::decode(&b));
        return Ok(());
    }
    if what == "--abmp" {
        let name = name.ok_or("--abmp <name>")?;
        let b = g.read(name).ok_or("no such file")?;
        if let Ok(out) = std::env::var("QLIE_RAW") {
            // the unpacked animation data
            let at = b
                .windows(6)
                .position(|w| w == b"abdata")
                .ok_or("no abdata")?;
            let n = u32::from_le_bytes(b[at + 16..at + 20].try_into().unwrap()) as usize;
            let d = qlie::pack::decompress(&b[at + 20..at + 20 + n]).ok_or("bad 1PC")?;
            std::fs::write(out, d).map_err(|e| e.to_string())?;
        }
        let a = qlie::abmp::Abmp::parse(&b)?;
        println!("{} images, {} sounds", a.images.len(), a.sounds.len());
        for (i, im) in a.images.iter().enumerate() {
            let size = nexas::gfx::Image::decode(im).map(|m| (m.w, m.h));
            println!("image {i}: {} bytes {:?}", im.len(), size.ok());
            if let Some(Some(sub)) = a.subs.get(i) {
                let sizes: Vec<_> = sub
                    .images
                    .iter()
                    .map(|b| nexas::gfx::Image::decode(b).map(|m| (m.w, m.h)).ok())
                    .collect();
                println!(
                    "  an animation: pictures {sizes:?}, {} motions",
                    sub.motions.len()
                );
                for m in &sub.motions {
                    for l in &m.layers {
                        let k: Vec<_> = l
                            .keys
                            .iter()
                            .map(|k| (k.time, k.show, k.image, k.x, k.y, k.alpha))
                            .collect();
                        println!("    {:?} {:?}", l.name, k);
                    }
                }
            }
        }
        for m in &a.motions {
            println!("motion {:?}", m.name);
            for l in &m.layers {
                println!("  layer {:?}", l.name);
                for k in &l.keys {
                    println!(
                        "    {:>6} {:?} img {} at {},{} scale {},{} z {} rot {} add {:?} color {:?} a {:.2} ease {} anchor {:?} cmd {} {}",
                        k.time,
                        k.show,
                        k.image,
                        k.x,
                        k.y,
                        k.sx,
                        k.sy,
                        k.z,
                        k.rot,
                        k.add,
                        k.color,
                        k.alpha,
                        k.ease,
                        k.anchor,
                        k.cmd,
                        k.arg
                    );
                }
            }
        }
        return Ok(());
    }
    let mut names: Vec<String> = g.names().filter(|n| n.ends_with(".s")).cloned().collect();
    names.sort();
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for n in names {
        let text = qlie::script::decode(&g.read(&n).ok_or("unreadable")?);
        let mut load = |from: &str, p: &str| {
            let base = qlie::game::norm(from);
            let base = base.rsplit_once('/').map_or("", |b| b.0).to_string();
            let near = format!("{base}/{p}");
            if let Some(b) = g.read(&near) {
                return Some((near, qlie::script::decode(&b)));
            }
            g.read(p).map(|b| (p.to_string(), qlie::script::decode(&b)))
        };
        let u = Unit::load(&n, &text, &mut load);
        for o in &u.ops {
            if let Kind::Cmd(c, args) = &o.kind {
                *counts.entry(c.clone()).or_default() += 1;
                for a in args {
                    if matches!(a.expr, qlie::expr::Expr::Text(_)) && name.is_some() {
                        println!(
                            "{}:{}: text arg {}",
                            u.files[o.file as usize], o.line, a.raw
                        );
                    }
                }
            }
        }
        println!(
            "{n}: {} ops, {} labels, {} files",
            u.ops.len(),
            u.labels.len(),
            u.files.len()
        );
    }
    for (c, n) in counts {
        print!("{c}:{n} ");
    }
    println!();
    Ok(())
}
