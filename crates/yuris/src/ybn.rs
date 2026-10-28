//! The compiled project in ysbin/: the command table (ysc.ybn), labels (ysl.ybn), variables
//! (ysv.ybn), the list of scripts (yst_list.ybn), settings (yscfg.ybn) and one yst?????.ybn per
//! script file.

use crate::sjis;

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8], at: usize) -> Self {
        Reader { b, at }
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let out = self.b.get(self.at..self.at + n).ok_or("unexpected end of file")?;
        self.at += n;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn cstr(&mut self) -> Result<String, String> {
        let rest = &self.b[self.at.min(self.b.len())..];
        let n = rest.iter().position(|&c| c == 0).ok_or("unterminated name")?;
        self.at += n + 1;
        Ok(sjis::decode(&rest[..n]))
    }
}

fn magic(b: &[u8], m: &[u8; 4]) -> Result<(), String> {
    if b.get(..4) == Some(m) {
        Ok(())
    } else {
        Err(format!("not a {} file", String::from_utf8_lossy(m)))
    }
}

/// A command and the names of its keywords (ysc.ybn).
#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub keywords: Vec<String>,
}

pub fn commands(b: &[u8]) -> Result<Vec<Command>, String> {
    magic(b, b"YSCM")?;
    let mut r = Reader::new(b, 8);
    let count = r.u32()? as usize;
    r.u32()?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let name = r.cstr()?;
        let n = r.u8()?;
        let mut keywords = Vec::with_capacity(n as usize);
        for _ in 0..n {
            keywords.push(r.cstr()?);
            r.bytes(2)?;
        }
        out.push(Command { name, keywords });
    }
    Ok(out)
}

/// A label (ysl.ybn): where `#=NAME` points.
#[derive(Debug, Clone)]
pub struct Label {
    pub name: String,
    pub script: u16,
    pub ip: u32,
}

pub fn labels(b: &[u8]) -> Result<Vec<Label>, String> {
    magic(b, b"YSLB")?;
    let mut r = Reader::new(b, 8);
    let count = r.u32()? as usize;
    r.bytes(256 * 4)?; // first index by hash byte
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let n = r.u8()? as usize;
        let name = sjis::decode(r.bytes(n)?);
        r.u32()?; // hash
        let ip = r.u32()?;
        let script = r.u16()?;
        r.u16()?;
        out.push(Label { name, script, ip });
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarType {
    None,
    Int,
    Flt,
    Str,
}

/// A variable declared at compile time (ysv.ybn).
#[derive(Debug, Clone)]
pub struct VarDecl {
    /// 1: global (G_INT...), 2: in one script (INT... at the top level)
    pub scope: u8,
    /// 482: which of G_INT, G_INT2, G_INT3, G_INT4 (0..3) declared a global
    pub group: u8,
    pub script: u16,
    pub id: u16,
    pub ty: VarType,
    pub dims: Vec<u32>,
    /// Int/Flt: the raw 8 bytes; Str: an expression
    pub init: Vec<u8>,
}

pub fn variables(b: &[u8]) -> Result<Vec<VarDecl>, String> {
    // 482 has one more byte before the script number; the layout that ends with the file is it.
    variables_with(b, false).or_else(|e| variables_with(b, true).map_err(|_| e))
}

fn variables_with(b: &[u8], wide: bool) -> Result<Vec<VarDecl>, String> {
    magic(b, b"YSVR")?;
    let ver = version(b);
    let mut r = Reader::new(b, 8);
    let count = r.u16()? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let scope = r.u8()?;
        let group = if wide { r.u8()? } else { 0 };
        let script = r.u16()?;
        let id = r.u16()?;
        let ty = match r.u8()? {
            1 => VarType::Int,
            2 => VarType::Flt,
            3 => VarType::Str,
            _ => VarType::None,
        };
        let nd = r.u8()? as usize;
        let mut dims = Vec::with_capacity(nd);
        for _ in 0..nd {
            dims.push(r.u32()?);
        }
        let init = match ty {
            VarType::Int | VarType::Flt => r.bytes(8)?.to_vec(),
            VarType::Str => {
                let n = r.u16()? as usize;
                r.bytes(n)?.to_vec()
            }
            VarType::None => Vec::new(),
        };
        let id = sysvar(ver, id);
        out.push(VarDecl { scope, group, script, id, ty, dims, init });
    }
    if r.at != b.len() {
        return Err("variables do not end with the file".into());
    }
    Ok(out)
}

