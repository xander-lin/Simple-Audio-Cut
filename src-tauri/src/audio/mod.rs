mod engine;
#[cfg(not(feature = "native-audio"))]
mod mock;
#[cfg(feature = "native-audio")]
mod native;

pub use engine::{
    AudioEngine, DenoiseCompletion, DenoiseResult, EnvelopePoint, RecordingInfo, Region,
};
#[cfg(not(feature = "native-audio"))]
pub use mock::MockAudioEngine;
#[cfg(feature = "native-audio")]
pub use native::NativeAudioEngine;
