//! Prepares a game folder for the browser: detects and extracts SDRS shells, then prepares
//! Siglus EXE keys. Also accepts an explicitly selected SDRS executable.

use std::path::Path;
use std::process::exit;

use vnrun_prep::{prepare, sdrs};

const USAGE: &str = "usage: vnrun-prep <game dir>\n       vnrun-prep --sdrs <program.exe>";

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let (path, extract) = match args.as_slice() {
        [arg] if arg == "--help" || arg == "-h" => {
            println!("{USAGE}");
            return;
        }
        [dir] if !dir.to_string_lossy().starts_with('-') => (Path::new(dir), false),
        [flag, exe] if flag == "--sdrs" => (Path::new(exe), true),
        _ => {
            eprintln!("{USAGE}");
            exit(2)
        }
    };
    let result = if extract { sdrs(path) } else { prepare(path) };
    if let Err(e) = result {
        eprintln!("{}: {e}", path.display());
        exit(1)
    }
}
