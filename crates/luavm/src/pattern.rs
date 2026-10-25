//! Lua patterns (string.find, match, gmatch, gsub), after lstrlib.c.

const MAXCAPTURES: usize = 32;
const CAP_UNFINISHED: isize = -1;
const CAP_POSITION: isize = -2;

pub struct Matcher<'a> {
    src: &'a [u8],
    pat: &'a [u8],
    depth: usize,
    pub level: usize,
    pub capture: [(usize, isize); MAXCAPTURES],
}

pub enum Capture {
    Str(usize, usize),
    Pos(usize),
}

type R<T> = Result<T, String>;

fn class_end(p: &[u8], mut i: usize) -> R<usize> {
    let c = p[i];
    i += 1;
    if c == b'%' {
        if i >= p.len() {
            return Err("malformed pattern (ends with '%')".into());
        }
        return Ok(i + 1);
    }
    if c == b'[' {
        if i < p.len() && p[i] == b'^' {
            i += 1;
        }
        loop {
            if i >= p.len() {
                return Err("malformed pattern (missing ']')".into());
            }
            let cc = p[i];
            i += 1;
            if cc == b'%' && i < p.len() {
                i += 1;
            }
            if i >= p.len() {
                return Err("malformed pattern (missing ']')".into());
            }
            if p[i] == b']' {
                return Ok(i + 1);
            }
        }
    }
    Ok(i)
}

fn single_class(c: u8, cl: u8) -> bool {
    let r = match cl.to_ascii_lowercase() {
        b'a' => c.is_ascii_alphabetic(),
        b'c' => c.is_ascii_control(),
        b'd' => c.is_ascii_digit(),
        b'g' => c.is_ascii_graphic(),
        b'l' => c.is_ascii_lowercase(),
        b'p' => c.is_ascii_punctuation(),
        b's' => matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c),
        b'u' => c.is_ascii_uppercase(),
        b'w' => c.is_ascii_alphanumeric(),
        b'x' => c.is_ascii_hexdigit(),
        _ => return cl == c,
    };
    if cl.is_ascii_uppercase() { !r } else { r }
}

/// `p[start..end]` is a [set]
fn match_set(c: u8, p: &[u8], start: usize, end: usize) -> bool {
    let mut i = start + 1;
    let mut neg = false;
    if p[i] == b'^' {
        neg = true;
        i += 1;
    }
    let end = end - 1; // the ']'
    while i < end {
        if p[i] == b'%' {
            i += 1;
            if single_class(c, p[i]) {
                return !neg;
            }
            i += 1;
        } else if i + 2 < end && p[i + 1] == b'-' {
            if p[i] <= c && c <= p[i + 2] {
                return !neg;
            }
            i += 3;
        } else {
            if p[i] == c {
                return !neg;
            }
            i += 1;
        }
    }
    neg
}

impl<'a> Matcher<'a> {
    pub fn new(src: &'a [u8], pat: &'a [u8]) -> Self {
        Matcher {
            src,
            pat,
            depth: 0,
            level: 0,
            capture: [(0, 0); MAXCAPTURES],
        }
    }

    fn single(&self, s: usize, p: usize, ep: usize) -> bool {
        if s >= self.src.len() {
            return false;
        }
        let c = self.src[s];
        match self.pat[p] {
            b'.' => true,
            b'%' => single_class(c, self.pat[p + 1]),
            b'[' => match_set(c, self.pat, p, ep),
            pc => pc == c,
        }
    }

    pub fn do_match(&mut self, s: usize, p: usize) -> R<Option<usize>> {
        self.depth += 1;
        if self.depth > 200 {
            return Err("pattern too complex".into());
        }
        let r = self.do_match_inner(s, p);
        self.depth -= 1;
        r
    }

