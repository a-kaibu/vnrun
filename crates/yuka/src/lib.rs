//! The Yuka engine (HOOK, SMEE, feng, about 2007 to 2013). Its games keep everything in YKC001
//! archives (`*.ykc`, or `*.dat` in the older ones): YKS001 scripts, YKG pictures (PNG inside)
//! and Ogg Vorbis sounds. The scripts call the engine's functions by name (`GraphicLoad`,
//! `StrOut`, `MouseSelect`, ...), and the title, menus and window are scripts too, so the
//! engine here is an interpreter for them with those functions.

pub mod engine;
pub mod ykc;
pub mod ykg;
pub mod yks;
