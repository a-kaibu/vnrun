//! Runs a System-NNN game without a window and saves frames as PNG files.
//!
//!     nnn-run <game folder> <frames> <out prefix> [frame[:x:y[:kind]] ...]
//!
//! A bare `frame` is saved as `<out prefix><frame>.png`; `frame:x:y` clicks there on that frame
//! instead (`:x:y:r` a right click, `:x:y:m` only moves the pointer there), and `a-b/n` clicks
//! (as the Enter key) every n frames from a to b. The last frame is
//! always saved. NNN_FONT names a TrueType font for text, NNN_TRACE=1 prints what the scripts
//! do and NNN_TEXT=1 prints each new message.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nexas::files::{Disk, Files};

type Pointer = (i32, i32, char);

/// The game folder with paths matched without regard to case (the engine asks in lower case).
struct Folded {
    disk: Disk,
    paths: HashMap<String, String>,
}

fn walk(root: &Path, dir: &Path, out: &mut HashMap<String, String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            out.insert(rel.to_lowercase(), rel);
        }
    }
}

impl Folded {
    fn path(&self, p: &str) -> String {
        let key = p.replace('\\', "/").to_lowercase();
        self.paths
            .get(&key)
            .cloned()
            .unwrap_or_else(|| p.to_string())
    }
}

impl Files for Folded {
    fn size(&mut self, path: &str) -> Option<u64> {
        let p = self.path(path);
        self.disk.size(&p)
    }
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        let p = self.path(path);
        self.disk.read_range(&p, offset, len)
    }
    fn list(&mut self) -> Vec<String> {
        self.disk.list()
    }
    fn read_save(&mut self, name: &str) -> Option<Vec<u8>> {
        self.disk.read_save(name)
    }
    fn write_save(&mut self, name: &str, bytes: &[u8]) {
        self.disk.write_save(name, bytes)
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err("usage: nnn-run <game folder> <frames> <out prefix> [frame[:x:y] ...]".into());
    }
    let dir = PathBuf::from(&args[1]);
    let frames: u64 = args[2].parse().map_err(|_| "frames: not a number")?;
    let prefix = &args[3];
    let mut marks: Vec<(u64, Option<Pointer>)> = Vec::new();
    for m in &args[4..] {
        let n = |s: &str| s.parse::<i64>().map_err(|_| format!("{m}: not a number"));
        // from-to/step: a click (Enter) every step frames
        if let Some((range, step)) = m.split_once('/')
            && let Some((a, b)) = range.split_once('-')
        {
            let step = n(step)?.max(1) as usize;
            for f in (n(a)?..=n(b)?).step_by(step) {
                marks.push((f as u64, Some((-1, -1, 'c'))));
            }
            continue;
        }
        let v: Vec<&str> = m.split(':').collect();
        let click = if v.len() >= 3 {
            let kind = v.get(3).and_then(|k| k.chars().next()).unwrap_or('c');
            Some((n(v[1])? as i32, n(v[2])? as i32, kind))
        } else {
            None
        };
        marks.push((n(v[0])? as u64, click));
    }
    let mut paths = HashMap::new();
    walk(&dir, &dir, &mut paths);
    let files = Folded {
        disk: Disk {
            root: dir.clone(),
            saves: dir.join("vnrun-saves"),
        },
        paths,
    };
    let mut e = systemnnn::engine::Engine::new(Box::new(files))?;
    let log = std::env::var_os("NNN_TRACE").is_some();
    let show_text = std::env::var_os("NNN_TEXT").is_some();
    e.log = log;
    if let Some(f) = std::env::var_os("NNN_FONT") {
        e.set_font(&std::fs::read(f).map_err(|e| e.to_string())?)?;
    }
    let mut last_text = String::new();
    let mut last_mode = "";
    for f in 1..=frames {
        if log && marks.iter().any(|m| m.0 == f) {
            eprintln!("-- frame {f}: {}", e.position());
        }
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
        e.frame()?;
        e.take_sounds();
        if e.mode_name() != last_mode {
            last_mode = e.mode_name();
            eprintln!("nnn: frame {f}: {last_mode}");
        }
        let t = e.shown_text();
        if show_text && t != last_text && !t.is_empty() {
            eprintln!("nnn: frame {f}: {t}");
        }
        last_text = t;
        if f == frames || marks.iter().any(|m| m.0 == f && m.1.is_none()) {
            let out = format!("{prefix}{f}.png");
            let frame = e.render();
            image::save_buffer(&out, &frame.px, frame.w, frame.h, image::ColorType::Rgba8)
                .map_err(|err| format!("{out}: {err}"))?;
        }
        if e.ended() {
            eprintln!("nnn: the game ended at frame {f}");
            break;
        }
    }
    Ok(())
}
