//! The Majiro engine. The format and bytecode notes follow the public research of
//! majiro-py (trigger-segfault, MIT) and GARbro (morkt); the runtime is our own.
pub mod arc;
pub mod console;
pub mod engine;
pub mod gfx;
pub mod mjo;
pub mod rct;
pub mod save;
pub mod sys;
pub mod value;
pub mod vm;
