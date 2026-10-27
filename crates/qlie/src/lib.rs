//! QLIE: the engine of Clochette, Purple Software, Barista Lab and others. `pack` reads the
//! .pack archives of GameData, `abmp` the animated .b pictures, `script` and `expr` the .s
//! scripts, and `engine` runs them.

pub mod abmp;
pub mod cmds;
pub mod engine;
pub mod expr;
pub mod game;
pub mod obj;
pub mod pack;
pub mod pe;
pub mod save;
pub mod script;
pub mod text;
