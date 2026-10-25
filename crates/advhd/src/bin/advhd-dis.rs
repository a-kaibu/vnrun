//! Lists the commands of a game's scenario scripts.
//!
//!     advhd-dis <game folder> [script ...]   (default: every .ws2)

use advhd::files::{Archives, Disk, Files};
use advhd::ws2::{Arg, Script};

fn show(a: &Arg) -> String {
    match a {
        Arg::Int(i) => i.to_string(),
        Arg::Num(n) => format!("{n}f"),
        Arg::Str(s) => format!("{s:?}"),
        Arg::List(l) => format!("[{}]", l.iter().map(show).collect::<Vec<_>>().join(", ")),
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(
        args.get(1)
            .ok_or("usage: advhd-dis <game folder> [script ...]")?,
    );
    let mut disk = Disk {
        root: dir.clone(),
        saves: dir.join("vnrun-saves"),
    };
    let arcs = Archives::open(&mut disk);
    let exe = disk
        .list()
        .into_iter()
        .find(|n| n.to_lowercase() == "advhd.exe")
        .and_then(|n| disk.read(&n))
        .ok_or("no AdvHD.exe")?;
    let fm = advhd::exe::formats(&exe);
    let names: Vec<String> = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        arcs.names_with(".ws2")
    };
    let mut bad = 0;
    for n in names {
        let n = if n.contains('.') {
            n
        } else {
            format!("{n}.ws2")
        };
        let raw = arcs.read(&mut disk, &n).ok_or_else(|| format!("no {n}"))?;
        let s = Script::new(&n, &raw);
        println!("== {n}");
        let mut at = 0;
        while at < s.code.len() {
            if s.code[at] == 0xff {
                println!("{at:06x} ff end");
                break;
            }
            match s.read(&fm, at) {
                Ok(i) => {
                    println!(
                        "{at:06x} {:02x} {}",
                        i.op,
                        i.args.iter().map(show).collect::<Vec<_>>().join(", ")
                    );
                    at = i.next;
                }
                Err(e) => {
                    println!("{e}");
                    bad += 1;
                    break;
                }
            }
        }
    }
    if bad > 0 {
        eprintln!("{bad} scripts stopped at an unknown command");
    }
    Ok(())
}
