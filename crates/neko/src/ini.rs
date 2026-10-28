//! The settings files of cfg.pak (`init.ini`: "Key<tabs>value<tabs>// note").

use std::collections::HashMap;

pub struct Ini {
    values: HashMap<String, String>,
}

impl Ini {
    pub fn parse(b: &[u8]) -> Ini {
        let text = encoding_rs::SHIFT_JIS.decode(b).0;
        let mut values = HashMap::new();
        for line in text.lines() {
            let line = line.split("//").next().unwrap_or("").trim();
            let mut it = line.split_whitespace();
            if let (Some(k), Some(v)) = (it.next(), it.next()) {
                values.insert(k.to_lowercase(), v.to_string());
            }
        }
        Ini { values }
    }

    pub fn str(&self, k: &str) -> Option<&str> {
        self.values.get(&k.to_lowercase()).map(String::as_str)
    }

    pub fn int(&self, k: &str, default: i32) -> i32 {
        self.str(k).and_then(|v| v.parse().ok()).unwrap_or(default)
    }
}
