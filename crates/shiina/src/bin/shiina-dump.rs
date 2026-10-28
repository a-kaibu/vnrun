//! Inspects ShiinaRio game files.
//!
//!   shiina-dump list <file.war>               entries
//!   shiina-dump extract <file.war> <out dir>  every entry, decoded
use shiina::warc::{Scheme, Warc};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::rc::Rc;

fn open(path: &str) -> (Warc, fs::File) {
    let mut f = fs::File::open(path).expect("open");
    let len = f.metadata().unwrap().len();
    let schemes: Vec<Rc<Scheme>> = Scheme::all().into_iter().map(Rc::new).collect();
    let mut file = f.try_clone().unwrap();
    let mut read = |at: u64, n: usize| {
        let mut b = vec![0u8; n];
        file.seek(SeekFrom::Start(at)).unwrap();
        let got = file.read(&mut b).unwrap_or(0);
        b.truncate(got);
        b
    };
    let w = Warc::open(&mut read, len, &schemes).expect("not a WARC archive this build can read");
    f.seek(SeekFrom::Start(0)).unwrap();
    (w, f)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("list") => {
            let (w, _) = open(&args[2]);
            println!("WARC {} ({} entries)", w.version, w.entries.len());
            for e in &w.entries {
                println!(
                    "{:08x} {:9} {:9} {:08x} {}",
                    e.offset, e.size, e.unpacked_size, e.flags, e.name
                );
            }
        }
        Some("extract") => {
            let (w, mut f) = open(&args[2]);
            fs::create_dir_all(&args[3]).unwrap();
            for e in &w.entries {
                let mut b = vec![0u8; e.size as usize];
                f.seek(SeekFrom::Start(e.offset as u64)).unwrap();
                f.read_exact(&mut b).unwrap();
                let out = w.decode(e, b);
                fs::write(format!("{}/{}", args[3], e.name), out).unwrap();
            }
        }
        _ => eprintln!("usage: shiina-dump list|extract <file.war> [out]"),
    }
}
