//! SakanaGL (sakanagl.dll, the "novel" engine): the packages (pkg/*.sx and *.sxstorage), the
//! tokenized files, the editor's views (.ui) and the KAG-style scenario (.ks), run by
//! [`engine::Engine`].

pub mod assets;
pub mod engine;
pub mod eval;
pub mod gfx;
pub mod json;
pub mod ks;
pub mod obj;
pub mod pkg;
pub mod sx;
pub mod text;
pub mod ui;