fn version(b: &[u8]) -> u32 {
    b.get(4..8).map_or(0, |v| u32::from_le_bytes(v.try_into().unwrap()))
}

/// Later versions put new system variables among the old ones; this gives them the 466 numbers
/// the VM knows (and moves the new ones out of the way). The numbers were matched by the types
/// in ysv.ybn: 300, 474, 482, 550-555 games.
fn sysvar(ver: u32, id: u16) -> u16 {
    match ver {
        // 300 (涼風のメルト): no strings at 32..47, fewer below @_TEXT and among the mouse
        // ones; the script variables (490..) move past 512
        300 => match id {
            0..=31 => id,
            32..=44 => id + 16,
            58..=65 => id + 19,
            66..=78 => id + 21,
            79..=82 => id + 22,
            83.. => id + 23,
            _ => 10000 + id,
        },
        ..474 => id,
        482..500 => match id {
            0..=49 | 541.. => id,
            53..=61 => id - 3,
            97..=98 => id - 25,
            106..=540 => id - 28,
            _ => 10000 + id,
        },
        // 550 (舞風のメルト): 16 fewer below @_LC, one more gone above 60; the script variables
        // after them move along
        550 => match id {
            0..=31 => id,
            32..=60 => id + 16,
            _ => id + 17,
        },
        // 474 (女装神社) and 551 (竜翼のメロディア)
        474..482 | 500..553 => match id {
            0..=75 | 526.. => id,
            89..=525 => id - 13,
            _ => 10000 + id,
        },
        // 553 (GIB) and 554 (鯨神のティアスティラ)
        553..=554 => match id {
            0..=60 | 538.. => id,
            96..=97 => id - 24,
            103..=537 => id - 25,
            _ => 10000 + id,
        },
        // 555 (けもの道☆ガーリッシュスクエア): new PINT.. and RINT.. sets and 22 more strings
        _ => match id {
            0..=49 | 548.. => id,
            53..=56 => id - 3,
            60..=66 => id - 6,
            105..=106 => id - 33,
            113..=547 => id - 35,
            _ => 10000 + id,
        },
    }
}

/// Renumbers the variables in expression bytes (op u8, size u16, payload) for later versions.
fn remap_expr(ver: u32, e: &mut [u8]) {
    let mut i = 0;
    while i + 3 <= e.len() {
        let (op, n) = (e[i], u16::from_le_bytes([e[i + 1], e[i + 2]]) as usize);
        if matches!(op, b'H' | b'V' | b'v') && n == 3 && i + 6 <= e.len() && matches!(e[i + 3], b'@' | b'$' | b'#' | b'`') {
            let id = sysvar(ver, u16::from_le_bytes([e[i + 4], e[i + 5]]));
            e[i + 4..i + 6].copy_from_slice(&id.to_le_bytes());
        }
        i += 3 + n;
    }
}

/// The script files in compile order (yst_list.ybn): the index is the N of yst0000N.ybn.
pub fn script_names(b: &[u8]) -> Result<Vec<String>, String> {
    magic(b, b"YSTL")?;
    let mut r = Reader::new(b, 8);
    let count = r.u32()? as usize;
    let mut out = Vec::with_capacity(count);
    // Each name is followed by 16 bytes (466) or 20 (482): the one where entry i + 1 starts.
    let mut tail = 16;
    for i in 0..count {
        r.u32()?;
        let n = r.u32()? as usize;
        out.push(sjis::decode(r.bytes(n)?));
        if i == 0 && count > 1 {
            let next = |skip: usize| b.get(r.at + skip..r.at + skip + 4).map(|v| u32::from_le_bytes(v.try_into().unwrap()));
            if next(16) != Some(1) && next(20) == Some(1) {
                tail = 20;
            }
        }
        r.bytes(tail)?;
    }
    Ok(out)
}

/// yscfg.ybn: the screen size and the window title.
#[derive(Debug, Clone)]
pub struct Config {
    pub width: u32,
    pub height: u32,
    pub title: String,
}

