//! An AdvHD (WillPlus) engine. A game is a set of .arc archives: Rio.arc holds the scenario
//! scripts (.ws2, [`ws2`]), Script.arc the interface in precompiled Lua (run by `luavm`),
//! the others pictures (.png, .pna) and sounds. The argument layout of each script command
//! comes from the game's AdvHD.exe ([`exe`]), since it changes between engine versions.

mod builtin;
pub mod exe;
pub mod files;
pub mod ws2;

/// Shift_JIS (CP932) text, the encoding of script strings before engine 2.1.
pub fn sjis(b: &[u8]) -> String {
    encoding_rs::SHIFT_JIS.decode_without_bom_handling(b).0.into_owned()
}
