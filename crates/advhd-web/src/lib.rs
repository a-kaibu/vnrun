//! The AdvHD engine for the browser. web/src/engines/advhd.ts runs a [`Game`] once a frame and
//! draws its pictures with WebGPU; the game's files and saves come from the `advhd*` globals it
//! sets (backed by the player's GameFS).

use advhd::engine::Engine;
use advhd::files::Files;
use advhd::state::Sound;
use js_sys::{Array, Float32Array, Object, Reflect, Uint8Array};
use nexas::gfx::Draw;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// The size of a game file, or -1 when it does not exist.
    #[wasm_bindgen(js_name = advhdFileSize)]
    fn file_size(path: &str) -> f64;
    #[wasm_bindgen(js_name = advhdReadRange)]
    fn read_range(path: &str, offset: f64, length: f64) -> Option<Vec<u8>>;
    /// The names at the top of the game folder.
    #[wasm_bindgen(js_name = advhdList)]
    fn list() -> Vec<String>;
    #[wasm_bindgen(js_name = advhdReadSave)]
    fn read_save(name: &str) -> Option<Vec<u8>>;
    #[wasm_bindgen(js_name = advhdListSaves)]
    fn list_saves() -> Vec<String>;
    #[wasm_bindgen(js_name = advhdWriteSave)]
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
        read_save(name)
    }
    fn list_saves(&mut self) -> Vec<String> {
        list_saves()
    }
    fn write_save(&mut self, name: &str, bytes: &[u8]) {
        write_save(name, bytes)
    }
}

/// Numbers per draw in [`Game::draws`].
pub const DRAW_FIELDS: usize = 21;

#[wasm_bindgen]
pub struct Game {
    e: Engine,
    /// the last frame's draws, which hold the pictures `texture` hands out
    draws: Vec<Draw>,
}

#[wasm_bindgen]
impl Game {
    #[wasm_bindgen(constructor)]
    /// `font`: a TrueType font for the text (empty: none).
    pub fn new(font: &[u8]) -> Result<Game, JsError> {
        let mut e = Engine::new(Box::new(Host)).map_err(|e| JsError::new(&e))?;
        if !font.is_empty() {
            e.set_font(font).map_err(|e| JsError::new(&e))?;
        }
        e.set_time(js_sys::Date::now() / 1000.0);
        Ok(Game {
            e,
            draws: Vec::new(),
        })
    }

    pub fn width(&self) -> u32 {
        self.e.width()
    }

    pub fn height(&self) -> u32 {
        self.e.height()
    }

    /// Runs one frame (60 a second).
    pub fn frame(&mut self) -> Result<(), JsError> {
        self.e.frame().map_err(|e| JsError::new(&e))
    }

    pub fn ended(&self) -> bool {
        self.e.ended()
    }

    /// A click at a screen point; (-1, -1) is a click on nothing (Enter, Space).
    pub fn click(&mut self, x: f32, y: f32) {
        self.e.click(x, y);
    }

    pub fn right_click(&mut self, x: f32, y: f32) {
        self.e.right_click(x, y);
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.e.mouse_move(x, y);
    }

    /// Whether the game waits for typed text (a name).
    /// Sounds to start and stop since the last call: objects {kind: "play", ch, data, looped,
    /// volume, fade} or {kind: "stop", ch, fade} (fade in ms).
    pub fn sounds(&mut self) -> Array {
        let out = Array::new();
        for snd in self.e.take_sounds() {
            let o = Object::new();
            let set = |k: &str, v: JsValue| {
                let _ = Reflect::set(&o, &k.into(), &v);
            };
            match snd {
                Sound::Play {
                    ch,
                    data,
                    looped,
                    volume,
                    fade_ms,
                } => {
                    set("kind", "play".into());
                    set("ch", ch.into());
                    set("data", Uint8Array::from(&data[..]).into());
                    set("looped", looped.into());
                    set("volume", volume.into());
                    set("fade", fade_ms.into());
                }
                Sound::Stop { ch, fade_ms } => {
                    set("kind", "stop".into());
                    set("ch", ch.into());
                    set("fade", fade_ms.into());
                }
            }
            out.push(&o);
        }
        out
    }

    pub fn input_wanted(&self) -> bool {
        self.e.input_wanted()
    }

    pub fn enter_text(&mut self, text: &str) {
        self.e.enter_text(text);
    }

    /// Where the scenario is, for the console.
    pub fn describe(&self) -> String {
        self.e.describe()
    }

    /// The pictures to draw over black, bottom first, DRAW_FIELDS numbers each: picture,
    /// its width and height, the screen positions of its top left, top right, bottom left and
    /// bottom right corners (x y each), the part of the picture (x0 y0 x1 y1), opacity (0..1),
    /// added (1) or laid over (0), and the part of the screen it may cover (x0 y0 x1 y1).
    pub fn draws(&mut self) -> Float32Array {
        self.draws = self.e.draws();
        let mut out = Vec::with_capacity(self.draws.len() * DRAW_FIELDS);
        let (w, h) = (self.e.width() as f32, self.e.height() as f32);
        for d in &self.draws {
            out.extend([d.image.id as f32, d.image.w as f32, d.image.h as f32]);
            out.extend(d.corners().into_iter().flatten());
            out.extend(d.src.map(|v| v as f32));
            let c = d.place.clip;
            out.extend([
                d.place.alpha.clamp(0.0, 1.0),
                d.place.add as u8 as f32,
                c[0].clamp(0.0, w),
                c[1].clamp(0.0, h),
                c[2].clamp(0.0, w),
                c[3].clamp(0.0, h),
            ]);
        }
        Float32Array::from(&out[..])
    }

    /// The RGBA pixels (not premultiplied) of a picture of the last draws.
    pub fn texture(&self, id: u32) -> Uint8Array {
        self.draws.iter().find(|d| d.image.id == id).map_or_else(
            || Uint8Array::new_with_length(0),
            |d| Uint8Array::from(&d.image.px[..]),
        )
    }
}
