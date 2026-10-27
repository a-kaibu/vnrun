//! Lists a Marble .mbl archive, or saves one of its files decoded (PRS as PNG, scripts as
//! text, WADY as WAV).
//!
//!     marble-dump <archive.mbl> [name out]

use tmrhiro::marble::{mbl, prs, wady};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let b = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
    let index = mbl::Index::parse(&b, b.len() as u64)?;
    if args.len() < 4 {
        for name in &index.names {
            let e = index.entries[name];
            println!("{name} {} {}", e.offset, e.size);
        }
        return Ok(());
    }
    let e = index
        .entries
        .get(&args[2].to_uppercase())
        .ok_or("no such file")?;
    let data = &b[e.offset as usize..(e.offset + e.size) as usize];
    let out = &args[3];
    if data.starts_with(b"YB") {
        let img = prs::decode(data)?;
        image::save_buffer(out, &img.px, img.w, img.h, image::ColorType::Rgba8)
            .map_err(|e| e.to_string())
    } else if let Some(wav) = wady::to_wav(data) {
        std::fs::write(out, wav).map_err(|e| e.to_string())
    } else if out.ends_with(".txt") {
        std::fs::write(out, nexas::sjis::decode(&mbl::script(data))).map_err(|e| e.to_string())
    } else {
        std::fs::write(out, data).map_err(|e| e.to_string())
    }
}
