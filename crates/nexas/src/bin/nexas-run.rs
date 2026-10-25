//! Runs a NeXAS game without a window and saves frames as PNG files.
//!
//!     nexas-run <game folder> <frames> <out prefix> [frame[:x:y] ...]
//!
//! Each `frame` is saved as `<out prefix><frame>.png`; with `:x:y`, there is also a click there
//! on that frame. The last frame is always saved.

use std::path::Path;

use nexas::engine::{Engine, HEIGHT, WIDTH};
use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(
            "usage: nexas-run <game folder> <frames> <out prefix> [frame[:x:y] ...]".into(),
        );
    }
    let dir = Path::new(&args[1]);
    let frames: u64 = args[2].parse().map_err(|_| "frames: not a number")?;
    let prefix = &args[3];
    let mut marks: Vec<(u64, Option<(i32, i32)>)> = Vec::new();
    for m in &args[4..] {
        let v: Vec<&str> = m.split(':').collect();
        let n = |s: &str| s.parse::<i64>().map_err(|_| format!("{m}: not a number"));
        let click = if v.len() == 3 {
            Some((n(v[1])? as i32, n(v[2])? as i32))
        } else {
            None
        };
        marks.push((n(v[0])? as u64, click));
    }
    let disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let mut e = Engine::new(Box::new(disk))?;
    e.log = true;
    eprintln!("nexas: build {}", e.build.name);
    e.set_trace(std::env::var_os("NEXAS_TRACE").is_some());
    e.log_calls = std::env::var_os("NEXAS_CALLS").is_some();
    if let Some(f) = std::env::var_os("NEXAS_FONT") {
        e.set_font(&std::fs::read(f).map_err(|e| e.to_string())?)?;
    }
    for f in 1..=frames {
        for (_, click) in marks.iter().filter(|m| m.0 == f) {
            if let Some((x, y)) = click {
                e.click(*x, *y);
            }
        }
        e.frame()?;
        if f == frames || marks.iter().any(|m| m.0 == f) {
            let out = format!("{prefix}{f}.png");
            image::save_buffer(&out, &e.draw().px, WIDTH, HEIGHT, image::ColorType::Rgba8)
                .map_err(|err| format!("{out}: {err}"))?;
            if std::env::var_os("NEXAS_OBJS").is_some() {
                eprint!("{}", e.describe());
            }
        }
    }
    Ok(())
}
