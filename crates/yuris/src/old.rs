//! The first compiled format (version 222, いな☆こい！): no ysc.ybn, the command table is built
//! into the engine; the scripts keep their expressions as text and jump by byte offsets. Each
//! file is read into the later form ([`Script`], binary expressions, instruction numbers), so
//! the VM runs it unchanged.
//!
//! - yst?????.ybn: `YSTB`, version, code size, text size, text start, 3 zeros; the rest XOR
//!   0x80. An instruction is `op:u8 argc:u8 line:u32`, then per argument `keyword:u16 0:u16
//!   type:u32 size:u32 offset:u32` (type as later: low byte int/flt/str, high byte `+=`...).
//!   For IF/ELSE/LOOP the arguments after the first are jumps: `size` is the byte offset.
//! - expressions: postfix tokens with a space after each, `I12`, `F1.5`, `M"text"`, `H@01001`
//!   (variable), `H@01001 ( I1 , I2 )` (element), `H@01001 V` (the variable), operators as in
//!   the binary form; they end at a NUL.
//! - ysl.ybn: count, then 52 bytes each: name[32], hash, byte offset, script, 2 zeros.
//! - ysv.ybn: count, then id:u32 type:u8 dims:u8 sizes:u32[dims] and the start value (8 bytes,
//!   or for a string u16 length, u16, the expression).
//! - yst_list.ybn: count, then 272 bytes each: number, path[260], file time.
//! - yscfg.ybn: `YSCF0001`, width and height at 20, the title (NUL-ended) at 160.

use crate::sjis;
use crate::ybn::{Arg, Command, Config, Instr, Script, VarDecl, VarType};

