//! Tokens of ACPX C. Sources are Shift_JIS: strings and character constants keep their bytes,
//! and a lead byte takes the next byte with it (so a trail byte of 0x5C is not an escape).

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    /// value, unsigned
    Int(u32, bool),
    Float(f32),
    Str(Vec<u8>),
    Punct(&'static str),
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    /// first on its line
    pub bol: bool,
    /// whitespace before it
    pub space: bool,
    pub file: u32,
    pub line: u32,
}

const PUNCTS: [&str; 48] = [
    "<<=", ">>=", "...", "->", "++", "--", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+=",
    "-=", "*=", "/=", "%=", "&=", "|=", "^=", "##", "+", "-", "*", "/", "%", "&", "|", "^", "~",
    "!", "<", ">", "=", "?", ":", ";", ",", ".", "(", ")", "[", "]", "{", "}", "#",
];

pub fn is_lead(c: u8) -> bool {
    (0x81..=0x9f).contains(&c) || (0xe0..=0xfc).contains(&c)
}

fn escape(b: &[u8], i: &mut usize) -> u8 {
    let c = b.get(*i).copied().unwrap_or(0);
    *i += 1;
    match c {
        b'n' => b'\n',
        b't' => b'\t',
        b'r' => b'\r',
        b'0'..=b'7' => {
            let mut v = (c - b'0') as u32;
            for _ in 0..2 {
                match b.get(*i) {
                    Some(&d @ b'0'..=b'7') => {
                        v = v * 8 + (d - b'0') as u32;
                        *i += 1;
                    }
                    _ => break,
                }
            }
            v as u8
        }
        b'x' => {
            let mut v = 0u32;
            while let Some(&d) = b.get(*i) {
                let Some(h) = (d as char).to_digit(16) else { break };
                v = v * 16 + h;
                *i += 1;
            }
            v as u8
        }
        b'a' => 7,
        b'b' => 8,
        b'f' => 12,
        b'v' => 11,
        _ => c,
    }
}

/// The tokens of one file.
pub fn lex(b: &[u8], file: u32) -> Result<Vec<Token>, String> {
    let mut out: Vec<Token> = Vec::new();
    let (mut i, mut line, mut bol, mut space) = (0usize, 1u32, true, false);
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            line += 1;
            bol = true;
            space = true;
            i += 1;
            continue;
        }
        if c == b' ' || c == b'\t' || c == b'\r' || c == 0x0c || c == 0x1a {
            space = true;
            i += 1;
            continue;
        }
        if c == b'\\' && matches!(b.get(i + 1), Some(b'\n') | Some(b'\r')) {
            // a continued line
            i += 1;
            if b.get(i) == Some(&b'\r') {
                i += 1;
            }
            if b.get(i) == Some(&b'\n') {
                i += 1;
            }
            line += 1;
            space = true;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                if is_lead(b[i]) {
                    i += 1;
                }
                i += 1;
            }
            space = true;
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                if b[i] == b'\n' {
                    line += 1;
                } else if is_lead(b[i]) {
                    i += 1;
                }
                i += 1;
            }
            i += 2;
            space = true;
            continue;
        }
        if c == 0x81 && b.get(i + 1) == Some(&0x40) {
            // a full-width space
            i += 2;
            space = true;
            continue;
        }
        let start_line = line;
        let tok = if c.is_ascii_alphabetic() || c == b'_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            Tok::Ident(String::from_utf8_lossy(&b[s..i]).into_owned())
        } else if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let s = i;
            if c == b'0' && matches!(b.get(i + 1), Some(b'x') | Some(b'X')) {
                i += 2;
                let h = i;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let v = u64::from_str_radix(std::str::from_utf8(&b[h..i]).unwrap(), 16)
                    .map_err(|e| format!("line {line}: {e}"))?;
                let mut uns = v > i32::MAX as u64;
                while i < b.len() && matches!(b[i], b'u' | b'U' | b'l' | b'L') {
                    uns |= matches!(b[i], b'u' | b'U');
                    i += 1;
                }
                Tok::Int(v as u32, uns)
            } else {
                while i < b.len() && (b[i].is_ascii_digit()) {
                    i += 1;
                }
                let mut float = false;
                if b.get(i) == Some(&b'.') {
                    float = true;
                    i += 1;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if matches!(b.get(i), Some(b'e') | Some(b'E')) {
                    float = true;
                    i += 1;
                    if matches!(b.get(i), Some(b'+') | Some(b'-')) {
                        i += 1;
                    }
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let text = std::str::from_utf8(&b[s..i]).unwrap().to_string();
                if float || matches!(b.get(i), Some(b'f') | Some(b'F')) {
                    if matches!(b.get(i), Some(b'f') | Some(b'F') | Some(b'l') | Some(b'L')) {
                        i += 1;
                    }
                    Tok::Float(text.parse::<f64>().map_err(|e| format!("line {line}: {e}"))? as f32)
                } else {
                    let v: u64 = if text.len() > 1 && text.starts_with('0') {
                        u64::from_str_radix(&text[1..], 8).unwrap_or(0)
                    } else {
                        text.parse().map_err(|e| format!("line {line}: {e}"))?
                    };
                    let mut uns = v > i32::MAX as u64;
                    while i < b.len() && matches!(b[i], b'u' | b'U' | b'l' | b'L') {
                        uns |= matches!(b[i], b'u' | b'U');
                        i += 1;
                    }
                    Tok::Int(v as u32, uns)
                }
            }
        } else if c == b'"' || c == b'\'' {
            i += 1;
            let mut s = Vec::new();
            // the name in #include "..." has no escapes
            let n = out.len();
            let header = n >= 2
                && out[n - 2].tok == Tok::Punct("#")
                && out[n - 2].bol
                && out[n - 1].tok == Tok::Ident("include".into());
            if header {
                while i < b.len() && b[i] != c && b[i] != b'\n' {
                    s.push(b[i]);
                    i += 1;
                }
            }
            while i < b.len() && b[i] != c {
                if b[i] == b'\n' {
                    return Err(format!("line {line}: string not closed"));
                }
                if is_lead(b[i]) && i + 1 < b.len() {
                    s.push(b[i]);
                    s.push(b[i + 1]);
                    i += 2;
                } else if b[i] == b'\\' {
                    i += 1;
                    s.push(escape(b, &mut i));
                } else {
                    s.push(b[i]);
                    i += 1;
                }
            }
            i += 1;
            if c == b'"' {
                Tok::Str(s)
            } else {
                // a character constant: several bytes make one value, first byte highest
                let mut v: u32 = 0;
                for &x in &s {
                    v = (v << 8) | x as u32;
                }
                if s.len() == 1 {
                    v = s[0] as i8 as i32 as u32;
                }
                Tok::Int(v, false)
            }
        } else {
            let rest = &b[i..];
            let Some(p) = PUNCTS.iter().find(|p| rest.starts_with(p.as_bytes())) else {
                return Err(format!("line {line}: unexpected byte 0x{c:02x}"));
            };
            i += p.len();
            Tok::Punct(p)
        };
        out.push(Token {
            tok,
            bol,
            space,
            file,
            line: start_line,
        });
        bol = false;
        space = false;
    }
    Ok(out)
}
