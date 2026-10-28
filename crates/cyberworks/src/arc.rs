//! The archives: `ArcNN.dat` pairs of an index (Arc01, Arc02, Arc03, Arc07) and the data it
//! points into (Arc04, Arc05, Arc06, Arc08). An index is LZSS packed behind two eight digit
//! decimal sizes (each digit stored as `d ^ 0x7f`); its entries are numbered resources with a
//! two letter type ("a0" scripts, "b0" pictures, "k0" sounds, ...). The layout follows GARbro
//! (morkt, MIT), ArcDAT.cs.

use std::collections::HashMap;

use nexas::files::Files;

/// The LZSS of the archives: a 4 KiB window starting at 0xfee, flag bits low first, 1 = a byte.
pub fn lzss(d: &[u8], size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size);
    let mut win = [0u8; 4096];
    let mut wp = 0xfee;
    let mut i = 0;
    while out.len() < size && i < d.len() {
        let flags = d[i];
        i += 1;
        for b in 0..8 {
            if out.len() >= size || i >= d.len() {
                break;
            }
            if flags >> b & 1 == 1 {
                let c = d[i];
                i += 1;
                out.push(c);
                win[wp] = c;
                wp = (wp + 1) & 0xfff;
            } else {
                if i + 1 >= d.len() {
                    i = d.len();
                    break;
                }
                let (lo, hi) = (d[i] as usize, d[i + 1] as usize);
                i += 2;
                let off = lo | (hi & 0xf0) << 4;
                for k in 0..(hi & 0xf) + 3 {
                    let c = win[(off + k) & 0xfff];
                    out.push(c);
                    win[wp] = c;
                    wp = (wp + 1) & 0xfff;
                }
            }
        }
    }
    out.truncate(size);
    out
}

/// The digits, most significant first (`rev`: last), 0xff for a leading zero.
fn decimal(b: &[u8], rev: bool) -> usize {
    let digit = |&x: &u8| if x == 0xff { 0 } else { (x ^ 0x7f) as usize };
    if rev {
        b.iter().rev().fold(0, |v, x| v * 10 + digit(x))
    } else {
        b.iter().fold(0, |v, x| v * 10 + digit(x))
    }
}

/// An index file unpacked (`digits` per size: 8, or 4 in the oldest games).
pub fn unpack_toc(b: &[u8], digits: usize) -> Option<Vec<u8>> {
    // the order of the digits differs by game: the packed size names the rest of the file
    let rest = b.len().checked_sub(2 * digits)?;
    let ps = b.get(digits..2 * digits)?;
    let rev = [true, false]
        .into_iter()
        .find(|&r| decimal(ps, r) == rest)?;
    let us = decimal(b.get(..digits)?, rev);
    if us <= 4 || us > 0x100_0000 {
        return None;
    }
    Some(lzss(&b[2 * digits..], us))
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: u32,
    pub kind: [u8; 2],
    pub offset: u64,
    pub packed: usize,
    pub size: usize,
    /// which of the data files (Arc05, Arc05a, ...) it is in
    pub part: u8,
}

fn u32at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The entries of an unpacked index. The games store id, size, then offset and packed size in
/// either order; the order whose offsets follow on (allowing small gaps) is taken.
type RawEntry = (u32, u32, u32, u32, [u8; 2], u8);

