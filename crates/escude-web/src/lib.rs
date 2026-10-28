//! The Escu:de engine for the browser. web/src/engines/escude.ts runs a [`Game`] once a frame
//! and shows the screen it draws; the game's files come from the `escude*` globals it sets
//! (backed by the player's GameFS).

use escude::engine::Engine;
use js_sys::{Array, Object, Reflect, Uint8Array};
use nexas::files::Files;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// The size of a game file, or -1 when it does not exist.
    #[wasm_bindgen(js_name = escudeFileSize)]
    fn file_size(path: &str) -> f64;
    #[wasm_bindgen(js_name = escudeReadRange)]
    fn read_range(path: &str, offset: f64, length: f64) -> Option<Vec<u8>>;
    /// The names at the top of the game folder.
    #[wasm_bindgen(js_name = escudeList)]
    fn list() -> Vec<String>;
    #[wasm_bindgen(js_name = escudeReadSave)]
    fn read_save(name: &str) -> Option<Vec<u8>>;
    #[wasm_bindgen(js_name = escudeWriteSave)]
    fn write_save(name: &str, bytes: &[u8]);
}

struct Host;

impl Files for Host {
    fn size(&mut self, path: &str) -> Option<u64> {
        let n = file_size(path);
        (n >= 0.0).then_some(n as u64)
    }
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        read_range(path, offset as f64, len as f64)
    }
    fn list(&mut self) -> Vec<String> {
        list()
    }
    fn read_save(&mut self, name: &str) -> Option<Vec<u8>> {
        read_save(name).filter(|d| !d.is_empty())
    }
    fn write_save(&mut self, name: &str, bytes: &[u8]) {
        write_save(name, bytes)
    }
}

#[wasm_bindgen]
pub struct Game {
    e: Engine,
}

#[wasm_bindgen]
impl Game {
    #[wasm_bindgen(constructor)]
    /// `font`: a TrueType font for the text (empty: none).
    pub fn new(font: &[u8]) -> Result<Game, JsError> {
        let mut e = Engine::open(Box::new(Host)).map_err(|e| JsError::new(&e))?;
        if !font.is_empty() {
            e.set_font(font).map_err(|e| JsError::new(&e))?;
        }
        Ok(Game { e })
    }

    pub fn width(&self) -> u32 {
        self.e.width() as u32
    }

    pub fn height(&self) -> u32 {
        self.e.height() as u32
    }

    /// Runs the game until it waits for the next frame; `now` is ms since it started.
    pub fn frame(&mut self, now: f64) -> Result<(), JsError> {
        let r = self.e.frame(now).map_err(|e| JsError::new(&e));
        for l in self.e.st.log.drain(..) {
            web_sys_log(&l);
        }
        r
    }

    /// Counts the screens shown; the pixels change only when it does.
    pub fn serial(&self) -> f64 {
        self.e.st.frame_serial as f64
    }

    /// The screen: RGBA, width x height.
    pub fn pixels(&self) -> Uint8Array {
        Uint8Array::from(&self.e.st.frame[..])
    }

    pub fn ended(&self) -> bool {
        self.e.st.ended
    }

    pub fn point(&mut self, x: i32, y: i32) {
        self.e.point(x, y);
    }

    /// A mouse button went down: 0 left, 1 right, 2 middle.
    pub fn button_down(&mut self, button: u32) {
        self.e.button_down(button);
    }

    pub fn button_up(&mut self, button: u32) {
        self.e.button_up(button);
    }

    /// A key went down or up, by its Windows virtual-key code.
    pub fn key(&mut self, code: u32, down: bool) {
        self.e.key(code, down);
    }

    /// The wall clock: local time in seconds since 1970.
    pub fn set_clock(&mut self, secs: f64) {
        self.e.set_clock(secs as i64);
    }

    /// Sounds to start, stop or change: `channel`, `op` (play, stop, volume, pause, resume),
    /// `data` (for play), `volume` (0..1) and `repeat`.
    pub fn sounds(&mut self) -> Array {
        let out = Array::new();
        for s in self.e.take_sounds() {
            let event = Object::new();
            let set = |k: &str, v: &JsValue| {
                Reflect::set(&event, &k.into(), v).unwrap();
            };
            set("channel", &JsValue::from_f64(s.channel as f64));
            set("op", &JsValue::from_str(s.op));
            if let Some(d) = &s.data {
                set("data", &Uint8Array::from(d.as_slice()).into());
            }
            set("volume", &JsValue::from_f64(s.volume as f64));
            set("repeat", &JsValue::from_bool(s.repeat));
            out.push(&event);
        }
        out
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn web_sys_log(s: &str);
}
