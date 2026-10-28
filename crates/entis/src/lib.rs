//! Entis GLS (Leshade Entis): the game is a set of NOA archives (Data/*.noa) holding the
//! compiled Cotopha script (script.csx), ERI pictures and MIO sounds. The archive passwords are
//! listed in the exe's IDR_COTOMI resource.
//! The 2024 generation has no Cotopha program but xscript scenarios ([`xs`]).

pub mod csx;
pub mod engine;
pub mod eri;
pub mod erisa;
pub mod game;
pub mod mio;
pub mod natives;
pub mod noa;
pub mod pe;
pub mod save;
pub mod skin;
pub mod text;
pub mod value;
pub mod vm;
pub mod xs;
