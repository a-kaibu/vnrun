//! Runs a LiveMaker game without a window and saves frames as PNG files.
//!
//!     livemaker-run <game folder> <frames> <out prefix> [frame[:x:y] ...]
//!
//! Each `frame` is saved as `<out prefix><frame>.png`; with `:x:y`, there is also a click there
//! on that frame (`:x:y:r` a right click, `:x:y:m` only moves the pointer there); `:u` and `:d`
//! turn the wheel up or down, `:k13` presses a key (Windows key codes) and `:tabc` types abc.
//! The last frame
//! is always saved. LIVEMAKER_FONT names a TrueType font for text, LIVEMAKER_TRACE=1 prints the
//! commands (2: also the per-frame idle handlers).

use std::path::Path;

use nexas::files::Disk;

/// where and how the pointer acts: c click, r right click, m move, u/d wheel, k key (code in
/// x), t typing (the text)
type Pointer = (i32, i32, char, String);

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(
            "usage: livemaker-run <game folder> <frames> <out prefix> [frame[:x:y] ...]".into(),
        );
    }
    let dir = Path::new(&args[1]);
    let frames: u64 = args[2].parse().map_err(|_| "frames: not a number")?;
    let prefix = &args[3];
    let mut marks: Vec<(u64, Option<Pointer>)> = Vec::new();
    for m in &args[4..] {
        let v: Vec<&str> = m.split(':').collect();
        let n = |s: &str| s.parse::<i64>().map_err(|_| format!("{m}: not a number"));
        let click = if v.len() == 2 {
            let kind = v[1].chars().next().unwrap_or('k');
            let code = if kind == 'k' {
                n(&v[1][1..])? as i32
            } else {
                0
            };
            Some((code, 0, kind, v[1][kind.len_utf8()..].to_string()))
        } else if v.len() >= 3 {
            let kind = v.get(3).and_then(|k| k.chars().next()).unwrap_or('c');
            Some((n(v[1])? as i32, n(v[2])? as i32, kind, String::new()))
        } else {
            None
        };
        marks.push((n(v[0])? as u64, click));
    }
    let disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let mut e = livemaker::engine::Engine::open(Box::new(disk))?;
    let log = std::env::var("LIVEMAKER_TRACE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    e.set_log(log);
    if let Some(f) = std::env::var_os("LIVEMAKER_FONT") {
        e.set_font(&std::fs::read(f).map_err(|e| e.to_string())?)?;
    }
    for f in 1..=frames {
        if log > 0 && marks.iter().any(|m| m.0 == f) {
            eprintln!("-- frame {f}");
        }
        for (_, click) in marks.iter().filter(|m| m.0 == f) {
            if let Some((x, y, kind, text)) = click {
                match kind {
                    't' => e.type_text(text),
                    'u' | 'd' => e.wheel(*kind == 'u'),
                    'k' => e.key(*x as i64),
                    'r' => {
                        e.point(*x, *y);
                        e.cancel()
                    }
                    'm' => e.point(*x, *y),
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
            eprintln!("livemaker: the game ended at frame {f}");
            break;
        }
    }
    Ok(())
}
