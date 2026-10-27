//! Compiled LiveMaker scripts (.lsb): one "chart" of commands.
//!
//! ```text
//! version:u32 (103..=117) flags:u8 kinds:u32 bits_len:u32
//! kinds x bits[bits_len]       which properties each component command takes (bit i = property i+1)
//! count:u32 commands
//! command = kind:u8 indent:u32 mute:u8 not_update:u8 line:u32 fields...
//! ```
//!
//! Expressions ("LiveParser") are a list of steps; each step computes a named temporary
//! (`____1`, ...) or a variable from its operands, and the last one is the result.

use crate::archive::Reader;
use crate::tpword::TpWord;

#[derive(Debug, Clone)]
pub enum Param {
    Var(String),
    Int(i32),
    Float(f64),
    Flag(u8),
    Str(String),
}

#[derive(Debug, Clone)]
pub struct Step {
    pub op: u8,
    pub name: String,
    pub func: u8,
    pub args: Vec<Param>,
}

/// An expression; empty when the field was left blank.
#[derive(Debug, Clone, Default)]
pub struct Expr(pub Vec<Step>);

#[derive(Debug, Clone)]
pub struct LabelRef {
    pub page: String,
    pub label: u32,
}

#[derive(Debug, Clone)]
pub enum Cmd {
    If(Expr),
    Elseif(Expr),
    Else,
    Label(String),
    Jump(LabelRef, Expr),
    Call {
        to: LabelRef,
        result: String,
        cond: Expr,
        params: Vec<Expr>,
    },
    Exit(Expr),
    Wait {
        cond: Expr,
        time: Expr,
        stop: Expr,
    },
    /// BoxNew, ImgNew, MesNew, Button, Sound, ...: the command kind and (property, value)
    New(u8, Vec<(u8, Expr)>),
    Flip {
        wipe: Expr,
        time: Expr,
        reverse: Expr,
        act: Expr,
        targets: Vec<Expr>,
        delete: Expr,
        params: Vec<Expr>,
        source: Expr,
        stop: Expr,
    },
    Calc(Expr),
    VarNew {
        name: String,
        ty: u8,
        init: Expr,
        scope: u8,
    },
    VarDel(String),
    GetProp {
        obj: Expr,
        prop: Expr,
        var: String,
    },
    SetProp {
        obj: Expr,
        prop: Expr,
        value: Expr,
    },
    ObjDel(Expr),
    TextIns {
        text: TpWord,
        target: Expr,
        hist: Expr,
        wait: Expr,
        stop: Expr,
    },
    MovieStop {
        target: Expr,
        time: Expr,
        wait: Expr,
    },
    ClrHist,
    MenuClose(Expr),
    Comment,
    TextClr(Expr),
    CallHist(Expr),
    While {
        cond: Expr,
        end: u32,
    },
    WhileInit(Expr),
    WhileLoop {
        calc: Expr,
        start: u32,
    },
    Break {
        cond: Expr,
        end: u32,
    },
    Continue {
        cond: Expr,
        start: u32,
    },
    GameSave {
        no: Expr,
        caption: Expr,
    },
    GameLoad(Expr),
    PcReset {
        to: LabelRef,
        all: bool,
    },
    Reset {
        to: LabelRef,
        all: bool,
    },
    Terminate,
    DoEvent,
    ClrRead,
    MediaPlay(Expr),
    PropMotion {
        name: Expr,
        obj: Expr,
        prop: Expr,
        value: Expr,
        time: Expr,
        kind: Expr,
    },
    FormatHist,
    Cabinet {
        load: bool,
        props: Vec<(u8, Expr)>,
        act: Expr,
        targets: Vec<Expr>,
    },
    Ifdef,
    Ifndef,
    Endif,
}

#[derive(Debug, Clone)]
pub struct Command {
    pub indent: u32,
    pub mute: bool,
    pub line: u32,
    pub cmd: Cmd,
}

#[derive(Debug, Default)]
pub struct Script {
    pub version: u32,
    pub commands: Vec<Command>,
}

