//! The Kaguya engine (アトリエかぐや, 2006 era: WFL1 archives and .scr bytecode). The archive
//! layout follows GARbro (morkt); the bytecode was read from the trial's own program and the
//! runtime is our own.
pub mod arc;
pub mod engine;
pub mod pic;
pub mod scr;
pub mod text;