/// The commands in their numbering (from the 222 engine).
const COMMANDS: [(&str, &str); 76] = [
    (
        "CG",
        "ID NO E A X Y SX SY Z FILE MID MNO ME MX MY MFILE MLINK T TID TNO TA MODE FRAME",
    ),
    (
        "CGACT",
        "ID NO ID2 NO2 Z2 SET SETSTR X Y X2 Y2 SX SY SIZE SIZE2 ROTATION SAIDO MEIDO AKARUSA MONO VERTICAL HORIZONTAL MOSAIC BLUR DIFFUSION REVERSE RECTPAINT TEXT COPY COPY2 SWAP SWAP2",
    ),
    ("CGEND", "ID NO"),
    (
        "CGINFO",
        "ID NO EXIST E A X Y Z MID ME MX MY MFILE MLINK T TID TA MODE FRAME TOTALFRAME SX SY GIFLOOP GIFWAIT ONMOUSE COLOR LET SET SET2",
    ),
    ("CLIPACT", "TEXT SET"),
    ("CLIPINFO", "TEXT LET"),
    (
        "DIALOG",
        "CAPTION STR STR2 STR3 STR4 STR5 STR6 STR7 STR8 DEFSTR LETSTR LET OK OKCANCEL YESNO YESNOCANCEL INPUT DEFBUTTON FILEOPEN FILEEXT FOLDER CHECKBOX SLIDER A A2 A3 A4 A5 A6 A7 A8 E E2 E3 E4 E5 E6 E7 E8 CHECK CHECK2 CHECK3 CHECK4 CHECK5 CHECK6 CHECK7 CHECK8 NOWAIT",
    ),
    ("ELSE", ""),
    ("END", ""),
    // NO SET, then COM_ and each command name
    ("ERROR", "NO SET"),
    ("ERRORINFO", "NO LET"),
    (
        "FILEINFO",
        "FOLDER FILE CGFILE SOUNDFILE EXIST SIZE RELATION LET",
    ),
    ("FILEEXEC", "FILE PARAM"),
    ("FLT", ""),
    ("FONT", "NO NAME ANTIALIASING COLOR SIZE"),
    (
        "FONTINFO",
        "NO NAME ANTIALIASING COLOR SIZE NUM PITCH LANG X Y SX SY SET SETSTR LET",
    ),
    ("FPS", "SET SKIP"),
    ("G_FLT", ""),
    ("G_INT", ""),
    ("G_STR", ""),
    ("G_VAR", ""),
    ("GO", "#"),
    (
        "GOSUB",
        "# PINT PINT2 PINT3 PINT4 PINT5 PINT6 PINT7 PINT8 PFLT PFLT2 PFLT3 PFLT4 PFLT5 PFLT6 PFLT7 PFLT8 PSTR PSTR2 PSTR3 PSTR4 PSTR5 PSTR6 PSTR7 PSTR8",
    ),
    ("IF", ""),
    ("IFBLEND", ""),
    ("IFBREAK", "LV"),
    ("IFCONTINUE", "LV"),
    ("IFEND", ""),
    ("INT", ""),
    ("INPUT", "KEY PAD MOUSE"),
    ("LABEL", "#"),
    ("LABELINFO", "# LET EXIST"),
    ("LET", ""),
    ("LOAD", "FILE DNO LET ID NO"),
    ("LOOP", "SET"),
    ("LOOPBREAK", "LV"),
    ("LOOPCONTINUE", "LV"),
    ("LOOPEND", ""),
    ("MACRO", ""),
    (
        "MATH",
        "LET SET SET2 SIN ASIN SINH COS ACOS COSH TAN ATAN ATAN2 TANH SQRT ABS EXP LOG LOG10 POW RND",
    ),
    ("MENU", "ID NAME Z E A CHECK CODE"),
    ("MENUEND", "ID"),
    ("MENUINFO", "LET ID NAME EXIST Z E A CHECK CODE"),
    ("MOUSE", "MODE TIME MOVE CLICK WHEEL X Y"),
    ("MOVIE", "FILE PLAY X Y SX SY METHOD"),
    ("MOVIEEND", ""),
    ("MOVIEINFO", "PLAY X Y SX SY LET"),
    ("REGINFO", "KEY NAME LET"),
    (
        "RETURN",
        "RINT RINT2 RINT3 RINT4 RINT5 RINT6 RINT7 RINT8 RFLT RFLT2 RFLT3 RFLT4 RFLT5 RFLT6 RFLT7 RFLT8 RSTR RSTR2 RSTR3 RSTR4 RSTR5 RSTR6 RSTR7 RSTR8 #",
    ),
    ("RETURNBREAK", ""),
    ("RETURNCODE", ""),
    ("S_FLT", ""),
    ("S_INT", ""),
    ("S_STR", ""),
    ("S_VAR", ""),
    ("SAVE", "FILE DNO SET ID NO"),
    (
        "SOUND",
        "ID NO E FILE PLAY FADE START RESTART END LOOP VOLUME PAN PITCH SPEED",
    ),
    ("SOUNDEND", "ID NO"),
    (
        "SOUNDINFO",
        "ID NO EXIST E PLAY VOLUME PAN PITCH FILE FILETYPE START RESTART END LOOP LOOPCOUNT TIME TOTALTIME CHANNEL BIT SAMPLING DATALEFT DATARIGHT LET",
    ),
    ("STR", ""),
    ("SYSTEM", "SCREEN_X1 SCREEN_Y1 SCREEN_X2 SCREEN_Y2"),
    ("TASK", "ID NO E Z # SCRIPTPOS TEXTPOS"),
    ("TASKEND", "ID NO"),
    (
        "TASKINFO",
        "ID NO LET EXIST E Z FILE FILENO LINE TEXTLINE GOLABEL GOSUBLABEL IFNEST GOSUBNEST LOOPNEST SCRIPTPOS TEXTPOS",
    ),
    (
        "TEXT",
        "ID NO OUTPUTMODE WAIT RETURN PAGE CLICK AUTORETURN AUTOPAGE AUTOCLICK",
    ),
    (
        "TEXTINFO",
        "ID NO OUTPUTMODE CX CY AUTORETURN AUTOPAGE AUTOCLICK LET SET",
    ),
    ("VAR", ""),
    (
        "VARACT",
        "SET LET CUT COPY POS LENGTH TYPE UPPER LOWER HANTOZEN ZENTOHAN REDIM DIMSIZE DIMSIZE2 DIMSIZE3 DIMSIZE4 DIMSIZE5 DIMSIZE6 DIMSIZE7 DIMSIZE8",
    ),
    (
        "VARINFO",
        "SET LET TYPE DIMNUM DIMSIZE DIMSIZE2 DIMSIZE3 DIMSIZE4 DIMSIZE5 DIMSIZE6 DIMSIZE7 DIMSIZE8 LENGTH",
    ),
    ("VWORD", ""),
    ("WAIT", "FRAME TIME"),
    (
        "WINDOW",
        "NO CAPTION X Y Z SX SY ACTIVE FULLSCREENBUTTON FULLSCREEN CLOSEMODE CGID MENUID",
    ),
    ("WINDOWEND", "NO"),
    (
        "WINDOWINFO",
        "NO LET EXIST E ACTIVE CAPTION X Y Z SX SY FULLSCREEN CLOSEMODE",
    ),
    ("WORD", ""),
    (
        "PRODUCT",
        "COM LET PINT PINT2 PINT3 PINT4 PINT5 PINT6 PINT7 PINT8 PSTR",
    ),
];

