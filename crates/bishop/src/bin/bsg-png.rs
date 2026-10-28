//! Saves a picture of a Bishop archive as PNG: `bsg-png <file.bsa> <name> <out.png>`, or
//! lists its names: `bsg-png <file.bsa> - <prefix>`.

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        return Err("usage: bsg-png <file.bsa> <name> <out.png>".into());
    }
    let b = std::fs::read(&a[1]).map_err(|e| e.to_string())?;
    let (ver, count, at) = bishop::bsa::header(&b).ok_or("not a BSA")?;
    let idx = bishop::bsa::Index::parse(ver, count, &b[at as usize..]);
    // `bsg-png <file.bsa> - <prefix>`: lists the names that start with prefix
    if a[2] == "-" {
        let mut names: Vec<&String> = idx
            .entries
            .keys()
            .filter(|k| k.starts_with(&a[3].to_lowercase()))
            .collect();
        names.sort();
        for k in names {
            println!("{k}");
        }
        return Ok(());
    }
    let (o, n) = *idx
        .entries
        .get(&a[2].to_lowercase())
        .ok_or("no such entry")?;
    let p = bishop::bsg::decode(&b[o as usize..o as usize + n])?;
    eprintln!("{}x{} at {},{}", p.image.w, p.image.h, p.x, p.y);
    image::save_buffer(
        &a[3],
        &p.image.px,
        p.image.w,
        p.image.h,
        image::ColorType::Rgba8,
    )
    .map_err(|e| e.to_string())
}
