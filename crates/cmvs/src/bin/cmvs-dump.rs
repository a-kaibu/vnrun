//! Lists or extracts the entries of a CMVS archive; PB3 pictures are saved as PNG.
//!
//!     cmvs-dump <game folder> <archive.cpz>                 list
//!     cmvs-dump <game folder> <archive.cpz> <name> <out>    extract
//!     cmvs-dump <game folder> <movie.cmv> <out.png>         the first frame

use nexas::files::{Disk, Files};

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        return Err("usage: cmvs-dump <game folder> <archive.cpz> [<name> <out>]".into());
    }
    let mut disk = Disk {
        root: a[1].clone().into(),
        saves: std::env::temp_dir(),
    };
    if a[2].ends_with(".cmv") {
        let b = disk.read(&a[2]).ok_or("cannot read")?;
        let img = cmvs::pb3::cmv_first_frame(&b)?;
        let out = a.get(3).ok_or("out?")?;
        return image::save_buffer(out, &img.px, img.w, img.h, image::ColorType::Rgba8)
            .map_err(|e| e.to_string());
    }
    let cpz = cmvs::cpz::Cpz::open(&mut disk, &a[2])?;
    if a.len() < 5 {
        let mut names: Vec<_> = cpz.entries.iter().collect();
        names.sort_by_key(|e| e.1.offset);
        for (n, e) in names {
            println!("{n}\t{}", e.size);
        }
        return Ok(());
    }
    let d = cpz.read(&mut disk, &a[3]).ok_or("no such entry")?;
    if cmvs::pb3::header(&d).is_some() {
        let dir = a[3].rsplit_once('/').map_or("", |d| d.0).to_string();
        let img = cmvs::pb3::decode(&d, &mut |n| {
            let full = if dir.is_empty() {
                format!("{n}.pb3")
            } else {
                format!("{dir}/{n}.pb3")
            };
            let b = cpz.read(&mut disk, &full)?;
            cmvs::pb3::decode(&b, &mut |_| None).ok()
        })?;
        image::save_buffer(&a[4], &img.px, img.w, img.h, image::ColorType::Rgba8)
            .map_err(|e| e.to_string())?;
    } else {
        std::fs::write(&a[4], d).map_err(|e| e.to_string())?;
    }
    let _ = disk.list();
    Ok(())
}