    fn do_match_inner(&mut self, mut s: usize, mut p: usize) -> R<Option<usize>> {
        let pat = self.pat;
        loop {
            if p >= pat.len() {
                return Ok(Some(s));
            }
            match pat[p] {
                b'(' => {
                    return if p + 1 < pat.len() && pat[p + 1] == b')' {
                        self.start_capture(s, p + 2, CAP_POSITION)
                    } else {
                        self.start_capture(s, p + 1, CAP_UNFINISHED)
                    };
                }
                b')' => return self.end_capture(s, p + 1),
                b'$' if p + 1 == pat.len() => {
                    return Ok(if s == self.src.len() { Some(s) } else { None });
                }
                b'%' if p + 1 < pat.len() && pat[p + 1] == b'b' => {
                    if p + 3 >= pat.len() {
                        return Err("missing arguments to '%b'".into());
                    }
                    if s >= self.src.len() || self.src[s] != pat[p + 2] {
                        return Ok(None);
                    }
                    let (open, close) = (pat[p + 2], pat[p + 3]);
                    let mut cont = 1;
                    let mut i = s + 1;
                    let mut found = None;
                    while i < self.src.len() {
                        let c = self.src[i];
                        if c == close {
                            cont -= 1;
                            if cont == 0 {
                                found = Some(i + 1);
                                break;
                            }
                        } else if c == open {
                            cont += 1;
                        }
                        i += 1;
                    }
                    match found {
                        Some(e) => {
                            s = e;
                            p += 4;
                            continue;
                        }
                        None => return Ok(None),
                    }
                }
                b'%' if p + 1 < pat.len() && pat[p + 1] == b'f' => {
                    p += 2;
                    if p >= pat.len() || pat[p] != b'[' {
                        return Err("missing '[' after '%f' in pattern".into());
                    }
                    let ep = class_end(pat, p)?;
                    let prev = if s == 0 { 0 } else { self.src[s - 1] };
                    let cur = self.src.get(s).copied().unwrap_or(0);
                    if !match_set(prev, pat, p, ep) && match_set(cur, pat, p, ep) {
                        p = ep;
                        continue;
                    }
                    return Ok(None);
                }
                b'%' if p + 1 < pat.len() && pat[p + 1].is_ascii_digit() => {
                    let l = (pat[p + 1] - b'1') as usize;
                    if l >= self.level || self.capture[l].1 == CAP_UNFINISHED {
                        return Err("invalid capture index".into());
                    }
                    let (cs, cl) = self.capture[l];
                    let cl = cl as usize;
                    if self.src.len() - s >= cl && self.src[cs..cs + cl] == self.src[s..s + cl] {
                        s += cl;
                        p += 2;
                        continue;
                    }
                    return Ok(None);
                }
                _ => {}
            }
            let ep = class_end(pat, p)?;
            let m = self.single(s, p, ep);
            match pat.get(ep) {
                Some(b'?') => {
                    if m
                        && let Some(r) = self.do_match(s + 1, ep + 1)? {
                            return Ok(Some(r));
                        }
                    p = ep + 1;
                }
                Some(b'+') => {
                    return if m {
                        self.max_expand(s + 1, p, ep)
                    } else {
                        Ok(None)
                    };
                }
                Some(b'*') => return self.max_expand(s, p, ep),
                Some(b'-') => return self.min_expand(s, p, ep),
                _ => {
                    if !m {
                        return Ok(None);
                    }
                    s += 1;
                    p = ep;
                }
            }
        }
    }

    fn max_expand(&mut self, s: usize, p: usize, ep: usize) -> R<Option<usize>> {
        let mut i = 0;
        while self.single(s + i, p, ep) {
            i += 1;
        }
        loop {
            if let Some(r) = self.do_match(s + i, ep + 1)? {
                return Ok(Some(r));
            }
            if i == 0 {
                return Ok(None);
            }
            i -= 1;
        }
    }

    fn min_expand(&mut self, mut s: usize, p: usize, ep: usize) -> R<Option<usize>> {
        loop {
            if let Some(r) = self.do_match(s, ep + 1)? {
                return Ok(Some(r));
            }
            if self.single(s, p, ep) {
                s += 1;
            } else {
                return Ok(None);
            }
        }
    }

    fn start_capture(&mut self, s: usize, p: usize, what: isize) -> R<Option<usize>> {
        if self.level >= MAXCAPTURES {
            return Err("too many captures".into());
        }
        self.capture[self.level] = (s, what);
        self.level += 1;
        let r = self.do_match(s, p)?;
        if r.is_none() {
            self.level -= 1;
        }
        Ok(r)
    }

    fn end_capture(&mut self, s: usize, p: usize) -> R<Option<usize>> {
        let l = (0..self.level)
            .rev()
            .find(|&i| self.capture[i].1 == CAP_UNFINISHED)
            .ok_or("invalid pattern capture")?;
        self.capture[l].1 = (s - self.capture[l].0) as isize;
        let r = self.do_match(s, p)?;
        if r.is_none() {
            self.capture[l].1 = CAP_UNFINISHED;
        }
        Ok(r)
    }

    /// Capture `i`; with no captures, capture 0 is the whole match `s..e`.
    pub fn get(&self, i: usize, s: usize, e: usize) -> Capture {
        if i >= self.level {
            return Capture::Str(s, e);
        }
        let (cs, cl) = self.capture[i];
        if cl == CAP_POSITION {
            Capture::Pos(cs + 1)
        } else {
            Capture::Str(cs, cs + cl.max(0) as usize)
        }
    }
}

/// Whether the pattern has no special characters (plain find).
pub fn is_plain(p: &[u8]) -> bool {
    !p.iter().any(|c| b"^$*+?.([%-".contains(c))
}
