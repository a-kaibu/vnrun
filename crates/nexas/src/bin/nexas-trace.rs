//! Runs a NeXAS game's scripts with every builtin logged and doing nothing, to see what a game
//! asks of the engine.
//!
//!     nexas-trace <game folder> [frames]

use std::path::Path;
use std::rc::Rc;

use nexas::files::{Archives, Disk, Files};
use nexas::script;
use nexas::vm::{Flow, Globals, Host, Step, Thread, Value};

struct Log;

impl Host for Log {
    fn builtin(
        &mut self,
        _vm: &mut Thread,
        _g: &mut Globals,
        id: u32,
        args: &[Value],
    ) -> Result<(Value, Flow), String> {
        let a: Vec<String> = args
            .iter()
            .map(|v| match v {
                Value::Int(i) => i.to_string(),
                Value::Str(s) => format!("{:?}", nexas::sjis::decode(s)),
            })
            .collect();
        eprintln!("  b{id:#x}({})", a.join(", "));
        // jump to a file
        if let (0x45, Some(Value::Str(f))) = (id, args.first()) {
            return Ok((
                Value::Int(0),
                Flow::Jump {
                    file: nexas::sjis::decode(f),
                    label: args.get(1).map_or(0, |v| v.int() as u32),
                },
            ));
        }
        Ok((Value::Int(0), Flow::Next))
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let dir = Path::new(
        args.get(1)
            .ok_or("usage: nexas-trace <game folder> [frames]")?,
    );
    let frames: usize = args.get(2).map_or(10, |s| s.parse().unwrap_or(10));
    let mut disk = Disk {
        root: dir.to_path_buf(),
        saves: dir.join("vnrun-saves"),
    };
    let arcs = Archives::open(&mut disk);
    let load = |disk: &mut Disk, name: &str| -> Result<Rc<script::Script>, String> {
        let b = arcs
            .read(disk as &mut dyn Files, "Script", name)
            .ok_or(format!("Script.pac has no {name}"))?;
        script::parse(&b)
            .map(Rc::new)
            .map_err(|e| format!("{name}: {e}"))
    };
    let mut g = Globals::default();
    let mut t = Thread::new("System.bin", load(&mut disk, "System.bin")?);
    t.trace = std::env::var_os("NEXAS_TRACE").is_some();
    t.goto_label(0);
    let mut host = Log;
    for frame in 0..frames {
        eprintln!("-- frame {frame}");
        let mut n = 0;
        loop {
            n += 1;
            if n > 5_000_000 {
                return Err(format!("{} runs without yielding", t.name));
            }
            match t.step(&mut g, &mut host)? {
                Step::Next => {}
                Step::Yield => break,
                Step::End => return Err(format!("{} ended", t.name)),
                Step::Jump { file, label } => {
                    eprintln!("== {file} label {label}");
                    let trace = t.trace;
                    t = Thread::new(&file, load(&mut disk, &file)?);
                    t.trace = trace;
                    if !t.goto_label(label) {
                        return Err(format!("{file} has no label {label}"));
                    }
                }
            }
        }
    }
    Ok(())
}
