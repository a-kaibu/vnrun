//! CrossOver's own engine (Adv.exe, 2009): `PACK` archives in DATA/, GR2 pictures packed with
//! LL5.dll's run lengths, SCD scripts packed with "*Pola*" and a stack machine to run them.

pub mod engine;
pub mod gr2;
pub mod pack;
pub mod pola;
pub mod scd;
