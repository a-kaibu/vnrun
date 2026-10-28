//! BSXScript (bsxx.dat): "BSXScript 2.0" or "3.x" (the whole file NOTed when the signature
//! does not match). After the signature: the stack size (0x10), the register count (0x14), the
//! sizes of two variable areas (0x1c system, 0x20 global), the register table (count, entries
//! at the end of the header), then (offset, size) pairs: the code, the labels (code offset,
//! name position), their names, the strings (positions, UTF-16 text), the system variable
//! names, the global variable names, the speaker names and the messages. 3.x adds one word
//! before the code's pair and two more tables before the speaker names. The older "BSScript
//! Rev.6" (bsx.dat, 2007-2008) has the 2.0 layout with Shift-JIS text (byte positions).

fn u32_at(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4)
        .map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
}

#[derive(Debug, Default)]
pub struct Script {
    /// the format's major version (2 or 3; 1 for BSScript Rev.6)
    pub major: u8,
    pub code: Vec<u8>,
    /// label number -> code offset
    pub labels: Vec<u32>,
    pub label_names: Vec<String>,
    pub strings: Vec<String>,
    pub names: Vec<String>,
    pub messages: Vec<String>,
    pub global_names: Vec<String>,
    pub stack: usize,
    pub regs: usize,
    pub sys: usize,
    pub globals: usize,
}

/// The UTF-16 string at unit `at` of a text area.
fn wstr(text: &[u8], at: usize) -> String {
    let mut out = Vec::new();
    let mut i = at * 2;
    while let Some(s) = text.get(i..i + 2) {
        let c = u16::from_le_bytes([s[0], s[1]]);
        if c == 0 {
            break;
        }
        out.push(c);
        i += 2;
    }
    String::from_utf16_lossy(&out)
}

/// The Shift-JIS string at byte `at` of a text area.
fn sstr(text: &[u8], at: usize) -> String {
    let t = text.get(at..).unwrap_or(&[]);
    let end = t.iter().position(|&c| c == 0).unwrap_or(t.len());
    nexas::sjis::decode(&t[..end])
}

/// A table of positions (u32 each) into a text area.
fn table(b: &[u8], idx: (usize, usize), text: (usize, usize), sjis: bool) -> Vec<String> {
    let t = b.get(text.0..text.0 + text.1).unwrap_or(&[]);
    (0..idx.1 / 4)
        .map(|i| {
            let at = u32_at(b, idx.0 + i * 4) as usize;
            if sjis { sstr(t, at) } else { wstr(t, at) }
        })
        .collect()
}

impl Script {
    pub fn parse(raw: &[u8]) -> Result<Script, String> {
        let b: Vec<u8> = if raw.starts_with(b"BSXScript") || raw.starts_with(b"BSScript") {
            raw.to_vec()
        } else {
            raw.iter().map(|c| !c).collect()
        };
        let sjis = b.starts_with(b"BSScript Rev.6");
        if !b.starts_with(b"BSXScript ") && !sjis {
            return Err("bsxx.dat: not a BSXScript".into());
        }
        let major = if sjis { b'1' } else { b[10] };
        // the (offset, size) pairs start at 0x28 (2.x) or 0x2c (3.x)
        let p = if major >= b'3' { 0x2c } else { 0x28 };
        let pair = |k: usize| {
            (
                u32_at(&b, p + k * 8) as usize,
                u32_at(&b, p + 4 + k * 8) as usize,
            )
        };
        let (code_at, code_len) = pair(0);
        // 2.x: (labels at, size), (label names at, ...): the pairs from here on are offset first
        let rest = p + 8;
        let pr = |k: usize| {
            (
                u32_at(&b, rest + k * 8 + 4) as usize,
                u32_at(&b, rest + k * 8 + 8) as usize,
            )
        };
        // the label table: offset at rest+4 with its size; names after
        let lab = pr(0);
        let lab_names = pr(1);
        let str_idx = pr(2);
        let strs = pr(3);
        let gname_idx = pr(6);
        let gnames = pr(7);
        // 3.x: two more tables (empty in the trials) before the speaker names
        let more = if major >= b'3' { 2 } else { 0 };
        let name_idx = pr(8 + more);
        let names = pr(9 + more);
        let msg_idx = pr(10 + more);
        let msgs = pr(11 + more);
        let code = b
            .get(code_at..code_at + code_len)
            .ok_or("bsxx.dat: code out of range")?
            .to_vec();
        let ln = b.get(lab_names.0..lab_names.0 + lab_names.1).unwrap_or(&[]);
        let mut labels = Vec::new();
        let mut label_names = Vec::new();
        for i in 0..lab.1 / 8 {
            labels.push(u32_at(&b, lab.0 + i * 8));
            let at = u32_at(&b, lab.0 + i * 8 + 4) as usize;
            label_names.push(if sjis { sstr(ln, at) } else { wstr(ln, at) });
        }
        Ok(Script {
            major: major.saturating_sub(b'0'),
            code,
            labels,
            label_names,
            strings: table(&b, str_idx, strs, sjis),
            names: table(&b, name_idx, names, sjis),
            messages: table(&b, msg_idx, msgs, sjis),
            global_names: table(&b, gname_idx, gnames, sjis),
            stack: (u32_at(&b, 0x10) as usize).clamp(16, 1 << 16),
            regs: (u32_at(&b, 0x14) as usize).clamp(1, 1 << 12),
            sys: (u32_at(&b, 0x1c) as usize).min(1 << 16),
            globals: (u32_at(&b, 0x20) as usize).min(1 << 20),
        })
    }

    pub fn label(&self, name: &str) -> Option<usize> {
        let i = self.label_names.iter().position(|n| n == name)?;
        self.labels.get(i).map(|&o| o as usize)
    }

    pub fn string(&self, i: i32) -> &str {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.strings.get(i))
            .map_or("", |s| s.as_str())
    }
}