/// Command kinds that create a component.
pub const BOX_NEW: u8 = 0x08;
pub const IMG_NEW: u8 = 0x09;
pub const MES_NEW: u8 = 0x0a;
pub const TIMER: u8 = 0x0b;
pub const MOVIE: u8 = 0x0c;
pub const CINEMA: u8 = 0x17;
pub const CAPTION: u8 = 0x18;
pub const MENU: u8 = 0x19;
pub const BUTTON: u8 = 0x1e;
pub const PARTICLE_NEW: u8 = 0x24;
pub const FIRE_NEW: u8 = 0x25;
pub const SOUND: u8 = 0x2a;
pub const EDIT_NEW: u8 = 0x2b;
pub const MEMO_NEW: u8 = 0x2c;
pub const MAP_IMG_NEW: u8 = 0x30;
pub const WAVE_NEW: u8 = 0x31;
pub const TILE_NEW: u8 = 0x32;
pub const SLIDER_NEW: u8 = 0x33;
pub const SCROLLBAR_NEW: u8 = 0x34;
pub const GAUGE_NEW: u8 = 0x35;
pub const CG_CAPTION: u8 = 0x36;
pub const PREV_MENU_NEW: u8 = 0x38;

fn is_component(kind: u8) -> bool {
    matches!(
        kind,
        BOX_NEW
            | IMG_NEW
            | MES_NEW
            | TIMER
            | MOVIE
            | CINEMA
            | CAPTION
            | MENU
            | BUTTON
            | PARTICLE_NEW
            | FIRE_NEW
            | SOUND
            | EDIT_NEW
            | MEMO_NEW
            | MAP_IMG_NEW
            | WAVE_NEW
            | TILE_NEW
            | SLIDER_NEW
            | SCROLLBAR_NEW
            | GAUGE_NEW
            | CG_CAPTION
            | PREV_MENU_NEW
    )
}

/// An 80-bit extended float.
fn ext_float(b: &[u8]) -> f64 {
    let mant = u64::from_le_bytes(b[..8].try_into().unwrap());
    let se = u16::from_le_bytes([b[8], b[9]]);
    let sign = if se & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (se & 0x7fff) as i32;
    if exp == 0 && mant == 0 {
        return 0.0;
    }
    sign * (mant as f64 / (1u64 << 63) as f64) * 2f64.powi(exp - 16383)
}

impl Reader<'_> {
    fn param(&mut self) -> Result<Param, String> {
        Ok(match self.u8()? {
            1 => Param::Int(self.i32()?),
            2 => Param::Float(ext_float(self.bytes(10)?)),
            3 => Param::Flag(self.u8()?),
            4 => Param::Str(self.str()?),
            _ => Param::Var(self.str()?),
        })
    }

    pub fn expr(&mut self) -> Result<Expr, String> {
        let n = self.u32()? as usize;
        if n > 100_000 {
            return Err("expression too long".into());
        }
        let mut steps = Vec::with_capacity(n);
        for _ in 0..n {
            let op = self.u8()?;
            let name = self.str()?;
            let count = self.u32()? as usize;
            let func = if op == 0x0b { self.u8()? } else { 0 };
            if count > 100_000 {
                return Err("expression too long".into());
            }
            let mut args = Vec::with_capacity(count);
            for _ in 0..count {
                args.push(self.param()?);
            }
            steps.push(Step {
                op,
                name,
                func,
                args,
            });
        }
        Ok(Expr(steps))
    }

    fn exprs(&mut self) -> Result<Vec<Expr>, String> {
        let n = self.u32()? as usize;
        (0..n).map(|_| self.expr()).collect()
    }

    fn label_ref(&mut self) -> Result<LabelRef, String> {
        Ok(LabelRef {
            page: self.str()?,
            label: self.u32()?,
        })
    }
}

