//! sxls SX [OUTDIR [FILTER]]: lists a SakanaGL package; with OUTDIR, extracts the files whose path
//! contains FILTER (all without one).
use std::io::{Read, Seek, SeekFrom};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sx = std::path::Path::new(&a[1]);
    let idx = sakanagl::sx::open(&std::fs::read(sx).unwrap()).unwrap();
    let ix = sakanagl::pkg::Index::parse(&idx).unwrap();
    let stem = sx.file_name().unwrap().to_str().unwrap();
    let base = &stem[..stem.rfind('(').unwrap()];
    let dir = sx.parent().unwrap();
    let mut files: Vec<_> = ix
        .storages
        .iter()
        .map(|s| std::fs::File::open(dir.join(format!("{base}-{}.sxstorage", s.name))).unwrap())
        .collect();
    let filter = a.get(3).map(String::as_str).unwrap_or("");
    for p in &ix.order {
        let e = ix.files[p];
        if a.len() < 3 {
            println!("{p} st{} fl{} @{} {}", e.storage, e.flags, e.block, e.size);
            continue;
        }
        if !p.contains(filter) {
            continue;
        }
        let f = &mut files[e.storage as usize];
        f.seek(SeekFrom::Start(e.block as u64 * 16)).unwrap();
        let mut raw = vec![0; e.size as usize];
        f.read_exact(&mut raw).unwrap();
        match sakanagl::sx::file(&raw, e.flags, e.block) {
            Ok(d) => {
                let out = std::path::Path::new(&a[2]).join(p);
                std::fs::create_dir_all(out.parent().unwrap()).unwrap();
                std::fs::write(out, d).unwrap();
            }
            Err(err) => println!("{p}: {err}"),
        }
    }
}
