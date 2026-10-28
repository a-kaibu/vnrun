//! Native game preparation shared by the CLI and game server.
//! SDRS shells are extracted before embedded cxdec and Siglus key preparation.

mod asar;
mod krkr;
mod nwjs;

use std::fs;
use std::path::{Path, PathBuf};

use sdrs_core::pe::{Bits, PEFile};
use sdrs_core::sdrs::program::SdrsProgram;
use siglus_key_recovery::KeyStatus;

fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Makes sure a Siglus game folder carries the EXE key the browser needs.
fn siglus(dir: &Path, scene_path: &Path) -> Result<(), String> {
    let Some(game_path) = find(dir, "Gameexe.dat") else {
        println!("siglus: no Gameexe.dat (plain Gameexe.ini needs no key)");
        return Ok(());
    };
    let (game, scene) = (read(&game_path)?, read(scene_path)?);
    if !siglus_key_recovery::resources_require_exe_key(&game, &scene)? {
        println!("siglus: no EXE key needed");
        return Ok(());
    }
    let configured = siglus_assets::key_toml::load_key16_from_project_dir(dir)
        .map_err(|e| format!("key.toml: {e:#}"))?;
    if let Some(key) = configured {
        match siglus_key_recovery::check_key(&game, &scene, &key)? {
            KeyStatus::Accepted | KeyStatus::Unverifiable => {
                println!("siglus: key.toml already opens the game");
                return Ok(());
            }
            KeyStatus::Mismatch => println!("siglus: key.toml does not open the game; recovering"),
        }
    }
    if !siglus_key_recovery::scene_pack_is_compressed(&scene)? {
        return Err("Scene.pck is uncompressed; the EXE key cannot be recovered from it".into());
    }
    let key = siglus_key_recovery::recover_key_from_resources(&game, &scene)?;
    let path = siglus_assets::key_toml::write_key16_to_project_dir(dir, key)
        .map_err(|e| format!("write key.toml: {e:#}"))?;
    println!("siglus: recovered the EXE key → {}", path.display());
    Ok(())
}

/// Extracts SDRS shells, prepares embedded cxdec parameters, then recovers Siglus keys.
pub fn prepare(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err("not a game directory".into());
    }
    let mut executables = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            executables.push(entry.path());
        }
    }
    executables.sort();
    if asar::unpack(dir)? {
        println!("asar: unpacked app.asar in {}", dir.display());
    }
    if find(dir, "package.json").is_none() {
        for path in &executables {
            if nwjs::unpack(path, dir)? {
                println!("nwjs: unpacked the app in {}", path.display());
                break;
            }
        }
    }
    nwjs::apply_patches(dir)?;
    for path in &executables {
        let bytes = read(path)?;
        // Read the PE overlay signature before loading the SDRS config. This distinguishes
        // unrelated EXEs from damaged SDRS shells, whose load errors must stop preparation.
        if PEFile::load(Bits::X32, &bytes).is_some_and(|pe| {
            bytes
                .get(pe.overlay_data_file_offset() as usize..)
                .is_some_and(|overlay| overlay.starts_with(b"SdW"))
        }) {
            extract_sdrs(path)?;
        }
    }
    let mut cxdec = None;
    // SDRS may move the shell and introduce a differently named executable.
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !entry.file_type().map_err(|e| e.to_string())?.is_file()
            || !path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("exe"))
        {
            continue;
        }
        let bytes = read(&path)?;
        if let Some(profile) = krkr::recover(&bytes) {
            if cxdec.as_ref().is_some_and(|old| old != &profile) {
                return Err("krkr: multiple incompatible cxdec profiles in this folder".into());
            }
            cxdec = Some(profile);
        }
    }
    if let Some(profile) = cxdec {
        let path = dir.join("vnrun-cxdec.bin");
        if fs::read(&path).ok().as_ref() != Some(&profile) {
            fs::write(&path, profile).map_err(|e| format!("krkr: write cxdec parameters: {e}"))?;
        }
        println!(
            "krkr: prepared embedded cxdec parameters → {}",
            path.display()
        );
    }
    prepare_siglus(dir)
}

fn prepare_siglus(dir: &Path) -> Result<(), String> {
    if let Some(scene) = find(dir, "Scene.pck") {
        siglus(dir, &scene)
    } else {
        println!("siglus: no Scene.pck; no key preparation needed");
        Ok(())
    }
}

fn extract_sdrs(path: &Path) -> Result<PathBuf, String> {
    // An absolute path also gives a usable parent for a bare filename such as game.exe.
    let path = path.canonicalize().map_err(|e| format!("sdrs: {e}"))?;
    let mut program = SdrsProgram::default();
    program.load(&path).map_err(|e| format!("sdrs: {e}"))?;
    let dir = path.parent().expect("loaded executable has a parent");
    let backup = program.extract().map_err(|e| format!("sdrs: {e}"))?;
    println!(
        "sdrs: extracted → {}",
        dir.join(program.extracted_file_name()).display()
    );
    // Report this before further preparation: its failure does not undo SDRS extraction.
    println!("sdrs: originals backed up → {}", backup.display());
    Ok(dir.to_path_buf())
}

/// Explicitly extracts an SDRS shell, then prepares the surrounding game folder.
pub fn sdrs(path: &Path) -> Result<(), String> {
    let dir = extract_sdrs(path)?;
    prepare(&dir)
}