impl Script {
    pub fn parse(b: &[u8]) -> Result<Script, String> {
        let mut r = Reader { b, at: 0 };
        let version = r.u32()?;
        if !(100..=117).contains(&version) {
            return Err(format!("lsb version {version}"));
        }
        let _flags = r.u8()?;
        let kinds = r.u32()? as usize;
        let bits_len = r.u32()? as usize;
        let mut props: Vec<Vec<u8>> = Vec::with_capacity(kinds);
        for _ in 0..kinds {
            let bits = r.bytes(bits_len)?;
            let mut list = Vec::new();
            for (i, byte) in bits.iter().enumerate() {
                for bit in 0..8 {
                    if byte & (1 << bit) != 0 {
                        list.push((i * 8 + bit + 1) as u8);
                    }
                }
            }
            props.push(list);
        }
        let count = r.u32()? as usize;
        let mut commands = Vec::with_capacity(count.min(1 << 20));
        for _ in 0..count {
            let at = r.at;
            let c = read_command(&mut r, version, &props)
                .map_err(|e| format!("command {} at {at:#x}: {e}", commands.len()))?;
            commands.push(c);
        }
        Ok(Script { version, commands })
    }
}

fn read_command(r: &mut Reader, v: u32, props: &[Vec<u8>]) -> Result<Command, String> {
    let kind = r.u8()?;
    let indent = r.u32()?;
    let mute = r.u8()? != 0;
    let _not_update = r.u8()?;
    let line = r.u32()?;
    let opt = |r: &mut Reader, present: bool| -> Result<Expr, String> {
        if present { r.expr() } else { Ok(Expr::default()) }
    };
    let cmd = match kind {
        0x00 => Cmd::If(r.expr()?),
        0x01 => Cmd::Elseif(r.expr()?),
        0x02 => Cmd::Else,
        0x03 => Cmd::Label(r.str()?),
        0x04 => Cmd::Jump(r.label_ref()?, r.expr()?),
        0x05 => Cmd::Call {
            to: r.label_ref()?,
            result: r.str()?,
            cond: r.expr()?,
            params: r.exprs()?,
        },
        0x06 => Cmd::Exit(r.expr()?),
        0x07 => Cmd::Wait {
            cond: r.expr()?,
            time: r.expr()?,
            stop: opt(r, v > 0x6a)?,
        },
        0x0d => {
            let wipe = r.expr()?;
            let time = r.expr()?;
            let reverse = r.expr()?;
            let act = r.expr()?;
            let targets = r.exprs()?;
            let delete = r.expr()?;
            let params = vec![r.expr()?, r.expr()?];
            let source = opt(r, v > 0x64)?;
            let stop = opt(r, v > 0x6a)?;
            let _difference = opt(r, v > 0x74)?;
            Cmd::Flip {
                wipe,
                time,
                reverse,
                act,
                targets,
                delete,
                params,
                source,
                stop,
            }
        }
        0x0e => Cmd::Calc(r.expr()?),
        0x0f => Cmd::VarNew {
            name: r.str()?,
            ty: r.u8()?,
            init: r.expr()?,
            scope: r.u8()?,
        },
        0x10 => Cmd::VarDel(r.str()?),
        0x11 => Cmd::GetProp {
            obj: r.expr()?,
            prop: r.expr()?,
            var: r.str()?,
        },
        0x12 => Cmd::SetProp {
            obj: r.expr()?,
            prop: r.expr()?,
            value: r.expr()?,
        },
        0x13 => Cmd::ObjDel(r.expr()?),
        0x14 => {
            let n = r.u32()? as usize;
            let text = TpWord::parse(r.bytes(n)?)?;
            Cmd::TextIns {
                text,
                target: r.expr()?,
                hist: r.expr()?,
                wait: r.expr()?,
                stop: opt(r, v > 0x6a)?,
            }
        }
        0x15 => {
            let target = r.expr()?;
            let time = r.expr()?;
            let wait = r.expr()?;
            opt(r, v > 0x6a)?;
            Cmd::MovieStop { target, time, wait }
        }
        0x16 => Cmd::ClrHist,
        0x1a => Cmd::MenuClose(r.expr()?),
        0x1b => {
            r.str()?;
            Cmd::Comment
        }
        0x1c => Cmd::TextClr(r.expr()?),
        0x1d => {
            let target = r.expr()?;
            r.expr()?;
            r.expr()?;
            r.expr()?;
            opt(r, v > 0x6e)?;
            Cmd::CallHist(target)
        }
        0x1f => Cmd::While {
            cond: r.expr()?,
            end: r.u32()?,
        },
        0x20 => Cmd::WhileInit(r.expr()?),
        0x21 => Cmd::WhileLoop {
            calc: r.expr()?,
            start: r.u32()?,
        },
        0x22 => Cmd::Break {
            cond: r.expr()?,
            end: r.u32()?,
        },
        0x23 => Cmd::Continue {
            cond: r.expr()?,
            start: r.u32()?,
        },
        0x26 => {
            let no = r.expr()?;
            r.str()?;
            if v > 0x68 {
                r.u32()?;
            }
            Cmd::GameSave {
                no,
                caption: r.expr()?,
            }
        }
        0x27 => Cmd::GameLoad(r.expr()?),
        0x28 => Cmd::PcReset {
            to: r.label_ref()?,
            all: r.u8()? != 0,
        },
        0x29 => Cmd::Reset {
            to: r.label_ref()?,
            all: r.u8()? != 0,
        },
        0x2d => Cmd::Terminate,
        0x2e => Cmd::DoEvent,
        0x2f => Cmd::ClrRead,
        0x37 => Cmd::MediaPlay(r.expr()?),
        0x39 => {
            let c = Cmd::PropMotion {
                name: r.expr()?,
                obj: r.expr()?,
                prop: r.expr()?,
                value: r.expr()?,
                time: r.expr()?,
                kind: r.expr()?,
            };
            opt(r, v > 0x6b)?;
            c
        }
        0x3a => {
            r.expr()?;
            opt(r, v > 0x6e)?;
            Cmd::FormatHist
        }
        0x3b | 0x3c => {
            let list = props.get(kind as usize).map_or(&[][..], |p| &p[..]);
            let mut out = Vec::with_capacity(list.len());
            for &p in list {
                out.push((p, r.expr()?));
            }
            Cmd::Cabinet {
                load: kind == 0x3c,
                props: out,
                act: r.expr()?,
                targets: r.exprs()?,
            }
        }
        0x3d => Cmd::Ifdef,
        0x3e => Cmd::Ifndef,
        0x3f => Cmd::Endif,
        k if is_component(k) => {
            let list = props.get(k as usize).map_or(&[][..], |p| &p[..]);
            let mut out = Vec::with_capacity(list.len());
            for &p in list {
                out.push((p, r.expr()?));
            }
            Cmd::New(k, out)
        }
        k => return Err(format!("unknown command {k:#x}")),
    };
    Ok(Command {
        indent,
        mute,
        line,
        cmd,
    })
}

