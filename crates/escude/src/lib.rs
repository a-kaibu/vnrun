//! Escu:de (エスクード): the game is ACPX C source in ESC-ARC archives (the .bin files beside
//! the exe), compiled here and run on a small virtual machine whose built-in functions draw,
//! play sounds and read files.

pub mod archive;
pub mod c;
pub mod engine;
pub mod gfx;
pub mod vm;
