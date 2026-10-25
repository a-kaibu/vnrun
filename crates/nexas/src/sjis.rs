//! Shift_JIS (CP932) text, the encoding of every name and string of a NeXAS game.

pub fn decode(b: &[u8]) -> String {
    encoding_rs::SHIFT_JIS
        .decode_without_bom_handling(b)
        .0
        .into_owned()
}

pub fn encode(s: &str) -> Vec<u8> {
    encoding_rs::SHIFT_JIS.encode(s).0.into_owned()
}

/// The characters of a string: one byte, or two for a double-byte character.
pub fn chars(b: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let n = if matches!(b[i], 0x81..=0x9f | 0xe0..=0xfc) && i + 1 < b.len() {
            2
        } else {
            1
        };
        out.push(&b[i..i + n]);
        i += n;
    }
    out
}

/// A half-width character as full-width (HANTOZEN).
pub fn to_zen(c: char) -> char {
    match c {
        ' ' => '\u{3000}',
        '!'..='~' => char::from_u32(c as u32 - 0x21 + 0xff01).unwrap_or(c),
        _ => c,
    }
}

/// A full-width character as half-width, where there is one (ZENTOHAN).
pub fn to_han(c: char) -> char {
    match c {
        '\u{3000}' => ' ',
        '\u{ff01}'..='\u{ff5e}' => char::from_u32(c as u32 - 0xff01 + 0x21).unwrap_or(c),
        _ => c,
    }
}
