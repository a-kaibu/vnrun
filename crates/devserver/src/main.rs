//! Serves the built player (dist/) and one game folder over the protocol in docs/protocol.md,
//! so a game can be played in a browser.
//! Prepares SDRS shells and Siglus keys before detecting the engine and serving the game.
//!
//!     vnrun-devserver --game <ゲームのフォルダ> [--dist dist] [--saves <dir>] [--addr 127.0.0.1:8090]
//!                     [--engine siglus|reallive|avg32|uk2|bgi|softpal|fvp|artemis|nscripter|kirikiri|html] [--title T] [--nls sjis]

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::process::exit;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use serde::Serialize;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const SAVE_LIMIT: u64 = 64 * 1024 * 1024;
const WORKERS: usize = 8;
const SAVE_STAGING: &str = ".vnrun-staging";

#[derive(Clone)]
struct Config {
    dist: PathBuf,
    game: PathBuf,
    saves: PathBuf,
    addr: String,
    engine: String,
    title: String,
    nls: Option<String>,
    max_save_bytes: u64,
    library: Option<Arc<Library>>,
}

struct Library {
    root: PathBuf,
    games: Mutex<HashMap<String, GameSlot>>,
}

type CachedGame = (Option<SystemTime>, Arc<Config>);
type GameSlot = Arc<Mutex<Option<CachedGame>>>;

#[derive(Serialize)]
struct LibraryEntry {
    id: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    engine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    url: String,
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
        "usage: vnrun-devserver (--game <dir> | --library <dir>) [--dist dist] [--saves <dir>] [--addr 127.0.0.1:8090] \
         [--engine siglus|reallive|avg32|uk2|bgi|softpal|fvp|artemis|nscripter|kirikiri|html] [--title T] [--nls sjis] [--max-save-bytes N]"
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
    let mut library = None;
    let mut max_save_bytes = SAVE_LIMIT;
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--dist" => dist = value().into(),
            "--game" => game = Some(PathBuf::from(value())),
            "--library" => library = Some(PathBuf::from(value())),
            "--saves" => saves = Some(PathBuf::from(value())),
            "--addr" => addr = value(),
            "--engine" => engine = Some(value()),
            "--title" => title = Some(value()),
            "--nls" => nls = Some(value()),
            "--max-save-bytes" => max_save_bytes = value().parse().unwrap_or_else(|_| usage()),
            _ => usage(),
        }
    }
    if let Some(root) = library {
        if game.is_some() || engine.is_some() || title.is_some() || nls.is_some() {
            usage();
        }
        fs::read_dir(&root).unwrap_or_else(|e| {
            eprintln!("{}: {e}", root.display());
            exit(1)
        });
        return Config {
            dist,
            game: PathBuf::new(),
            saves: saves.unwrap_or_else(|| PathBuf::from("saves")),
            addr,
            engine: String::new(),
            title: String::new(),
            nls: None,
            max_save_bytes,
            library: Some(Arc::new(Library {
                root,
                games: Mutex::new(HashMap::new()),
            })),
        };
    }
    let game = game.unwrap_or_else(|| usage());
    // Prepare before engine detection: SDRS may wrap the files that identify the engine.
    vnrun_prep::prepare(&game).unwrap_or_else(|e| {
        eprintln!("{}: {e}", game.display());
        exit(1)
    });
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
        max_save_bytes,
        library: None,
    }
}

/// Whether a Data folder (any case) holds NOA archives.
fn noa_in_data(game: &Path) -> bool {
    let Ok(rd) = fs::read_dir(game) else {
        return false;
    };
    rd.flatten()
        .filter(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case("data"))
        .filter_map(|e| fs::read_dir(e.path()).ok())
        .flat_map(|d| d.flatten())
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .to_lowercase()
                .ends_with(".noa")
        })
}

/// Whether scenario.arc at the top of the game folder is a MajiroArc archive.
fn majiro_arc(game: &Path, names: &[String]) -> bool {
    if !names.iter().any(|n| n == "scenario.arc") {
        return false;
    }
    fs::read_dir(game).into_iter().flatten().flatten().any(|e| {
        if !e
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case("scenario.arc")
        {
            return false;
        }
        let mut head = [0u8; 10];
        fs::File::open(e.path())
            .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
            .is_ok()
            && &head == b"MajiroArcV"
    })
}

