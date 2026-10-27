//! Entis GLS (Leshade Entis): the game is a set of NOA archives (Data/*.noa) holding the
//! compiled Cotopha script (script.csx), ERI pictures and MIO sounds. The archive passwords are
//! listed in the exe's IDR_COTOMI resource.

pub mod csx;
pub mod eri;
pub mod erisa;
pub mod game;
pub mod noa;
pub mod pe;
