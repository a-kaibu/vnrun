//! Runs a CrossOver (Adv.exe) game without a window and saves frames as PNG files.
//!
//!     crossover-run <game folder> <frames> <out prefix> [frame[:x:y[:kind]] ...]
//!
//! A bare `frame` is saved as `<out prefix><frame>.png`; `frame:x:y` clicks there on that frame
//! instead (`:x:y:r` a right click, `:x:y:m` only moves the pointer there, `:x:y:s`
//! turns skipping on or off). The last frame is
//! always saved. CROSSOVER_FONT names a TrueType font for text, CROSSOVER_LOG=1 logs the
//! instructions.

use std::path::Path;

use nexas::files::Disk;

type Pointer = (i32, i32, char);

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(
            "usage: crossover-run <game folder> <frames> <out prefix> [frame[:x:y] ...]".into(),
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
    let mut e = crossover::engine::Engine::open(Box::new(disk))?;
    e.log = std::env::var_os("CROSSOVER_LOG").is_some();
    if let Some(f) = std::env::var_os("CROSSOVER_FONT") {
        e.set_font(&std::fs::read(f).map_err(|e| e.to_string())?)?;
    }
    for f in 1..=frames {
        if e.log && marks.iter().any(|m| m.0 == f) {
            eprintln!("-- frame {f}");
        }
        for (_, click) in marks.iter().filter(|m| m.0 == f) {
            if let Some((x, y, kind)) = click {
                e.point(*x, *y);
                match kind {
                    'r' => e.cancel(),
                    's' => e.key(6),
                    'm' => {}
                    _ => e.click(*x, *y),
                }
            }
        }
        e.frame()?;
        e.take_sounds();
        if f == frames || marks.iter().any(|m| m.0 == f && m.1.is_none()) {
            let out = format!("{prefix}{f}.png");
            let frame = e.render();
            image::save_buffer(&out, &frame.px, frame.w, frame.h, image::ColorType::Rgba8)
                .map_err(|err| format!("{out}: {err}"))?;
        }
        if e.ended() {
            eprintln!("crossover: the game ended at frame {f}");
            break;
        }
    }
    Ok(())
}
