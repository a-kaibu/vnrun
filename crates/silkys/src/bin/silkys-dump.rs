//! Looks into a Silky's game's archives.
//!
//!     silkys-dump <game folder> list
//!     silkys-dump <game folder> extract <name> <out file>   (unpacked)

use std::path::Path;

use nexas::files::Disk;
use silkys::arc::Archives;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err("usage: silkys-dump <game folder> list | extract <name> <out>".into());
    }
    let dir = Path::new(&args[1]);
    let mut disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let arcs = Archives::open(&mut disk);
    match args[2].as_str() {
        "list" => {
            let mut v: Vec<_> = arcs.entries().collect();
            v.sort_by(|a, b| (&a.arc, &a.name).cmp(&(&b.arc, &b.name)));
            for e in v {
                println!("{}\t{}\t{}\t{}", e.arc, e.name, e.packed, e.size);
            }
        }
        "extract" => {
            let name = args.get(3).ok_or("a name")?;
            let out = args.get(4).ok_or("an output file")?;
            let b = arcs
                .read(&mut disk, name)
                .ok_or(format!("{name}: not found"))?;
            std::fs::write(out, &b).map_err(|e| e.to_string())?;
        }
        "png" => {
            let name = args.get(3).ok_or("a name")?;
            let out = args.get(4).ok_or("an output file")?;
            let b = arcs
                .read(&mut disk, name)
                .ok_or(format!("{name}: not found"))?;
            let img = silkys::akb::decode(&b, None)?;
            image::save_buffer(out, &img.px, img.w, img.h, image::ColorType::Rgba8)
                .map_err(|e| e.to_string())?;
            let (a, ..) = silkys::akb::header(&b)?;
            println!(
                "{}x{} inner {} {} {} {} base {:?}",
                img.w, img.h, a.x0, a.y0, a.x1, a.y1, a.base
            );
        }
        "dis" => {
            let name = args.get(3).ok_or("a name")?;
            let b = arcs
                .read(&mut disk, name)
                .ok_or(format!("{name}: not found"))?;
            let m = silkys::mes::Mes::parse(&b)?;
            println!("; marks {:?}", m.marks);
            let mut pc = 0;
            let mut line = String::new();
            while pc < m.code.len() {
                match m.op(pc) {
                    Ok((op, next)) => {
                        let t = match &op {
                            silkys::mes::Op::Text(s) | silkys::mes::Op::Str(s) => {
                                format!(
                                    "{:?} {}",
                                    std::mem::discriminant(&op),
                                    nexas::sjis::decode(s)
                                )
                            }
                            _ => format!("{op:?}"),
                        };
                        use silkys::mes::Op::*;
                        if line.is_empty() {
                            line = format!("{pc:06x}:");
                        }
                        line += &format!(" {t}");
                        if matches!(
                            op,
                            Syscall
                                | LibCall
                                | Jz(_)
                                | Jmp(_)
                                | Ret
                                | Yield
                                | Store(_)
                                | Text(_)
                                | Choice(_)
                                | LibReg(_)
                                | MsgId(_)
                                | MsgId2(_)
                                | Escape(_)
                                | Mark(_)
                        ) {
                            println!("{line}");
                            line.clear();
                        }
                        pc = next;
                    }
                    Err(e) => {
                        println!("{e}");
                        break;
                    }
                }
            }
        }
        _ => return Err("list, extract or dis".into()),
    }
    Ok(())
}
