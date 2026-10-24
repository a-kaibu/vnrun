//! Prepares a game folder for the browser player: work that is too heavy (or impossible
//! without threads) in the browser is done once here, natively, and left in the folder.
//!
//!     vnrun-prep <ゲームのフォルダ>
//!
//! SiglusEngine: when Scene.pck / Gameexe.dat need the second-layer EXE key and the folder has
//! no key.toml that opens them, recovers the key from the two files and writes key.toml. The
//! browser build of siglus_rs never recovers keys itself (it only reads key.toml).
//!
//! Prints one line per action and exits non-zero when the folder still cannot be played.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use siglus_key_recovery::KeyStatus;

fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| {
        eprintln!("{}: {e}", path.display());
        exit(1)
    })
}

/// Makes sure a Siglus game folder carries the EXE key the browser needs.
fn siglus(dir: &Path, scene_path: &Path) -> Result<(), String> {
    let Some(game_path) = find(dir, "Gameexe.dat") else {
        println!("siglus: no Gameexe.dat (plain Gameexe.ini needs no key)");
        return Ok(());
    };
    let (game, scene) = (read(&game_path), read(scene_path));
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir] = args.as_slice() else {
        eprintln!("usage: vnrun-prep <game dir>");
        exit(2)
    };
    let dir = Path::new(dir);
    let result = if let Some(scene) = find(dir, "Scene.pck") {
        siglus(dir, &scene)
    } else {
        println!("nothing to prepare");
        Ok(())
    };
    if let Err(e) = result {
        eprintln!("{}: {e}", dir.display());
        exit(1)
    }
}
