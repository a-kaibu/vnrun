//! NW.js games can ship their app as a zip appended to nw.exe ("copy /b nw.exe+app.nw"). The
//! browser player needs the files themselves, so the zip is unpacked next to the EXE.

use std::fs::{self, File};
use std::io;
use std::path::Path;

use zip::ZipArchive;

/// Unpacks the app zip in `exe` into `dir` when it holds a package.json; returns whether it did.
pub fn unpack(exe: &Path, dir: &Path) -> Result<bool, String> {
    let err = |e: &dyn std::fmt::Display| format!("nwjs: {}: {e}", exe.display());
    let file = File::open(exe).map_err(|e| err(&e))?;
    // A plain EXE has no central directory; that is not an error.
    let Ok(mut zip) = ZipArchive::new(file) else {
        return Ok(false);
    };
    if zip.index_for_name("package.json").is_none() {
        return Ok(false);
    }
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
    Ok(true)
}
