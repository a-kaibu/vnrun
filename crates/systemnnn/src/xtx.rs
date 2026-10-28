//! Settings and lists: `*.xtx` are CP932 text, `*.fxf` the same XORed with 0xFF. A setting is a
//! line `name value`; lines starting with `@` or `//` are off.

use std::collections::HashMap;

pub fn text(b: &[u8], fxf: bool) -> String {
    if fxf {
        let x: Vec<u8> = b.iter().map(|c| c ^ 0xff).collect();
        nexas::sjis::decode(&x)
    } else {
        nexas::sjis::decode(b)
    }
}

#[derive(Default, Debug, Clone)]
pub struct Config {
    pub map: HashMap<String, String>,
}

impl Config {
    pub fn parse(t: &str) -> Config {
        let mut map = HashMap::new();
        for l in t.lines() {
            let l = l.trim();
            if l.is_empty() || l.starts_with('@') || l.starts_with("//") {
                continue;
            }
            let mut it = l.splitn(2, [' ', '\t']);
            let k = it.next().unwrap_or("").to_lowercase();
            let v = it.next().unwrap_or("").trim();
            map.entry(k).or_insert_with(|| v.to_string());
        }
        Config { map }
    }

    pub fn s(&self, k: &str) -> Option<&str> {
        self.map
            .get(&k.to_lowercase())
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
    }

    pub fn i(&self, k: &str) -> Option<i32> {
        self.s(k)?.split_whitespace().next()?.parse().ok()
    }

    pub fn i_or(&self, k: &str, d: i32) -> i32 {
        self.i(k).unwrap_or(d)
    }
}

/// The first field of each line of a list (`name,comment`).
pub fn list(t: &str) -> Vec<String> {
    t.lines()
        .map(|l| l.split(',').next().unwrap_or("").trim().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings() {
        let c = Config::parse("modename title\n@off 1\n\tprintX 112\nfilename ta_x\n");
        assert_eq!(c.s("modeName"), Some("title"));
        assert_eq!(c.i("printx"), Some(112));
        assert_eq!(c.s("off"), None);
        assert_eq!(list("a,b\n?,c")[1], "?");
    }
}
