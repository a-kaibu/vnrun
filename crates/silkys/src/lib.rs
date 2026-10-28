//! Silky's newer engine (the AI6WIN generation: `Script.arc`, `data.arc`, `layer.arc`, ...):
//! archives, AKB pictures and the `.mes` stack scripts. The archive and picture formats
//! follow GARbro (MIT, by morkt: `ArcAi6Win.cs`, `ImageAKB.cs`); the script instruction set
//! follows VNTextPatch (MIT, by arcusmaximus: `SilkysPlusDisassembler.cs`).

pub mod akb;
pub mod arc;
pub mod engine;
pub mod mes;
pub mod vm;
