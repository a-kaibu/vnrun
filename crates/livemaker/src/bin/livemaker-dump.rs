//! Looks inside a LiveMaker game.
//!
//!     livemaker-dump <game folder>                 lists the archive
//!     livemaker-dump <game folder> <name> [out]    prints a chart (.lsb) or saves a file
//!                                                  (.gal as PNG)

use livemaker::archive::Archive;
use livemaker::lsb::{Cmd, Script};
use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return Err("usage: livemaker-dump <game folder> [name [out]]".into());
    }
    let dir = std::path::PathBuf::from(&args[1]);
    let mut disk = Disk {
        root: dir.clone(),
        saves: dir.join("vnrun-saves"),
    };
    let arc = Archive::open(&mut disk)?;
    let Some(name) = args.get(2) else {
        for n in &arc.names {
            let e = arc.entries[&livemaker::archive::key(n)];
            println!("{:>10} {} {n}", e.size, e.kind);
        }
        return Ok(());
    };
    if name == "--all" {
        // Every chart as text into a folder.
        let out = std::path::PathBuf::from(args.get(3).ok_or("--all <out folder>")?);
        for n in &arc.names {
            if !n.to_lowercase().ends_with(".lsb") {
                continue;
            }
            let data = arc.read(&mut disk, n).ok_or(format!("{n}: cannot read"))?;
            let text = match Script::parse(&data) {
                Ok(s) => show(&s),
                Err(e) => format!("{e}\n"),
            };
            let path = out.join(n.replace('\\', "/") + ".txt");
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&path, text).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let data = arc
        .read(&mut disk, name)
        .ok_or(format!("{name}: not found"))?;
    if let Some(out) = args.get(3) {
        if livemaker::gal::is_gal(&data) && !out.ends_with(".bin") {
            let img = livemaker::gal::decode(&data)?;
            image::save_buffer(out, &img.px, img.w, img.h, image::ColorType::Rgba8)
                .map_err(|e| e.to_string())?;
        } else {
            std::fs::write(out, &data).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let s = Script::parse(&data)?;
    print!("{}", show(&s));
    Ok(())
}

fn show(s: &Script) -> String {
    use std::fmt::Write;
    let mut out = format!("version {}\n", s.version);
    for (i, c) in s.commands.iter().enumerate() {
        let pad = "  ".repeat(c.indent as usize);
        let mute = if c.mute { "#" } else { "" };
        match &c.cmd {
            Cmd::TextIns { text, target, .. } => {
                let _ = writeln!(out, "{i:4} {:5} {pad}{mute}TextIns {}", c.line, target);
                for l in text.plain().lines() {
                    let _ = writeln!(out, "           {pad}  | {l}");
                }
            }
            cmd => {
                let _ = writeln!(out, "{i:4} {:5} {pad}{mute}{cmd}", c.line);
            }
        }
        if std::env::var_os("LM_RAW").is_some() {
            let _ = writeln!(out, "      {:?}", c.cmd);
        }
    }
    out
}
