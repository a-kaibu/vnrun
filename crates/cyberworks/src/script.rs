//! The scripts ('a' entries, numbered from 0): statements of a u32 length and tokens.
//!
//! ```text
//! 'M' '#' N<cmd> args...      a command        'N' i32: a number   'A' i32: a variable
//! 'M' '$' N<layer> args...    a layer command  'S' u32 n, n bytes: text (each byte ^ n)
//! 'M' 'L' N0                  a label (jumps go to statement numbers)
//! 'S' ...                     a line of the message
//! ```
//!
//! Text is Shift_JIS ending in 0xfe; `ff ff n <n letters> m <m letters>` is a reading (n
//! letters) over a word (m letters).

use encoding_rs::SHIFT_JIS;

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Num(i32),
    Var(i32),
    Str(String),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    /// a numbered command and its arguments
    Cmd(i32, Vec<Arg>),
    /// a layer command: the layer, then its arguments
    Layer(Vec<Arg>),
    Label,
    Text(String),
    Other,
}

fn u32at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn sjis_len(b: u8) -> usize {
    if (0x81..=0x9f).contains(&b) || (0xe0..=0xfc).contains(&b) {
        2
    } else {
        1
    }
}

/// A text token's bytes made readable: the readings are dropped, the words kept.
pub fn text(s: &[u8]) -> String {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    let letters = |i: &mut usize, n: usize, keep: Option<&mut Vec<u8>>| {
        let start = *i;
        for _ in 0..n {
            if *i >= s.len() {
                break;
            }
            *i += sjis_len(s[*i]);
        }
        if let Some(k) = keep {
            k.extend_from_slice(&s[start..(*i).min(s.len())]);
        }
    };
    while i < s.len() {
        match s[i] {
            0xfe => i += 1,
            0xff if s.get(i + 1) == Some(&0xff) => {
                i += 2;
                let n = s.get(i).copied().unwrap_or(0) as usize;
                i += 1;
                letters(&mut i, n, None);
                let m = s.get(i).copied().unwrap_or(0) as usize;
                i += 1;
                letters(&mut i, m, Some(&mut out));
            }
            b if !(0x20..0xfd).contains(&b) => i += 1,
            b => {
                let n = sjis_len(b).min(s.len() - i);
                out.extend_from_slice(&s[i..i + n]);
                i += n;
            }
        }
    }
    SHIFT_JIS.decode(&out).0.into_owned()
}

fn tokens(b: &[u8]) -> Vec<Result<Arg, u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        i += 1;
        match c {
            b'N' | b'A' | b'V' => {
                let v = u32at(b, i).unwrap_or(0) as i32;
                i += 4;
                out.push(Ok(if c == b'N' { Arg::Num(v) } else { Arg::Var(v) }));
            }
            b'S' => {
                let n = u32at(b, i).unwrap_or(0) as usize;
                let s: Vec<u8> = b
                    .get(i + 4..(i + 4 + n).min(b.len()))
                    .unwrap_or(&[])
                    .iter()
                    .map(|x| x ^ n as u8)
                    .collect();
                i += 4 + n;
                out.push(Ok(Arg::Str(text(&s))));
            }
            _ => out.push(Err(c)),
        }
    }
    out
}

pub fn parse(b: &[u8]) -> Vec<Stmt> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(n) = u32at(b, at) {
        let n = n as usize;
        let Some(body) = b.get(at + 4..at + 4 + n) else {
            break;
        };
        at += 4 + n;
        let t = tokens(body);
        let args = |from: usize| -> Vec<Arg> {
            t[from.min(t.len())..]
                .iter()
                .filter_map(|x| x.clone().ok())
                .collect()
        };
        out.push(match t.as_slice() {
            [Err(b'M'), Err(b'#'), Ok(Arg::Num(c)), ..] => Stmt::Cmd(*c, args(3)),
            [Err(b'M'), Err(b'$'), ..] => Stmt::Layer(args(2)),
            [Err(b'M'), Err(b'L'), ..] => Stmt::Label,
            [Ok(Arg::Str(s)), ..] => Stmt::Text(s.clone()),
            _ => Stmt::Other,
        });
    }
    out
}
