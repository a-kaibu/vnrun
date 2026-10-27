//! Just enough of the PE format to find a named resource in the game's exe.

fn u16le(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn u32le(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// The data of the first resource named `name` (of any type and language).
pub fn resource<'a>(exe: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let pe = u32le(exe, 0x3c)? as usize;
    if exe.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let nsec = u16le(exe, pe + 6)? as usize;
    let opt = pe + 24;
    let optsize = u16le(exe, pe + 20)? as usize;
    let magic = u16le(exe, opt)?;
    let dirs = opt + if magic == 0x20b { 112 } else { 96 };
    let rsrc_rva = u32le(exe, dirs + 2 * 8)?;
    let secs = opt + optsize;
    let to_off = |rva: u32| -> Option<usize> {
        for i in 0..nsec {
            let s = secs + i * 40;
            let va = u32le(exe, s + 12)?;
            let vsize = u32le(exe, s + 8)?.max(u32le(exe, s + 16)?);
            if rva >= va && rva < va + vsize {
                return Some((rva - va + u32le(exe, s + 20)?) as usize);
            }
        }
        None
    };
    let root = to_off(rsrc_rva)?;
    let entries = |dir: usize| -> Option<Vec<(u32, u32)>> {
        let n = u16le(exe, dir + 12)? as usize + u16le(exe, dir + 14)? as usize;
        (0..n)
            .map(|i| Some((u32le(exe, dir + 16 + i * 8)?, u32le(exe, dir + 20 + i * 8)?)))
            .collect()
    };
    let name_of = |id: u32| -> Option<String> {
        if id & 0x8000_0000 == 0 {
            return None;
        }
        let o = root + (id & 0x7fff_ffff) as usize;
        let n = u16le(exe, o)? as usize;
        let u: Vec<u16> = (0..n).filter_map(|i| u16le(exe, o + 2 + i * 2)).collect();
        Some(String::from_utf16_lossy(&u))
    };
    for (_, tdir) in entries(root)? {
        if tdir & 0x8000_0000 == 0 {
            continue;
        }
        let tdir = root + (tdir & 0x7fff_ffff) as usize;
        for (nid, ndir) in entries(tdir)? {
            if name_of(nid).as_deref() != Some(name) || ndir & 0x8000_0000 == 0 {
                continue;
            }
            let ldir = root + (ndir & 0x7fff_ffff) as usize;
            let (_, leaf) = *entries(ldir)?.first()?;
            let leaf = root + leaf as usize;
            let rva = u32le(exe, leaf)?;
            let size = u32le(exe, leaf + 4)? as usize;
            let o = to_off(rva)?;
            return exe.get(o..o + size);
        }
    }
    None
}
