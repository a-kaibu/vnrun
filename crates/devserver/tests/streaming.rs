use std::fs::{self, File};
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

fn connect(addr: &str) -> TcpStream {
    let stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
}

#[test]
fn slow_transfer_does_not_block_other_requests_or_expose_partial_saves() {
    let dir = tempfile::tempdir().unwrap();
    let game = dir.path().join("game");
    let saves = dir.path().join("saves");
    fs::create_dir(&game).unwrap();
    fs::create_dir(&saves).unwrap();
    fs::write(game.join("system.arc"), []).unwrap();
    File::create(game.join("big.bin"))
        .unwrap()
        .set_len(128 * 1024 * 1024)
        .unwrap();
    fs::write(saves.join("slot.dat"), b"old").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    drop(listener);
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_vnrun-devserver"))
            .args(["--game"])
            .arg(&game)
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
    let mut slow = connect(&addr);
    slow.write_all(b"GET /game/files/big.bin HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut header = [0; 1];
    slow.read_exact(&mut header).unwrap();
    let mut upload = connect(&addr);
    upload.write_all(b"PUT /game/saves/slot.dat HTTP/1.1\r\nHost: localhost\r\nContent-Length: 6\r\nConnection: close\r\n\r\nnew").unwrap();
    let mut get = connect(&addr);
    get.write_all(
        b"GET /game/saves/slot.dat HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .unwrap();
    let mut response = Vec::new();
    get.read_to_end(&mut response).unwrap();
    assert!(response.ends_with(b"old"));
    upload.write_all(b"!!!").unwrap();
    let mut response = String::new();
    upload.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 204"), "{response}");
    assert_eq!(fs::read(saves.join("slot.dat")).unwrap(), b"new!!!");
}
