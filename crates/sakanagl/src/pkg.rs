//! The package index (`<name>-001(00).sx`, unpacked by [`crate::sx::open`]; parsed as in
//! sakanagl.dll 0x1800eb6c0 and 0x1800eb570) and the storages it points into
//! (`<name>-001-<storage>.sxstorage`).

use std::collections::HashMap;

/// One stored file.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub storage: u16,
    pub flags: u16,
    /// Offset in 16-byte blocks.
    pub block: u32,
    pub size: u32,
}

#[derive(Clone, Debug)]
pub struct Storage {
    /// The storage file's suffix: `<base>-<name>.sxstorage`.
    pub name: String,
    pub blocks: u32,
}

#[derive(Debug, Default)]
pub struct Index {
    pub storages: Vec<Storage>,
    /// Path (no leading slash, as stored) to entry.
    pub files: HashMap<String, Entry>,
    /// Paths in index order.
    pub order: Vec<String>,
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self.b.get(self.p..self.p + n).ok_or("index truncated")?;
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
}

impl Index {
    pub fn parse(b: &[u8]) -> Result<Index, String> {
        let mut r = Rd { b, p: 8 };
        let n = r.u32()? as usize;
        let mut names = Vec::with_capacity(n);
        for _ in 0..n {
            let l = r.u8()? as usize;
            names.push(String::from_utf8_lossy(r.take(l)?).into_owned());
        }
        let n = r.u32()? as usize;
        let mut ents = Vec::with_capacity(n);
        for _ in 0..n {
            ents.push(Entry {
                storage: r.u16()?,
                flags: r.u16()?,
                block: r.u32()?,
                size: r.u32()?,
            });
        }
        let n = r.u16()? as usize;
        let mut storages = Vec::with_capacity(n);
        for _ in 0..n {
            let name = r.u32()? as usize;
            r.take(4)?;
            r.take(4)?;
            let blocks = r.u32()?;
            r.take(8 + 16)?;
            storages.push(Storage {
                name: names.get(name).cloned().unwrap_or_default(),
                blocks,
            });
        }
        let n = r.u16()? as usize;
        r.take(24 * n)?;
        let mut ix = Index {
            storages,
            ..Default::default()
        };
        let mut stack: Vec<(String, u16)> = Vec::new();
        // Each node: u16 children, u32 name, u32 entry (u32::MAX for directories), then the children.
        loop {
            let kids = r.u16()?;
            let name = r.u32()? as usize;
            let ent = r.u32()?;
            let nm = names.get(name).map(String::as_str).unwrap_or("");
            let path = match stack.last() {
                Some((p, _)) if !p.is_empty() => format!("{p}/{nm}"),
                _ => nm.to_string(),
            };
            if kids > 0 {
                stack.push((path, kids));
                continue;
            }
            if let Some(e) = ents.get(ent as usize) {
                ix.order.push(path.clone());
                ix.files.insert(path, *e);
            }
            // Pop finished directories.
            loop {
                match stack.last_mut() {
                    None => return Ok(ix),
                    Some((_, k)) => {
                        *k -= 1;
                        if *k > 0 {
                            break;
                        }
                        stack.pop();
                    }
                }
            }
        }
    }

    /// Case-insensitive lookup with a fallback over `/`-joined paths.
    pub fn find(&self, path: &str) -> Option<(&str, Entry)> {
        let p = path.trim_start_matches('/');
        if let Some((k, e)) = self.files.get_key_value(p) {
            return Some((k.as_str(), *e));
        }
        let l = p.to_ascii_lowercase();
        self.files
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == l)
            .map(|(k, e)| (k.as_str(), *e))
    }
}
