//! The NekoSDK engine (Waffle's trials): NEKOPACK4 .pak archives, BMP pictures with .alp
//! opacity, Ogg sounds and the "NEKOSDK_ADVSCRIPT2" scripts of the scenario editor. The archive
//! layout follows GARbro (morkt, MIT, ArcFormats/NekoSDK/ArcPAK.cs); the scripts, pictures and
//! the screen were read from the trials' own files.
pub mod arc;
pub mod engine;
pub mod ini;
pub mod pic;
pub mod script;
