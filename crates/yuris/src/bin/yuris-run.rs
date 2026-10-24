//! Runs a YU-RIS game without a screen and saves screenshots, to check the engine.
//!
//!     yuris-run <game folder> <out prefix> [steps]
//!
//! steps (comma separated), at 60 frames a second:
//!   wait:<frames>    run frames
//!   at:<x>;<y>       move the mouse there and click (in game pixels)
//!   key:<code>       press a key (sysvar::code numbers, e.g. 98 Enter)
//!   shot:<name>      save the screen to <out prefix>_<name>.png
//!
//! Saves go to <out prefix>-saves/. The font is web/public/fonts/ipag.ttf unless YURIS_FONT
//! names another.

use std::path::PathBuf;

use yuris::engine::{Engine, Event};
use yuris::files::Disk;
use yuris::sysvar::code;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err("usage: yuris-run <game folder> <out prefix> [steps]".into());
    }
    let out = &args[2];
    let steps = args.get(3).cloned().unwrap_or_else(|| "wait:120,shot:a".into());
    let font_path = std::env::var("YURIS_FONT").unwrap_or_else(|_| "web/public/fonts/ipag.ttf".into());
    let font = std::fs::read(&font_path).ok();
    let disk = Disk { game: PathBuf::from(&args[1]), saves: PathBuf::from(format!("{out}-saves")) };
    let mut e = Engine::new(Box::new(disk), font.as_deref())?;
    e.vm.state.trace = std::env::var_os("YURIS_TRACE").is_some();
    let mut frame = 0u64;
    let date = [2026, 10, 24, 6, 12, 0, 0, 0];
    let run = |e: &mut Engine, n: u64, frame: &mut u64| -> Result<(), String> {
        for _ in 0..n {
            e.frame(*frame as f64 * 1000.0 / 60.0, date)?;
            *frame += 1;
            for ev in std::mem::take(&mut e.world.events) {
                match ev {
                    // nothing plays here: sounds end at once
                    Event::Play { id, .. } => e.sound_ended(&id),
                    Event::Title(t) => eprintln!("title: {t}"),
                    _ => {}
                }
            }
            if e.vm.ended {
                return Err("the game ended".into());
            }
        }
        Ok(())
    };
    for step in steps.split(',') {
        let (k, v) = step.split_once(':').unwrap_or((step, ""));
        match k {
            "wait" => run(&mut e, v.parse().unwrap_or(60), &mut frame)?,
            "at" => {
                let (x, y) = v.split_once(';').ok_or("at:<x>;<y>")?;
                e.input.x = x.parse().unwrap_or(0);
                e.input.y = y.parse().unwrap_or(0);
                run(&mut e, 2, &mut frame)?;
                e.input.held[code::MOUSE_L] = true;
                run(&mut e, 3, &mut frame)?;
                e.input.held[code::MOUSE_L] = false;
            }
            "key" => {
                let c: usize = v.parse().map_err(|_| "key:<code>")?;
                e.input.held[c] = true;
                run(&mut e, 3, &mut frame)?;
                e.input.held[c] = false;
            }
            "shot" => {
                let img = e.screenshot();
                let path = format!("{out}_{v}.png");
                image::save_buffer(&path, &img.px, img.w, img.h, image::ColorType::Rgba8).map_err(|e| e.to_string())?;
                eprintln!("frame {frame}: {path}");
            }
            _ => return Err(format!("unknown step {step}")),
        }
    }
    Ok(())
}
