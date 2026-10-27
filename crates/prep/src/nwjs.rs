//! NW.js games can ship their app as a zip appended to nw.exe ("copy /b nw.exe+app.nw"). The
//! browser player needs the files themselves, so the zip is unpacked next to the EXE.
//! TyranoScript games can also ship an update patch (<name>.tpatch, a zip of changed files) next
//! to the EXE, which the game applies over itself at startup with Node; it is applied here.

use std::fs::{self, File};
use std::io::{self, Cursor, Read, Seek};
use std::path::Path;

use zip::ZipArchive;

/// Where the zip at the end of `bytes` starts: its central directory offsets count from there,
/// so they are off by the size of the EXE in front of it.
fn zip_start(bytes: &[u8]) -> Option<usize> {
    let from = bytes.len().saturating_sub(22 + 0xffff);
    let eocd = from + bytes[from..].windows(4).rposition(|w| w == b"PK\x05\x06")?;
    let field = |at: usize| {
        let b = bytes.get(eocd + at..eocd + at + 4)?;
        Some(u32::from_le_bytes(b.try_into().ok()?) as usize)
    };
    let (size, offset) = (field(12)?, field(16)?);
    eocd.checked_sub(size)?.checked_sub(offset)
}

/// Writes every entry of `zip` under `dir`.
fn extract<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    dir: &Path,
    err: impl Fn(&dyn std::fmt::Display) -> String,
) -> Result<(), String> {
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| err(&e))?;
        // Skips names that would leave the folder.
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let out = dir.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| err(&e))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| err(&e))?;
        }
        let mut w = File::create(&out).map_err(|e| err(&e))?;
        io::copy(&mut entry, &mut w).map_err(|e| err(&e))?;
    }
    Ok(())
}

/// Unpacks the app zip in `exe` into `dir` when it holds a package.json; returns whether it did.
pub fn unpack(exe: &Path, dir: &Path) -> Result<bool, String> {
    let err = |e: &dyn std::fmt::Display| format!("nwjs: {}: {e}", exe.display());
    let bytes = fs::read(exe).map_err(|e| err(&e))?;
    // A plain EXE has no zip at its end; that is not an error.
    let Some(start) = zip_start(&bytes) else {
        return Ok(false);
    };
    let Ok(mut zip) = ZipArchive::new(Cursor::new(&bytes[start..])) else {
        return Ok(false);
    };
    if zip.index_for_name("package.json").is_none() {
        return Ok(false);
    }
    extract(&mut zip, dir, err)?;
    Ok(true)
}

/// Applies each TyranoScript update patch (*.tpatch) in `dir` over the game, then renames it to
/// *.tpatch.applied so that neither this nor the game applies it again.
pub fn apply_patches(dir: &Path) -> Result<(), String> {
    let mut patches: Vec<_> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("tpatch"))
        })
        .collect();
    patches.sort();
    for patch in patches {
        let err = |e: &dyn std::fmt::Display| format!("tpatch: {}: {e}", patch.display());
        let file = File::open(&patch).map_err(|e| err(&e))?;
        let mut zip = ZipArchive::new(file).map_err(|e| err(&e))?;
        extract(&mut zip, dir, err)?;
        let mut done = patch.clone().into_os_string();
        done.push(".applied");
        fs::rename(&patch, done).map_err(|e| err(&e))?;
        println!("tpatch: applied {}", patch.display());
    }
    Ok(())
}
