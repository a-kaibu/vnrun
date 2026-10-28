//! Lists the files of a game's .fpk archives, or writes one out (pictures as PNG).
//!
//!     cargo run --release -p interheart --example fpkls <game folder> [name [out]]
//!
//! With `--title <program.exe>` it prints the title buttons the program's table holds, and with
//! `--window <program.exe>` its message window, choices and the buttons on the window.

use interheart::exe;
use interheart::fpk::Archives;
use nexas::files::Disk;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a[1] == "--title" || a[1] == "--window" {
        let b = std::fs::read(&a[2]).unwrap();
        if a[1] == "--window" {
            println!("{:?}", exe::message_window(&b));
            for c in exe::choices(&b).unwrap_or_default() {
                println!("choice {c:?}");
            }
        }
        let group = if a[1] == "--title" {
            "Title"
        } else {
            "Message"
        };
        for b in exe::buttons(&b, group).unwrap_or_default() {
            println!("{b:?}");
        }
        return;
    }
    let root = std::path::PathBuf::from(&a[1]);
    let mut disk = Disk {
        saves: root.join("vnrun-saves"),
        root,
    };
    let arcs = Archives::open(&mut disk);
    match a.get(2) {
        None => {
            for (pack, index) in &arcs.packs {
                for n in index.entries.keys() {
                    println!("{pack}\t{n}");
                }
            }
        }
        Some(name) => {
            let b = arcs.read(&mut disk, name).expect("no such file");
            let out = a.get(3).map_or(name.as_str(), |s| s);
            let l = name.to_lowercase();
            if l.ends_with(".kg") || l.ends_with(".bmp") {
                let img = interheart::pic::decode(&b).expect("not a picture");
                image::save_buffer(out, &img.px, img.w, img.h, image::ColorType::Rgba8).unwrap();
            } else {
                std::fs::write(out, b).unwrap();
            }
        }
    }
}
