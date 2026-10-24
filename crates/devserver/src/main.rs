//! Serves the built player (dist/) and one game folder over the protocol in docs/protocol.md,
//! so a game can be played in a browser.
//!
//!     vnrun-devserver --game <ゲームのフォルダ> [--dist dist] [--saves <dir>] [--addr 127.0.0.1:8090]
//!                     [--engine siglus|reallive|avg32|uk2|bgi] [--title T] [--nls sjis]

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::process::exit;
use std::time::UNIX_EPOCH;

use percent_encoding::percent_decode_str;
use serde::Serialize;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

struct Config {
    dist: PathBuf,
    game: PathBuf,
    saves: PathBuf,
    addr: String,
    engine: String,
    title: String,
    nls: Option<String>,
}

#[derive(Serialize)]
struct Entry {
    p: String,
    s: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    m: Option<u64>,
}

#[derive(Serialize)]
struct Manifest<'a> {
    title: &'a str,
    engine: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    nls: Option<&'a str>,
    files: Vec<Entry>,
}

fn usage() -> ! {
    eprintln!(
        "usage: vnrun-devserver --game <dir> [--dist dist] [--saves <dir>] [--addr 127.0.0.1:8090] \
         [--engine siglus|reallive|avg32|uk2|bgi] [--title T] [--nls sjis]"
    );
    exit(2)
}

fn parse_args() -> Config {
    let mut args = std::env::args().skip(1);
    let (mut dist, mut game, mut saves, mut addr) = (
        PathBuf::from("dist"),
        None,
        None,
        "127.0.0.1:8090".to_string(),
    );
    let (mut engine, mut title, mut nls) = (None, None, None);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--dist" => dist = value().into(),
            "--game" => game = Some(PathBuf::from(value())),
            "--saves" => saves = Some(PathBuf::from(value())),
            "--addr" => addr = value(),
            "--engine" => engine = Some(value()),
            "--title" => title = Some(value()),
            "--nls" => nls = Some(value()),
            _ => usage(),
        }
    }
    let game = game.unwrap_or_else(|| usage());
    let name = game
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".into());
    let engine = engine.or_else(|| detect(&game)).unwrap_or_else(|| {
        eprintln!("engine not detected in {}; pass --engine", game.display());
        exit(2)
    });
    Config {
        dist,
        saves: saves.unwrap_or_else(|| PathBuf::from("saves").join(&name)),
        game,
        addr,
        engine,
        title: title.unwrap_or(name),
        nls,
    }
}

/// Guesses the engine from the files at the top of the game folder.
fn detect(game: &Path) -> Option<String> {
    let names: Vec<String> = fs::read_dir(game)
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect();
    let has = |n: &str| names.iter().any(|x| x == n);
    if has("scene.pck") {
        Some("siglus".into())
    } else if has("seen.txt") {
        Some("reallive".into())
    } else if has("system.arc") {
        Some("bgi".into())
    } else {
        None
    }
}

/// Lists the files below dir as paths relative to it.
fn walk(dir: &Path, prefix: &str, out: &mut Vec<Entry>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            walk(&e.path(), &rel, out);
        } else if md.is_file() {
            let m = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64);
            out.push(Entry {
                p: rel,
                s: md.len(),
                m,
            });
        }
    }
}

/// Turns a URL path below a root into a relative path, refusing anything that could leave it.
fn safe_rel(url_path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for seg in url_path.split('/') {
        let seg = percent_decode_str(seg).decode_utf8().ok()?;
        if seg.is_empty() || seg.contains(['\\', '\0']) {
            return None;
        }
        let p = Path::new(seg.as_ref());
        if !matches!(p.components().next(), Some(Component::Normal(_)))
            || p.components().count() != 1
        {
            return None;
        }
        out.push(p);
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("valid header")
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "wasm" => "application/wasm",
        "png" => "image/png",
        _ => "application/octet-stream",
    }
}

fn status(req: Request, code: u16, msg: &str) {
    let _ = req.respond(Response::from_string(msg).with_status_code(StatusCode(code)));
}

fn json<T: Serialize>(req: Request, v: &T) {
    let body = serde_json::to_vec(v).expect("serializable");
    let _ = req.respond(
        Response::from_data(body)
            .with_header(header("Content-Type", "application/json"))
            .with_header(header("Cache-Control", "no-store")),
    );
}

