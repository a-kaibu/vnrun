//! Propeller's engine ("Script Engine v1.0a", Bullet Butlers 2007 and later): MPK archives,
//! MGR pictures (LZ-packed BMP), MSC scripts run by a byte-code machine with threads and
//! sprites. Formats and commands were read from BB.exe; GARbro's Propeller/ArcMPK.cs and
//! ImageMGR.cs (MIT) describe the archive and picture formats the same way.

pub mod engine;
pub mod mgr;
pub mod mpk;
pub mod msc;