/// An AZ System archive ("ARC\x1a") at the top of the game folder.
fn azsys_arc(game: &Path) -> bool {
    fs::read_dir(game).into_iter().flatten().flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        let mut head = [0u8; 4];
        name.ends_with(".arc")
            && fs::File::open(e.path())
                .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
                .is_ok()
            && &head == b"ARC\x1a"
    })
}

/// Whether a .bin file at the top of the game folder is an ESC-ARC archive.
/// A YKC001 archive at the top of the game folder (Yuka).
fn ykc_archive(game: &Path, names: &[String]) -> bool {
    if names.iter().any(|n| n.ends_with(".ykc")) {
        return true;
    }
    fs::read_dir(game).into_iter().flatten().flatten().any(|e| {
        if !e
            .file_name()
            .to_string_lossy()
            .to_lowercase()
            .ends_with(".dat")
        {
            return false;
        }
        let mut head = [0u8; 6];
        fs::File::open(e.path())
            .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
            .is_ok()
            && &head == b"YKC001"
    })
}

fn esc_arc(game: &Path, names: &[String]) -> bool {
    if !names.iter().any(|n| n.ends_with(".bin")) {
        return false;
    }
    fs::read_dir(game).into_iter().flatten().flatten().any(|e| {
        if !e
            .file_name()
            .to_string_lossy()
            .to_lowercase()
            .ends_with(".bin")
        {
            return false;
        }
        let mut head = [0u8; 7];
        fs::File::open(e.path())
            .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
            .is_ok()
            && &head == b"ESC-ARC"
    })
}

