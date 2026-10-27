//! The game folder: GameData/*.pack, and the key file some archives need.

use nexas::files::Files;

/// key.fkey (next to the archives, the game or in DLL), or the RESKEY resource of the exe.
pub fn key_file(files: &mut dyn Files) -> Option<Vec<u8>> {
    for p in ["GameData/key.fkey", "key.fkey", "DLL/key.fkey"] {
        if let Some(b) = files.read(p) {
            return Some(b);
        }
    }
    let exes: Vec<String> = files
        .list()
        .into_iter()
        .filter(|n| n.to_ascii_lowercase().ends_with(".exe"))
        .collect();
    for exe in exes {
        if let Some(pe) = files.read(&exe)
            && let Some(k) = crate::pe::resource(&pe, "#10", "RESKEY")
        {
            return Some(k);
        }
    }
    None
}
