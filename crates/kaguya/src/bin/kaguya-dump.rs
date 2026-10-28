//! kaguya-dump list ARC | extract ARC OUTDIR [NAME,...] | png PIC OUT.png

use std::path::Path;

use kaguya::arc::Arc;
use nexas::files::Disk;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args[1] == "png" {
        let b = std::fs::read(&args[2]).unwrap();
        let img = match kaguya::pic::parts(&b) {
            Ok(p) => {
                for q in &p.parts {
                    println!("{} {} {:?} at {} {}", q.state, q.name, q.src, q.x, q.y);
                }
                p.atlas
            }
            Err(_) => kaguya::pic::decode(&b).unwrap().img,
        };
        image::save_buffer(&args[3], &img.px, img.w, img.h, image::ColorType::Rgba8).unwrap();
        return;
    }
    let p = Path::new(&args[2]);
    let mut disk = Disk {
        root: p.parent().unwrap().to_path_buf(),
        saves: "/nonexistent".into(),
    };
    let name = p.file_name().unwrap().to_str().unwrap();
    let arc = Arc::open(&mut disk, name).unwrap();
    match args[1].as_str() {
        "list" => {
            for e in &arc.entries {
                println!("{}\t{}\t{}\t{}", e.name, e.offset, e.size, e.packed);
            }
        }
        "extract" => {
            let out = Path::new(&args[3]);
            std::fs::create_dir_all(out).unwrap();
            let want: Vec<String> = args
                .get(4)
                .map(|s| s.split(',').map(kaguya::arc::key).collect())
                .unwrap_or_default();
            for e in &arc.entries {
                if !want.is_empty() && !want.contains(&kaguya::arc::key(&e.name)) {
                    continue;
                }
                std::fs::write(out.join(&e.name), arc.read(&mut disk, e).unwrap()).unwrap();
            }
        }
        _ => eprintln!("kaguya-dump list ARC | extract ARC OUTDIR [NAME,..]"),
    }
}