/// The first YU-RIS (2006) keeps only pac/*.ypf beside the exe.
fn pac_ypf(game: &Path, names: &[String]) -> bool {
    let Some(pac) = names.iter().find(|n| *n == "pac") else {
        return false;
    };
    let dir = fs::read_dir(game)
        .into_iter()
        .flatten()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().to_lowercase() == *pac);
    dir.and_then(|d| fs::read_dir(d.path()).ok())
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .to_lowercase()
                .ends_with(".ypf")
        })
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
    } else if has("system40.ini")
        || has("alicestart.ini")
        || names.iter().any(|n| n.ends_with(".ain"))
    {
        // AliceSoft System4: the .ain code beside its start .ini
        Some("system4".into())
    } else if has("seen.txt") {
        Some("reallive".into())
    } else if has("advhd.exe") && names.iter().any(|n| n.ends_with(".arc")) {
        Some("advhd".into())
    } else if (has("scr.arc") && has("tblstr.arc"))
        || (has("arc") && game.join("ARC").join("scr.arc").is_file())
        || (has("arc") && game.join("arc").join("scr.arc").is_file())
        || (has("scr.arc") && has("params.dat") && has("message.dat"))
    {
        // Kaguya (2006 era): WFL1 archives with .ari indexes, the scripts in scr.arc
        // (beside the exe, or in ARC/ from about 2013); the LINK generation (2010s):
        // LINK archives beside params.dat and message.dat
        Some("kaguya".into())
    } else if has("arc01.dat") && has("arc04.dat") && has("arc05.dat") {
        // Cyberworks / TinkerBell: index files Arc01..03 beside the data Arc04..06
        Some("cyberworks".into())
    } else if has("scr.aos") && has("grp.aos") {
        // LiLiM: scr.aos (text scripts) beside grp.aos, cv.aos and the sfaNN.exe
        Some("lilim".into())
    } else if names.iter().any(|n| n.ends_with(".war")) {
        // ShiinaRio: WARC archives (.WAR) beside the exe and its [椎名里緒] .ini
        Some("shiina".into())
    } else if has("script.arc") && has("layer.arc") {
        // Silky's (the AI6WIN generation): Script.arc beside data.arc, layer.arc, music.arc
        Some("silkys".into())
    } else if has("ags.exe") && has("misc") {
        // Ivory: ags.exe beside misc/*.pk (fPK archives: script, bg, chr, grp, se)
        Some("ivory".into())
    } else if has("script.dat") && has("arc0.dat") {
        // NonColor: script.dat (text scripts) beside arc0.dat.. (ACV1 / DAT archives)
        Some("noncolor".into())
    } else if azsys_arc(game) {
        // AZ System: one ARC archive (CPB pictures, ASB scripts, tables) beside the exe
        Some("azsys".into())
    } else if has("game_sys.dat") {
        // PJADV (bootUP!/Pajamas): GAMEDAT PAC archives game_sys.dat, graph.dat, voice.dat..
        Some("pjadv".into())
    } else if has("systeminfo.json") && has("script.arc") && has("image.arc") {
        // Eomaia (FG REMAKE): systeminfo.json beside script.arc, image.arc, voice.arc..
        Some("eomaia".into())
    } else if majiro_arc(game, &names) {
        // Majiro: MajiroArc archives (scenario.arc, data.arc, ...) beside the exe
        Some("majiro".into())
    } else if has("system.arc") {
        Some("bgi".into())
    } else if has("ysbin.ypf") || has("ysbin") || has("yscfg.dat") || pac_ypf(game, &names) {
        Some("yuris".into())
    } else if has("srp.pac") || has("mg_data.mbl") {
        Some("tmrhiro".into())
    } else if has("live.dll") || has("game.ext") {
        // LiveMaker: the archive is appended to the exe (live.dll beside it) or in game.dat/.ext
        Some("livemaker".into())
    } else if has("cs2.exe") || names.iter().any(|n| n == "scene.int") {
        // CatSystem2: cs2.exe beside the .int archives
        Some("catsystem2".into())
    } else if has("md_scr.med") {
        // DxLib MED (fudegaki-soft, trois): md_scr.med beside md_gra.med and the others
        Some("med".into())
    } else if has("ll5.dll") && has("data") {
        // CrossOver (Adv.exe): DATA/ holds the PACK archives SCRIPT.PAC, *.GR2 and *.VIC
        Some("crossover".into())
    } else if has("scr.xfl") && names.iter().any(|n| n.starts_with("grp") && n.ends_with(".xfl")) {
        // Liar-soft: scr.xfl beside the grp*.xfl pictures
        Some("liar".into())
    } else if (has("scr.pak") && has("cfg.pak"))
        || (has("gamedata") && game.join("gamedata").join("scr.pak").is_file())
    {
        // NekoSDK (Waffle): NEKOPACK archives scr/flow/cfg/obj.pak, often in gamedata/
        Some("neko".into())
    } else if has("gamedata") && has_qlie_pack(game) {
        // QLIE: GameData/data0.pack beside the exe
        Some("qlie".into())
    } else if names
        .iter()
        .any(|n| n.ends_with(".noa") || n.ends_with(".csx"))
        || noa_in_data(game)
    {
        // Entis GLS: the .csx script or NOA archives at the top, or NOA archives in Data/
        Some("entis".into())
    } else if esc_arc(game, &names) {
        // Escu:de: ESC-ARC archives (script.bin, gfx.bin, ...) beside the exe
        Some("escude".into())
    } else if names.iter().any(|n| n.ends_with("_sce.dat")) {
        // Crowd / ANIM: the encrypted <game>_sce.dat scenario beside cg/, bgm/ and voice/
        Some("anim".into())
    } else if ykc_archive(game, &names) {
        // Yuka (HOOK, SMEE, feng): YKC001 archives (*.ykc, or *.dat in the older ones)
        Some("yuka".into())
    } else if has("spt") && has("dwq") {
        // System-NNN (Black Cyc and others): spt/*.spt scripts beside dwq/*.gpk picture packs
        Some("systemnnn".into())
    } else if has("bsxx.dat") || (has("bsx.dat") && names.iter().any(|n| n.ends_with(".bsa"))) {
        // Bishop: bsxx.dat (BSXScript) or the older bsx.dat beside the .bsa archives
        Some("bishop".into())
    } else if has("sys4ini.bin") || has("sys5ini.bin") {
        // Eushully AGE: the sys4ini.bin index of the .alf archives
        Some("eushully".into())
    } else if has("data.fpk") {
        // Interheart / Candy Soft: data.fpk (the scenario) beside the other .fpk archives
        Some("interheart".into())
    } else if has("script.pac") {
        Some("nexas".into())
    } else if has("data.pac") {
        Some("softpal".into())
    } else if names.iter().any(|n| n.ends_with(".hcb")) {
        Some("fvp".into())
    } else if names.iter().any(|n| n.ends_with(".pfs")) || has("system.ini") {
        Some("artemis".into())
    } else if has("nscript.dat")
        || has("0.txt")
        || has("00.txt")
        || names
            .iter()
            .any(|n| n.ends_with(".nsa") || n.ends_with(".sar"))
    {
        Some("nscripter".into())
    } else if has("package.json") || has("index.html") || has("www") || has("package.nw") {
        Some("html".into())
    } else if names.iter().any(|n| n.ends_with(".xp3"))
        || has("startup.tjs")
        || has("data")
        || has_xp3_data(game)
    {
        Some("kirikiri".into())
    } else {
        None
    }
}

