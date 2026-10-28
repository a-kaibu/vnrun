//! Runs a Majiro game without a window and saves frames as PNG files.
//!
//!     majiro-run <game folder> <frames> <out prefix> [frame[:x:y[:r|m]] ...]
//!
//! Each `frame` is saved as `<out prefix><frame>.png`; with `:x:y`, there is also a click there
//! on that frame (`:x:y:r` a right click, `:x:y:m` only moves the pointer there). The last frame
//! is always saved. MAJIRO_FONT names a TrueType font for text; MAJIRO_TRACE=1 prints every warning
//! and where the threads are at the end, MAJIRO_TRACE=3 also every script call.

use std::path::Path;

use nexas::files::Disk;

/// where and how the pointer acts: c click, r right click, m move
type Pointer = (i32, i32, char);

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(
            "usage: majiro-run <game folder> <frames> <out prefix> [frame[:x:y] ...]".into(),
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
    let mut e = majiro::engine::Engine::open(Box::new(disk))?;
    let log = std::env::var("MAJIRO_TRACE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    e.log = log;
    if let Some(f) = std::env::var_os("MAJIRO_FONT") {
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
            eprintln!("majiro: the game ended at frame {f}");
            break;
        }
    }
    if log > 0 {
        for s in e.describe_sprites() {
            eprintln!("majiro: {s}");
        }
        for w in e.where_threads() {
            eprintln!("majiro: thread at {w}");
        }
        // MAJIRO_PAGES=13,16: those pages (as shown, alpha included) next to the frames
        if let Ok(ids) = std::env::var("MAJIRO_PAGES") {
            for id in ids.split(',').filter_map(|s| s.trim().parse::<i32>().ok()) {
                if let Some(p) = e.gfx.pages.get(&id) {
                    eprintln!(
                        "majiro: page {id}: {}x{} alpha {} anti {:?}",
                        p.w(),
                        p.h(),
                        p.alpha,
                        p.anti
                    );
                }
                if let Some(img) = e.gfx.shown(id) {
                    let out = format!("{prefix}page{id}.png");
                    image::save_buffer(&out, &img.px, img.w, img.h, image::ColorType::Rgba8)
                        .map_err(|err| format!("{out}: {err}"))?;
                }
            }
        }
    }
    Ok(())
}
