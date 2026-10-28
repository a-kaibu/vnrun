//! System-NNN (Black Cyc, Metalogiq, Cyc Rose and others, about 2005 to 2023). The game folder
//! holds `dwq/*.gpk` picture packs (indexed by `*.gtb`), `cdvaw/*.vpk` voice packs (`*.vtb`),
//! `vaw/` and `wgq/` sounds and music, `init/*.xtx` (or `init2/*.fxf`) settings and `spt/*.spt`
//! scripts. The scripts are compiled records of words; the engine here runs them, draws the
//! layers they set up and puts our own menus in place of the game's system screens.

pub mod dwq;
pub mod engine;
pub mod pack;
pub mod spt;
pub mod vaw;
pub mod xtx;
