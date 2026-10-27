//! NOA archives ("Entis\x1a" ERISA-Archive): a tree of "DirEntry" records; each file is a
//! "filedata" record, stored, ERISA-N packed or BSHF encrypted with a per-archive password
//! that the exe lists in its IDR_COTOMI resource.

use nexas::files::Files;

const RAW: u32 = 0;
const ERISA_CODE: u32 = 0x8000_0010;
const BSHF: u32 = 0x4000_0000;

#[derive(Clone, Debug)]
pub struct Entry {
    /// path inside the archive, '/' separated
    pub name: String,
    pub size: u64,
    /// absolute offset of the "filedata" record in the file
    pub offset: u64,
    pub enc: u32,
}

pub struct Noa {
    /// the game file that holds the archive
    pub file: String,
    pub entries: Vec<Entry>,
    pub password: Option<String>,
}

fn u32le(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn u64le(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

impl Noa {
    /// Reads the index of the archive that starts at `base` in `file`.
    pub fn open(files: &mut dyn Files, file: &str, base: u64) -> Result<Noa, String> {
        let head = files
            .read_range(file, base, 0x40)
            .ok_or_else(|| format!("{file}: cannot read"))?;
        if !(head.starts_with(b"Entis\x1a") || head.starts_with(b"VIST\x1a")) {
            return Err(format!("{file}: not a NOA archive"));
        }
        let mut entries = Vec::new();
        walk(files, file, base + 0x40, "", &mut entries, 0)?;
        Ok(Noa {
            file: file.to_string(),
            entries,
            password: None,
        })
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
    }

    pub fn encrypted(&self) -> bool {
        self.entries.iter().any(|e| e.enc == BSHF)
    }

    /// The contents of an entry.
    pub fn read(&self, files: &mut dyn Files, e: &Entry) -> Result<Vec<u8>, String> {
        let head = files
            .read_range(&self.file, e.offset, 16)
            .ok_or_else(|| format!("{}: cannot read", e.name))?;
        let size = u64le(&head, 8).unwrap_or(0);
        if size <= 4 {
            return Ok(Vec::new());
        }
        let raw = files
            .read_range(&self.file, e.offset + 16, size as usize)
            .ok_or_else(|| format!("{}: cannot read", e.name))?;
        match e.enc {
            RAW => Ok(raw),
            ERISA_CODE => Ok(crate::erisa::nemesis(
                &raw[..raw.len() - 4],
                e.size as usize,
            )),
            BSHF => {
                let pass = self
                    .password
                    .as_deref()
                    .ok_or_else(|| format!("{}: encrypted and no password", e.name))?;
                let mut out = crate::erisa::bshf(&raw[..raw.len() - 4], pass);
                out.truncate(e.size as usize);
                Ok(out)
            }
            other => Err(format!("{}: encryption {other:#x} not supported", e.name)),
        }
    }
}

fn walk(
    files: &mut dyn Files,
    file: &str,
    at: u64,
    dir: &str,
    out: &mut Vec<Entry>,
    depth: u32,
) -> Result<(), String> {
    let bad = || format!("{file}: bad directory at {at:#x}");
    let head = files.read_range(file, at, 16).ok_or_else(bad)?;
    if &head[..8] != b"DirEntry" || depth > 16 {
        return Err(bad());
    }
    let size = u64le(&head, 8).ok_or_else(bad)?;
    if size == 0 || size > 1 << 28 {
        return Err(bad());
    }
    let d = files
        .read_range(file, at + 16, size as usize)
        .ok_or_else(bad)?;
    let count = u32le(&d, 0).ok_or_else(bad)?;
    let mut p = 4usize;
    for _ in 0..count {
        let fsize = u64le(&d, p).ok_or_else(bad)?;
        let attr = u32le(&d, p + 8).ok_or_else(bad)?;
        let enc = u32le(&d, p + 12).ok_or_else(bad)?;
        let off = u64le(&d, p + 16).ok_or_else(bad)?;
        p += 32;
        let extra = u32le(&d, p).ok_or_else(bad)? as usize;
        p += 4 + extra;
        let nlen = u32le(&d, p).ok_or_else(bad)? as usize;
        p += 4;
        let raw = d.get(p..p + nlen).ok_or_else(bad)?;
        p += nlen;
        let raw = raw.split(|&c| c == 0).next().unwrap_or(&[]);
        let (name, _, _) = encoding_rs::SHIFT_JIS.decode(raw);
        let name = if dir.is_empty() {
            name.into_owned()
        } else {
            format!("{dir}/{name}")
        };
        match attr {
            0x10 => walk(files, file, at + off + 0x10, &name, out, depth + 1)?,
            0x20 | 0x40 => break,
            _ => out.push(Entry {
                name,
                size: fsize,
                offset: at + off,
                enc,
            }),
        }
    }
    Ok(())
}

/// The archive passwords the exe lists: (archive file name, key) from the ERISA-N packed XML in
/// its IDR_COTOMI resource.
pub fn exe_keys(exe: &[u8]) -> Vec<(String, String)> {
    let Some(res) = crate::pe::resource(exe, "IDR_COTOMI") else {
        return Vec::new();
    };
    let xml = crate::erisa::nemesis(res, 1 << 20);
    let (text, _, _) = encoding_rs::SHIFT_JIS.decode(&xml);
    let mut out = Vec::new();
    for tag in text.split('<').filter(|t| t.starts_with("archive")) {
        if let (Some(p), Some(k)) = (attr(tag, "path"), attr(tag, "key")) {
            let name = p.rsplit(['/', '\\']).next().unwrap_or(&p).to_string();
            out.push((name, k));
        }
    }
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let i = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let j = tag[i..].find('"')?;
    Some(tag[i..i + j].to_string())
}
