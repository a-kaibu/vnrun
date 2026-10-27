//! Looks inside a QLIE game.
//!
//!     qlie-dump <game folder> <GameData/dataN.pack>               lists an archive
//!     qlie-dump <game folder> <GameData/dataN.pack> <name> <out>  saves an entry
//!     qlie-dump <game folder> <GameData/dataN.pack> - <out dir>   saves every entry

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
