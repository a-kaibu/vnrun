//! A YU-RIS engine: reads the compiled project of a game (ysbin/) and runs it.
//!
//! The formats are documented in the modules: [`ypf`] (archives), [`ybn`] (compiled project),
//! [`expr`] (expressions). [`vm`] runs the scripts and hands everything that is not control
//! flow to a [`vm::Host`].

pub mod engine;
pub mod expr;
pub mod files;
pub mod image;
pub mod layers;
pub mod sjis;
pub mod sysvar;
pub mod text;
pub mod vm;
pub mod ybn;
pub mod ypf;
