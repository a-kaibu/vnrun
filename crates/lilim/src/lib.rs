//! LiLiM's engine (sfaNN.exe and its later builds): `.aos` archives, ABM/BMP pictures, Ogg
//! sounds and plain-text `.scr` scripts. The formats follow GARbro (MIT, by morkt): the
//! archives (`ArcAOS.cs`), the Huffman coding of the scripts and `.cmp` pictures
//! (`HuffmanCompression.cs`) and the ABM pictures (`ImageABM.cs`, `ArcABM.cs`).

pub mod arc;
pub mod engine;
pub mod pic;
pub mod script;
