//! kaguya-run GAME_DIR FONT.ttf FRAMES [OUT_DIR]: runs the game headless, clicking on its
//! own (on the title, the first item), and prints the messages; every 60th frame goes to
//! OUT_DIR as a PNG when given (KAGUYA_EVERY changes that; KAGUYA_CLICK, every how many
//! frames it clicks: 20).

use kaguya::engine::Engine;
use nexas::files::Disk;
use nexas::gfx::{Image, draw};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let disk = Disk {
        root: args[1].clone().into(),
        saves: "/nonexistent".into(),
    };
    let mut e = Engine::open(Box::new(disk)).unwrap();
    e.log = true;
    e.set_font(&std::fs::read(&args[2]).unwrap()).unwrap();
    let frames: usize = args[3].parse().unwrap();
    let every: usize = std::env::var("KAGUYA_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let click: usize = std::env::var("KAGUYA_CLICK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    for f in 0..frames {
        e.frame().unwrap();
        e.take_sounds();
        if f % click == click - 1 {
            e.click(660, 480);
        }
        if e.ended() {
            break;
        }
        if let Some(out) = args.get(4)
            && f % every == 0
        {
            let mut img = Image::new(e.width, e.height);
            img.fill([0, 0, 0, 255]);
            for d in e.draws() {
                draw(&mut img, &d.image, d.src, d.dst, &d.place);
            }
            image::save_buffer(
                format!("{out}/{f:06}.png"),
                &img.px,
                img.w,
                img.h,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
}
