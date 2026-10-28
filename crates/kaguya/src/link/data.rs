//! params.dat, message.dat and the [SCR-Ver5.1] scripts of the LINK engine (2015,
//! ハラミタマ). Read from the trial's files; the key's place follows GARbro's ArcLINK.cs.

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn i32le(b: &[u8], at: usize) -> Option<i32> {
    u32le(b, at).map(|v| v as i32)
}

fn i16le(b: &[u8], at: usize) -> Option<i16> {
    u16le(b, at).map(|v| v as i16)
}

/// A u16 length and that many bytes of UTF-16.
fn utf16(b: &[u8], at: &mut usize) -> Option<String> {
    let n = u16le(b, *at)? as usize;
    let raw = b.get(*at + 2..*at + 2 + n)?;
    *at += 2 + n;
    let w: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&w).ok()
}

/// params.dat ("[SCR-PARAMS]v05.6"):
///
/// ```text
/// 0x13 u32 width, u32 height; 0x1b a u8-long chunk, then (u16 length, UTF-16) the title,
/// an empty string, the maker, a byte, the hero's given and family names; ...
/// u32 width*height*4 and the key (the XOR of the encrypted pictures); ...
/// the resources: u32 count, then [u16 length, UTF-16 name, u8] (index 0 empty)
/// the pictures: u32 count, then [u8 n, n x u32 resource] (the layers of one picture, 0 none)
/// ```
pub struct Params {
    pub width: u32,
    pub height: u32,
    pub title: String,
    pub hero: String,
    pub key: Vec<u8>,
    pub names: Vec<String>,
    pub pictures: Vec<Vec<u32>>,
}

impl Params {
    pub fn parse(b: &[u8]) -> Result<Params, String> {
        if !b.starts_with(b"[SCR-PARAMS]v05") {
            return Err("not [SCR-PARAMS]v05".into());
        }
        let width = u32le(b, 0x13).ok_or("short params")?;
        let height = u32le(b, 0x17).ok_or("short params")?;
        if !(320..=4096).contains(&width) || !(200..=4096).contains(&height) {
            return Err(format!("params: screen {width}x{height}?"));
        }
        let mut at = 0x1b + 1 + b[0x1b] as usize;
        let title = utf16(b, &mut at).unwrap_or_default();
        let _ = utf16(b, &mut at);
        let _maker = utf16(b, &mut at);
        at += 1;
        let hero = utf16(b, &mut at).unwrap_or_default();
        // the key: its length is the size of the screen in BGRA
        let len = width * height * 4;
        let pat = len.to_le_bytes();
        let k = b
            .windows(4)
            .position(|w| w == pat)
            .filter(|&p| p + 4 + len as usize <= b.len())
            .ok_or("params: no key")?;
        let key = b[k + 4..k + 4 + len as usize].to_vec();
        let mut at = k + 4 + len as usize;
        // the resource names: the first count followed by an empty name whose entries all
        // parse and end in an extension
        let names = loop {
            if at + 8 > b.len() {
                return Err("params: no resource names".into());
            }
            if let Some(n) = u32le(b, at)
                && (16..100_000).contains(&n)
                && u16le(b, at + 4) == Some(0)
                && let Some((names, end)) = names_at(b, at + 4, n as usize)
            {
                at = end;
                break names;
            }
            at += 1;
        };
        let n = u32le(b, at).ok_or("params: no pictures")? as usize;
        at += 4;
        let mut pictures = Vec::with_capacity(n.min(100_000));
        for _ in 0..n {
            let c = *b.get(at).ok_or("short pictures")? as usize;
            let ids = (0..c)
                .map(|i| u32le(b, at + 1 + 4 * i))
                .collect::<Option<Vec<_>>>()
                .ok_or("short pictures")?;
            pictures.push(ids);
            at += 1 + 4 * c;
        }
        Ok(Params {
            width,
            height,
            title,
            hero,
            key,
            names,
            pictures,
        })
    }

