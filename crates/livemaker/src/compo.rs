//! Components: the things on screen (and the invisible ones: sounds, timers) that the charts
//! create by name and read and change by property number.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use nexas::gfx::Image;

use crate::project::MenuItem;
use crate::text::{Fonts, rgb};
use crate::tpword::Glyph;
use crate::value::Value;

// Property numbers (the charts' numbers are one less).
pub const NAME: u8 = 1;
pub const PARENT: u8 = 2;
pub const SOURCE: u8 = 3;
pub const LEFT: u8 = 4;
pub const TOP: u8 = 5;
pub const WIDTH: u8 = 6;
pub const HEIGHT: u8 = 7;
pub const COLOR: u8 = 9;
pub const ALPHA: u8 = 12;
pub const PRIORITY: u8 = 13;
pub const FONTHEIGHT: u8 = 17;
pub const LINESPACE: u8 = 19;
pub const FONTCOLOR: u8 = 20;
pub const FONTBORDERCOLOR: u8 = 22;
pub const DELAY: u8 = 27;
pub const PAUSED: u8 = 28;
pub const VOLUME: u8 = 29;
pub const REPEAT: u8 = 30;
pub const ONPLAYING: u8 = 33;
pub const ONNOTIFY: u8 = 34;
pub const ONPLAYEND: u8 = 44;
pub const INDEX: u8 = 45;
pub const COUNT: u8 = 46;
pub const VISIBLE: u8 = 48;
pub const TEXT: u8 = 51;
pub const ROWCOUNT: u8 = 50;
pub const TOPINDEX: u8 = 98;
pub const AUTOSCRAP: u8 = 72;
pub const ONSELECT: u8 = 73;
pub const RCLICKSCRAP: u8 = 74;
pub const ONCLOSING: u8 = 77;
pub const ONCLOSED: u8 = 78;
pub const CARETX: u8 = 79;
pub const CARETY: u8 = 80;
pub const IGNOREMOUSE: u8 = 81;
pub const ONRBTNDOWN: u8 = 39;
pub const TEXTPAUSED: u8 = 82;
pub const TEXTDELAY: u8 = 83;
pub const HOVERSOURCE: u8 = 84;
pub const PRESSEDSOURCE: u8 = 85;
pub const ONPLAYSTART: u8 = 102;
pub const ONIDLE: u8 = 114;
pub const PLAYING: u8 = 139;
pub const COMPOTYPE: u8 = 141;
pub const FONTBORDER: u8 = 143;
pub const CARETHEIGHT: u8 = 162;

/// What kind of value a property holds.
#[derive(PartialEq)]
pub enum Class {
    Int,
    Flag,
    Str,
}

pub fn class(p: u8) -> Class {
    match p {
        1..=3
        | 16
        | 33..=42
        | 44
        | 47
        | 51
        | 55..=63
        | 70
        | 71
        | 73
        | 75..=78
        | 84
        | 85
        | 96
        | 97
        | 102..=107
        | 114
        | 122
        | 123
        | 130
        | 134
        | 136
        | 137
        | 145..=147
        | 149
        | 150
        | 152 => Class::Str,
        26
        | 28
        | 30
        | 48
        | 72
        | 74
        | 82
        | 87..=89
        | 99
        | 101
        | 112
        | 113
        | 135
        | 138..=140
        | 148
        | 151
        | 153
        | 159
        | 166 => Class::Flag,
        _ => Class::Int,
    }
}

pub fn coerce(p: u8, v: Value) -> Value {
    match class(p) {
        Class::Str => Value::Str(v.to_string()),
        Class::Flag => Value::Flag(v.truthy()),
        Class::Int => match v {
            Value::Float(_) | Value::Int(_) => v,
            v => Value::Int(v.int()),
        },
    }
}

/// The message box's text as it plays.
#[derive(Default)]
pub struct Mes {
    pub queue: VecDeque<Glyph>,
    /// characters shown: the character and its cell's top left
    pub chars: Vec<(char, i32, i32)>,
    pub x: i32,
    pub y: i32,
    pub indent: i32,
    /// stopped at the end of a page (or a pause) until TextClr
    pub waiting: bool,
    pub acc: f64,
    pub text: String,
    pub image: Option<Rc<Image>>,
    pub dirty: bool,
    /// its lines go to the backlog; the line being kept
    pub hist: bool,
    pub hist_y: Option<i32>,
    /// lines begun since the page was cleared
    pub row: i32,
}

pub struct Obj {
    pub kind: u8,
    pub props: HashMap<u8, Value>,
    pub seq: u64,
    pub img: Option<Rc<Image>>,
    pub mes: Option<Box<Mes>>,
    /// preview menu buttons, or the lines of a menu
    pub items: Vec<MenuItem>,
    pub disabled: Vec<bool>,
    pub hover: Option<usize>,
    /// the text picture of a caption or button
    pub label: Option<Rc<Image>>,
    /// a menu: its borders (left, top, right, bottom) and line height, and each line drawn
    /// plain and under the pointer
    pub geom: [i32; 5],
    pub lines: Vec<[Rc<Image>; 2]>,
    /// when a sound or timer started (ms)
    pub t0: f64,
    /// sound length (ms), 0 when not known
    pub dur: f64,
    pub playing: bool,
    /// the cabinet holding it (put away, not drawn)
    pub cab: Option<String>,
}

