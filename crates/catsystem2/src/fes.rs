//! Screen scripts (.fes): plain Shift_JIS text, zlib-packed.
//!
//! ```text
//! "FES\0" packed:u32 size:u32 ?:u32 zlib(text)
//! ```
//!
//! The text is made of `#` blocks: `#DEFINE` (object kinds and names, `pl[6]` for six),
//! `#OBJECT col col ...` (a table of object settings), `#MACRO`, `#SUBCOMMAND`, `#PRELOAD`,
//! `#KEYBLOCK`, and code blocks named by their label (`#START`, `#WAIT`, `#btn1.PUSH_L`, ...).

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Def {
    /// kind (PLANE, IMAGE, ...) and name, maybe with a count: `pl[6]`
    Define(String, String),
    /// the column names and the rows (object name first)
    Object(Vec<String>, Vec<Vec<String>>),
}

#[derive(Debug, Default)]
pub struct Fes {
    pub macros: HashMap<String, String>,
    pub defs: Vec<Def>,
    /// the code: labels are kept as lines starting with '#'
    pub lines: Vec<String>,
    /// label -> the line after it
    pub labels: HashMap<String, usize>,
}

const BLOCKS: [&str; 6] = [
    "SUBCOMMAND",
    "MACRO",
    "DEFINE",
    "OBJECT",
    "PRELOAD",
    "KEYBLOCK",
];

impl Fes {
    pub fn parse_packed(b: &[u8]) -> Result<Fes, String> {
        let text = crate::cst::unpack(b, b"FES\0", 16)?;
        Ok(Fes::parse(&crate::sjis(&text)))
    }

    pub fn parse(text: &str) -> Fes {
        let mut f = Fes::default();
        let mut block = String::new();
        let mut table: Option<(Vec<String>, Vec<Vec<String>>)> = None;
        let flush = |f: &mut Fes, table: &mut Option<(Vec<String>, Vec<Vec<String>>)>| {
            if let Some((cols, rows)) = table.take() {
                f.defs.push(Def::Object(cols, rows));
            }
        };
        for raw in text.lines() {
            let line = match raw.find("//") {
                Some(i) => &raw[..i],
                None => raw,
            }
            .trim();
            if line.is_empty() {
                continue;
            }
            if let Some(head) = line.strip_prefix('#') {
                flush(&mut f, &mut table);
                let mut words = head.split_whitespace();
                let first = words.next().unwrap_or("");
                if BLOCKS.contains(&first) {
                    block = first.to_string();
                    if first == "OBJECT" {
                        table = Some((words.map(str::to_string).collect(), Vec::new()));
                    }
                } else {
                    block.clear();
                    f.lines.push(line.to_string());
                    f.labels.insert(first.to_string(), f.lines.len());
                }
                continue;
            }
            match block.as_str() {
                "" => f.lines.push(line.to_string()),
                "MACRO" => {
                    let mut w = line.splitn(2, char::is_whitespace);
                    let k = w.next().unwrap_or("").to_string();
                    f.macros
                        .insert(k, w.next().unwrap_or("").trim().to_string());
                }
                "DEFINE" => {
                    let mut w = line.split_whitespace();
                    if let (Some(k), Some(n)) = (w.next(), w.next()) {
                        f.defs.push(Def::Define(k.to_string(), n.to_string()));
                    }
                }
                "OBJECT" => {
                    if let Some((_, rows)) = &mut table {
                        rows.push(line.split_whitespace().map(str::to_string).collect());
                    }
                }
                _ => {}
            }
        }
        flush(&mut f, &mut table);
        f
    }
}
