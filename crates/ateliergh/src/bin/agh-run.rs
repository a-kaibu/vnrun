//! Runs an Atelier G/H game without a window and saves frames as PNG files.
//!
//!     agh-run <game folder> <frames> <out prefix> [frame[:x:y[:m]] ...]
//!
//! A bare `frame` is saved as `<out prefix><frame>.png`; `frame:x:y` clicks there on that frame
//! (`:x:y:m` only moves the pointer). The last frame is always saved. AGH_FONT names a TrueType
//! font, AGH_TRACE=1 prints each script item; the text shown and the commands not carried out
//! are printed at the end.

use std::path::Path;

use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err("usage: agh-run <game folder> <frames> <out prefix> [frame[:x:y] ...]".into());
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
    let mut e = ateliergh::engine::Engine::new(Box::new(disk))?;
    e.log = std::env::var_os("AGH_TRACE").is_some();
    if let Some(f) = std::env::var_os("AGH_FONT") {
        e.set_font(&std::fs::read(f).map_err(|e| e.to_string())?)?;
    }
    let mut shown = 0;
    for f in 1..=frames {
        for (_, act) in marks.iter().filter(|m| m.0 == f && !m.1.is_empty()) {
            let n = |i: usize| act.get(i).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
            let (x, y) = (n(0), n(1));
            e.point(x, y);
            if act.get(2).map(String::as_str) != Some("m") {
                e.click(x, y);
            }
        }
        e.frame()?;
        e.take_sounds();
        while shown < e.text_log.len() {
            println!("[{f}] {}", e.text_log[shown]);
            shown += 1;
        }
        if f == frames || marks.iter().any(|m| m.0 == f && m.1.is_empty()) {
            let out = format!("{prefix}{f}.png");
            let frame = e.render();
            image::save_buffer(&out, &frame.px, frame.w, frame.h, image::ColorType::Rgba8)
                .map_err(|err| format!("{out}: {err}"))?;
        }
        if e.ended() {
            eprintln!("agh: the game ended at frame {f}");
            break;
        }
    }
    let unknown: Vec<String> = e.unknown.iter().map(|(k, v)| format!("{k}:{v}")).collect();
    eprintln!("not carried out: {}", unknown.join(" "));
    Ok(())
}
