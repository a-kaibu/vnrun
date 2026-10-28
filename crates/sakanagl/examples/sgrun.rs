//! sgrun GAME_DIR OUT_DIR STEPS: runs the game natively and writes screenshots.
//! STEPS is comma separated: `wait:N` (frames), `click:X;Y` (screen pixels, 1920x1080), `key`
//! (a click from the keyboard), `shot:NAME`, `cancel`, `skip:N` (frames with skip held).
use std::path::PathBuf;

use sakanagl::assets::Host;
use sakanagl::engine::Engine;
use sakanagl::gfx::{Img, paint};

struct Dir {
    root: PathBuf,
    saves: PathBuf,
}

impl Host for Dir {
    fn size(&mut self, path: &str) -> Option<u64> {
        std::fs::metadata(self.root.join(path))
            .ok()
            .map(|m| m.len())
    }
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(self.root.join(path)).ok()?;
        f.seek(SeekFrom::Start(offset)).ok()?;
        let mut b = vec![0; len];
        let n = f.read(&mut b).ok()?;
        b.truncate(n);
        Some(b)
    }
    fn list(&mut self, dir: &str) -> Vec<String> {
        std::fs::read_dir(self.root.join(dir))
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
    fn read_save(&mut self, name: &str) -> Option<Vec<u8>> {
        std::fs::read(self.saves.join(name)).ok()
    }
    fn write_save(&mut self, name: &str, bytes: &[u8]) {
        let _ = std::fs::create_dir_all(&self.saves);
        let _ = std::fs::write(self.saves.join(name), bytes);
    }
}

fn shot(e: &mut Engine, path: &std::path::Path) {
    let (w, h) = e.screen;
    let mut out = Img::new(w, h);
    for p in out.px.chunks_exact_mut(4) {
        p[3] = 255;
    }
    for d in e.draws() {
        paint(&mut out, &d);
    }
    // half size keeps the files small
    let mut small = image::RgbaImage::new(w / 2, h / 2);
    for y in 0..h / 2 {
        for x in 0..w / 2 {
            let i = (((y * 2) * w + x * 2) * 4) as usize;
            small.put_pixel(
                x,
                y,
                image::Rgba([out.px[i], out.px[i + 1], out.px[i + 2], 255]),
            );
        }
    }
    small.save(path).unwrap();
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(&a[1]);
    let out = PathBuf::from(&a[2]);
    std::fs::create_dir_all(&out).unwrap();
    let host = Dir {
        root: root.clone(),
        saves: out.join("saves"),
    };
    let mut e = Engine::new(Box::new(host)).unwrap();
    for step in a[3].split(',') {
        let (k, v) = step.split_once(':').unwrap_or((step, ""));
        match k {
            "wait" => {
                for _ in 0..v.parse::<u32>().unwrap() {
                    e.frame().unwrap();
                    e.take_sounds();
                }
            }
            "skip" => {
                e.set_skip(true);
                for _ in 0..v.parse::<u32>().unwrap() {
                    e.set_skip(true);
                    e.frame().unwrap();
                    e.take_sounds();
                }
                e.set_skip(false);
            }
            "click" => {
                let (x, y) = v.split_once(';').unwrap();
                let (x, y) = (x.parse().unwrap(), y.parse().unwrap());
                e.draws();
                e.point(x, y);
                e.click(x, y);
            }
            "key" => {
                e.draws();
                e.click(-1, -1);
            }
            "cancel" => e.cancel(),
            "dump" => std::fs::write(out.join(format!("{v}.txt")), e.dump()).unwrap(),
            "shot" => shot(&mut e, &out.join(format!("{v}.png"))),
            _ => panic!("step {step}"),
        }
    }
    for w in &e.warnings {
        println!("warn: {w}");
    }
}