    pub fn find(&self, name: &str) -> Option<u32> {
        self.names.iter().position(|n| n == name).map(|i| i as u32)
    }
}

fn names_at(b: &[u8], mut at: usize, n: usize) -> Option<(Vec<String>, usize)> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let s = utf16(b, &mut at)?;
        at += 1;
        let ext_ok = s
            .rsplit_once('.')
            .is_some_and(|(_, e)| (2..=5).contains(&e.len()) && e.is_ascii());
        if (i == 0) != s.is_empty() || (i > 0 && !ext_ok) {
            return None;
        }
        out.push(s);
    }
    Some((out, at))
}

pub struct Line {
    pub text: String,
    /// "sei0038" (voice.arc's sei0038.ogg)
    pub voice: Option<String>,
    /// the speaker (an index into `Messages::names`; 0 the hero), if any
    pub speaker: Option<usize>,
}

/// message.dat ("[SCR-MESSAGE]ver4.0"), every byte of each string inverted:
///
/// ```text
/// 0x15 u32 count, [u16 length, cp932] the speakers (0: the hero, "\u{e000}")
/// u32 count, [u16 length, cp932] the choices
/// u32 count, [u32 size, then size bytes: i32 length, cp932 text, and 0x01 + a UTF-16
///   voice name ("sei0038.voice") or 0x00]
/// u32 count, [i32 speaker (-1 none), u8 n, n x u32 line] who says each line
/// ```
pub struct Messages {
    pub names: Vec<String>,
    pub choices: Vec<String>,
    pub lines: Vec<Line>,
}

fn sjis(raw: &[u8]) -> String {
    encoding_rs::SHIFT_JIS.decode(raw).0.into_owned()
}

impl Messages {
    pub fn parse(b: &[u8]) -> Result<Messages, String> {
        if !b.starts_with(b"[SCR-MESSAGE]") {
            return Err("not [SCR-MESSAGE]".into());
        }
        let short = || "short message.dat".to_string();
        let mut at = 0x15;
        let strings = |at: &mut usize| -> Option<Vec<String>> {
            let n = u32le(b, *at)? as usize;
            *at += 4;
            let mut out = Vec::new();
            for _ in 0..n {
                let len = u16le(b, *at)? as usize;
                let raw: Vec<u8> = b.get(*at + 2..*at + 2 + len)?.iter().map(|c| !c).collect();
                let raw: Vec<u8> = raw.into_iter().take_while(|&c| c != 0).collect();
                out.push(sjis(&raw));
                *at += 2 + len;
            }
            Some(out)
        };
        let names = strings(&mut at).ok_or_else(short)?;
        let choices = strings(&mut at).ok_or_else(short)?;
        let n = u32le(b, at).ok_or_else(short)? as usize;
        at += 4;
        let mut lines = Vec::with_capacity(n.min(1 << 20));
        for _ in 0..n {
            let size = u32le(b, at).ok_or_else(short)? as usize;
            let r: Vec<u8> = b
                .get(at + 4..at + 4 + size)
                .ok_or_else(short)?
                .iter()
                .map(|c| !c)
                .collect();
            at += 4 + size;
            let len = (i32le(&r, 0).unwrap_or(0).max(0) as usize).min(r.len().saturating_sub(4));
            let raw: Vec<u8> = r[4..4 + len]
                .iter()
                .copied()
                .take_while(|&c| c != 0)
                .collect();
            let text = sjis(&raw).replace('\r', "");
            let text = text.trim_end_matches('\n').to_string();
            let rest = &r[4 + len..];
            let voice = (rest.first() == Some(&1)).then(|| {
                let w: Vec<u16> = rest[1..]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|&c| c != 0)
                    .collect();
                let v = String::from_utf16_lossy(&w);
                crate::link::arc::stem(&v).to_string()
            });
            lines.push(Line {
                text,
                voice,
                speaker: None,
            });
        }
        if let Some(n) = u32le(b, at) {
            at += 4;
            for _ in 0..n {
                let (Some(who), Some(&c)) = (i32le(b, at), b.get(at + 4)) else {
                    break;
                };
                at += 5;
                for k in 0..c as usize {
                    if let Some(id) = u32le(b, at + 4 * k)
                        && let Some(l) = lines.get_mut(id as usize)
                        && who >= 0
                    {
                        l.speaker = Some(who as usize);
                    }
                }
                at += 4 * c as usize;
            }
        }
        Ok(Messages {
            names,
            choices,
            lines,
        })
    }
}

