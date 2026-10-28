//! The Crowd / ANIM ADV engine (アニム and its sister brands, about 2007 to 2011): the scenario
//! is bytecode in `<game>_sce.dat` (encrypted like the GAX pictures), the pictures CWP files
//! (PNG pieces) in `cg/`, the music Ogg Vorbis in `bgm/*.eog` and the voices and sounds in
//! `voice/<game>_wave.pck`.

pub mod data;
pub mod engine;
pub mod script;
