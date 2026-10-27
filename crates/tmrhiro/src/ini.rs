//! The settings files in srp.pac (`_system`, `_charaview`, `_filemacro`, ...): `[SECTION]`
//! headers and `KEY=value` lines. Older games keep them as text lines; newer ones as
//! length:u16 + Shift_JIS records.

use std::collections::HashMap;

#[derive(Default)]
pub struct Ini {
    /// "SECTION/KEY" (upper case) to value
    pub values: HashMap<String, String>,
    /// the lines of each section, in order
    pub sections: HashMap<String, Vec<String>>,
}

pub fn lines(b: &[u8]) -> Vec<String> {
    if b.first() == Some(&b'[') {
        return nexas::sjis::decode(b)
            .lines()
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at + 2 <= b.len() {
        let n = u16::from_le_bytes([b[at], b[at + 1]]) as usize;
        let end = (at + 2 + n).min(b.len());
        out.push(nexas::sjis::decode(&b[at + 2..end]));
        at = end;
    }
    out
}

impl Ini {
    pub fn parse(b: &[u8]) -> Ini {
        let mut ini = Ini::default();
        ini.add(b);
        ini
    }

    pub fn add(&mut self, b: &[u8]) {
        let mut section = String::new();
        for line in lines(b) {
            let line = line.trim();
            if let Some(s) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                section = s.to_uppercase();
                continue;
            }
            if line.is_empty() {
                continue;
            }
            self.sections
                .entry(section.clone())
                .or_default()
                .push(line.to_string());
            if let Some((k, v)) = line.split_once('=') {
                self.values.insert(
                    format!("{section}/{}", k.trim().to_uppercase()),
                    v.trim().to_string(),
                );
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }

    pub fn int(&self, key: &str, default: i32) -> i32 {
        self.get(key)
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(default)
    }

    /// A comma separated list of numbers.
    pub fn ints(&self, key: &str) -> Vec<i32> {
        self.get(key)
            .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect())
            .unwrap_or_default()
    }

    /// A hex colour such as `ffffff`.
    pub fn color(&self, key: &str, default: [u8; 3]) -> [u8; 3] {
        self.get(key)
            .and_then(|v| u32::from_str_radix(v.trim().trim_start_matches("0x"), 16).ok())
            .map_or(default, |c| [(c >> 16) as u8, (c >> 8) as u8, c as u8])
    }
}
