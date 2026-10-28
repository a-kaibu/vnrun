//! ACPX C: the language Escu:de games are written in, compiled to a small stack machine.

pub mod ast;
pub mod code;
pub mod compile;
pub mod lex;
pub mod parse;
pub mod pp;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

/// Reads a program and what it includes. `load` gives a file by its path.
pub fn parse_program(
    path: &str,
    base: &str,
    load: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Result<ast::Unit, String> {
    let mut pre = pp::Pre::new(load);
    let toks = pre.run(path, base)?;
    let traps: HashMap<String, pp::Trap> = std::mem::take(&mut pre.traps);
    let files = std::mem::take(&mut pre.files);
    parse::Parser::new(toks, files).unit(&traps)
}