pub fn commands() -> Vec<Command> {
    COMMANDS
        .iter()
        .map(|(name, kws)| {
            let mut keywords: Vec<String> = kws.split_whitespace().map(String::from).collect();
            if *name == "ERROR" {
                keywords.extend(COMMANDS.iter().map(|(n, _)| format!("COM_{n}")));
            }
            Command {
                name: name.to_string(),
                keywords,
            }
        })
        .collect()
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
        .ok_or_else(|| "unexpected end of file".into())
}

fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// The system variables in the later numbering (see [`crate::sysvar`]); the scripts' own
/// variables start at 1000 and keep their numbers. Matched by how ERIS uses them.
pub fn sysvar(id: u16) -> u16 {
    match id {
        0..=13 => id,
        14 => 48,            // @_LC
        16..=23 => id + 34,  // @_PINT .. @_RFLG
        34 => 79,            // @_SCREEN_COLOR
        38 => 84,            // @_WINDOW_EXEC_CLOSE
        39 => 87,            // $_TEXT
        40 => 88,            // $_TEXT_FLAG
        43 => 92,            // $_CHR
        48 => 97,            // @_MENUCODE
        49 => 98,            // @_MOUSE_X
        53 => 103,           // @_MOUSE_Y
        58..=60 => id + 51,  // @_INP, @_KEYLAST, @_MOUSELAST
        61..=328 => id + 51, // @_MOUSE_L.. (held), @_MOUSENUM_L.. (count)
        387..=520 => id - 7, // @_CODE_x
        1000.. => id,
        _ => 20000 + id,
    }
}

/// The script files: the index is the N of yst0000N.ybn (0 has none).
pub fn script_names(b: &[u8]) -> Result<Vec<String>, String> {
    let count = u32_at(b, 0)? as usize;
    let mut out = Vec::new();
    for i in 0..count {
        let at = 4 + i * 272;
        let n = u32_at(b, at)? as usize;
        let name = sjis::decode(cstr(b.get(at + 4..at + 264).ok_or("short script list")?));
        if out.len() <= n {
            out.resize(n + 1, String::new());
        }
        out[n] = name;
    }
    Ok(out)
}

/// Labels: name, script and byte offset.
pub fn labels(b: &[u8]) -> Result<Vec<(String, usize, u32)>, String> {
    let count = u32_at(b, 0)? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = 4 + i * 52;
        let name = sjis::decode(cstr(b.get(at..at + 32).ok_or("short label list")?));
        out.push((name, u32_at(b, at + 40)? as usize, u32_at(b, at + 36)?));
    }
    Ok(out)
}