/// Whether GameData (any case) holds QLIE .pack archives.
fn has_qlie_pack(game: &Path) -> bool {
    fs::read_dir(game)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("gamedata")
        })
        .filter_map(|e| fs::read_dir(e.path()).ok())
        .flat_map(|d| d.flatten())
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .to_lowercase()
                .ends_with(".pack")
        })
}

// Some KiriKiri releases call the main XP3 archive data.bin.
fn has_xp3_data(game: &Path) -> bool {
    fs::read_dir(game).into_iter().flatten().flatten().any(|e| {
        if !e
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case("data.bin")
        {
            return false;
        }
        let mut magic = [0; 11];
        File::open(e.path())
            .and_then(|mut f| f.read_exact(&mut magic))
            .is_ok()
            && magic == *b"XP3\r\n \n\x1a\x8b\x67\x01"
    })
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
        "htm" => "text/html; charset=utf-8",
        "mjs" => "text/javascript",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
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

/// Serves a file, honouring a single `Range: bytes=a-b`. With `X-Vnrun-Pad: 1` the body starts
/// with one extra zero byte (docs/protocol.md).
fn send_file(req: Request, path: &Path) {
    let Ok(mut f) = File::open(path) else {
        return status(req, 404, "not found");
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let ct = header("Content-Type", content_type(path));
    let pad: &'static [u8] = if req
        .headers()
        .iter()
        .any(|h| h.field.equiv("X-Vnrun-Pad") && h.value.as_str() == "1")
    {
        &[0]
    } else {
        &[]
    };
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
            pad.chain(f.take(n)),
            Some(pad.len() + n as usize),
            None,
        );
        let _ = req.respond(resp);
        return;
    }
    let resp = Response::new(
        StatusCode(200),
        vec![ct, header("Accept-Ranges", "bytes")],
        pad.chain(f),
        Some(pad.len() + len as usize),
        None,
    );
    let _ = req.respond(resp);
}

/// The game file at rel, matching names without regard to case when there is no exact match
/// (games made on Windows do not keep the case of their names consistent).
fn fold_case(root: &Path, rel: &Path) -> PathBuf {
    let mut out = root.to_path_buf();
    for part in rel.iter() {
        let exact = out.join(part);
        if exact.exists() {
            out = exact;
            continue;
        }
        let want = part.to_string_lossy().to_lowercase();
        let found = fs::read_dir(&out).ok().and_then(|rd| {
            rd.flatten()
                .find(|e| e.file_name().to_string_lossy().to_lowercase() == want)
        });
        out = found.map_or(exact, |e| e.path());
    }
    out
}

/// Whether the browser is loading the request as the page of an iframe.
fn is_frame(req: &Request) -> bool {
    req.headers()
        .iter()
        .any(|h| h.field.equiv("Sec-Fetch-Dest") && h.value.as_str() == "iframe")
}

