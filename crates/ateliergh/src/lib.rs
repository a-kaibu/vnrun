//! Atelier G/H's ADV system (学園ＮＴＲ, 2012): `.arc` archives whose index is masked by a short
//! repeating key, LZSS-packed files, 24/32-bit BMP pictures, CSV settings (`.lib`, every byte
//! plus 0x82) and plain-text scripts of `<Tag args>` lines.

pub mod arc;
pub mod bmp;
pub mod engine;
pub mod script;
pub mod text;
