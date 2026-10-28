//! The archive passphrase in cs2.exe's resources (after GARbro's IntOpener.GetPassFromExe):
//! resource V_CODE2/DATA is Blowfish-encrypted with the key in KEY_CODE/KEY (each byte XOR 0xCD),
//! or "windmill" when there is none.

use blowfish::cipher::KeyInit;

/// A resource of a PE file by type and name (both named, upper case), first language.
pub fn resource(pe: &[u8], kind: &str, name: &str) -> Option<Vec<u8>> {
    let u16_ = |at: usize| Some(u16::from_le_bytes(pe.get(at..at + 2)?.try_into().ok()?));
    let u32_ = |at: usize| Some(u32::from_le_bytes(pe.get(at..at + 4)?.try_into().ok()?));
    let nt = u32_(0x3c)? as usize;
    if pe.get(nt..nt + 4)? != b"PE\0\0" {
        return None;
    }
    let sections = u16_(nt + 6)? as usize;
    let opt = nt + 24;
    let opt_size = u16_(nt + 20)? as usize;
    let dirs = match u16_(opt)? {
        0x10b => opt + 96,
        0x20b => opt + 112,
        _ => return None,
    };
    let rsrc_rva = u32_(dirs + 2 * 8)?;
    if rsrc_rva == 0 {
        return None;
    }
    // RVA -> file offset
    let table = opt + opt_size;
    let off = |rva: u32| -> Option<usize> {
        (0..sections).find_map(|i| {
            let s = table + i * 40;
            let (va, vsize, raw) = (
                u32_(s + 12)?,
                u32_(s + 8)?.max(u32_(s + 16)?),
                u32_(s + 20)?,
            );
            (rva >= va && rva < va + vsize).then(|| (rva - va + raw) as usize)
        })
    };
    let root = off(rsrc_rva)?;
    // the entry of a directory with this name (or the first one when `want` is None)
    let find = |dir: usize, want: Option<&str>| -> Option<u32> {
        let n = u16_(dir + 12)? as usize + u16_(dir + 14)? as usize;
        (0..n).find_map(|i| {
            let e = dir + 16 + i * 8;
            let (id, target) = (u32_(e)?, u32_(e + 4)?);
            let Some(want) = want else {
                return Some(target);
            };
            if id & 0x8000_0000 == 0 {
                return None;
            }
            let at = root + (id & 0x7fff_ffff) as usize;
            let len = u16_(at)? as usize;
            let s: Vec<u16> = (0..len)
                .map(|k| u16_(at + 2 + k * 2))
                .collect::<Option<_>>()?;
            String::from_utf16_lossy(&s)
                .eq_ignore_ascii_case(want)
                .then_some(target)
        })
    };
    let sub = |t: u32| (t & 0x8000_0000 != 0).then(|| root + (t & 0x7fff_ffff) as usize);
    let t = sub(find(root, Some(kind))?)?;
    let n = sub(find(t, Some(name))?)?;
    let leaf = find(n, None)?;
    if leaf & 0x8000_0000 != 0 {
        return None;
    }
    let data = root + leaf as usize;
    let (rva, size) = (u32_(data)?, u32_(data + 4)? as usize);
    let at = off(rva)?;
    pe.get(at..at + size).map(<[u8]>::to_vec)
}

/// The passphrase of the game's archives, from cs2.exe.
pub fn passphrase(pe: &[u8]) -> Option<Vec<u8>> {
    // V_CODE in the older exes (スズノネセブン！, 2008)
    let mut code = resource(pe, "V_CODE2", "DATA").or_else(|| resource(pe, "V_CODE", "DATA"))?;
    if code.len() < 8 {
        return None;
    }
    let key = match resource(pe, "KEY_CODE", "KEY") {
        Some(k) => k.iter().map(|b| b ^ 0xcd).collect(),
        None => b"windmill".to_vec(),
    };
    let bf = blowfish::Blowfish::<byteorder::LE>::new_from_slice(&key).ok()?;
    crate::int::decrypt(&bf, &mut code);
    let len = code.iter().position(|&c| c == 0).unwrap_or(code.len());
    code.truncate(len);
    Some(code)
}