/// One layer of a scene: a picture (params' pictures) on a slot (0 the background, then
/// the characters and the effects), at a place.
#[derive(Clone, Debug)]
pub struct Layer {
    pub picture: u32,
    pub slot: u8,
    pub x: i32,
    pub y: i32,
}

/// A script ("[SCR-Ver5.1]"): u32 the code's size, then instructions (u16 op, u16 size with
/// these 4 bytes, the operands; jumps name offsets in the file); after the code "[SAVE]" (u32
/// n, n offsets) and "[LAYER]": u32 n, n u32 offsets of the scenes, each a u8 count of layers
/// (u16 size, u32 picture, u8 slot, i16 x, i16 y, ...).
pub struct Script {
    pub name: String,
    pub data: Vec<u8>,
    pub end: usize,
    pub scenes: Vec<Vec<Layer>>,
}

pub struct Ins<'a> {
    pub op: u16,
    pub args: &'a [u8],
    pub next: usize,
}

impl Ins<'_> {
    pub fn u8(&self, at: usize) -> u8 {
        self.args.get(at).copied().unwrap_or(0)
    }
    pub fn u16(&self, at: usize) -> u16 {
        u16le(self.args, at).unwrap_or(0)
    }
    pub fn i32(&self, at: usize) -> i32 {
        i32le(self.args, at).unwrap_or(-1)
    }
}

impl Script {
    pub fn parse(name: &str, data: Vec<u8>) -> Result<Script, String> {
        if !data.starts_with(b"[SCR-Ver5") {
            return Err(format!("{name}: not [SCR-Ver5]"));
        }
        let size = u32le(&data, 12).ok_or("short script")? as usize;
        let end = (16 + size).min(data.len());
        let mut scenes = Vec::new();
        if let Some(p) = data[end..]
            .windows(7)
            .position(|w| w == b"[LAYER]")
            .map(|p| end + p + 7)
        {
            let t = &data[p..];
            let n = u32le(t, 0).unwrap_or(0) as usize;
            let base = 4 + 4 * n;
            for i in 0..n.min(100_000) {
                let mut layers = Vec::new();
                if let Some(o) = u32le(t, 4 + 4 * i) {
                    let mut q = base + o as usize;
                    let c = t.get(q).copied().unwrap_or(0);
                    q += 1;
                    for _ in 0..c {
                        let Some(sz) = u16le(t, q) else { break };
                        let r = &t[(q + 2).min(t.len())..(q + sz as usize).min(t.len())];
                        if let (Some(picture), Some(&slot), Some(x), Some(y)) =
                            (u32le(r, 0), r.get(4), i16le(r, 5), i16le(r, 7))
                        {
                            layers.push(Layer {
                                picture,
                                slot,
                                x: x as i32,
                                y: y as i32,
                            });
                        }
                        q += sz.max(2) as usize;
                    }
                }
                layers.sort_by_key(|l| l.slot);
                scenes.push(layers);
            }
        }
        Ok(Script {
            name: name.to_string(),
            data,
            end,
            scenes,
        })
    }

    pub fn ins(&self, at: usize) -> Option<Ins<'_>> {
        if at < 16 || at + 4 > self.end {
            return None;
        }
        let op = u16le(&self.data, at)?;
        let size = u16le(&self.data, at + 2)? as usize;
        if size < 4 || at + size > self.end {
            return None;
        }
        Some(Ins {
            op,
            args: &self.data[at + 4..at + size],
            next: at + size,
        })
    }
}
