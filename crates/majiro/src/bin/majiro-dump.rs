//! majiro-dump list ARC | extract ARC OUTDIR [EXT,...] | dis SCRIPT.mjo [NAMES] | png PIC.rct OUT.png
//! (png: with the "_.rc8" mask next to it, if there is one)
//! NAMES: a file of known names, one a line, to label hashes.

use std::collections::HashMap;
use std::path::Path;

use majiro::arc::Arc;
use majiro::mjo::{Script, decode, hash, mnemonic};
use nexas::files::Disk;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let usage =
        "majiro-dump list ARC | extract ARC OUTDIR [EXT,..] | dis SCRIPT.mjo [NAMES] | png PIC OUT";
    match args.get(1).map(String::as_str) {
        Some("png") => {
            let pic = majiro::rct::parse(&std::fs::read(&args[2]).unwrap()).unwrap();
            let stem = args[2].trim_end_matches(".rct").trim_end_matches(".rc8");
            let mask = std::fs::read(format!("{stem}_.rc8"))
                .ok()
                .and_then(|b| majiro::rct::parse(&b).ok());
            let img = majiro::gfx::to_image(&pic, mask.as_ref());
            image::save_buffer(&args[3], &img.px, img.w, img.h, image::ColorType::Rgba8).unwrap();
        }
        Some("list") | Some("extract") => {
            let p = Path::new(&args[2]);
            let mut disk = Disk {
                root: p.parent().unwrap().to_path_buf(),
                saves: "/nonexistent".into(),
            };
            let name = p.file_name().unwrap().to_str().unwrap();
            let arc = Arc::open(&mut disk, name).unwrap();
            if args[1] == "list" {
                for e in &arc.entries {
                    println!("{}\t{}\t{}", e.name, e.offset, e.size);
                }
                return;
            }
            let out = Path::new(&args[3]);
            std::fs::create_dir_all(out).unwrap();
            let exts: Vec<String> = args
                .get(4)
                .map(|s| s.split(',').map(|x| x.to_lowercase()).collect())
                .unwrap_or_default();
            for e in &arc.entries {
                let lower = e.name.to_lowercase();
                if !exts.is_empty() && !exts.iter().any(|x| lower.ends_with(x.as_str())) {
                    continue;
                }
                std::fs::write(out.join(&e.name), arc.read(&mut disk, e).unwrap()).unwrap();
            }
        }
        Some("dis") => {
            let bytes = std::fs::read(&args[2]).unwrap();
            let s = Script::parse(&args[2], &bytes).unwrap();
            let mut names = HashMap::new();
            if let Some(f) = args.get(3) {
                for l in std::fs::read_to_string(f).unwrap().lines() {
                    names.insert(hash(l), l.to_string());
                }
            }
            let label = |h: u32| {
                names
                    .get(&h)
                    .cloned()
                    .unwrap_or_else(|| format!("${h:08x}"))
            };
            let funcs: HashMap<u32, u32> = s.functions.iter().map(|(h, o)| (*o, *h)).collect();
            println!("main {:#x}", s.main);
            let mut at = 0;
            while at < s.code.len() {
                if let Some(h) = funcs.get(&(at as u32)) {
                    println!("\nfunc {}:", label(*h));
                }
                let op = match decode(&s.code, at) {
                    Ok(op) => op,
                    Err(e) => {
                        println!("{e}");
                        break;
                    }
                };
                let mut line = format!("{at:06x} {}", mnemonic(op.code));
                match op.code {
                    0x1b0..=0x320 | 0x802 | 0x837 => {
                        line += &format!(" f={:04x} {} o={}", op.flags, label(op.hash), op.var)
                    }
                    0x800 | 0x83a => line += &format!(" {}", op.int),
                    0x803 => line += &format!(" {}", op.float),
                    0x801 | 0x840 | 0x842 => {
                        line += &format!(" {:?}", encoding_rs::SHIFT_JIS.decode(&op.text).0)
                    }
                    0x80f | 0x810 | 0x834 | 0x835 => {
                        line += &format!(" {} ({})", label(op.hash), op.argc)
                    }
                    0x829 | 0x836 => line += &format!(" {:?}", op.types),
                    0x850 => {
                        let base = op.next as i32;
                        let t: Vec<String> = op
                            .cases
                            .iter()
                            .enumerate()
                            .map(|(i, c)| {
                                format!("{:x}", base - 4 * (op.cases.len() - i) as i32 + c)
                            })
                            .collect();
                        line += &format!(" [{}]", t.join(" "));
                    }
                    _ if !op.text.is_empty() => {}
                    _ => {}
                }
                if matches!(op.code, 0x82c..=0x82e | 0x830..=0x833 | 0x838 | 0x839 | 0x83b..=0x83d | 0x843 | 0x845 | 0x847)
                {
                    line += &format!(" -> {:x}", op.next as i32 + op.jump);
                }
                println!("{line}");
                at = op.next;
            }
        }
        _ => eprintln!("{usage}"),
    }
}
