//! The project settings (live.lpb) and preview menus (.lpm).

use crate::archive::Reader;
use crate::value::Value;

pub struct Project {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// the chart the game starts with
    pub init: String,
    /// the system variables (StatusBGMVolume, ...) with their first values
    pub vars: Vec<(String, u8, Value)>,
}

impl Project {
    /// ```text
    /// version:u32 name w:u32 h:u32 w:u32 h:u32 init [exit (v > 0x6d)] dir u32 u8 [u8 (v >= 0x6a)]
    /// [audio formats (v >= 0x6d)] [u8 (v >= 0x71)] [u8 (v >= 0x72)] [u8 (v >= 0x74)]
    /// disk prompt, exit prompt, count:u32 x (type:u8 name value)
    /// ```
    pub fn parse(b: &[u8]) -> Result<Project, String> {
        let mut r = Reader { b, at: 0 };
        let v = r.u32()?;
        let name = r.str()?;
        let width = r.u32()?;
        let height = r.u32()?;
        r.u32()?;
        r.u32()?;
        let init = r.str()?;
        if v > 0x6d {
            r.str()?;
        }
        r.str()?;
        r.u32()?;
        r.u8()?;
        if v >= 0x6a {
            r.u8()?;
        }
        if v >= 0x6d {
            r.str()?;
        }
        for min in [0x71, 0x72, 0x74] {
            if v >= min {
                r.u8()?;
            }
        }
        r.str()?;
        r.str()?;
        let mut vars = Vec::new();
        // The list is the last thing we need; a short or odd tail only loses variables.
        if let Ok(n) = r.u32() {
            for _ in 0..n.min(10_000) {
                let Ok(ty) = r.u8() else { break };
                let Ok(name) = r.str() else { break };
                let value = match ty {
                    1 => r.i32().map(|n| Value::Int(n as i64)),
                    2 => r.bytes(10).map(|_| Value::Float(0.0)),
                    3 => r.u8().map(|b| Value::Flag(b != 0)),
                    4 => r.str().map(Value::Str),
                    _ => break,
                };
                let Ok(value) = value else { break };
                vars.push((name, ty, value));
            }
        }
        Ok(Project {
            name,
            width: if width == 0 { 640 } else { width },
            height: if height == 0 { 480 } else { height },
            init,
            vars,
        })
    }
}

/// One button of a preview menu.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MenuItem {
    pub name: String,
    pub left: i32,
    pub top: i32,
    pub group: i64,
    /// pictures: normal, under the pointer, pressed, disabled
    pub path: String,
    pub hover: String,
    pub down: String,
    pub disabled: String,
}

/// A preview menu (.lpm): buttons with pictures, in the XML form of LiveMaker 3.
pub fn parse_lpm(text: &str) -> Vec<MenuItem> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<Item>") {
        let end = rest[start..]
            .find("</Item>")
            .map_or(rest.len(), |e| start + e);
        let body = &rest[start..end];
        let tag = |t: &str| -> String {
            let open = format!("<{t}>");
            let close = format!("</{t}>");
            body.find(&open)
                .and_then(|a| {
                    let a = a + open.len();
                    body[a..].find(&close).map(|b| unescape(&body[a..a + b]))
                })
                .unwrap_or_default()
        };
        let num = |t: &str| tag(t).trim().parse::<i64>().unwrap_or(0);
        let path = tag("Path");
        let pick = |t: &str| {
            let s = tag(t);
            if s.is_empty() { path.clone() } else { s }
        };
        out.push(MenuItem {
            name: tag("Name"),
            left: num("Left") as i32,
            top: num("Top") as i32,
            group: num("Group"),
            hover: pick("InImagePath"),
            down: pick("DownImagePath"),
            disabled: pick("DisImagePath"),
            path: pick("OutImagePath"),
        });
        rest = &rest[(end + 1).min(rest.len())..];
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
