//! Exercise the actual prep binary with synthetic PE/SDRS data, without game assets.

mod common;

use std::fs;
use std::process::{Command, Output};

use common::Game;

impl Game {
    fn prep(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_vnrun-prep"))
            .current_dir(self.dir.path())
            .args(args)
            .output()
            .unwrap()
    }
}

#[test]
fn extracts_bare_unicode_filename_and_continues_siglus_preparation() {
    let game = Game::new();
    fs::write(game.dir.path().join("SCENE.PCK"), []).unwrap();
    let output = game.prep(&["--sdrs", "シェル.exe"]);
    assert!(output.status.success(), "{output:?}");
    game.assert_extracted(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("siglus: no Gameexe.dat"));
}

#[test]
fn extraction_failure_preserves_originals_and_stops_preparation() {
    let game = Game::new();
    fs::write(game.dir.path().join("data.bin"), b"invalid signature").unwrap();
    fs::write(game.dir.path().join("Scene.pck"), []).unwrap();
    let output = game.prep(&["--sdrs", "シェル.exe"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("sdrs:"));
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read(game.dir.path().join("シェル.exe")).unwrap(),
        game.shell
    );
    assert_eq!(
        fs::read(game.dir.path().join("data.bin")).unwrap(),
        b"invalid signature"
    );
    assert_eq!(fs::read_dir(game.dir.path()).unwrap().count(), 3);
}

#[test]
fn siglus_failure_keeps_extraction_and_reports_backup() {
    let game = Game::new();
    fs::write(game.dir.path().join("Scene.pck"), []).unwrap();
    fs::write(game.dir.path().join("Gameexe.dat"), []).unwrap();
    let output = game.prep(&["--sdrs", "シェル.exe"]);
    assert_eq!(output.status.code(), Some(1));
    game.assert_extracted(&output);
}

#[test]
fn directory_mode_detects_sdrs_and_is_idempotent() {
    let game = Game::new();
    // Windows file extensions are case insensitive, even when hosted on Linux.
    fs::rename(
        game.dir.path().join("シェル.exe"),
        game.dir.path().join("シェル.EXE"),
    )
    .unwrap();
    let output = game.prep(&["."]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read(game.dir.path().join("game.exe")).unwrap(),
        game.exe
    );
    assert_eq!(
        fs::read(game.dir.path().join("data.bin")).unwrap(),
        game.plain
    );
    let count = fs::read_dir(game.dir.path()).unwrap().count();
    let output = game.prep(&["."]);
    assert!(output.status.success(), "{output:?}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("sdrs:"));
    assert_eq!(fs::read_dir(game.dir.path()).unwrap().count(), count);
}

#[test]
fn invalid_input_and_arguments_fail_without_writes() {
    let game = Game::new();
    for args in [
        vec![],
        vec!["--sdrs"],
        vec!["--unknown"],
        vec!["--sdrs", "シェル.exe", "extra"],
    ] {
        assert_eq!(game.prep(&args).status.code(), Some(2));
    }
    for args in [
        vec!["--sdrs", "missing.exe"],
        vec!["--sdrs", "data.bin"],
        vec!["missing-dir"],
    ] {
        assert_eq!(game.prep(&args).status.code(), Some(1));
    }
    assert_eq!(fs::read_dir(game.dir.path()).unwrap().count(), 2);
    assert_eq!(
        fs::read(game.dir.path().join("シェル.exe")).unwrap(),
        game.shell
    );
    assert_eq!(
        fs::read(game.dir.path().join("data.bin")).unwrap(),
        game.encrypted
    );
}

#[test]
fn auto_detection_ignores_unrelated_executables_and_backups() {
    let game = Game::new();
    // The previously prepared game's EXE has no SDRS overlay.
    fs::write(game.dir.path().join("シェル.exe"), &game.exe).unwrap();
    fs::write(game.dir.path().join("uninstall.exe"), b"not a PE").unwrap();
    let backup = game.dir.path().join("sdrs_backup_old");
    fs::create_dir(&backup).unwrap();
    fs::write(backup.join("シェル.exe"), &game.shell).unwrap();
    let output = game.prep(&["."]);
    assert!(output.status.success(), "{output:?}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("sdrs:"));
    assert_eq!(
        fs::read(game.dir.path().join("data.bin")).unwrap(),
        game.encrypted
    );
    assert_eq!(fs::read(backup.join("シェル.exe")).unwrap(), game.shell);
    assert_eq!(fs::read_dir(game.dir.path()).unwrap().count(), 4);
}

#[test]
fn auto_detection_reports_damaged_and_unsupported_sdrs_shells() {
    for offset in [0xC00 + 3, 0xC00 + 4] {
        let game = Game::new();
        let mut shell = game.shell.clone();
        shell[offset] = 0xFF; // unknown version, or mismatched config checksum
        fs::write(game.dir.path().join("シェル.exe"), &shell).unwrap();
        let output = game.prep(&["."]);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("sdrs:"));
        assert_eq!(fs::read(game.dir.path().join("シェル.exe")).unwrap(), shell);
        assert_eq!(
            fs::read(game.dir.path().join("data.bin")).unwrap(),
            game.encrypted
        );
        assert_eq!(fs::read_dir(game.dir.path()).unwrap().count(), 2);
    }
}