pub fn config(b: &[u8]) -> Result<Config, String> {
    magic(b, b"YSCF")?;
    // 263 (マギウステイル) has the size 4 bytes later and the title 4 bytes earlier
    let (size_at, title_at) = if version(b) < 300 { (20, 0x3c) } else { (16, 0x40) };
    let mut r = Reader::new(b, size_at);
    let width = r.u32()?;
    let height = r.u32()?;
    r.at = title_at;
    let n = r.u16()? as usize;
    let title = sjis::decode(r.bytes(n)?);
    Ok(Config { width, height, title })
}

/// One argument of an instruction: the keyword it sets and its value.
#[derive(Debug, Clone)]
pub struct Arg {
    /// index into the command's keywords
    pub keyword: u16,
    pub ty: u16,
    /// expression bytes; for IF/ELSE/LOOP after the first, `jump` is the target instead
    pub value: Vec<u8>,
    pub jump: u32,
}

#[derive(Debug, Clone)]
pub struct Instr {
    pub op: u8,
    pub args: Vec<Arg>,
    pub line: u32,
}

/// A compiled script (yst?????.ybn).
#[derive(Debug, Clone, Default)]
pub struct Script {
    pub code: Vec<Instr>,
}

// The default key of the script sections; projects can set their own. The argument section
// gives it away: the offset of the first argument's value is 0.
const DEFAULT_KEY: [u8; 4] = [0xd3, 0x6f, 0xac, 0x96];

/// The key of a script's sections, when it has an argument to give it away.
pub fn script_key(b: &[u8]) -> Option<[u8; 4]> {
    let h = |i: usize| b.get(8 + i * 4..12 + i * 4).map(|v| u32::from_le_bytes(v.try_into().unwrap()) as usize);
    let at = 32 + h(1)?;
    (h(2)? >= 12).then(|| b.get(at + 8..at + 12)?.try_into().ok())?
}

/// A script; `key` is the project's key for scripts without arguments (which do not show it).
pub fn script(b: &[u8], key: Option<[u8; 4]>) -> Result<Script, String> {
    magic(b, b"YSTB")?;
    let ver = version(b);
    let mut r = Reader::new(b, 8);
    let count = r.u32()? as usize;
    let code_size = r.u32()? as usize;
    let arg_size = r.u32()? as usize;
    let res_size = r.u32()? as usize;
    let line_size = r.u32()? as usize;
    r.u32()?;
    let code = r.bytes(code_size)?;
    let args = r.bytes(arg_size)?;
    let res = r.bytes(res_size)?;
    let lines = r.bytes(line_size)?;
    let key: [u8; 4] = if args.len() >= 12 { args[8..12].try_into().unwrap() } else { key.unwrap_or(DEFAULT_KEY) };
    let dec = |s: &[u8]| -> Vec<u8> { s.iter().enumerate().map(|(i, c)| c ^ key[i & 3]).collect() };
    let (code, args, res, lines) = (dec(code), dec(args), dec(res), dec(lines));
    let mut out = Vec::with_capacity(count);
    let mut ai = 0;
    for i in 0..count {
        let c = code.get(i * 4..i * 4 + 4).ok_or("short code")?;
        let (op, argc) = (c[0], c[1] as usize);
        let line = lines.get(i * 4..i * 4 + 4).map_or(0, |l| u32::from_le_bytes(l.try_into().unwrap()));
        let mut list = Vec::with_capacity(argc);
        for _ in 0..argc {
            let a = args.get(ai * 12..ai * 12 + 12).ok_or("short arguments")?;
            ai += 1;
            let keyword = u16::from_le_bytes([a[0], a[1]]);
            let ty = u16::from_le_bytes([a[2], a[3]]);
            let size = u32::from_le_bytes(a[4..8].try_into().unwrap());
            let off = u32::from_le_bytes(a[8..12].try_into().unwrap()) as usize;
            let mut value = res.get(off..off + size as usize).map(<[u8]>::to_vec).unwrap_or_default();
            remap_expr(ver, &mut value);
            list.push(Arg { keyword, ty, value, jump: size });
        }
        out.push(Instr { op, args: list, line });
    }
    Ok(Script { code: out })
}
