#[path = "../../prep/tests/common/mod.rs"]
#[allow(dead_code)]
mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn request(addr: &str, method: &str, path: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", body.len()).unwrap();
    s.write_all(body).unwrap();
    let mut bytes = Vec::new();
    s.read_to_end(&mut bytes).unwrap();
    let i = bytes.windows(4).position(|p| p == b"\r\n\r\n").unwrap();
    let status = std::str::from_utf8(&bytes[..i])
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (status, bytes[i + 4..].to_vec())
}

#[test]
fn added_games_are_prepared_and_saves_are_isolated() {
    let tmp = tempfile::tempdir().unwrap();
    let library = tmp.path().join("library");
    let saves = tmp.path().join("saves");
    fs::create_dir(&library).unwrap();
    let first = library.join("one");
    fs::create_dir(&first).unwrap();
    fs::write(first.join("system.arc"), b"first").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    drop(listener);
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_vnrun-devserver"))
            .args(["--library"])
            .arg(&library)
            .args(["--saves"])
            .arg(&saves)
            .args(["--addr", &addr])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while TcpStream::connect(&addr).is_err() {
        assert!(Instant::now() < deadline && server.0.try_wait().unwrap().is_none());
        thread::sleep(Duration::from_millis(20));
    }
    let (status, list) = request(&addr, "GET", "/api/games", b"");
    assert_eq!(status, 200);
    let list: serde_json::Value = serde_json::from_slice(&list).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["engine"], "bgi");

    let fixture = common::Game::new();
    let second = library.join("二");
    fs::create_dir(&second).unwrap();
    fs::write(second.join("シェル.exe"), &fixture.shell).unwrap();
    fs::write(second.join("data.bin"), &fixture.encrypted).unwrap();
    fs::write(second.join("system.arc"), b"second").unwrap();
    let broken = library.join("broken");
    fs::create_dir(&broken).unwrap();
    fs::write(broken.join("unknown.dat"), b"?").unwrap();
    let (_, list) = request(&addr, "GET", "/api/games", b"");
    let list: serde_json::Value = serde_json::from_slice(&list).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 3);
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == "broken" && v["error"].is_string())
    );
    assert_eq!(fs::read(second.join("game.exe")).unwrap(), fixture.exe);
    assert_eq!(fs::read(second.join("data.bin")).unwrap(), fixture.plain);
    let prefix = "/games/%E4%BA%8C";
    assert_eq!(
        request(&addr, "GET", &format!("{prefix}/files/system.arc"), b"").1,
        b"second"
    );
    assert_eq!(
        request(&addr, "PUT", "/games/one/saves/slot", b"first").0,
        204
    );
    assert_eq!(
        request(&addr, "PUT", &format!("{prefix}/saves/slot"), b"second").0,
        204
    );
    assert_eq!(
        request(&addr, "GET", "/games/one/saves/slot", b"").1,
        b"first"
    );
    assert_eq!(
        request(&addr, "GET", &format!("{prefix}/saves/slot"), b"").1,
        b"second"
    );
    assert_eq!(
        request(&addr, "GET", "/games/%2e%2e/manifest.json", b"").0,
        400
    );
    assert_eq!(request(&addr, "GET", "/game/files/system.arc", b"").0, 404);
    fs::write(broken.join("system.arc"), b"repaired").unwrap();
    assert_eq!(
        request(&addr, "GET", "/games/broken/manifest.json", b"").0,
        200
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&first, library.join("link")).unwrap();
        assert_eq!(
            request(&addr, "GET", "/games/link/manifest.json", b"").0,
            404
        );
    }
}
