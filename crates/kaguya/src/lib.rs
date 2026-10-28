//! The Kaguya engine (アトリエかぐや). The 2006 era: WFL1 archives and .scr bytecode (engine);
//! the 2010s: LINK archives, params.dat and message.dat (link). The archive layouts follow
//! GARbro (morkt); the bytecode was read from the trials' own files and the runtime is our own.
pub mod arc;
pub mod engine;
pub mod exe;
pub mod link;
pub mod pic;
pub mod scr;
pub mod slots;
pub mod text;

use nexas::files::Files;
use nexas::gfx::Draw;

use engine::SoundEvent;

/// Either generation, by the files of the game folder.
pub enum Game {
    Wfl(Box<engine::Engine>),
    Link(Box<link::engine::Engine>),
}

macro_rules! each {
    ($self:ident, $e:ident => $body:expr) => {
        match $self {
            Game::Wfl($e) => $body,
            Game::Link($e) => $body,
        }
    };
}

impl Game {
    pub fn open(mut files: Box<dyn Files>) -> Result<Game, String> {
        if link::engine::is_link(files.as_mut()) {
            return Ok(Game::Link(Box::new(link::engine::Engine::open(files)?)));
        }
        Ok(Game::Wfl(Box::new(engine::Engine::open(files)?)))
    }
    pub fn width(&self) -> u32 {
        each!(self, e => e.width)
    }
    pub fn height(&self) -> u32 {
        each!(self, e => e.height)
    }
    pub fn set_log(&mut self, on: bool) {
        each!(self, e => e.log = on)
    }
    pub fn set_font(&mut self, ttf: &[u8]) -> Result<(), String> {
        each!(self, e => e.set_font(ttf))
    }
    pub fn frame(&mut self) -> Result<(), String> {
        each!(self, e => e.frame())
    }
    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        each!(self, e => e.take_sounds())
    }
    pub fn point(&mut self, x: i32, y: i32) {
        each!(self, e => e.point(x, y))
    }
    pub fn click(&mut self, x: i32, y: i32) {
        each!(self, e => e.click(x, y))
    }
    pub fn cancel(&mut self) {
        each!(self, e => e.cancel())
    }
    pub fn key(&mut self, k: usize) {
        each!(self, e => e.key(k))
    }
    pub fn wheel(&mut self, up: bool) {
        each!(self, e => e.wheel(up))
    }
    pub fn choosing(&self) -> bool {
        each!(self, e => e.choosing())
    }
    pub fn ended(&self) -> bool {
        each!(self, e => e.ended())
    }
    pub fn draws(&mut self) -> Vec<Draw> {
        each!(self, e => e.draws())
    }
}