/// Serves a page of a game made as a web page (engine `html`) with the player's hook as its
/// first script (docs/protocol.md).
fn send_frame_page(req: Request, path: &Path) {
    const HOOK: &str = "<script>parent.vnrunFrame&&parent.vnrunFrame(window)</script>";
    let Ok(page) = fs::read_to_string(path) else {
        return status(req, 404, "not found");
    };
    let lower = page.to_ascii_lowercase();
    // After <head ...>, else after the doctype (before it would put the page in quirks mode).
    let at = lower
        .find("<head")
        .or_else(|| lower.find("<!doctype"))
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        .unwrap_or(0);
    let body = format!("{}{HOOK}{}", &page[..at], &page[at..]);
    let _ = req.respond(
        Response::from_string(body)
            .with_header(header("Content-Type", "text/html; charset=utf-8"))
            .with_header(header("Cache-Control", "no-store")),
    );
}

impl Library {
    fn game(&self, cfg: &Config, id: &str) -> Result<Arc<Config>, String> {
        let path = self.root.join(id);
        if !fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_dir()
        {
            return Err("not a game directory".into());
        }
        // Preparation changes files in place. Serialize each game, but let other games stream.
        let slot = {
            let mut games = self.games.lock().map_err(|e| e.to_string())?;
            Arc::clone(games.entry(id.to_string()).or_default())
        };
        let mut cached = slot.lock().map_err(|e| e.to_string())?;
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !meta.file_type().is_dir() {
            return Err("not a game directory".into());
        }
        if let Some((modified, game)) = cached.as_ref()
            && *modified == meta.modified().ok()
        {
            return Ok(Arc::clone(game));
        }
        *cached = None;
        vnrun_prep::prepare(&path).map_err(|e| e.to_string())?;
        let engine = detect(&path).ok_or("engine not detected")?;
        let game = Arc::new(Config {
            dist: cfg.dist.clone(),
            game: path,
            saves: cfg.saves.join(id),
            addr: cfg.addr.clone(),
            engine,
            title: id.to_string(),
            nls: None,
            max_save_bytes: cfg.max_save_bytes,
            library: None,
        });
        let modified = fs::metadata(&game.game).and_then(|m| m.modified()).ok();
        *cached = Some((modified, Arc::clone(&game)));
        Ok(game)
    }

    fn entries(&self, cfg: &Config) -> io::Result<Vec<LibraryEntry>> {
        let mut entries = Vec::new();
        for item in fs::read_dir(&self.root)? {
            let item = item?;
            if !item.file_type()?.is_dir() {
                continue;
            }
            let Ok(id) = item.file_name().into_string() else {
                continue;
            };
            let url = format!("/games/{}", utf8_percent_encode(&id, NON_ALPHANUMERIC));
            let (engine, error) = match self.game(cfg, &id) {
                Ok(game) => (Some(game.engine.clone()), None),
                Err(error) => (None, Some(error)),
            };
            entries.push(LibraryEntry {
                title: id.clone(),
                id,
                engine,
                error,
                url,
            });
        }
        entries.sort_by(|a, b| a.title.cmp(&b.title));
        Ok(entries)
    }
}

fn handle(cfg: &Config, req: Request) {
    let url = req.url().split('?').next().unwrap_or("").to_string();
    if let Some(library) = &cfg.library {
        if url == "/api/games" {
            return match library.entries(cfg) {
                Ok(entries) => json(req, &entries),
                Err(error) => {
                    eprintln!("library: {error}");
                    status(req, 500, "library unavailable")
                }
            };
        }
        if let Some(rest) = url.strip_prefix("/games/") {
            let Some((encoded_id, path)) = rest.split_once('/') else {
                return status(req, 404, "not found");
            };
            let Some(id) = safe_rel(encoded_id).filter(|p| p.components().count() == 1) else {
                return status(req, 400, "bad game id");
            };
            let Some(id) = id.to_str() else {
                return status(req, 400, "bad game id");
            };
            return match library.game(cfg, id) {
                Ok(game) => handle_game(&game, req, &format!("/game/{path}")),
                Err(error) => {
                    eprintln!("game {id}: {error}");
                    status(req, 404, "game unavailable")
                }
            };
        }
        if url == "/" {
            let resp = Response::empty(StatusCode(303))
                .with_header(Header::from_bytes("Location", "/library.html").unwrap());
            let _ = req.respond(resp);
            return;
        }
        if url == "/game" || url.starts_with("/game/") {
            return status(req, 404, "not found");
        }
    }
    handle_game(cfg, req, &url)
}

