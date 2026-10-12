//! The engine behind plate: media discovery and decoding, the frame cache and
//! loaders, audio, color management and review notes. No UI code lives here.

pub mod audio;
pub mod bake;
pub mod cache;
pub mod captions;
pub mod color;
pub mod disk;
pub mod display;
pub mod frame;
pub mod layers;
pub mod loader;
pub mod lut;
pub mod media;
pub mod notes;
pub mod prefs;
pub mod scan;
pub mod source;

pub use plate_native;
