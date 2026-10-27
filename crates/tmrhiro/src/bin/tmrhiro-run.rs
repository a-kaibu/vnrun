//! Runs a Tmr-Hiro game without a window and saves frames as PNG files.
//!
//!     tmrhiro-run <game folder> <frames> <out prefix> [frame[:x:y] ...]
//!
//! Each `frame` is saved as `<out prefix><frame>.png`; with `:x:y`, there is also a click there
//! on that frame (`:x:y:r` a right click, `:x:y:m` only moves the pointer there, `:x:y:u` and `:x:y:d` turn
//! the wheel up and down). The last
//! frame is always saved. TMRHIRO_FONT names a TrueType font for text, TMRHIRO_TRACE=1 prints
//! each command and TMRHIRO_START=<script> starts at that script.

use std::path::Path;

type Pointer = (i32, i32, char);

use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(
            "usage: tmrhiro-run <game folder> <frames> <out prefix> [frame[:x:y] ...]".into(),
        );
    }
    let dir = Path::new(&args[1]);
    let frames: u64 = args[2].parse().map_err(|_| "frames: not a number")?;
    let prefix = &args[3];
    // frame, then where and how the pointer acts (c click, r right click, m move)
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
    let mut e = tmrhiro::open(Box::new(disk))?;
    let log = std::env::var_os("TMRHIRO_TRACE").is_some();
    e.set_log(log);
    if let Some(f) = std::env::var_os("TMRHIRO_FONT") {
        e.set_font(&std::fs::read(f).map_err(|e| e.to_string())?)?;
    }
    if let Some(name) = std::env::var_os("TMRHIRO_START") {
        e.start_at(&name.to_string_lossy());
    }
    for f in 1..=frames {
        if log && marks.iter().any(|m| m.0 == f) {
            eprintln!("-- frame {f}");
        }
        for (_, click) in marks.iter().filter(|m| m.0 == f) {
            if let Some((x, y, kind)) = click {
                e.point(*x, *y);
                match kind {
                    'r' => e.cancel(),
                    'm' => {}
                    'u' => e.wheel(true),
                    'd' => e.wheel(false),
                    _ => e.click(*x, *y),
                }
            }
        }
        e.frame()?;
        e.take_sounds();
        if f == frames || marks.iter().any(|m| m.0 == f) {
            let out = format!("{prefix}{f}.png");
            let frame = e.render();
            image::save_buffer(&out, &frame.px, frame.w, frame.h, image::ColorType::Rgba8)
                .map_err(|err| format!("{out}: {err}"))?;
        }
        if e.ended() {
            eprintln!("tmrhiro: the game ended at frame {f}");
            break;
        }
    }
    Ok(())
}
