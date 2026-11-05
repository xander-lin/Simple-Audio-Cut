//! Shared, GUI-independent implementation of Simple Audio Cut.
//!
//! The crate is deliberately free of any windowing toolkit dependency so the
//! Tauri build and the native GUI build run the exact same audio pipeline,
//! editing model and tests.

pub mod analysis;
pub mod audio;
pub mod buffer;
pub mod denoise;
pub mod direct_edit;
pub mod export_state;
pub mod import_names;
pub mod playback;
pub mod region;
pub mod timeline;

pub use buffer::AudioBuffer;
pub use region::{EnvelopePoint, Region};
