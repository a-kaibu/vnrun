//! The YU-RIS engine for the browser. web/src/engines/yuris.ts drives a [`Game`] once a frame,
//! draws its layers with WebGPU and plays its sounds; the game's files and saves come from the
//! `yuris*` globals it sets (backed by the player's GameFS).

use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use yuris::engine::{Engine, Event};
use yuris::files::Files;

#[wasm_bindgen]
extern "C" {
    /// The size of a game file, or -1 when it does not exist.
    #[wasm_bindgen(js_name = yurisFileSize)]
    fn file_size(path: &str) -> f64;
    #[wasm_bindgen(js_name = yurisReadRange)]
    fn read_range(path: &str, offset: f64, length: f64) -> Option<Vec<u8>>;
    /// The names at the top of the game folder.
    #[wasm_bindgen(js_name = yurisRoot)]
    fn root() -> Vec<String>;
    #[wasm_bindgen(js_name = yurisReadSave)]
    fn read_save(name: &str) -> Option<Vec<u8>>;
    #[wasm_bindgen(js_name = yurisWriteSave)]
    fn write_save(name: &str, bytes: &[u8]);
    #[wasm_bindgen(js_name = yurisRemoveSave)]
    fn remove_save(name: &str);
    /// Shows a message with 1 to 3 buttons; returns which was pressed (1 = the leftmost).
    #[wasm_bindgen(js_name = yurisDialog)]
    fn dialog(text: &str, caption: &str, buttons: u32) -> i32;
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
    fn root(&mut self) -> Vec<String> {
        root()
    }
    fn read_save(&mut self, name: &str) -> Option<Vec<u8>> {
        read_save(name)
    }
    fn write_save(&mut self, name: &str, bytes: &[u8]) {
        write_save(name, bytes)
    }
    fn remove_save(&mut self, name: &str) {
        remove_save(name)
    }
    fn dialog(&mut self, text: &str, caption: &str, buttons: u32) -> i64 {
        dialog(text, caption, buttons) as i64
    }
}

/// Numbers per draw in [`Game::draws`].
pub const DRAW_FIELDS: usize = 18;

#[wasm_bindgen]
pub struct Game {
    e: Engine,
}

#[wasm_bindgen]
impl Game {
    /// Opens the game; `font` is the TrueType font the text is drawn with (may be empty).
    #[wasm_bindgen(constructor)]
    pub fn new(font: &[u8]) -> Result<Game, JsError> {
        let font = (!font.is_empty()).then_some(font);
        let e = Engine::new(Box::new(Host), font).map_err(|e| JsError::new(&e))?;
        Ok(Game { e })
    }

    pub fn width(&self) -> u32 {
        self.e.size().0
    }

    pub fn height(&self) -> u32 {
        self.e.size().1
    }

    /// Runs one frame. `now` is milliseconds since the start; `date` the local time: year,
    /// month, day, weekday, hour, minute, second, millisecond.
    pub fn frame(&mut self, now: f64, date: &[i32]) -> Result<(), JsError> {
        let mut d = [0i64; 8];
        for (to, from) in d.iter_mut().zip(date) {
            *to = *from as i64;
        }
        self.e.frame(now, d).map_err(|e| JsError::new(&e))
    }

    /// The game has ended (END, or the window was closed by the scripts).
    pub fn ended(&self) -> bool {
        self.e.vm.ended
    }

    /// The layers to draw, back to front, DRAW_FIELDS numbers each: texture, version, x, y,
    /// alpha (0..=256), blend mode, clip x0 y0 x1 y1, mask texture (-1 for none) and its x y,
    /// rule texture (-1 for none), its x y, progress (0..=256) and softness.
    pub fn draws(&self) -> Vec<i32> {
        let mut out = Vec::new();
        let n = |v: i64| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        for d in self.e.draws() {
            let (mt, mx, my) = d.mask.map_or((-1, 0, 0), |(t, x, y)| (t as i64, x, y));
            let (rt, rx, ry, t, ta) = d.rule.map_or((-1, 0, 0, 256, 0), |(r, x, y, t, ta)| (r as i64, x, y, t, ta));
            for v in [
                d.tex as i64, d.version as i64, d.x, d.y, d.a, d.mode, d.clip.x0, d.clip.y0, d.clip.x1, d.clip.y1, mt, mx,
                my, rt, rx, ry, t, ta,
            ] {
                out.push(n(v));
            }
        }
        out
    }

    /// Bumped whenever the texture's pixels change.
    pub fn texture_version(&self, tex: u32) -> u32 {
        self.e.world.layers.map.values().find(|l| l.tex == tex).map_or(0, |l| l.version)
    }

    pub fn texture_width(&self, tex: u32) -> u32 {
        self.e.texture(tex).map_or(0, |i| i.w)
    }

    pub fn texture_height(&self, tex: u32) -> u32 {
        self.e.texture(tex).map_or(0, |i| i.h)
    }

    /// The RGBA pixels of a texture (not premultiplied), rows top first.
    pub fn texture(&self, tex: u32) -> Uint8Array {
        self.e.texture(tex).map_or_else(|| Uint8Array::new_with_length(0), |i| Uint8Array::from(&i.px[..]))
    }

    pub fn set_mouse(&mut self, x: i32, y: i32) {
        self.e.input.x = x as i64;
        self.e.input.y = y as i64;
    }

    /// Holds or releases a button by its YU-RIS code (yuris::sysvar::code).
    pub fn set_button(&mut self, code: usize, held: bool) {
        if let Some(b) = self.e.input.held.get_mut(code) {
            *b = held;
        }
    }

    /// The wheel turned up (> 0) or down (< 0) since the last frame.
    pub fn wheel(&mut self, delta: i32) {
        self.e.input.wheel = delta;
    }

    /// What happened for the frontend since the last call, as objects with a `kind` of play,
    /// stop, volume, pause or title.
    pub fn events(&mut self) -> Array {
        let out = Array::new();
        for ev in std::mem::take(&mut self.e.world.events) {
            let o = Object::new();
            let set = |k: &str, v: JsValue| {
                let _ = Reflect::set(&o, &k.into(), &v);
            };
            match ev {
                Event::Play { id, data, looped, volume, fade_ms, start_ms } => {
                    set("kind", "play".into());
                    set("id", id.into());
                    set("data", Uint8Array::from(&data[..]).into());
                    set("looped", looped.into());
                    set("volume", volume.into());
                    set("fade", fade_ms.into());
                    set("start", start_ms.into());
                }
                Event::Stop { id, fade_ms } => {
                    set("kind", "stop".into());
                    set("id", id.into());
                    set("fade", fade_ms.into());
                }
                Event::Volume { id, volume, fade_ms } => {
                    set("kind", "volume".into());
                    set("id", id.into());
                    set("volume", volume.into());
                    set("fade", fade_ms.into());
                }
                Event::Pause { id, paused } => {
                    set("kind", "pause".into());
                    set("id", id.into());
                    set("paused", paused.into());
                }
                Event::Title(t) => {
                    set("kind", "title".into());
                    set("title", t.into());
                }
            }
            out.push(&o);
        }
        out
    }

    /// A sound the frontend played has ended.
    pub fn sound_ended(&mut self, id: &str) {
        self.e.sound_ended(id);
    }
}
