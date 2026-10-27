//! Runs a CatSystem2 game without a window and saves frames as PNG files.
//!
//!     cs2-run <game folder> <frames> <out prefix> [frame[:x:y] ...]
//!
//! Each `frame` is saved as `<out prefix><frame>.png`; with `:x:y`, there is also a click there
//! on that frame (`:x:y:r` a right click, `:x:y:u` the wheel up, `:x:y:m` only moves the pointer there). The last frame
//! is always saved. CS2_FONT names a TrueType font for text, CS2_TRACE=1 prints the
//! commands (2: also every command). CS2_SKIP_TO=scene:line runs the story without waits or clicks
//! until that line of that scene.

use std::path::Path;

use nexas::files::Disk;

/// where and how the pointer acts: c click, r right click, u wheel up, m move
type Pointer = (i32, i32, char);

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err("usage: cs2-run <game folder> <frames> <out prefix> [frame[:x:y] ...]".into());
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
    let mut e = catsystem2::engine::Engine::open(Box::new(disk))?;
    let log = std::env::var("CS2_TRACE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    e.log = log;
    if let Ok(s) = std::env::var("CS2_SKIP_TO") {
        // scene:line, e.g. 00_01:5020
        let (n, l) = s.split_once(':').ok_or("CS2_SKIP_TO=scene:line")?;
        e.skip_to = Some((n.to_string(), l.parse().map_err(|_| "CS2_SKIP_TO line")?));
    }
    if let Some(f) = std::env::var_os("CS2_FONT") {
        e.set_font(&std::fs::read(f).map_err(|e| e.to_string())?)?;
    }
    for f in 1..=frames {
        if log > 0 && marks.iter().any(|m| m.0 == f) {
            eprintln!("-- frame {f}");
        }
        for (_, click) in marks.iter().filter(|m| m.0 == f) {
            if let Some((x, y, kind)) = click {
                e.point(*x, *y);
                match kind {
                    'r' => e.cancel(),
                    'u' => e.wheel(true),
                    'm' => {}
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
            eprintln!("cs2: the game ended at frame {f}");
            break;
        }
    }
    if log > 0 {
        // what is on the screen at the end
        for (i, o) in e.objs.iter().enumerate() {
            if let Some(o) = o.as_ref().filter(|o| o.disp) {
                eprintln!(
                    "obj {i} {:?} owner {} plane {:?} pos {:?} base {:?} scale {:?} alpha {} pri {} {} {:?}",
                    o.kind,
                    o.owner,
                    o.plane,
                    o.pos,
                    o.base,
                    o.scale,
                    o.alpha,
                    o.pri,
                    o.file,
                    o.parts
                );
            }
        }
    }
    Ok(())
}
