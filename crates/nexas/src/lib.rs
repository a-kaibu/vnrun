//! A NeXAS (Giga) engine: reads the PAC archives of a game and runs its compiled scripts.
//!
//! The formats are documented in the modules: [`pac`] (archives), [`script`] (compiled scripts),
//! [`vm`] (the script machine).

pub mod files;
pub mod pac;
pub mod script;
pub mod sjis;
pub mod vm;
