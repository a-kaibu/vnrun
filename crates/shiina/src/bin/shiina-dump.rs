//! Inspects ShiinaRio game files.
//!
//!   shiina-dump list <file.war>               entries
//!   shiina-dump extract <file.war> <out dir>  every entry, decoded
//!   shiina-dump png <file.s25> <out dir>      each frame of a picture as PNG (and its place)
//!   shiina-dump key <file.war> <entry> <magic>
//!                                             a trial's helper key: each scheme's helper key
//!                                             XORed with every byte and with every value of its
//!                                             last two bytes, until <entry> decodes to <magic>
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
        Some("key") => {
            let (w, mut f) = open(&args[2]);
            let e = w.find(&args[3]).expect("no such entry").clone();
            let mut raw = vec![0u8; e.size as usize];
            f.seek(SeekFrom::Start(e.offset as u64)).unwrap();
            f.read_exact(&mut raw).unwrap();
            for base in Scheme::all() {
                let orig: Vec<u8> = base
                    .helper_key
                    .iter()
                    .flat_map(|k| k.to_le_bytes())
                    .collect();
                let len = orig.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
                if len < 2 {
                    continue;
                }
                let try_key = |bytes: &[u8]| {
                    let mut s = base.clone();
                    s.helper_key = bytes
                        .chunks_exact(4)
                        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                        .collect();
                    let mut t = w.clone();
                    t.scheme = Some(Rc::new(s));
                    if t.decode(&e, raw.clone()).starts_with(args[4].as_bytes()) {
                        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                        println!("{}: helper key {hex}", base.name);
                    }
                };
                // keys of one family are one string XORed with a byte, the last two bytes apart
                for c in 0..=255u8 {
                    for v in 0..=0xffffu32 {
                        let mut bytes = orig.clone();
                        for b in &mut bytes[..len] {
                            *b ^= c;
                        }
                        bytes[len - 2] = v as u8;
                        bytes[len - 1] = (v >> 8) as u8;
                        try_key(&bytes);
                    }
                }
            }
        }
        Some("png") => {
            let b = fs::read(&args[2]).expect("read");
            fs::create_dir_all(&args[3]).unwrap();
            let stem = std::path::Path::new(&args[2])
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .to_string();
            for k in 0..shiina::s25::count(&b) {
                let Some(f) = shiina::s25::frame(&b, k) else {
                    continue;
                };
                println!("{k} {}x{} at {} {}", f.image.w, f.image.h, f.x, f.y);
                image::save_buffer(
                    format!("{}/{stem}_{k:03}.png", args[3]),
                    &f.image.px,
                    f.image.w,
                    f.image.h,
                    image::ExtendedColorType::Rgba8,
                )
                .unwrap();
            }
        }
        _ => eprintln!("usage: shiina-dump list|extract|png <file> [out]"),
    }
}
