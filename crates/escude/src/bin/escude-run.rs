//! Runs an Escu:de game without a screen, for testing: frames at 60 a second, input at given
//! frames, and the screen saved as PNG.
//!
//!     escude-run <game folder> <out prefix> <steps>
//!
//! steps (comma separated): `run:<frames>`, `shot:<name>`, `click:<x>;<y>`, `rclick`,
//! `key:<code>`, `move:<x>;<y>`.

use escude::engine::Engine;
use nexas::files::Disk;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err("usage: escude-run <game folder> <out prefix> <steps>".into());
    }
    let dir = std::path::PathBuf::from(&args[1]);
    let disk = Disk {
        root: dir.clone(),
        saves: dir.join("vnrun-saves"),
    };
    let mut e = Engine::open(Box::new(disk))?;
    if let Ok(font) = std::fs::read("/home/user/vnrun/web/public/fonts/ipag.ttf") {
        e.set_font(&font)?;
    } else if let Some(p) = std::env::var_os("ESCUDE_FONT") {
        e.set_font(&std::fs::read(p).map_err(|e| e.to_string())?)?;
    }
    if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        e.set_clock(d.as_secs() as i64);
    }
    let mut frame = 0u64;
    let started = std::time::Instant::now();
    let run = |e: &mut Engine, n: u64, frame: &mut u64| -> Result<(), String> {
        for _ in 0..n {
            *frame += 1;
            let r = e.frame(*frame as f64 * 1000.0 / 60.0);
            for l in e.st.log.drain(..) {
                eprintln!("[{frame}] {l}");
            }
            r.map_err(|err| format!("frame {frame}: {err}"))?;
            if e.st.ended {
                eprintln!("ended at frame {frame}");
                break;
            }
        }
        Ok(())
    };
    for step in args[3].split(',') {
        let (k, v) = step.split_once(':').unwrap_or((step, ""));
        let nums: Vec<i32> = v.split(';').filter_map(|s| s.parse().ok()).collect();
        match k {
            "run" => run(&mut e, v.parse().unwrap_or(1), &mut frame)?,
            // profile:<frames>: time the built-ins over some frames
            "profile" => {
                e.st.profile = Some(Default::default());
                let (t, steps) = (std::time::Instant::now(), e.vm.steps);
                run(&mut e, v.parse().unwrap_or(1), &mut frame)?;
                let total = t.elapsed().as_secs_f64();
                let mut rows: Vec<_> = e.st.profile.take().unwrap().into_iter().collect();
                rows.sort_by(|a, b| b.1.1.total_cmp(&a.1.1));
                let natives: f64 = rows.iter().map(|r| r.1.1).sum();
                eprintln!(
                    "{total:.2}s, {:.2}s in built-ins, {} instructions",
                    natives,
                    e.vm.steps - steps
                );
                for (n, (c, s)) in rows.iter().take(15) {
                    eprintln!("  {n:<14} {c:>8} {s:.3}s");
                }
            }
            // trace:<frames>: log the built-ins called over some frames
            "trace" => {
                e.st.trace = true;
                run(&mut e, v.parse().unwrap_or(1), &mut frame)?;
                e.st.trace = false;
            }
            "check" => {
                for err in e.compile_all() {
                    eprintln!("{err}");
                }
            }
            "shot" => {
                let (w, h) = (e.width() as u32, e.height() as u32);
                let img =
                    image::RgbaImage::from_raw(w, h, e.st.frame.clone()).ok_or("frame size")?;
                let path = format!("{}_{v}.png", args[2]);
                img.save(&path).map_err(|e| e.to_string())?;
                eprintln!("[{frame}] {path} ({:.1}s)", started.elapsed().as_secs_f32());
            }
            // plane:<n>: save a plane (as the game drew it) as PNG
            "plane" => {
                let n = nums[0];
                let p = e.st.planes.get(n).cloned().ok_or("no such plane")?;
                let px = e.st.planes.read(&e.vm, n, [0, 0, p.w, p.h]);
                let rgba: Vec<u8> = px.iter().flat_map(|c| [c[2], c[1], c[0], 255]).collect();
                let img =
                    image::RgbaImage::from_raw(p.w as u32, p.h as u32, rgba).ok_or("plane size")?;
                let path = format!("{}_plane{n}.png", args[2]);
                img.save(&path).map_err(|e| e.to_string())?;
                eprintln!("[{frame}] {path} ({}x{} {} bit)", p.w, p.h, p.depth);
            }
            "move" => e.point(nums[0], nums[1]),
            "click" => {
                e.point(nums[0], nums[1]);
                e.button_down(0);
                run(&mut e, 1, &mut frame)?;
                e.button_up(0);
            }
            "rclick" => {
                e.button_down(1);
                run(&mut e, 1, &mut frame)?;
                e.button_up(1);
            }
            "key" => {
                e.key(nums[0] as u32, true);
                run(&mut e, 1, &mut frame)?;
                e.key(nums[0] as u32, false);
            }
            _ => return Err(format!("unknown step {k}")),
        }
    }
    Ok(())
}
