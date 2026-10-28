//! PRODUCT: commands a game's own engine build adds. けもの道☆ガーリッシュスクエア (555) reads its
//! scenario .txt files at run time with them; the numbers (COM=) and what they do were read
//! from its exe:
//!
//! | COM | does |
//! |---|---|
//! | 21 | lists the folder PSTR into $_FILES(n) (names inside it) and @_FILES (the count) |
//! | 22 | splits the line PSTR into tokens, see [`tokens`] |
//! | 23 | the lengths of strings from address PINT on, into numbers from address PINT2 on, up to the first empty one; LET = how many |
//! | 29 | LET = 0: the scenario is in loose files (not compiled in), so COM 21 lists them |

/// What [`tokens`] gives each token (the int array COM 22 fills).
pub mod kind {
    /// a line break: CR inside a /* */ comment, or a // comment (which ends the line)
    pub const BREAK: i64 = 0;
    pub const INCREMENT: i64 = 0x0b;
    pub const ADD: i64 = 0x0c;
    /// any other single character (`=`, `<`, CR, ...)
    pub const SYMBOL: i64 = 0x10;
    /// "..." with its quotes
    pub const QUOTED: i64 = 0x1f;
    /// a run of digits, letters, `_ . /` and full-width characters (also `-1`)
    pub const WORD: i64 = 0x20;
    pub const BACKSLASH: i64 = 0x33;
    pub const OPEN: i64 = 0x34;
    pub const CLOSE: i64 = 0x35;
    pub const COMMA: i64 = 0x36;
    pub const AT: i64 = 0x37;
    pub const DOLLAR: i64 = 0x38;
    pub const HASH: i64 = 0x39;
}

fn lead(b: u8) -> bool {
    matches!(b, 0x81..=0x9f | 0xe0..=0xfc)
}

#[derive(Clone, Copy, PartialEq)]
enum St {
    Start,
    Word,
    Quote,
    Spaces,
    Tabs,
    Comment,
}

/// COM 22: the tokens of a line (Shift_JIS) and their kinds, as the exe's tokenizer splits it,
/// with whether the line ends inside a /* */ comment. `in_comment` is that state from the line
/// before; a line cut by // gives no new state (the exe leaves it as it was). Spaces separate tokens and are dropped; a word or quote the line ends in is dropped
/// too (the lines keep their CR, which ends them).
pub fn tokens(s: &[u8], in_comment: bool) -> (Vec<(Vec<u8>, i64)>, Option<bool>) {
    let mut out: Vec<(Vec<u8>, i64)> = Vec::new();
    let mut comment = in_comment;
    let mut st = if in_comment { St::Comment } else { St::Start };
    let (mut i, mut start) = (0usize, 0usize);
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    while i < s.len() {
        let (c, next) = (s[i], at(i + 1));
        let len = if lead(c) { 2 } else { 1 };
        match st {
            St::Start => {
                let one = |kind: i64, out: &mut Vec<(Vec<u8>, i64)>| out.push((vec![c], kind));
                match c {
                    b'0'..=b'9' | b'-' | b'A'..=b'Z' | b'a'..=b'z' | b'_' | b'.' => {
                        i += 1;
                        st = St::Word;
                    }
                    _ if lead(c) => {
                        i += len;
                        st = St::Word;
                    }
                    b'"' => {
                        i += 1;
                        st = St::Quote;
                    }
                    b' ' => {
                        i += 1;
                        st = St::Spaces;
                    }
                    b'\t' => {
                        i += 1;
                        st = St::Tabs;
                    }
                    b'/' if next == b'/' => {
                        out.push((b"\r".to_vec(), kind::BREAK));
                        return (out, None);
                    }
                    b'/' if next == b'*' => {
                        i += 1;
                        comment = true;
                        st = St::Comment;
                    }
                    b'\\' | b'(' | b')' | b',' | b'@' | b'$' | b'#' => {
                        let kind = match c {
                            b'\\' => kind::BACKSLASH,
                            b'(' => kind::OPEN,
                            b')' => kind::CLOSE,
                            b',' => kind::COMMA,
                            b'@' => kind::AT,
                            b'$' => kind::DOLLAR,
                            _ => kind::HASH,
                        };
                        one(kind, &mut out);
                        // `#=` is one `#`
                        i += if c == b'#' && next == b'=' { 2 } else { 1 };
                        start = i;
                    }
                    b'+' if next == b'+' || next == b'=' => {
                        out.push((vec![c, next], if next == b'+' { kind::INCREMENT } else { kind::ADD }));
                        i += 2;
                        start = i;
                    }
                    b';' => {
                        i += 1;
                        start = i;
                    }
                    _ => {
                        out.push((s[start.min(i)..=i].to_vec(), kind::SYMBOL));
                        i += len;
                        start = i;
                    }
                }
            }
            St::Word => {
                if c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'/') || lead(c) {
                    i += len;
                } else {
                    out.push((s[start..i].to_vec(), kind::WORD));
                    start = i;
                    st = St::Start;
                }
            }
            St::Quote => {
                if c == b'"' || c == b'\r' {
                    out.push((s[start..=i].to_vec(), kind::QUOTED));
                    i += len;
                    start = i;
                    st = St::Start;
                } else {
                    i += 1;
                }
            }
            St::Spaces | St::Tabs => {
                if c == if st == St::Spaces { b' ' } else { b'\t' } {
                    i += 1;
                } else {
                    start = i;
                    st = St::Start;
                }
            }
            St::Comment => {
                if c == b'*' && next == b'/' {
                    i += 2;
                    start = i;
                    comment = false;
                    st = St::Start;
                } else if c == b'\r' {
                    out.push((b"\r".to_vec(), kind::BREAK));
                    i += 1;
                } else {
                    i += 1;
                }
            }
        }
    }
    (out, Some(comment))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(s: &str) -> Vec<(String, i64)> {
        tokens(s.as_bytes(), false).0.into_iter().map(|(t, k)| (String::from_utf8(t).unwrap(), k)).collect()
    }

    #[test]
    fn splits_a_command() {
        let t = texts("\\bg(\"a b\",-1) @x+=2;\r\n");
        let want: Vec<(String, i64)> = [
            ("\\", kind::BACKSLASH),
            ("bg", kind::WORD),
            ("(", kind::OPEN),
            ("\"a b\"", kind::QUOTED),
            (",", kind::COMMA),
            ("-1", kind::WORD),
            (")", kind::CLOSE),
            ("@", kind::AT),
            ("x", kind::WORD),
            ("+=", kind::ADD),
            ("2", kind::WORD),
            ("\r", kind::SYMBOL),
            ("\n", kind::SYMBOL),
        ]
        .iter()
        .map(|&(s, k)| (s.to_string(), k))
        .collect();
        assert_eq!(t, want);
    }

    #[test]
    fn comments() {
        assert_eq!(texts("a // b\r"), vec![("a".into(), kind::WORD), ("\r".into(), kind::BREAK)]);
        let (t, open) = tokens(b"a /* b\r", false);
        assert_eq!(open, Some(true));
        assert_eq!(t.len(), 2);
        let (t, open) = tokens(b"c */d\r", true);
        assert_eq!(open, Some(false));
        assert_eq!(t[0], (b"d".to_vec(), kind::WORD));
    }
}