// Text forms, for the dump tool and traces.

use std::collections::HashMap;
use std::fmt;

use crate::names;

impl fmt::Display for Param {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Param::Var(s) => write!(f, "{s}"),
            Param::Int(n) => write!(f, "{n}"),
            Param::Float(x) => write!(f, "{x}"),
            Param::Flag(b) => write!(f, "{}", if *b != 0 { "TRUE" } else { "FALSE" }),
            Param::Str(s) => write!(f, "{:?}", s),
        }
    }
}

fn op_symbol(op: u8) -> &'static str {
    match op {
        2 => "+",
        3 => "-",
        4 => "*",
        5 => "/",
        6 => "%",
        7 => "|",
        8 => "&",
        9 => "^",
        12 => "==",
        13 => ">",
        14 => "<",
        15 => ">=",
        16 => "<=",
        17 => "<<",
        18 => ">>",
        19 => "++",
        20 => "!=",
        _ => "?",
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut temps: HashMap<&str, String> = HashMap::new();
        let mut last = String::new();
        for s in &self.0 {
            let args: Vec<String> = s
                .args
                .iter()
                .map(|p| match p {
                    Param::Var(v) if v.starts_with("____") => {
                        temps.get(v.as_str()).cloned().unwrap_or(v.clone())
                    }
                    p => p.to_string(),
                })
                .collect();
            let text = match s.op {
                1 => args.last().cloned().unwrap_or_default(),
                10 => {
                    let mut t = args.first().cloned().unwrap_or_default();
                    for a in &args[1.min(args.len())..] {
                        t += &format!("[{a}]");
                    }
                    t
                }
                11 => format!("{}({})", names::func(s.func), args.join(", ")),
                op if args.len() == 2 => format!("({} {} {})", args[0], op_symbol(op), args[1]),
                op => format!("{}{:?}", op_symbol(op), args),
            };
            last = if s.op == 1 && !s.name.starts_with("____") {
                format!("{} = {text}", s.name)
            } else {
                text.clone()
            };
            temps.insert(&s.name, text);
        }
        write!(f, "{last}")
    }
}

