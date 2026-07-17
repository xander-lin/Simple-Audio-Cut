use super::{AudioEngine, DenoiseCompletion, DenoiseResult, RecordingInfo};

pub struct MockAudioEngine;

impl MockAudioEngine {
    pub fn new() -> Self {
        Self
    }
}

impl AudioEngine for MockAudioEngine {
    fn start_recording(&self) -> Result<(), String> {
        Ok(())
    }

    fn stop_recording(&self, target_lufs: f64) -> Result<RecordingInfo, String> {
        Ok(RecordingInfo {
            id: "mock-recording".into(),
            name: "Mock recording".into(),
            path: std::env::temp_dir()
                .join("simple-audio-cut/mock.wav")
                .display()
                .to_string(),
            duration_seconds: 0.0,
            integrated_lufs: Some(target_lufs),
        })
    }

    fn import_audio(&self, source: &str, target_lufs: f64) -> Result<RecordingInfo, String> {
        self.normalize_to_lufs(source, target_lufs)
    }

    fn normalize_to_lufs(&self, source: &str, target_lufs: f64) -> Result<RecordingInfo, String> {
        Ok(RecordingInfo {
            id: "mock-normalized".into(),
            name: "Mock recording".into(),
            path: source.into(),
            duration_seconds: 0.0,
            integrated_lufs: Some(target_lufs),
        })
    }

    fn start_denoise(
        &self,
        recording_id: String,
        source: String,
        completion: DenoiseCompletion,
    ) -> Result<(), String> {
        completion(Ok(DenoiseResult {
            recording_id,
            path: source,
            integrated_lufs: None,
        }));
        Ok(())
    }

    fn export_edit(
        &self,
        source: &str,
        _deleted_regions: &[super::engine::Region],
        _envelope_points: &[super::engine::EnvelopePoint],
        destination: &str,
    ) -> Result<String, String> {
        std::fs::copy(source, destination)
            .map_err(|error| format!("Unable to create mock export: {error}"))?;
        Ok(destination.into())
    }
}
