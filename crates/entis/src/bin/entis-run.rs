//! Runs an Entis GLS game without a window and saves frames as PNG files.
//!
//!     entis-run <game folder> <frames> <out prefix> [frame[:x:y[:r|m]] ...]
//!
//! Each `frame` is saved as `<out prefix><frame>.png`; with `:x:y`, there is also a click there
//! on that frame (`:x:y:r` a right click, `:x:y:m` only moves the pointer there). The last frame
//! is always saved. ENTIS_FONT names a TrueType font for text; ENTIS_TRACE=1 prints every warning
//! and where the threads are at the end, ENTIS_TRACE=3 also every script call.

use std::path::Path;

use nexas::files::Disk;

/// where and how the pointer acts: c click, r right click, m move
type Pointer = (i32, i32, char);

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(
            "usage: entis-run <game folder> <frames> <out prefix> [frame[:x:y] ...]".into(),
        );
    }
    let dir = Path::new(&args[1]);
    let frames: u64 = args[2].parse().map_err(|_| "frames: not a number")?;
    let prefix = &args[3];
    let mut marks: Vec<(u64, Option<Pointer>)> = Vec::new();
    for m in &args[4..] {
        let v: Vec<&str> = m.split(':').collect();
        let n = |s: &str| s.parse::<i64>().map_err(|_| format!("{m}: not a number"));
        let click = if v.len() >= 3 {
            let kind = v.get(3).and_then(|k| k.chars().next()).unwrap_or('c');
            Some((n(v[1])? as i32, n(v[2])? as i32, kind))
        } else {
            None
        };
        marks.push((n(v[0])? as u64, click));
    }
    let disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let paths = entis::game::walk_dir(dir);
    let mut e = entis::engine::Engine::open(Box::new(disk), Some(paths))?;
    let log = std::env::var("ENTIS_TRACE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    e.set_log(log);
    if let Some(f) = std::env::var_os("ENTIS_FONT") {
        e.set_font(&std::fs::read(f).map_err(|e| e.to_string())?)?;
    }
    for f in 1..=frames {
        for (_, click) in marks.iter().filter(|m| m.0 == f) {
            if let Some((x, y, kind)) = click {
                e.point(*x, *y);
                match kind {
                    'r' => e.cancel(),
                    'm' => {}
                    _ => e.click(*x, *y),
                }
            }
        }
        e.clock = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as f64);
        e.frame()?;
        e.take_sounds();
        if f == frames || marks.iter().any(|m| m.0 == f) {
            let out = format!("{prefix}{f}.png");
            let frame = e.render();
            image::save_buffer(&out, &frame.px, frame.w, frame.h, image::ColorType::Rgba8)
                .map_err(|err| format!("{out}: {err}"))?;
        }
        if e.ended() {
            eprintln!("entis: the game ended at frame {f}");
            break;
        }
    }
    if log > 0 {
        for w in e.where_threads() {
            eprintln!("entis: thread at {w}");
        }
    }
    Ok(())
}
