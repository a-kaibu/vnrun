//! The SakanaGL engine for the browser. web/src/engines/sakanagl.ts runs a [`Game`] once a frame
//! and draws its pictures with WebGPU; the game's files and saves come from the `sakanagl*`
//! globals it sets (backed by the player's GameFS).

use js_sys::{Array, Float32Array, Object, Reflect, Uint8Array};
use sakanagl::assets::Host;
use sakanagl::gfx::Draw;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// The size of a game file, or -1 when it does not exist.
    #[wasm_bindgen(js_name = sakanaglFileSize)]
    fn file_size(path: &str) -> f64;
    #[wasm_bindgen(js_name = sakanaglReadRange)]
    fn read_range(path: &str, offset: f64, length: f64) -> Option<Vec<u8>>;
    /// The names in a folder of the game.
    #[wasm_bindgen(js_name = sakanaglList)]
    fn list(dir: &str) -> Vec<String>;
    #[wasm_bindgen(js_name = sakanaglReadSave)]
    fn read_save(name: &str) -> Option<Vec<u8>>;
    #[wasm_bindgen(js_name = sakanaglWriteSave)]
    fn write_save(name: &str, bytes: &[u8]);
}

struct Web;

impl Host for Web {
    fn size(&mut self, path: &str) -> Option<u64> {
        let n = file_size(path);
        (n >= 0.0).then_some(n as u64)
    }
    fn read_range(&mut self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        read_range(path, offset as f64, len as f64)
    }
    fn list(&mut self, dir: &str) -> Vec<String> {
        list(dir)
    }
    fn read_save(&mut self, name: &str) -> Option<Vec<u8>> {
        read_save(name)
    }
    fn write_save(&mut self, name: &str, bytes: &[u8]) {
        write_save(name, bytes)
    }
}

/// Numbers per draw in [`Game::draws`].
pub const DRAW_FIELDS: usize = 21;

#[wasm_bindgen]
pub struct Game {
    e: sakanagl::engine::Engine,
    /// the last frame's draws, which hold the pictures `texture` hands out
    draws: Vec<Draw>,
}

fn set(o: &Object, k: &str, v: &JsValue) {
    Reflect::set(o, &k.into(), v).unwrap();
}

#[wasm_bindgen]
impl Game {
    #[wasm_bindgen(constructor)]
    /// `font`: a TrueType font for the text when the game's own cannot be read (empty: none).
    pub fn new(font: &[u8]) -> Result<Game, JsError> {
        let mut e = sakanagl::engine::Engine::new(Box::new(Web)).map_err(|e| JsError::new(&e))?;
        if !font.is_empty() {
            e.set_font(font).map_err(|e| JsError::new(&e))?;
        }
        Ok(Game {
            e,
            draws: Vec::new(),
        })
    }

    pub fn width(&self) -> u32 {
        self.e.screen.0
    }

    pub fn height(&self) -> u32 {
        self.e.screen.1
    }

    pub fn point(&mut self, x: i32, y: i32) {
        self.e.point(x, y);
    }

    /// Runs one frame (60 a second).
    pub fn frame(&mut self) -> Result<(), JsError> {
        self.e.frame().map_err(|e| JsError::new(&e))
    }

    pub fn sounds(&mut self) -> Array {
        let out = Array::new();
        for s in self.e.take_sounds() {
            let o = Object::new();
            set(&o, "data", &Uint8Array::from(s.data.as_slice()));
            set(&o, "volume", &JsValue::from_f64(s.volume as f64));
            set(&o, "bgm", &JsValue::from_bool(s.bgm));
            set(&o, "voice", &JsValue::from_bool(s.voice));
            set(
                &o,
                "effectChannel",
                &s.effect_channel
                    .map_or(JsValue::NULL, |n| JsValue::from_f64(n as f64)),
            );
            set(&o, "stop", &JsValue::from_bool(s.stop));
            set(&o, "fadeMs", &JsValue::from_f64(s.fade_ms as f64));
            set(&o, "looped", &JsValue::from_bool(s.looped));
            out.push(&o);
        }
        out
    }

    pub fn ended(&self) -> bool {
        self.e.ended()
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.e.click(x, y);
    }

    /// A right click or Escape: closes a menu, or hides the message window.
    pub fn cancel(&mut self) {
        self.e.cancel();
    }

    /// The wheel: up opens the log.
    pub fn wheel(&mut self, up: bool) {
        self.e.wheel(up);
    }

    /// Ctrl held: skips text.
    #[wasm_bindgen(js_name = setSkip)]
    pub fn set_skip(&mut self, on: bool) {
        self.e.set_skip(on);
    }

    /// The pictures to draw over black, bottom first, DRAW_FIELDS numbers each: picture,
    /// its width and height, the screen positions of its top left, top right, bottom left and
    /// bottom right corners (x y each), the part of the picture (x0 y0 x1 y1), opacity (0..1),
    /// added (1) or laid over (0), and the part of the screen it may cover (x0 y0 x1 y1).
    pub fn draws(&mut self) -> Float32Array {
        self.draws = self.e.draws();
        let mut out = Vec::with_capacity(self.draws.len() * DRAW_FIELDS);
        for d in &self.draws {
            out.extend([d.img.id as f32, d.img.w as f32, d.img.h as f32]);
            out.extend(d.corners.into_iter().flatten());
            out.extend(d.src);
            out.extend([d.alpha.clamp(0.0, 1.0), d.add as u8 as f32]);
            out.extend(d.clip);
        }
        Float32Array::from(&out[..])
    }

    /// The RGBA pixels (not premultiplied) of a picture of the last draws.
    pub fn texture(&self, id: u32) -> Uint8Array {
        self.draws.iter().find(|d| d.img.id == id).map_or_else(
            || Uint8Array::new_with_length(0),
            |d| Uint8Array::from(&d.img.px[..]),
        )
    }
}