pub fn variables(b: &[u8]) -> Result<Vec<VarDecl>, String> {
    let count = u32_at(b, 0)? as usize;
    let mut at = 4;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let id = u32_at(b, at)? as u16;
        let (ty, nd) = (
            *b.get(at + 4).ok_or("short variables")?,
            *b.get(at + 5).ok_or("short variables")? as usize,
        );
        at += 6;
        let mut dims = Vec::with_capacity(nd);
        for _ in 0..nd {
            dims.push(u32_at(b, at)?);
            at += 4;
        }
        let (ty, init) = match ty {
            1 | 2 => {
                let v = b.get(at..at + 8).ok_or("short variables")?.to_vec();
                at += 8;
                (if ty == 1 { VarType::Int } else { VarType::Flt }, v)
            }
            3 => {
                let n = b
                    .get(at..at + 2)
                    .map(|v| u16::from_le_bytes([v[0], v[1]]) as usize)
                    .ok_or("short variables")?;
                let e = b.get(at + 4..at + 4 + n).ok_or("short variables")?;
                at += 4 + n;
                (VarType::Str, expr(e))
            }
            _ => (VarType::None, Vec::new()),
        };
        out.push(VarDecl {
            scope: 1,
            group: 0,
            script: 0,
            id: sysvar(id),
            ty,
            dims,
            init,
        });
    }
    Ok(out)
}

pub fn config(b: &[u8]) -> Result<Config, String> {
    let width = u32_at(b, 20)?;
    let height = u32_at(b, 24)?;
    let title = sjis::decode(cstr(b.get(160..).unwrap_or_default()));
    Ok(Config {
        width,
        height,
        title,
    })
}

/// Whether yscfg.ybn is of this format.
pub fn is_config(b: &[u8]) -> bool {
    b.get(..8) == Some(b"YSCF0001")
}

fn is_lead(c: u8) -> bool {
    matches!(c, 0x81..=0x9f | 0xe0..=0xfc)
}

/// Splits a text expression into its tokens; a string runs to the quote that ends it.
fn split(t: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if t[i] == b' ' {
            i += 1;
            continue;
        }
        let start = i;
        if t[i] == b'M' && matches!(t.get(i + 1), Some(b'"' | b'\'')) {
            let q = t[i + 1];
            i += 2;
            while i < t.len() {
                if t[i] == q && t.get(i + 1).is_none_or(|&c| c == b' ') {
                    i += 1;
                    break;
                }
                i += if is_lead(t[i]) { 2 } else { 1 };
            }
            i = i.min(t.len());
        } else {
            while i < t.len() && t[i] != b' ' {
                i += 1;
            }
        }
        out.push(&t[start..i]);
    }
    out
}

