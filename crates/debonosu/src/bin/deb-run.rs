//! Runs a DebonosuWorks game without a window and saves frames as PNG files.
//!
//!     deb-run <game folder> <frames> <out prefix> [frame[:x:y[:m]] ...]
//!
//! A bare `frame` is saved as `<out prefix><frame>.png` (with a status line); `frame:x:y` clicks
//! there on that frame (`:x:y:m` only moves the pointer). The last frame is always saved.
//! DEB_FONT names a TrueType font, DEB_LOG=1 prints the text and what is missing, DEB_TRACE=1
//! the labels the scripts go to.

use std::path::Path;

use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err("usage: deb-run <game folder> <frames> <out prefix> [frame[:x:y] ...]".into());
    }
    let dir = Path::new(&args[1]);
    let frames: u64 = args[2].parse().map_err(|_| "frames: not a number")?;
    let prefix = &args[3];
    let mut marks: Vec<(u64, Vec<String>)> = Vec::new();
    for m in &args[4..] {
        let v: Vec<String> = m.split(':').map(str::to_string).collect();
        let f = v[0]
            .parse::<u64>()
            .map_err(|_| format!("{m}: not a number"))?;
        marks.push((f, v[1..].to_vec()));
    }
    let disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let mut e = debonosu::engine::Engine::new(Box::new(disk))?;
    if let Some(f) = std::env::var_os("DEB_FONT") {
        e.set_font(&std::fs::read(f).map_err(|e| e.to_string())?)?;
    }
    for f in 1..=frames {
        for (_, act) in marks.iter().filter(|m| m.0 == f && !m.1.is_empty()) {
            let n = |i: usize| act.get(i).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
            let (x, y) = (n(0), n(1));
            e.point(x, y);
            if act.get(2).map(String::as_str) != Some("m") {
                e.click(x, y);
            }
        }
        let r = e.frame();
        for s in e.take_sounds() {
            if std::env::var_os("DEB_LOG").is_some() {
                eprintln!(
                    "[{f}] sound ch{} {} bytes{}",
                    s.channel,
                    s.data.len(),
                    if s.stop { " stop" } else { "" }
                );
            }
        }
        let stop = r.is_err() || e.halted().is_some() || e.ended();
        let save = f == frames || marks.iter().any(|m| m.0 == f && m.1.is_empty()) || stop;
        if save {
            println!("{f}: {}", e.status());
            let out = format!("{prefix}{f}.png");
            let frame = e.render();
            image::save_buffer(&out, &frame.px, frame.w, frame.h, image::ColorType::Rgba8)
                .map_err(|err| format!("{out}: {err}"))?;
        }
        if stop {
            eprintln!("deb: stopped at frame {f}");
            break;
        }
    }
    Ok(())
}
