//! LiveMaker 2/3 (Ru Software): the game is a vf archive appended to the exe (or game.dat /
//! game.ext beside it) holding the charts (.lsb), pictures (.gal, .png, .jpg, .bmp), sounds
//! and the project settings (live.lpb).

pub mod archive;
pub mod gal;
pub mod lsb;
pub mod names;
pub mod tpword;
pub mod text;
pub mod value;
