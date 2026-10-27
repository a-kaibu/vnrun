//! CatSystem2 (CS2): the game folder holds cs2.exe and .int archives (scene.int with the .cst
//! scenes, fes.int with the .fes screen scripts, image.int with .hg3 pictures, ...).

pub mod adv;
pub mod cst;
pub mod engine;
pub mod exe;
pub mod expr;
pub mod fes;
pub mod game;
pub mod hg3;
pub mod int;
pub mod text;

pub fn sjis(b: &[u8]) -> String {
    nexas::sjis::decode(b)
}
