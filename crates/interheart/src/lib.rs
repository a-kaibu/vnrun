//! The Interheart / Candy Soft ADV engine ("INTERHEARTADV1": Inter Heart, Inter Heart Glossy,
//! Sweet Heart, Shiba Soft, Marshmallow Soft, Hachimitsu Soft, Dessert Soft and others, about
//! 2010 onwards): FPK archives (data.fpk holds the scenario, CG/KG/FCG/chip.fpk the pictures,
//! BGM/SE/CV*.fpk the sounds), KG and BMP pictures, and the scenario as the text ACT_<letter>.TXT
//! the game reads its messages from.

pub mod engine;
pub mod fpk;
pub mod pic;
pub mod script;
