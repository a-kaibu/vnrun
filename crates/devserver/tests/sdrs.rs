//! Verify game registration prepares SDRS before the server starts serving files.

#[path = "../../prep/tests/common/mod.rs"]
mod common;

use std::fs;
use std::process::{Command, Output};

use common::Game;

fn register(game: &Game, engine: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vnrun-devserver"));
    command.args(["--game"]).arg(game.dir.path());
    // A deliberately invalid listen address stops the process after registration, without
    // opening a socket or leaving a long-running server behind.
    command.args(["--addr", "invalid"]);
    if let Some(engine) = engine {
        command.args(["--engine", engine]);
    }
    command.output().unwrap()
}

#[test]
fn registration_prepares_sdrs_before_listening_and_skips_it_on_restart() {
    let game = Game::new();
    fs::write(game.dir.path().join("system.arc"), []).unwrap();
    let output = register(&game, None);
    assert_eq!(output.status.code(), Some(1)); // only the deliberate listen failure
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("listen invalid:"),
        "{output:?}"
    );
    game.assert_extracted(&output);
    let count = fs::read_dir(game.dir.path()).unwrap().count();
    let output = register(&game, None);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("listen invalid:"),
        "{output:?}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("sdrs:"));
    assert_eq!(fs::read_dir(game.dir.path()).unwrap().count(), count);
}

#[test]
fn registration_stops_on_extraction_failure_even_with_an_explicit_engine() {
    let game = Game::new();
    fs::write(game.dir.path().join("data.bin"), b"invalid signature").unwrap();
    let output = register(&game, Some("bgi"));
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("sdrs:"), "{output:?}");
    assert!(!error.contains("listen invalid:"));
    assert_eq!(
        fs::read(game.dir.path().join("シェル.exe")).unwrap(),
        game.shell
    );
    assert_eq!(
        fs::read(game.dir.path().join("data.bin")).unwrap(),
        b"invalid signature"
    );
    assert_eq!(fs::read_dir(game.dir.path()).unwrap().count(), 2);
}