fn handle_game(cfg: &Config, mut req: Request, url: &str) {
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
        let path = fold_case(&cfg.game, &rel);
        if is_frame(&req) && matches!(content_type(&path), "text/html; charset=utf-8") {
            return send_frame_page(req, &path);
        }
        return send_file(req, &path);
    }
    if url == "/game/saves/" {
        let mut list = Vec::new();
        walk(&cfg.saves, "", &mut list);
        list.retain(|entry| !entry.p.starts_with(&format!("{SAVE_STAGING}/")));
        return json(req, &list);
    }
    if let Some(rest) = url.strip_prefix("/game/saves/") {
        let Some(rel) = safe_rel(rest) else {
            return status(req, 400, "bad path");
        };
        if rel.components().next() == Some(Component::Normal(SAVE_STAGING.as_ref())) {
            return status(req, 404, "not found");
        }
        let path = cfg.saves.join(rel);
        return match method {
            Method::Get => send_file(req, &path),
            Method::Put => match write_save(
                &cfg.saves,
                &path,
                req.body_length(),
                cfg.max_save_bytes,
                req.as_reader(),
            ) {
                Ok(()) => status(req, 204, ""),
                Err(SaveError::TooLarge) => status(req, 413, "save too large"),
                Err(SaveError::Incomplete) => status(req, 400, "incomplete upload"),
                Err(SaveError::Io(error)) => {
                    eprintln!("save {}: {error}", path.display());
                    status(req, 500, "write failed")
                }
            },
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

enum SaveError {
    TooLarge,
    Incomplete,
    Io(io::Error),
}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

// Stream into a private directory on the same filesystem, then replace the save in one rename.
// A reader sees either the old complete save or the new complete save, never a partial upload.
fn write_save(
    root: &Path,
    path: &Path,
    expected: Option<usize>,
    limit: u64,
    input: &mut dyn Read,
) -> Result<(), SaveError> {
    if expected.is_some_and(|n| n as u64 > limit) {
        return Err(SaveError::TooLarge);
    }
    fs::create_dir_all(root)?;
    let staging = root.join(SAVE_STAGING);
    fs::create_dir_all(&staging)?;
    let mut tmp = tempfile::NamedTempFile::new_in(staging)?;
    let bytes = io::copy(&mut input.take(limit.saturating_add(1)), &mut tmp)?;
    if bytes > limit {
        return Err(SaveError::TooLarge);
    }
    if expected.is_some_and(|n| n as u64 != bytes) {
        return Err(SaveError::Incomplete);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    tmp.persist(path).map_err(|e| SaveError::Io(e.error))?;
    Ok(())
}

fn main() {
    let cfg = Arc::new(parse_args());
    let server = Server::http(&cfg.addr).unwrap_or_else(|e| {
        eprintln!("listen {}: {e}", cfg.addr);
        exit(1)
    });
    if let Some(library) = &cfg.library {
        eprintln!("{} → http://{}/", library.root.display(), cfg.addr);
    } else {
        eprintln!("{} ({}) → http://{}/", cfg.title, cfg.engine, cfg.addr);
    }
    // tiny_http 0.12 gives each connection a thread from its own pool for as long as the client
    // keeps it alive. When the browser opens several connections at once (a page fetching all
    // its saves), the pool can queue one of them while it believes an idle thread will take it,
    // and that thread takes another: the queued connection is then only read once some other
    // connection closes, and the browser keeps its connections open, so the page waits forever
    // (nanawind titles, whose seven saves are fetched together, stayed on the loading screen).
    // A connection that closes at once lets a pool thread finish a task and take the queued one.
    if let Some(addr) = server.server_addr().to_ip() {
        thread::spawn(move || {
            loop {
                thread::sleep(std::time::Duration::from_millis(200));
                let _ = std::net::TcpStream::connect(addr);
            }
        });
    }
    // A large response can block on a slow client. Keep a bounded worker pool so another
    // game's range request, save, or manifest is not blocked behind it.
    let (tx, rx) = mpsc::sync_channel::<Request>(WORKERS * 2);
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..WORKERS {
        let (cfg, rx) = (Arc::clone(&cfg), Arc::clone(&rx));
        thread::spawn(move || {
            loop {
                let request = match rx.lock() {
                    Ok(rx) => rx.recv(),
                    Err(_) => return,
                };
                match request {
                    Ok(req) => handle(&cfg, req),
                    Err(_) => return,
                }
            }
        });
    }
    for req in server.incoming_requests() {
        if tx.send(req).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod detection_tests {
    use super::*;

    #[test]
    fn preparing_one_title_does_not_block_another() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("playing")).unwrap();
        fs::write(dir.path().join("playing/system.arc"), b"game").unwrap();
        let library = Arc::new(Library {
            root: dir.path().into(),
            games: Mutex::new(HashMap::new()),
        });
        let slow = Arc::new(Mutex::new(None));
        library
            .games
            .lock()
            .unwrap()
            .insert("preparing".into(), Arc::clone(&slow));
        let _guard = slow.lock().unwrap();
        let cfg = Config {
            dist: PathBuf::new(),
            game: PathBuf::new(),
            saves: dir.path().join("saves"),
            addr: String::new(),
            engine: String::new(),
            title: String::new(),
            nls: None,
            max_save_bytes: SAVE_LIMIT,
            library: None,
        };
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            tx.send(library.game(&cfg, "playing").map(|g| g.engine.clone()))
                .unwrap()
        });
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .unwrap(),
            "bgi"
        );
    }

    #[test]
    fn advhd_with_archives_is_detected_before_bgi() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("ADVHD.EXE"), b"").unwrap();
        assert_eq!(detect(dir.path()), None);
        fs::write(dir.path().join("Rio.arc"), b"").unwrap();
        fs::write(dir.path().join("System.arc"), b"").unwrap();
        assert_eq!(detect(dir.path()).as_deref(), Some("advhd"));
        fs::remove_file(dir.path().join("ADVHD.EXE")).unwrap();
        assert_eq!(detect(dir.path()).as_deref(), Some("bgi"));
    }

    #[test]
    fn pjadv_game_sys_dat_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["game_sys.dat", "graph.dat", "voice.dat", "nana_trial.exe"] {
            fs::write(dir.path().join(n), b"GAMEDAT PAC2").unwrap();
        }
        assert_eq!(detect(dir.path()).as_deref(), Some("pjadv"));
    }

    #[test]
    fn eomaia_systeminfo_json_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        for n in [
            "systeminfo.json",
            "script.arc",
            "image.arc",
            "yakin_re_tr.exe",
        ] {
            fs::write(dir.path().join(n), b"").unwrap();
        }
        assert_eq!(detect(dir.path()).as_deref(), Some("eomaia"));
    }

    #[test]
    fn save_replacement_rejects_incomplete_and_oversized_uploads() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("saves");
        fs::create_dir(&root).unwrap();
        let path = root.join("slot.dat");
        fs::write(&path, b"old").unwrap();
        assert!(matches!(
            write_save(&root, &path, Some(4), 8, &mut &b"new"[..]),
            Err(SaveError::Incomplete)
        ));
        assert!(matches!(
            write_save(&root, &path, None, 2, &mut &b"new"[..]),
            Err(SaveError::TooLarge)
        ));
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert!(write_save(&root, &path, Some(3), 8, &mut &b"new"[..]).is_ok());
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read_dir(root.join(SAVE_STAGING)).unwrap().count(), 0);
    }

    #[test]
    fn data_bin_requires_xp3_signature_and_ignores_filename_case() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("DATA.BIN");
        fs::write(&path, b"arbitrary binary data").unwrap();
        assert_eq!(detect(dir.path()), None);
        fs::write(&path, b"XP3\r\n \n\x1a\x8b\x67\x01").unwrap();
        assert_eq!(detect(dir.path()).as_deref(), Some("kirikiri"));
        fs::write(path, b"XP3").unwrap();
        assert_eq!(detect(dir.path()), None);
    }
}
