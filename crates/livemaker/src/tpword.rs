//! Novel text (the body of TextIns): "TpWord" + a 3-digit version, then tables of styles,
//! conditions and links, then the glyphs.

use crate::archive::Reader;
use crate::lsb::Expr;

#[derive(Debug, Clone, Default)]
pub struct Style {
    pub count: u32,
    /// Font height (0: the box's own)
    pub size: u32,
    pub color: u32,
    pub border: u32,
    pub ruby: String,
}

#[derive(Debug, Clone)]
pub enum Glyph {
    Char {
        ch: char,
        style: i32,
        link: i32,
        speed: u32,
    },
    /// alignment of the line (1 left, 2 right, 3 centre)
    Div(u8),
    /// 0 line break, 1 page (wait, then clear), 2 pause (wait), 3 clear
    Return(u8),
    Indent,
    Undent,
    Event(String),
    Var {
        name: String,
        style: i32,
    },
    Img {
        src: String,
    },
    HistChar(String),
}

#[derive(Debug, Clone, Default)]
pub struct TpWord {
    pub version: u32,
    pub styles: Vec<Style>,
    pub links: Vec<String>,
    pub body: Vec<Glyph>,
}

impl TpWord {
    pub fn parse(b: &[u8]) -> Result<TpWord, String> {
        let mut r = Reader { b, at: 0 };
        if r.bytes(6)? != b"TpWord" {
            return Err("not TpWord text".into());
        }
        let v: u32 = std::str::from_utf8(r.bytes(3)?)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or("TpWord version")?;
        let mut styles = Vec::new();
        for _ in 0..r.u32()? {
            let count = r.u32()?;
            let size = r.u32()?;
            let color = r.u32()?;
            let border = r.u32()?;
            r.bytes(2)?;
            if v < 100 {
                r.u8()?;
            } else {
                r.u32()?;
            }
            r.str()?;
            let ruby = r.str()?;
            if v >= 100 {
                r.u32()?;
                r.u32()?;
            }
            styles.push(Style {
                count,
                size,
                color,
                border,
                ruby,
            });
        }
        if v >= 104 {
            for _ in 0..r.u32()? {
                r.u32()?;
                r.str()?;
            }
        }
        let mut links = Vec::new();
        if v >= 105 {
            for _ in 0..r.u32()? {
                r.u32()?;
                links.push(r.str()?);
                r.str()?;
            }
        }
        let n = r.u32()?;
        let mut body = Vec::with_capacity(n.min(1 << 16) as usize);
        for _ in 0..n {
            let kind = r.u8()?;
            let cond = |r: &mut Reader| -> Result<(), String> {
                if v >= 104 {
                    r.i32()?;
                }
                Ok(())
            };
            // link and speed of "real" glyphs (characters and pictures)
            let real = |r: &mut Reader| -> Result<(i32, u32), String> {
                cond(r)?;
                let link = if v < 105 {
                    r.str()?;
                    0
                } else {
                    r.i32()?
                };
                Ok((link, r.u32()?))
            };
            let g = match kind {
                1 => {
                    let (link, speed) = real(&mut r)?;
                    let code = r.bytes(2)?;
                    let code = u16::from_le_bytes([code[0], code[1]]);
                    let bytes = if code > 0xff {
                        vec![(code >> 8) as u8, code as u8]
                    } else {
                        vec![code as u8]
                    };
                    let ch = nexas::sjis::decode(&bytes).chars().next().unwrap_or(' ');
                    Glyph::Char {
                        ch,
                        style: r.i32()?,
                        link,
                        speed,
                    }
                }
                2 => {
                    cond(&mut r)?;
                    let align = r.u8()?;
                    if v >= 105 {
                        r.i32()?;
                        r.i32()?;
                        r.u8()?;
                    }
                    Glyph::Div(align)
                }
                3 => {
                    cond(&mut r)?;
                    Glyph::Return(r.u8()?)
                }
                4 => {
                    cond(&mut r)?;
                    Glyph::Indent
                }
                5 => {
                    cond(&mut r)?;
                    Glyph::Undent
                }
                6 => {
                    cond(&mut r)?;
                    Glyph::Event(r.str()?)
                }
                7 | 10 => {
                    cond(&mut r)?;
                    let style = r.i32()?;
                    if v > 100 {
                        r.u32()?;
                    }
                    if v > 100 && v < 105 {
                        r.str()?;
                    }
                    if v >= 105 {
                        r.i32()?;
                    }
                    let name = if v < 102 {
                        let e: Expr = r.expr()?;
                        e.0.last().map(|s| s.name.clone()).unwrap_or_default()
                    } else {
                        r.str()?
                    };
                    if kind == 7 {
                        Glyph::Var { name, style }
                    } else {
                        Glyph::HistChar(name)
                    }
                }
                9 => {
                    real(&mut r)?;
                    let src = r.str()?;
                    r.u8()?;
                    if v >= 103 {
                        r.str()?;
                    }
                    if v >= 105 {
                        r.bytes(16)?;
                        r.str()?;
                    }
                    Glyph::Img { src }
                }
                // LiveMaker skips what it does not know; so do we, a byte at a time
                _ => continue,
            };
            body.push(g);
        }
        Ok(TpWord {
            version: v,
            styles,
            links,
            body,
        })
    }

    /// The text without tags, for logs.
    pub fn plain(&self) -> String {
        let mut s = String::new();
        for g in &self.body {
            match g {
                Glyph::Char { ch, .. } => s.push(*ch),
                Glyph::Return(0) => s.push('\n'),
                Glyph::Return(1) => s.push_str("<PG>\n"),
                Glyph::Return(2) => s.push_str("<PS>\n"),
                Glyph::Return(_) => s.push_str("<CLR>\n"),
                Glyph::Var { name, .. } => s.push_str(&format!("{{{name}}}")),
                Glyph::Event(e) => s.push_str(&format!("<EVENT {}>", e.replace("\r\n", " | "))),
                _ => {}
            }
        }
        s
    }
}
