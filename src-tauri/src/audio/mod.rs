mod engine;
#[cfg(not(feature = "native-audio"))]
mod mock;
#[cfg(feature = "native-audio")]
mod native;

pub use engine::{
    reserve_export_path, AudioEngine, DenoiseCompletion, DenoiseResult, DenoiseUpdate, ExportEdit,
    ExportResult, ImportCompletion, ImportUpdate, RecordingInfo,
};
#[cfg(feature = "native-audio")]
pub use engine::{EnvelopePoint, Region};
#[cfg(not(feature = "native-audio"))]
pub use mock::MockAudioEngine;
#[cfg(feature = "native-audio")]
pub use native::NativeAudioEngine;