impl fmt::Display for LabelRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.page, self.label)
    }
}

fn list(v: &[Expr]) -> String {
    v.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", ")
}

impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Cmd::If(e) => write!(f, "If {e}"),
            Cmd::Elseif(e) => write!(f, "Elseif {e}"),
            Cmd::Else => write!(f, "Else"),
            Cmd::Label(n) => write!(f, "Label {n}"),
            Cmd::Jump(to, c) => write!(f, "Jump {to} if {c}"),
            Cmd::Call {
                to,
                result,
                cond,
                params,
            } => write!(f, "Call {to} ({}) -> {result} if {cond}", list(params)),
            Cmd::Exit(e) => write!(f, "Exit {e}"),
            Cmd::Wait { cond, time, stop } => write!(f, "Wait {cond} time {time} stop {stop}"),
            Cmd::New(k, props) => {
                write!(f, "{}", names::COMMANDS[*k as usize])?;
                for (p, e) in props {
                    let e = e.to_string();
                    if !e.is_empty() {
                        write!(f, " {}={e}", names::prop(*p))?;
                    }
                }
                Ok(())
            }
            Cmd::Flip {
                wipe,
                time,
                targets,
                source,
                ..
            } => write!(
                f,
                "Flip {wipe} time {time} targets [{}] source {source}",
                list(targets)
            ),
            Cmd::Calc(e) => write!(f, "Calc {e}"),
            Cmd::VarNew {
                name,
                ty,
                init,
                scope,
            } => write!(f, "VarNew {name} type {ty} = {init} scope {scope}"),
            Cmd::VarDel(n) => write!(f, "VarDel {n}"),
            Cmd::GetProp { obj, prop, var } => write!(f, "GetProp {obj}.{prop} -> {var}"),
            Cmd::SetProp { obj, prop, value } => write!(f, "SetProp {obj}.{prop} = {value}"),
            Cmd::ObjDel(e) => write!(f, "ObjDel {e}"),
            Cmd::TextIns { target, .. } => write!(f, "TextIns {target}"),
            Cmd::MovieStop { target, .. } => write!(f, "MovieStop {target}"),
            Cmd::MenuClose(e) => write!(f, "MenuClose {e}"),
            Cmd::TextClr(e) => write!(f, "TextClr {e}"),
            Cmd::CallHist(e) => write!(f, "CallHist {e}"),
            Cmd::While { cond, end } => write!(f, "While {cond} end {end}"),
            Cmd::WhileInit(e) => write!(f, "WhileInit {e}"),
            Cmd::WhileLoop { calc, start } => write!(f, "WhileLoop {calc} start {start}"),
            Cmd::Break { cond, end } => write!(f, "Break {cond} end {end}"),
            Cmd::Continue { cond, start } => write!(f, "Continue {cond} start {start}"),
            Cmd::GameSave { no, .. } => write!(f, "GameSave {no}"),
            Cmd::GameLoad(e) => write!(f, "GameLoad {e}"),
            Cmd::PcReset { to, all } => write!(f, "PCReset {to} all {all}"),
            Cmd::Reset { to, all } => write!(f, "Reset {to} all {all}"),
            Cmd::MediaPlay(e) => write!(f, "MediaPlay {e}"),
            Cmd::PropMotion {
                obj,
                prop,
                value,
                time,
                kind,
                ..
            } => write!(
                f,
                "PropMotion {obj}.{prop} -> {value} time {time} kind {kind}"
            ),
            Cmd::Cabinet {
                load,
                props,
                act,
                targets,
            } => {
                write!(f, "{}", if *load { "LoadCabinet" } else { "SaveCabinet" })?;
                for (p, e) in props {
                    let e = e.to_string();
                    if !e.is_empty() {
                        write!(f, " {}={e}", names::prop(*p))?;
                    }
                }
                write!(f, " act {act} [{}]", list(targets))
            }
            other => write!(f, "{other:?}"),
        }
    }
}
