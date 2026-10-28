//! WFL1 archives (`.arc`), with their index alone in a `.ari` file next to them. An entry is an
//! i32 name length, the name (cp932, every byte inverted, with a leading "\"), a u16 mode and
//! a u32 size; mode 1 adds the unpacked size and its data is LZSS packed. The `.ari` holds the
//! same headers without the data, so the data offsets add up from 4.

use std::collections::HashMap;

use nexas::files::Files;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub offset: u64,
    pub size: usize,
    /// LZSS packed: its unpacked size is the u32 just before `offset`
    pub packed: bool,
}

pub struct Arc {
    pub file: String,
    pub entries: Vec<Entry>,
    by_name: HashMap<String, usize>,
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Strips the extension and folds the case: the names the scripts use.
pub fn key(name: &str) -> String {
    let base = name.rsplit_once('.').map_or(name, |(b, _)| b);
    base.to_lowercase()
}

impl Arc {
    /// Opens `file` (an `.arc`) with the index of the `.ari` next to it, or by walking it.
    pub fn open(files: &mut dyn Files, file: &str) -> Result<Arc, String> {
        let head = files.read_range(file, 0, 4).ok_or("short file")?;
        if head != b"WFL1" {
            return Err(format!("{file}: not a WFL1 archive"));
        }
        let ari = format!("{}.ari", &file[..file.len() - 4]);
        let mut entries = Vec::new();
        let parse = |b: &[u8], at: &mut usize, data: &mut u64, from_ari: bool| -> Option<Entry> {
            let n = u32le(b, *at)? as usize;
            let raw = b.get(*at + 4..*at + 4 + n)?;
            let raw: Vec<u8> = raw.iter().map(|x| !x).collect();
            let name = encoding_rs::SHIFT_JIS.decode(&raw).0;
            let name = name.trim_end_matches('\0').trim_start_matches('\\');
            let name = name.rsplit('\\').next().unwrap_or(name).to_string();
            *at += 4 + n;
            let mode = u16::from_le_bytes(b.get(*at..*at + 2)?.try_into().ok()?);
            let size = u32le(b, *at + 2)? as usize;
            *at += 6;
            *data += 4 + n as u64 + 6;
            let packed = mode == 1;
            if packed {
                // the unpacked size is in the .arc, not in the .ari
                if !from_ari {
                    *at += 4;
                }
                *data += 4;
            }
            let e = Entry {
                name,
                offset: *data,
                size,
                packed,
            };
            *data += size as u64;
            Some(e)
        };
        let mut data = 4u64;
        if let Some(b) = files.read(&ari) {
            let mut at = 0;
            while at < b.len() {
                match parse(&b, &mut at, &mut data, true) {
                    Some(e) => entries.push(e),
                    None => break,
                }
            }
        } else {
            let total = files.size(file).unwrap_or(0);
            while data + 10 < total {
                let Some(b) = files.read_range(file, data, 4) else {
                    break;
                };
                let n = u32le(&b, 0).unwrap_or(0) as usize;
                if n > 1024 {
                    break;
                }
                let Some(b) = files.read_range(file, data, 4 + n + 10) else {
                    break;
                };
                let mut at = 0;
                let mut d = data;
                match parse(&b, &mut at, &mut d, false) {
                    Some(e) => {
                        data = d;
                        entries.push(e);
                    }
                    None => break,
                }
            }
        }
        let by_name = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (key(&e.name), i))
            .collect();
        Ok(Arc {
            file: file.to_string(),
            entries,
            by_name,
        })
    }

    /// An entry by name, with or without its extension.
    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.by_name.get(&key(name)).map(|&i| &self.entries[i])
    }

    pub fn read(&self, files: &mut dyn Files, e: &Entry) -> Option<Vec<u8>> {
        if !e.packed {
            return files.read_range(&self.file, e.offset, e.size);
        }
        let raw = files.read_range(&self.file, e.offset - 4, e.size + 4)?;
        Some(unpack(&raw[4..], u32le(&raw, 0)? as usize))
    }
}

/// LZSS with the flag bits and fields read MSB first from a bit stream: 1 + 8 bits a byte,
/// 0 + a 12-bit window position + 4 bits of count (+2). The window has 4096 bytes and the
/// first byte goes to position 1.
pub fn unpack(data: &[u8], size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size);
    let mut frame = [0u8; 4096];
    let mut fp = 1usize;
    let (mut bits, mut nb, mut pos) = (0u32, 0u32, 0usize);
    let mut get = |n: u32| -> Option<u32> {
        while nb < n {
            bits = (bits << 8) | *data.get(pos)? as u32;
            pos += 1;
            nb += 8;
        }
        nb -= n;
        Some((bits >> nb) & ((1 << n) - 1))
    };
    while out.len() < size {
        let Some(b) = get(1) else { break };
        if b == 1 {
            let Some(c) = get(8) else { break };
            out.push(c as u8);
            frame[fp] = c as u8;
            fp = (fp + 1) & 4095;
        } else {
            let Some(off) = get(12) else { break };
            if off == 0 {
                break;
            }
            let Some(cnt) = get(4) else { break };
            for i in 0..cnt as usize + 2 {
                let c = frame[(off as usize + i) & 4095];
                out.push(c);
                frame[fp] = c;
                fp = (fp + 1) & 4095;
            }
        }
    }
    out.truncate(size);
    out
}
