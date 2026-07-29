mod engine;
#[cfg(not(feature = "native-audio"))]
mod mock;
#[cfg(feature = "native-audio")]
mod native;

pub use engine::{
    reserve_export_path, AudioEngine, DenoiseAvailability, DenoiseCompletion, DenoiseResult,
    DenoiseUpdate, ExportEdit, ExportResult, RecordingInfo,
};
#[cfg(feature = "native-audio")]
pub use engine::{EnvelopePoint, Region};
#[cfg(not(feature = "native-audio"))]
pub use mock::MockAudioEngine;
#[cfg(feature = "native-audio")]
pub use native::NativeAudioEngine;
