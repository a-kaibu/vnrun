//! PNA: a layered picture, like a Photoshop file kept apart. Character sprites keep their poses
//! and faces in one, the interface its buttons in each state.
//!
//! ```text
//! "PNAP" ?:u32 w:u32 h:u32 n:u32  n x entry  PNG data of each picture entry, in order
//! entry (0x28 bytes) = kind:u32 id:i32 x:i32 y:i32 w:u32 h:u32 ?:u32 opacity:f64 size:u32
//! ```
//!
//! Kind 0 is a picture, 1 and 2 open and close a group. Scripts and Lua name the parts by their
//! entry number (groups count too); the last entry is the bottom, so parts are drawn from the
//! last up. The PNG data is premultiplied by alpha.

use nexas::gfx::Image;
use std::cell::OnceCell;
use std::rc::Rc;

pub struct Part {
    /// the entry number
    pub id: i32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    data: std::ops::Range<usize>,
    image: OnceCell<Option<Rc<Image>>>,
}

pub struct Pna {
    pub w: i32,
    pub h: i32,
    /// the pictures, bottom first
    pub parts: Vec<Part>,
    bytes: Vec<u8>,
}

impl Pna {
    pub fn parse(bytes: Vec<u8>) -> Result<Pna, String> {
        if !bytes.starts_with(b"PNAP") {
            return Err("not a PNA picture".into());
        }
        let u32_at = |i: usize| -> Result<u32, String> {
            Ok(u32::from_le_bytes(bytes.get(i..i + 4).ok_or("PNA is cut short")?.try_into().unwrap()))
        };
        let (w, h, n) = (u32_at(8)? as i32, u32_at(12)? as i32, u32_at(16)? as usize);
        let mut at = 0x14 + n * 0x28;
        let mut parts = Vec::new();
        for i in 0..n {
            let e = 0x14 + i * 0x28;
            let size = u32_at(e + 0x24)? as usize;
            let picture = u32_at(e)? == 0 && size > 0;
            parts.push(Part {
                id: i as i32,
                x: u32_at(e + 8)? as i32,
                y: u32_at(e + 12)? as i32,
                w: u32_at(e + 16)? as i32,
                h: u32_at(e + 20)? as i32,
                data: if picture { at..at + size } else { 0..0 },
                image: OnceCell::new(),
            });
            at += size;
        }
        if at > bytes.len() {
            return Err("PNA is cut short".into());
        }
        parts.reverse();
        Ok(Pna { w, h, parts, bytes })
    }

    pub fn part(&self, id: i32) -> Option<&Part> {
        self.parts.iter().find(|p| p.id == id)
    }

    /// The picture of a part, decoded the first time it is asked for.
    pub fn image(&self, part: &Part) -> Option<Rc<Image>> {
        if part.data.is_empty() {
            return None;
        }
        part.image
            .get_or_init(|| match Image::decode(&self.bytes[part.data.clone()]) {
                Ok(mut img) => {
                    unpremultiply(&mut img);
                    Some(Rc::new(img))
                }
                Err(e) => {
                    eprintln!("advhd: PNA part {}: {e}", part.id);
                    None
                }
            })
            .clone()
    }
}

fn unpremultiply(img: &mut Image) {
    for p in img.px.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = (*c as u32 * 255 / a).min(255) as u8;
            }
        }
    }
}