pub fn entries(t: &[u8]) -> Vec<Entry> {
    let mut raw = Vec::new();
    let mut at = 0;
    while let Some(n) = u32at(t, at) {
        let n = n as usize;
        if n < 0x11 || at + 4 + n > t.len() {
            break;
        }
        let r = &t[at + 4..at + 4 + n];
        let v = |k| u32at(r, k).unwrap_or(0);
        let part = if n >= 0x17 { r[22] } else { 0 };
        raw.push((v(0), v(4), v(8), v(12), [r[16], r[17]], part));
        at += 4 + n;
    }
    // (id, size, a, b): a is the offset when the next a is a + b
    let follows = |off: fn(&RawEntry) -> (u32, u32)| {
        raw.windows(2)
            .filter(|w| w[0].5 == w[1].5)
            .filter(|w| {
                // the next one starts where this ends (or a little after: the newer games
                // put a RIFF header between the sounds)
                let (o, p) = off(&w[0]);
                let (end, next) = (o.wrapping_add(p), off(&w[1]).0);
                next >= end && next - end <= 64
            })
            .count()
    };
    let a_first = follows(|e| (e.2, e.3)) >= follows(|e| (e.3, e.2));
    raw.into_iter()
        .map(|(id, size, a, b, kind, part)| {
            let (offset, packed) = if a_first { (a, b) } else { (b, a) };
            Entry {
                id,
                kind,
                offset: offset as u64,
                packed: packed as usize,
                size: size as usize,
                part,
            }
        })
        .collect()
}

/// The index files and the data files they point into.
const PAIRS: [(&str, &str); 4] = [
    ("Arc01", "Arc04"),
    ("Arc02", "Arc05"),
    ("Arc03", "Arc06"),
    ("Arc07", "Arc08"),
];

/// Every entry of the game by type letter and id.
pub struct Archives {
    /// file names as listed, by lower case
    names: HashMap<String, String>,
    by_kind: HashMap<u8, Vec<(Entry, String)>>,
    pub title: String,
}

impl Archives {
    pub fn open(files: &mut dyn Files) -> Result<Archives, String> {
        let names: HashMap<String, String> = files
            .list()
            .into_iter()
            .map(|n| (n.to_lowercase(), n))
            .collect();
        let mut by_kind: HashMap<u8, Vec<(Entry, String)>> = HashMap::new();
        for (toc, data) in PAIRS {
            let Some(tn) = names.get(&format!("{}.dat", toc.to_lowercase())) else {
                continue;
            };
            let b = files.read(tn).ok_or_else(|| format!("{tn}: unreadable"))?;
            let t = unpack_toc(&b, 8).ok_or_else(|| format!("{tn}: not an index"))?;
            for e in entries(&t) {
                // Arc05.dat, then Arc05a.dat, Arc05b.dat, ... for parts 1, 2, ...
                let base = data.to_lowercase();
                let file = if e.part == 0 {
                    format!("{base}.dat")
                } else {
                    format!("{base}{}.dat", (b'a' + e.part - 1) as char)
                };
                let Some(file) = names.get(&file) else {
                    continue;
                };
                by_kind
                    .entry(e.kind[0])
                    .or_default()
                    .push((e, file.clone()));
            }
        }
        if by_kind.is_empty() {
            return Err("no Cyberworks archives (Arc01.dat ...)".into());
        }
        let mut title = String::new();
        if let Some(n) = names.get("arc00.dat")
            && let Some(b) = files.read(n)
            && let Some(t) = unpack_toc(&b, 8)
            && let Some(len) = u32at(&t, 0)
            && let Some(s) = t.get(4..4 + len as usize)
        {
            title = encoding_rs::SHIFT_JIS.decode(s).0.into_owned();
        }
        Ok(Archives {
            names,
            by_kind,
            title,
        })
    }

    pub fn has_file(&self, name: &str) -> bool {
        self.names.contains_key(&name.to_lowercase())
    }

    /// The entries of a type ('a' scripts, 'b' pictures, ...) in index order.
    pub fn of(&self, kind: u8) -> impl Iterator<Item = &Entry> {
        self.by_kind
            .get(&kind)
            .into_iter()
            .flatten()
            .map(|(e, _)| e)
    }

    pub fn find(&self, kind: u8, id: u32) -> Option<&Entry> {
        self.of(kind).find(|e| e.id == id)
    }

    pub fn read(&self, files: &mut dyn Files, e: &Entry) -> Option<Vec<u8>> {
        let (_, file) = self
            .by_kind
            .get(&e.kind[0])?
            .iter()
            .find(|(x, _)| x.id == e.id && x.offset == e.offset)?;
        let raw = files.read_range(file, e.offset, e.packed)?;
        Some(if e.packed != e.size {
            lzss(&raw, e.size)
        } else {
            raw
        })
    }
}