impl Obj {
    pub fn new(kind: u8, name: &str, seq: u64) -> Obj {
        let mut props = HashMap::new();
        props.insert(NAME, Value::Str(name.to_string()));
        Obj {
            kind,
            props,
            seq,
            img: None,
            mes: None,
            items: Vec::new(),
            disabled: Vec::new(),
            hover: None,
            label: None,
            geom: [0; 5],
            lines: Vec::new(),
            t0: 0.0,
            dur: 0.0,
            playing: false,
            cab: None,
        }
    }

    pub fn name(&self) -> &str {
        match self.props.get(&NAME) {
            Some(Value::Str(s)) => s,
            _ => "",
        }
    }

    pub fn parent(&self) -> &str {
        match self.props.get(&PARENT) {
            Some(Value::Str(s)) => s,
            _ => "",
        }
    }

    pub fn get(&self, p: u8) -> Value {
        if let Some(v) = self.props.get(&p) {
            return v.clone();
        }
        match p {
            VISIBLE => Value::Flag(true),
            ALPHA => Value::Int(255),
            VOLUME => Value::Int(1000),
            8 | 133 => Value::Int(100),
            TEXTDELAY => Value::Int(0),
            _ => match class(p) {
                Class::Str => Value::Str(String::new()),
                Class::Flag => Value::Flag(false),
                Class::Int => Value::Int(0),
            },
        }
    }

    pub fn int(&self, p: u8) -> i64 {
        self.get(p).int()
    }

    pub fn flag(&self, p: u8) -> bool {
        self.get(p).truthy()
    }

    pub fn str(&self, p: u8) -> String {
        self.get(p).to_string()
    }
}

/// Lays out one character in a message box; false when the box has no room left.
pub fn place(m: &mut Mes, fonts: &mut Fonts, c: char, size: u32, width: i32, line: i32) {
    let adv = fonts.advance(c, size);
    if m.x + adv > width && m.x > m.indent {
        m.x = m.indent;
        m.y += line;
        m.row += 1;
    }
    m.chars.push((c, m.x, m.y));
    m.x += adv;
    m.text.push(c);
    m.dirty = true;
}

pub fn newline(m: &mut Mes, line: i32) {
    m.x = m.indent;
    m.y += line;
    m.row += 1;
    m.text.push_str("\r\n");
}

pub fn clear(m: &mut Mes) {
    m.chars.clear();
    m.x = 0;
    m.y = 0;
    m.indent = 0;
    m.text.clear();
    m.waiting = false;
    m.dirty = true;
    m.hist_y = None;
    m.row = 0;
}

/// Moves the text up by whole lines until the line being written fits the box.
pub fn scroll(m: &mut Mes, size: u32, height: i32, line: i32) {
    if height <= 0 || line <= 0 || m.y + size as i32 <= height || m.y == 0 {
        return;
    }
    let lines = (m.y + size as i32 - height + line - 1) / line;
    let shift = (lines * line).min(m.y);
    m.chars.retain(|c| c.2 >= shift);
    for c in m.chars.iter_mut() {
        c.2 -= shift;
    }
    m.y -= shift;
    m.dirty = true;
}

/// Draws text into a new picture.
pub fn render_text(
    fonts: &mut Fonts,
    w: u32,
    h: u32,
    chars: &[(char, i32, i32)],
    size: u32,
    color: i64,
    border: Option<i64>,
) -> Image {
    let mut img = Image::new(w.clamp(1, 4096), h.clamp(1, 4096));
    let border = border.map(rgb);
    for &(c, x, y) in chars {
        fonts.draw(&mut img, x, y, c, size, rgb(color), border);
    }
    img
}

/// Lays out lines of text from (0, 0); returns the characters and the size they cover.
pub fn layout(
    fonts: &mut Fonts,
    text: &str,
    size: u32,
    line: i32,
) -> (Vec<(char, i32, i32)>, i32, i32) {
    let mut out = Vec::new();
    let (mut x, mut y, mut w) = (0, 0, 0);
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    for (i, l) in lines.iter().enumerate() {
        for c in l.chars() {
            out.push((c, x, y));
            x += fonts.advance(c, size);
        }
        w = w.max(x);
        x = 0;
        if i + 1 < lines.len() {
            y += line;
        }
    }
    (out, w, y + size as i32)
}

/// The length of an Ogg Vorbis or WAV sound in ms (0 when unknown).
pub fn duration(b: &[u8]) -> f64 {
    if b.len() > 44 && &b[..4] == b"RIFF" && &b[8..12] == b"WAVE" {
        let mut at = 12;
        let mut rate = 0u32;
        while at + 8 <= b.len() {
            let id = &b[at..at + 4];
            let len = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
            if id == b"fmt " && at + 20 <= b.len() {
                rate = u32::from_le_bytes(b[at + 16..at + 20].try_into().unwrap());
            }
            if id == b"data" && rate > 0 {
                return len as f64 * 1000.0 / rate as f64;
            }
            at += 8 + len + (len & 1);
        }
        return 0.0;
    }
    if b.len() < 64 || &b[..4] != b"OggS" {
        return 0.0;
    }
    // sample rate from the Vorbis identification header, samples from the last page
    let Some(v) = b.windows(7).position(|w| w == b"\x01vorbis") else {
        return 0.0;
    };
    if v + 16 > b.len() {
        return 0.0;
    }
    let rate = u32::from_le_bytes(b[v + 12..v + 16].try_into().unwrap());
    let Some(last) = b.windows(4).rposition(|w| w == b"OggS") else {
        return 0.0;
    };
    if last + 14 > b.len() || rate == 0 {
        return 0.0;
    }
    let granule = u64::from_le_bytes(b[last + 6..last + 14].try_into().unwrap());
    granule as f64 * 1000.0 / rate as f64
}
