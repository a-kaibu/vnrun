//! Saves pictures of a GRD archive as PNG files on their canvas.
//!
//!     tmrhiro-grd <archive.pac> <out dir> [name ...]

use tmrhiro::{grd, pac};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err("usage: tmrhiro-grd <archive.pac> <out dir> [name ...]".into());
    }
    let b = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    let index = pac::Index::parse(&b)?;
    for name in &index.names {
        if args.len() > 3 && !args[3..].iter().any(|n| n.eq_ignore_ascii_case(name)) {
            continue;
        }
        let e = index.find(name).unwrap();
        let data = &b[e.offset as usize..e.offset as usize + e.size as usize];
        match grd::decode(data) {
            Ok(g) => {
                let mut canvas = nexas::gfx::Image::new(g.canvas.0, g.canvas.1);
                for y in 0..g.image.h as i32 {
                    for x in 0..g.image.w as i32 {
                        let (cx, cy) = (g.x + x, g.y + y);
                        if cx < 0 || cy < 0 || cx >= canvas.w as i32 || cy >= canvas.h as i32 {
                            continue;
                        }
                        let s = ((y as u32 * g.image.w + x as u32) * 4) as usize;
                        let d = ((cy as u32 * canvas.w + cx as u32) * 4) as usize;
                        canvas.px[d..d + 4].copy_from_slice(&g.image.px[s..s + 4]);
                    }
                }
                let out = format!("{}/{name}.png", args[2]);
                image::save_buffer(
                    &out,
                    &canvas.px,
                    canvas.w,
                    canvas.h,
                    image::ColorType::Rgba8,
                )
                .map_err(|e| e.to_string())?;
                println!(
                    "{name}: {}x{} at {},{} on {}x{}",
                    g.image.w, g.image.h, g.x, g.y, g.canvas.0, g.canvas.1
                );
            }
            Err(err) => println!("{name}: {err}"),
        }
    }
    Ok(())
}
