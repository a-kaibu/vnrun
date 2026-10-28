//! bsxx.cfg: Shift_JIS text in `[section]`s of `key = value ; comment` lines; values are
//! often lists (`btn_skip, 4, 100, 578, game`).

use std::collections::HashMap;

#[derive(Default, Debug)]
pub struct Cfg {
    /// (section, key) -> value, all lower case keys
    map: HashMap<(String, String), String>,
}

impl Cfg {
    pub fn parse(text: &str) -> Cfg {
        let mut map = HashMap::new();
        let mut section = String::new();
        for line in text.lines() {
            let line = line.split(';').next().unwrap_or("").trim();
            if let Some(s) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                section = s.trim().to_lowercase();
            } else if let Some((k, v)) = line.split_once('=') {
                map.insert(
                    (section.clone(), k.trim().to_lowercase()),
                    v.trim().to_string(),
                );
            }
        }
        Cfg { map }
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.map
            .get(&(section.to_string(), key.to_string()))
            .map(|s| s.as_str())
    }

    /// A value as a list of its comma separated parts.
    pub fn list(&self, section: &str, key: &str) -> Vec<String> {
        self.get(section, key)
            .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default()
    }

    /// The numbers of a value (hex with 0x allowed); others are left out.
    pub fn nums(&self, section: &str, key: &str) -> Vec<i64> {
        self.list(section, key)
            .iter()
            .filter_map(|s| num(s))
            .collect()
    }
}

/// A number, or sums and products of numbers (`768-150`, `37+160*2`).
pub fn num(s: &str) -> Option<i64> {
    let mut total = 0i64;
    // the sum's terms: products of plain numbers
    let mut term: Option<i64> = None;
    let mut sign = 1i64;
    let mut op = '*';
    let mut rest = s.trim();
    loop {
        let end = rest
            .find(['+', '-', '*', '/'])
            .filter(|&i| i > 0 || rest.is_empty());
        let (tok, next) = match end {
            Some(i) => (&rest[..i], Some(rest.as_bytes()[i] as char)),
            None => (rest, None),
        };
        let v = plain(tok)?;
        term = Some(match (term, op) {
            (None, _) => v,
            (Some(t), '*') => t * v,
            (Some(t), _) => t.checked_div(v)?,
        });
        match next {
            None => return Some(total + sign * term?),
            Some(c @ ('+' | '-')) => {
                total += sign * term?;
                term = None;
                sign = if c == '-' { -1 } else { 1 };
            }
            Some(c) => op = c,
        }
        rest = rest[end? + 1..].trim_start();
    }
}

fn plain(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(h, 16).ok()
    } else if let Some(n) = s.strip_prefix('-') {
        plain(n).map(|v| -v)
    } else {
        s.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_and_lists() {
        let c = Cfg::parse(
            "[Title Phase]\nstart = title_menu_start, 2, 630, 266 ; x\nbg color = 0x55000000\n",
        );
        assert_eq!(c.list("title phase", "start")[0], "title_menu_start");
        assert_eq!(c.nums("title phase", "start"), vec![2, 630, 266]);
        assert_eq!(c.nums("title phase", "bg color"), vec![0x55000000]);
        assert_eq!(num("768-150"), Some(618));
        assert_eq!(num("37+160*2"), Some(357));
        assert_eq!(num("-4"), Some(-4));
        assert_eq!(num("8/2-1"), Some(3));
        assert_eq!(num("abc"), None);
    }
}
