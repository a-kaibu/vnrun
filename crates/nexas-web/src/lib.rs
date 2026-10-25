//! The NeXAS engine for the browser. web/src/engines/nexas.ts runs a [`Game`] once a frame and
//! draws its pictures with WebGPU; the game's files and saves come from the `nexas*` globals it
//! sets (backed by the player's GameFS).

use js_sys::{Float32Array, Uint8Array};
use nexas::engine::{Engine, HEIGHT, WIDTH};
use nexas::files::Files;
use nexas::gfx::Draw;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// The size of a game file, or -1 when it does not exist.
    #[wasm_bindgen(js_name = nexasFileSize)]
    fn file_size(path: &str) -> f64;
    #[wasm_bindgen(js_name = nexasReadRange)]
    fn read_range(path: &str, offset: f64, length: f64) -> Option<Vec<u8>>;
    /// The names at the top of the game folder.
    #[wasm_bindgen(js_name = nexasList)]
    fn list() -> Vec<String>;
    #[wasm_bindgen(js_name = nexasReadSave)]
    fn read_save(name: &str) -> Option<Vec<u8>>;
    #[wasm_bindgen(js_name = nexasWriteSave)]
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
    pub fn new() -> Result<Game, JsError> {
        let e = Engine::new(Box::new(Host)).map_err(|e| JsError::new(&e))?;
        Ok(Game {
            e,
            draws: Vec::new(),
        })
    }

    pub fn width(&self) -> u32 {
        WIDTH
    }

    pub fn height(&self) -> u32 {
        HEIGHT
    }

    /// Runs one frame (the scripts count on 60 a second).
    pub fn frame(&mut self) -> Result<(), JsError> {
        self.e.frame().map_err(|e| JsError::new(&e))
    }

    pub fn ended(&self) -> bool {
        self.e.ended()
    }

    pub fn click(&mut self, x: i32, y: i32) {
        self.e.click(x, y);
    }

    /// The pictures to draw over black, bottom first, DRAW_FIELDS numbers each: picture,
    /// its width and height, the screen positions of its top left, top right, bottom left and
    /// bottom right corners (x y each), the part of the picture (x0 y0 x1 y1), opacity (0..1),
    /// added (1) or laid over (0), and the part of the screen it may cover (x0 y0 x1 y1).
    pub fn draws(&mut self) -> Float32Array {
        self.draws = self.e.draws();
        let mut out = Vec::with_capacity(self.draws.len() * DRAW_FIELDS);
        let (w, h) = (WIDTH as f32, HEIGHT as f32);
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
