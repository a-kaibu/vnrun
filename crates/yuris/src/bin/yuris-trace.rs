//! Runs a YU-RIS game without a screen and prints the commands it gives, for working out what
//! the engine has to do.
//!
//!     yuris-trace <game folder> [frames]

use std::fs;
use std::path::Path;

use yuris::vm::{Call, Flow, Host, Program, State, Vm};
use yuris::ypf;

struct Trace;

impl Host for Trace {
    fn command(&mut self, vm: &mut State, call: &Call) -> Result<Flow, String> {
        let args: Vec<String> = call
            .args
            .iter()
            .map(|(k, _, e)| format!("{k}={}", vm.eval(e).map(|v| format!("{v:?}")).unwrap_or_else(|_| "?".into())))
            .collect();
        println!("[{}] {}[{}]", vm.task().id, call.name, args.join(" "));
        Ok(Flow::Next)
    }
    fn text(&mut self, vm: &mut State, text: &str) -> Result<Flow, String> {
        println!("[{}] TEXT {text:?}", vm.task().id);
        Ok(Flow::Next)
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let dir = Path::new(args.get(1).ok_or("usage: yuris-trace <game folder> [frames]")?);
    let frames: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let pack = fs::read(dir.join("ysbin.ypf")).map_err(|e| format!("ysbin.ypf: {e}"))?;
    let index = ypf::Index::parse(&pack)?;
    let mut read = |name: &str| {
        let e = index.find(&format!("ysbin/{name}"))?;
        e.unpack(&pack[e.offset as usize..(e.offset + e.packed_size) as usize]).ok()
    };
    let prog = Program::load(&mut read)?;
    let mut vm = Vm::new(prog)?;
    vm.state.trace = std::env::var_os("YURIS_TRACE").is_some();
    for f in 0..frames {
        vm.frame(f as f64 * 1000.0 / 60.0, &mut Trace)?;
        if vm.ended {
            break;
        }
    }
    Ok(())
}
