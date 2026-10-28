//! Electron games (TyranoScript's Electron build among them) ship their app as resources/app.asar.
//! The browser player needs the files themselves, so the archive is unpacked into the folder
//! that holds it (the game root then has the app's package.json and index.html).
//!
//! Layout: u32 4, u32 header size, then the header pickle (u32 payload size, u32 JSON length,
//! JSON); file data follows the header. The JSON is a tree of {"files": {name: node}}, where a
//! file node has "size" and "offset" (a decimal string, from the end of the header), or
//! "unpacked": true when it lives in app.asar.unpacked/ instead.

use std::fs;
use std::path::{Component, Path};

use serde_json::Value;

fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Writes the files under `node` (a folder of the header tree) below `out`.
fn extract(node: &Value, data: &[u8], unpacked: &Path, out: &Path) -> Result<(), String> {
    let Some(files) = node.get("files").and_then(Value::as_object) else {
        return Ok(());
    };
    for (name, child) in files {
        // Skips names that would leave the folder.
        let mut parts = Path::new(name).components();
        if !matches!(
            (parts.next(), parts.next()),
            (Some(Component::Normal(_)), None)
        ) {
            continue;
        }
        let (path, from) = (out.join(name), unpacked.join(name));
        if child.get("files").is_some() {
            fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            extract(child, data, &from, &path)?;
        } else if child.get("unpacked").and_then(Value::as_bool) == Some(true) {
            // Already a plain file next to the archive.
            if from.is_file() {
                fs::copy(&from, &path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        } else if let (Some(size), Some(offset)) = (
            child.get("size").and_then(Value::as_u64),
            child
                .get("offset")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u64>().ok()),
        ) {
            let bytes = usize::try_from(offset)
                .ok()
                .zip(usize::try_from(size).ok())
                .and_then(|(o, s)| data.get(o..o.checked_add(s)?))
                .ok_or_else(|| format!("{name}: outside the archive"))?;
            fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        // Symbolic links ("link") are left out.
    }
    Ok(())
}

/// Unpacks `dir`/app.asar into `dir` when the folder has no package.json yet; returns whether
/// it did.
pub fn unpack(dir: &Path) -> Result<bool, String> {
    let asar = dir.join("app.asar");
    if !asar.is_file() || dir.join("package.json").exists() {
        return Ok(false);
    }
    let err = |e: &dyn std::fmt::Display| format!("asar: {}: {e}", asar.display());
    let bytes = fs::read(&asar).map_err(|e| err(&e))?;
    let header = (|| {
        let size = u32_at(&bytes, 4)?;
        let len = u32_at(&bytes, 12)?;
        let json = bytes.get(16..16 + len)?;
        Some((8 + size, serde_json::from_slice::<Value>(json).ok()?))
    })()
    .ok_or_else(|| err(&"not an asar archive"))?;
    let (start, tree) = header;
    let data = bytes.get(start..).ok_or_else(|| err(&"truncated"))?;
    extract(&tree, data, &dir.join("app.asar.unpacked"), dir).map_err(|e| err(&e))?;
    Ok(true)
}