/// Serves a file, honouring a single `Range: bytes=a-b`.
fn send_file(req: Request, path: &Path) {
    let Ok(mut f) = File::open(path) else {
        return status(req, 404, "not found");
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let ct = header("Content-Type", content_type(path));
    let range = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .map(|h| h.value.as_str().to_string());
    if let Some((a, b)) = range
        .as_deref()
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.split_once('-'))
    {
        let (start, end) = match (a.parse::<u64>().ok(), b.parse::<u64>().ok()) {
            (Some(s), Some(e)) => (s, e.min(len.saturating_sub(1))),
            (Some(s), None) => (s, len.saturating_sub(1)),
            (None, Some(n)) => (len.saturating_sub(n), len.saturating_sub(1)),
            _ => return status(req, 416, "bad range"),
        };
        if start > end || start >= len {
            return status(req, 416, "bad range");
        }
        let n = end - start + 1;
        if f.seek(SeekFrom::Start(start)).is_err() {
            return status(req, 500, "seek failed");
        }
        let resp = Response::new(
            StatusCode(206),
            vec![
                ct,
                header("Content-Range", &format!("bytes {start}-{end}/{len}")),
            ],
            f.take(n),
            Some(n as usize),
            None,
        );
        let _ = req.respond(resp);
        return;
    }
    let resp = Response::new(
        StatusCode(200),
        vec![ct, header("Accept-Ranges", "bytes")],
        f,
        Some(len as usize),
        None,
    );
    let _ = req.respond(resp);
}

fn handle(cfg: &Config, mut req: Request) {
    let url = req.url().split('?').next().unwrap_or("").to_string();
    let method = req.method().clone();
    if url == "/" {
        let resp = Response::empty(StatusCode(303))
            .with_header(header("Location", "/index.html?game=/game"));
        let _ = req.respond(resp);
        return;
    }
    if url == "/game/manifest.json" {
        let mut files = Vec::new();
        walk(&cfg.game, "", &mut files);
        files.iter_mut().for_each(|e| e.m = None);
        let m = Manifest {
            title: &cfg.title,
            engine: &cfg.engine,
            nls: cfg.nls.as_deref(),
            files,
        };
        return json(req, &m);
    }
    if let Some(rest) = url.strip_prefix("/game/files/") {
        let Some(rel) = safe_rel(rest) else {
            return status(req, 400, "bad path");
        };
        return send_file(req, &cfg.game.join(rel));
    }
    if url == "/game/saves/" {
        let mut list = Vec::new();
        walk(&cfg.saves, "", &mut list);
        return json(req, &list);
    }
    if let Some(rest) = url.strip_prefix("/game/saves/") {
        let Some(rel) = safe_rel(rest) else {
            return status(req, 400, "bad path");
        };
        let path = cfg.saves.join(rel);
        return match method {
            Method::Get => send_file(req, &path),
            Method::Put => {
                let mut body = Vec::new();
                if req.as_reader().read_to_end(&mut body).is_err() {
                    return status(req, 400, "read failed");
                }
                let ok = path.parent().is_none_or(|d| fs::create_dir_all(d).is_ok())
                    && fs::write(&path, body).is_ok();
                if ok {
                    status(req, 204, "")
                } else {
                    status(req, 500, "write failed")
                }
            }
            Method::Delete => match fs::remove_file(&path) {
                Ok(()) => status(req, 204, ""),
                Err(_) => status(req, 404, "not found"),
            },
            _ => status(req, 405, "method not allowed"),
        };
    }
    let rel = url.trim_start_matches('/');
    match safe_rel(rel) {
        Some(rel) => send_file(req, &cfg.dist.join(rel)),
        None => status(req, 404, "not found"),
    }
}

fn main() {
    let cfg = parse_args();
    let server = Server::http(&cfg.addr).unwrap_or_else(|e| {
        eprintln!("listen {}: {e}", cfg.addr);
        exit(1)
    });
    eprintln!("{} ({}) → http://{}/", cfg.title, cfg.engine, cfg.addr);
    for req in server.incoming_requests() {
        handle(&cfg, req);
    }
}