/// The pictures of the newer games (2023 on): `Resources/Data000.dat` ... beside an index
/// `Resources/Data00.dat`: u32 file count, then for each file three lists, each a u32 count
/// and its items. The first list holds the pictures: (u32 unpacked, u32 stored, offset), the
/// offset a u32 in 2023 and a u64 in 2026 (an item of 12 or 16 bytes; the one that reads the
/// index to its end is taken). The second list is empty in the trials seen; the third holds
/// (u32, u32 n, n x (u32, u32)) groups (not used). The scripts name a picture
/// `(file + 1) * 10000000 + its place in the list`; items are LZSS packed when the sizes
/// differ.
#[derive(Default)]
pub struct DataStore {
    /// (file name, entries)
    files: Vec<(String, Vec<StoreItem>)>,
}

/// A picture's (unpacked, stored, offset) in a data file.
type StoreItem = (u32, u32, u64);

/// The pictures' lists of the index with items of `item` bytes, if that reads it to its end.
type DataLists = Vec<Vec<StoreItem>>;

fn data_lists(b: &[u8], item: usize) -> Option<DataLists> {
    let n = u32at(b, 0)? as usize;
    if n > 1000 {
        return None;
    }
    let mut at = 4;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let c = u32at(b, at)? as usize;
        at += 4;
        if c > 100_000 || at + c * item > b.len() {
            return None;
        }
        let list = (0..c)
            .map(|i| {
                let r = at + i * item;
                let off = if item == 16 {
                    u32at(b, r + 8).unwrap_or(0) as u64
                        | (u32at(b, r + 12).unwrap_or(0) as u64) << 32
                } else {
                    u32at(b, r + 8).unwrap_or(0) as u64
                };
                (u32at(b, r).unwrap_or(0), u32at(b, r + 4).unwrap_or(0), off)
            })
            .collect();
        at += c * item;
        if u32at(b, at)? != 0 {
            return None;
        }
        at += 4;
        let groups = u32at(b, at)? as usize;
        at += 4;
        for _ in 0..groups.min(100_000) {
            let k = u32at(b, at + 4)? as usize;
            at += 8 + 8 * k;
        }
        out.push(list);
    }
    (at == b.len()).then_some(out)
}

impl DataStore {
    pub fn open(files: &mut dyn Files) -> DataStore {
        let names: HashMap<String, String> = files
            .list()
            .into_iter()
            .map(|n| (n.to_lowercase(), n))
            .collect();
        // the folder listing may hold only the top level (then the usual spelling is tried)
        let path = |n: &str| {
            names
                .get(&format!("resources/{n}"))
                .cloned()
                .unwrap_or_else(|| {
                    let mut c = n.chars();
                    let first = c.next().map(|x| x.to_ascii_uppercase());
                    format!("Resources/{}{}", first.unwrap_or('D'), c.as_str())
                })
        };
        let Some(b) = files.read(&path("data00.dat")) else {
            return DataStore::default();
        };
        let Some(lists) = data_lists(&b, 16).or_else(|| data_lists(&b, 12)) else {
            return DataStore::default();
        };
        let files = lists
            .into_iter()
            .enumerate()
            .map(|(f, l)| (path(&format!("data{f:03}.dat")), l))
            .collect();
        DataStore { files }
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// A picture by the number the scripts give it.
    pub fn read(&self, files: &mut dyn Files, id: u32) -> Option<Vec<u8>> {
        let f = (id / 10_000_000).checked_sub(1)? as usize;
        let k = (id % 10_000_000) as usize;
        let (name, list) = self.files.get(f)?;
        let &(u, p, o) = list.get(k)?;
        if p == 0 {
            return None;
        }
        let raw = files.read_range(name, o, p as usize)?;
        // an unpacked size of 0: stored as it is (the JPEG 'b' pictures)
        Some(if p != u && u != 0 {
            lzss(&raw, u as usize)
        } else {
            raw
        })
    }
}
