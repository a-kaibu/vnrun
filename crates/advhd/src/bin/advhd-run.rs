//! Runs an AdvHD game without a window and saves frames as PNG files.
//!
//!     advhd-run <game folder> <frames> <out prefix> [frame[:x:y] ...]
//!
//! Each `frame` is saved as `<out prefix><frame>.png`; with `:x:y`, there is also a click there
//! on that frame (`-1:-1`: a click on nothing, as Enter). The last frame is always saved.
//! ADVHD_FONT names a TrueType font (default: web/public/fonts/ipag.ttf), ADVHD_TRACE lists
//! commands as they run, ADVHD_START=frame:script jumps to a script on that frame, ADVHD_NAME is the text for text fields.

use advhd::engine::Engine;
use advhd::files::Disk;
use std::path::Path;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err("usage: advhd-run <game folder> <frames> <out prefix> [frame[:x:y] ...]".into());
    }
    let dir = Path::new(&args[1]);
    let frames: u64 = args[2].parse().map_err(|_| "frames: not a number")?;
    let prefix = &args[3];
    let mut marks: Vec<(u64, Option<(f32, f32)>)> = Vec::new();
    for m in &args[4..] {
        let v: Vec<&str> = m.split(':').collect();
        let n = |s: &str| s.parse::<f32>().map_err(|_| format!("{m}: not a number"));
        let click = if v.len() == 3 { Some((n(v[1])?, n(v[2])?)) } else { None };
        marks.push((n(v[0])? as u64, click));
    }
    let disk = Disk { root: dir.to_path_buf(), saves: dir.join("vnrun-saves") };
    let mut e = Engine::new(Box::new(disk))?;
    e.set_trace(std::env::var_os("ADVHD_TRACE").is_some());
    let font = std::env::var("ADVHD_FONT").unwrap_or_else(|_| "web/public/fonts/ipag.ttf".into());
    match std::fs::read(&font) {
        Ok(b) => e.set_font(&b)?,
        Err(err) => eprintln!("advhd: {font}: {err} (no text)"),
    }
    // ADVHD_START=frame:script jumps to a script on that frame
    let start = std::env::var("ADVHD_START").ok();
    let start = start.as_deref().and_then(|s| s.split_once(':')).and_then(|(f, s)| Some((f.parse::<u64>().ok()?, s)));
    for f in 1..=frames {
        if let Some((_, s)) = start.filter(|x| x.0 == f) {
            e.goto(s)?;
        }
        for (_, click) in marks.iter().filter(|m| m.0 == f) {
            if let Some((x, y)) = click {
                e.click(*x, *y);
            }
        }
        e.frame()?;
        if e.input_wanted() {
            e.enter_text(&std::env::var("ADVHD_NAME").unwrap_or_else(|_| "vnrun".into()));
        }
        if f == frames || marks.iter().any(|m| m.0 == f) {
            let out = format!("{prefix}{f}.png");
            let img = e.render();
            image::save_buffer(&out, &img.px, img.w, img.h, image::ColorType::Rgba8).map_err(|err| format!("{out}: {err}"))?;
            eprintln!("{f}: {}", e.describe());
            if std::env::var_os("ADVHD_DRAWS").is_some() {
                for d in e.draws() {
                    eprintln!("  {}x{} src {:?} corners {:?} alpha {}", d.image.w, d.image.h, d.src, d.corners(), d.place.alpha);
                }
            }
        }
        if e.ended() {
            eprintln!("{f}: ended");
            break;
        }
    }
    Ok(())
}
