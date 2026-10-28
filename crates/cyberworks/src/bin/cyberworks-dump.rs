//! Lists or saves the resources of a Cyberworks game.
//!
//!     cyberworks-dump <game folder> list
//!     cyberworks-dump <game folder> pics <out dir> [first id] [last id]
//!     cyberworks-dump <game folder> raw <kind letter> <out dir>

use std::path::Path;

use cyberworks::arc::Archives;
use cyberworks::pic;
use nexas::files::Disk;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let dir = Path::new(&a[1]);
    let mut disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let arcs = Archives::open(&mut disk)?;
    println!("title {}", arcs.title);
    match a[2].as_str() {
        "list" => {
            for k in b'a'..=b'z' {
                let es: Vec<_> = arcs.of(k).collect();
                if let (Some(f), Some(l)) = (es.first(), es.last()) {
                    println!("{} {} ids {}..{}", k as char, es.len(), f.id, l.id);
                }
            }
        }
        "pics" => {
            let out = Path::new(&a[3]);
            let lo: u32 = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(0);
            let hi: u32 = a.get(5).and_then(|x| x.parse().ok()).unwrap_or(u32::MAX);
            let s = pic::Scheme::tinkerbell_2018();
            let mut last: Option<pic::Raw> = None;
            let es: Vec<_> = arcs
                .of(b'b')
                .chain(arcs.of(b'n'))
                .chain(arcs.of(b'o'))
                .cloned()
                .collect();
            for e in es {
                let Some(d) = arcs.read(&mut disk, &e) else {
                    continue;
                };
                let img = match d.first() {
                    Some(b'a') | Some(b'd') => match pic::decode_a(&d, &s, last.as_ref()) {
                        Ok(r) => {
                            let i = r.to_image(s.flipped);
                            if d[0] == b'a' && !pic::wants_base(&d, &s) {
                                last = Some(r);
                            }
                            Some(i)
                        }
                        Err(err) => {
                            println!("{} {}: {err}", e.id, d[0] as char);
                            None
                        }
                    },
                    Some(b'b') | Some(b'c') => {
                        let i = pic::png_of(&d).and_then(|p| nexas::gfx::Image::decode(p).ok());
                        last = i.as_ref().map(pic::Raw::from_image);
                        i
                    }
                    _ => None,
                };
                if e.id < lo || e.id > hi {
                    continue;
                }
                if let Some(i) = img {
                    println!("{} {} {}x{}", e.id, d[0] as char, i.w, i.h);
                    let p = out.join(format!("{}.png", e.id));
                    image::save_buffer(&p, &i.px, i.w, i.h, image::ColorType::Rgba8)
                        .map_err(|x| x.to_string())?;
                }
            }
        }
        "raw" => {
            let k = a[3].as_bytes()[0];
            let out = Path::new(&a[4]);
            let es: Vec<_> = arcs.of(k).cloned().collect();
            for e in es {
                if let Some(d) = arcs.read(&mut disk, &e) {
                    let p = out.join(format!("{:06}.{}", e.id, String::from_utf8_lossy(&e.kind)));
                    std::fs::write(p, d).map_err(|x| x.to_string())?;
                }
            }
        }
        _ => return Err("list, pics or raw".into()),
    }
    Ok(())
}
