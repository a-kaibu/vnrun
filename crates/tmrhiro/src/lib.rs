//! The Tmr-Hiro ADV System (Marine, Lune, Turumiku and other brands, about 2005 to 2023):
//! `srp.pac` holds the scenario scripts, `Agrd/Bgrd/Cgrd.pac` the pictures (GRD), `vic.pac`,
//! `se.pac`, `sse.pac` and `mus.pac` the voices, sounds and music.

pub mod engine;
pub mod grd;
pub mod ini;
pub mod marble;
pub mod pac;
pub mod save;
pub mod script;

use nexas::files::Files;
use nexas::gfx::{Draw, Image};

/// What the browser and the native runner need of an engine: the Tmr-Hiro ADV System or the
/// older Marble engine.
pub trait Game {
    fn screen(&self) -> (u32, u32);
    fn set_font(&mut self, font: &[u8]) -> Result<(), String>;
    fn set_log(&mut self, on: bool);
    fn start_at(&mut self, name: &str);
    fn point(&mut self, x: i32, y: i32);
    fn click(&mut self, x: i32, y: i32);
    fn cancel(&mut self);
    fn wheel(&mut self, up: bool);
    fn frame(&mut self) -> Result<(), String>;
    fn take_sounds(&mut self) -> Vec<engine::Sound>;
    fn draws(&mut self) -> Vec<Draw>;
    fn render(&mut self) -> Image;
    fn ended(&self) -> bool;
}

/// The engine for the game in `files`: Marble when it has `mg_data.mbl`.
pub fn open(mut files: Box<dyn Files>) -> Result<Box<dyn Game>, String> {
    if files
        .list()
        .iter()
        .any(|f| f.eq_ignore_ascii_case("mg_data.mbl"))
    {
        Ok(Box::new(marble::engine::Engine::new(files)?))
    } else {
        Ok(Box::new(engine::Engine::new(files)?))
    }
}

macro_rules! game {
    ($t:ty) => {
        impl Game for $t {
            fn screen(&self) -> (u32, u32) {
                self.screen
            }
            fn set_font(&mut self, font: &[u8]) -> Result<(), String> {
                <$t>::set_font(self, font)
            }
            fn set_log(&mut self, on: bool) {
                self.log = on;
            }
            fn start_at(&mut self, name: &str) {
                <$t>::start_at(self, name)
            }
            fn point(&mut self, x: i32, y: i32) {
                <$t>::point(self, x, y)
            }
            fn click(&mut self, x: i32, y: i32) {
                <$t>::click(self, x, y)
            }
            fn cancel(&mut self) {
                <$t>::cancel(self)
            }
            fn wheel(&mut self, up: bool) {
                <$t>::wheel(self, up)
            }
            fn frame(&mut self) -> Result<(), String> {
                <$t>::frame(self)
            }
            fn take_sounds(&mut self) -> Vec<engine::Sound> {
                <$t>::take_sounds(self)
            }
            fn draws(&mut self) -> Vec<Draw> {
                <$t>::draws(self)
            }
            fn render(&mut self) -> Image {
                <$t>::render(self)
            }
            fn ended(&self) -> bool {
                <$t>::ended(self)
            }
        }
    };
}

game!(engine::Engine);
game!(marble::engine::Engine);
