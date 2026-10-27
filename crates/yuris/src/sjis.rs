//! Shift_JIS (CP932) text, the encoding of every name and string in a YU-RIS project, or Big5
//! for the strings of a Traditional Chinese build (its archives still name files in Shift_JIS).

use std::sync::atomic::{AtomicBool, Ordering};

static BIG5: AtomicBool = AtomicBool::new(false);

/// Strings in the scripts are Big5 from now on (or Shift_JIS again).
pub fn set_big5(on: bool) {
    BIG5.store(on, Ordering::Relaxed);
}

fn big5() -> bool {
    BIG5.load(Ordering::Relaxed)
}

fn codec() -> &'static encoding_rs::Encoding {
    if big5() { encoding_rs::BIG5 } else { encoding_rs::SHIFT_JIS }
}

pub fn decode(b: &[u8]) -> String {
    let (s, bad) = codec().decode_without_bom_handling(b);
    // a Big5 build still has Shift_JIS strings of its own (file and font names)
    if bad { decode_name(b) } else { s.into_owned() }
}

pub fn encode(s: &str) -> Vec<u8> {
    codec().encode(s).0.into_owned()
}

/// A name stored in an archive index, which is Shift_JIS whatever the scripts use.
pub fn decode_name(b: &[u8]) -> String {
    encoding_rs::SHIFT_JIS.decode_without_bom_handling(b).0.into_owned()
}

/// A file name from a script as an archive spells it: a Big5 build's scripts keep the
/// Shift_JIS bytes of Japanese file names, which read as other characters in Big5.
pub fn archive_name(name: &str) -> Option<String> {
    if !big5() || name.is_ascii() {
        return None;
    }
    let (b, _, bad) = encoding_rs::BIG5.encode(name);
    (!bad).then(|| decode_name(&b))
}

/// Whether the text of a project's scripts is Big5: read as Shift_JIS, a lot of it comes out
/// as half-width katakana (Big5 lead bytes 0xa1..0xdf), and it reads as Big5 without errors.
pub fn looks_big5(text: &[u8]) -> bool {
    let (sjis, _) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(text);
    let wide = sjis.chars().filter(|c| !c.is_ascii()).count();
    let kana = sjis.chars().filter(|c| ('\u{ff61}'..='\u{ff9f}').contains(c)).count();
    let (_, bad) = encoding_rs::BIG5.decode_without_bom_handling(text);
    wide > 0 && kana * 3 > wide && !bad
}

/// The characters of a string: one byte, or two for a double-byte character.
pub fn chars(b: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let lead = if big5() { matches!(b[i], 0x81..=0xfe) } else { matches!(b[i], 0x81..=0x9f | 0xe0..=0xfc) };
        let n = if lead && i + 1 < b.len() { 2 } else { 1 };
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
