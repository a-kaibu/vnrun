//! What the scenario and the Lua interface share: the screen, flags and variables, game state
//! and the game's pictures.

use crate::files::{Archives, Files};
use crate::layers::{Content, Layer, Screen, TextBox};
use crate::pna::Pna;
use crate::text;
use nexas::gfx::Image;
use std::collections::HashMap;
use std::rc::Rc;

pub const FLAGS: usize = 8192;
pub const VARS: usize = 8192;

/// Text shown in a text layer: the characters, how many are out, and how it looks.
pub struct Text {
    pub chars: Vec<char>,
    pub shown: usize,
    pub size: f32,
    pub color: [u8; 3],
    /// what `shown` was when the picture was last drawn
    drawn: Option<usize>,
}

pub struct State {
    pub files: Box<dyn Files>,
    pub arcs: Archives,
    pub screen: Screen,
    /// ms since the start
    pub now: f64,
    pub flags: Vec<bool>,
    pub vars: Vec<f32>,
    pub strings: HashMap<i64, String>,
    /// LegacyGame__lua_SetGameState
    pub game_state: HashMap<i64, f64>,
    /// the interface asked the scenario to wait (LUA_PROCESS_GAME_PAUSED)
    pub paused: bool,
    /// other requests of the interface (LegacyGame__lua_NeedProcess), for the engine to act on
    pub requests: Vec<(i64, i64)>,
    pub registry: HashMap<String, i64>,
    /// where the mouse is (screen pixels)
    pub cursor: (f32, f32),
    pub font: Option<Rc<fontdue::Font>>,
    /// text layers' text, by (set, layer)
    pub texts: HashMap<(String, String), Text>,
    images: HashMap<String, Option<Rc<Image>>>,
    pnas: HashMap<String, Option<Rc<Pna>>>,
    /// the set of a text field waiting for the player's text (insertCommandLineLayer)
    pub input: Option<String>,
    /// interface calls the engine doesn't know, each reported once
    pub unknown: std::collections::HashSet<String>,
}

impl State {
    pub fn new(mut files: Box<dyn Files>, w: u32, h: u32) -> State {
        let arcs = Archives::open(files.as_mut());
        State {
            files,
            arcs,
            screen: Screen::new(w, h),
            now: 0.0,
            flags: vec![false; FLAGS],
            vars: vec![0.0; VARS],
            strings: HashMap::new(),
            game_state: HashMap::new(),
            paused: false,
            requests: Vec::new(),
            registry: HashMap::new(),
            cursor: (0.0, 0.0),
            font: None,
            texts: HashMap::new(),
            images: HashMap::new(),
            pnas: HashMap::new(),
            input: None,
            unknown: Default::default(),
        }
    }

    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        self.arcs.read(self.files.as_mut(), name)
    }

    pub fn image(&mut self, name: &str) -> Option<Rc<Image>> {
        let key = name.to_lowercase();
        if let Some(i) = self.images.get(&key) {
            return i.clone();
        }
        let img = self.read(name).and_then(|b| match Image::decode(&b) {
            Ok(i) => Some(Rc::new(i)),
            Err(e) => {
                eprintln!("advhd: {name}: {e}");
                None
            }
        });
        if img.is_none() {
            eprintln!("advhd: no picture {name}");
        }
        self.images.insert(key, img.clone());
        img
    }

    pub fn pna(&mut self, name: &str) -> Option<Rc<Pna>> {
        let key = name.to_lowercase();
        if let Some(p) = self.pnas.get(&key) {
            return p.clone();
        }
        let pna = self.read(name).and_then(|b| match Pna::parse(b) {
            Ok(p) => Some(Rc::new(p)),
            Err(e) => {
                eprintln!("advhd: {name}: {e}");
                None
            }
        });
        if pna.is_none() {
            eprintln!("advhd: no PNA {name}");
        }
        self.pnas.insert(key, pna.clone());
        pna
    }

    /// A layer showing a picture file (PNA or not, by its name).
    pub fn picture_layer(&mut self, name: &str, file: &str) -> Layer {
        let content = if file.to_lowercase().ends_with(".pna") {
            self.pna(file).map_or(Content::None, Content::Pna)
        } else {
            self.image(file).map_or(Content::None, Content::Image)
        };
        Layer { file: file.to_string(), ..Layer::new(name, content) }
    }

    pub fn flag(&self, i: i64) -> bool {
        usize::try_from(i).ok().and_then(|i| self.flags.get(i).copied()).unwrap_or(false)
    }

    pub fn set_flag(&mut self, i: i64, v: bool) {
        if let Some(f) = usize::try_from(i).ok().and_then(|i| self.flags.get_mut(i)) {
            *f = v;
        }
    }

    pub fn var(&self, i: i64) -> f32 {
        usize::try_from(i).ok().and_then(|i| self.vars.get(i).copied()).unwrap_or(0.0)
    }

    pub fn set_var(&mut self, i: i64, v: f32) {
        if let Some(x) = usize::try_from(i).ok().and_then(|i| self.vars.get_mut(i)) {
            *x = v;
        }
    }

    pub fn set_text(&mut self, set: &str, layer: &str, s: &str, shown: Option<usize>) {
        let chars = text::plain(s);
        let shown = shown.unwrap_or(chars.len());
        let t = self.texts.entry((set.to_string(), layer.to_string())).or_insert(Text {
            chars: Vec::new(),
            shown: 0,
            size: 28.0,
            color: [255, 255, 255],
            drawn: None,
        });
        t.chars = chars;
        t.shown = shown;
        t.drawn = None;
    }

    /// Draws text layers whose text changed.
    pub fn draw_texts(&mut self) {
        let Some(font) = self.font.clone() else { return };
        for ((set, layer), t) in &mut self.texts {
            if t.drawn == Some(t.shown) {
                continue;
            }
            let Some(l) = self.screen.layer(set, layer) else { continue };
            if let Content::Text(b) = &mut l.content {
                let st = text::Style { size: t.size, color: t.color, shadow: true };
                b.image = Some(Rc::new(text::render(&font, &t.chars, t.shown, b, &st)));
                t.drawn = Some(t.shown);
            }
        }
    }

    pub fn text_layer(&mut self, set: &str, name: &str, b: TextBox) {
        self.screen.add(set).insert(Layer::new(name, Content::Text(b)));
        if let Some(t) = self.texts.get_mut(&(set.to_string(), name.to_string())) {
            t.drawn = None;
        }
    }

    pub fn unknown(&mut self, what: &str) {
        if self.unknown.insert(what.to_string()) {
            eprintln!("advhd: not done yet: {what}");
        }
    }
}