fn token(out: &mut Vec<u8>, kind: u8, data: &[u8]) {
    out.push(kind);
    out.extend((data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
}

/// A text expression in the binary form of [`crate::expr`].
pub fn expr(t: &[u8]) -> Vec<u8> {
    let toks = split(cstr(t));
    let mut out = Vec::new();
    let mut k = 0;
    while k < toks.len() {
        let tok = toks[k];
        k += 1;
        let rest = std::str::from_utf8(&tok[1..]).unwrap_or("");
        match tok[0] {
            b'I' if tok.len() > 1 => {
                let v: i64 = rest.parse().unwrap_or(0);
                match i32::try_from(v) {
                    Ok(v) => token(&mut out, b'I', &v.to_le_bytes()),
                    Err(_) => token(&mut out, b'L', &v.to_le_bytes()),
                }
            }
            b'F' if tok.len() > 1 => token(
                &mut out,
                b'F',
                &rest.parse::<f64>().unwrap_or(0.0).to_le_bytes(),
            ),
            b'M' if tok.len() > 1 => token(&mut out, b'M', &tok[1..]),
            b'H' if tok.len() > 2 => {
                let id: u16 = std::str::from_utf8(&tok[2..])
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let mut data = vec![tok[1]];
                data.extend(sysvar(id).to_le_bytes());
                match toks.get(k).copied() {
                    Some(b"(") => {
                        k += 1;
                        token(&mut out, b'V', &data);
                    }
                    Some(b"V") => {
                        k += 1;
                        token(&mut out, b'v', &data);
                    }
                    _ => token(&mut out, b'H', &data),
                }
            }
            _ if tok.len() == 1 => token(&mut out, tok[0], &[]),
            _ => {}
        }
    }
    out
}

/// A script; the instructions' byte offsets come along for the labels.
pub fn script(b: &[u8], commands: &[Command]) -> Result<(Script, Vec<u32>), String> {
    if b.get(..4) != Some(b"YSTB") {
        return Err("not a YSTB file".into());
    }
    let code_size = u32_at(b, 8)? as usize;
    let res_size = u32_at(b, 12)? as usize;
    let body: Vec<u8> = b
        .get(32..32 + code_size + res_size)
        .ok_or("short script")?
        .iter()
        .map(|c| c ^ 0x80)
        .collect();
    let (code, res) = body.split_at(code_size);
    let mut at = 0;
    let mut out = Vec::new();
    let mut offsets = Vec::new();
    while at + 6 <= code.len() {
        offsets.push(at as u32);
        let (op, argc) = (code[at], code[at + 1] as usize);
        let line = u32_at(code, at + 2)?;
        at += 6;
        let name = commands.get(op as usize).map_or("", |c| c.name.as_str());
        let jumps = matches!(name, "IF" | "ELSE" | "LOOP");
        let mut args = Vec::with_capacity(argc);
        for i in 0..argc {
            let a = code.get(at..at + 16).ok_or("short arguments")?;
            at += 16;
            let keyword = u16::from_le_bytes([a[0], a[1]]);
            let ty = u32::from_le_bytes(a[4..8].try_into().unwrap()) as u16;
            let size = u32::from_le_bytes(a[8..12].try_into().unwrap());
            let off = u32::from_le_bytes(a[12..16].try_into().unwrap()) as usize;
            if jumps && i > 0 {
                args.push(Arg {
                    keyword,
                    ty,
                    value: Vec::new(),
                    jump: size,
                });
                continue;
            }
            let raw = res.get(off..off + size as usize).unwrap_or_default();
            let value = if ty & 0xff == 0 {
                raw.to_vec()
            } else {
                expr(raw)
            };
            args.push(Arg {
                keyword,
                ty,
                value,
                jump: size,
            });
        }
        out.push(Instr { op, args, line });
    }
    // byte offsets to instruction numbers
    let index = |off: u32| offsets.partition_point(|&o| o < off) as u32;
    for ins in &mut out {
        let name = commands
            .get(ins.op as usize)
            .map_or("", |c| c.name.as_str());
        if matches!(name, "IF" | "ELSE" | "LOOP") {
            for a in ins.args.iter_mut().skip(1) {
                if a.jump != 0 {
                    a.jump = index(a.jump);
                }
            }
        }
    }
    Ok((Script { code: out }, offsets))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{Place, Value, Vars, eval};

    struct NoVars;
    impl Vars for NoVars {
        fn get(&self, p: &Place) -> Value {
            Value::Int(p.var.id as i64 + p.index.iter().sum::<i64>())
        }
    }

    #[test]
    fn text_expressions() {
        assert_eq!(
            eval(&expr(b"I2 I3 * I1 + "), &NoVars).unwrap(),
            Value::Int(7)
        );
        assert_eq!(
            eval(&expr(b"M\"a b\" M\"c\" + \0junk"), &NoVars).unwrap(),
            Value::text("a bc")
        );
        assert_eq!(
            eval(&expr(b"H@01117 ( I10 , I1 ) "), &NoVars).unwrap(),
            Value::Int(1128)
        );
        assert_eq!(eval(&expr(b"H@00014 "), &NoVars).unwrap(), Value::Int(48));
        assert_eq!(eval(&expr(b"I1 R "), &NoVars).unwrap(), Value::Int(-1));
    }
}
