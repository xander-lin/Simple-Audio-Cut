use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub duration_seconds: f64,
    pub integrated_lufs: Option<f64>,
}

#[derive(Clone, Deserialize)]
pub struct Region {
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Deserialize)]
pub struct EnvelopePoint {
    pub time: f64,
    pub gain: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DenoiseResult {
    pub recording_id: String,
    pub path: String,
    pub integrated_lufs: Option<f64>,
}

pub type DenoiseCompletion = Box<dyn FnOnce(Result<DenoiseResult, String>) + Send>;

pub trait AudioEngine: Send + Sync {
    fn start_recording(&self) -> Result<(), String>;
    fn stop_recording(&self, target_lufs: f64) -> Result<RecordingInfo, String>;
    fn import_audio(&self, source: &str, target_lufs: f64) -> Result<RecordingInfo, String>;
    fn normalize_to_lufs(&self, source: &str, target_lufs: f64) -> Result<RecordingInfo, String>;
    fn start_denoise(
        &self,
        recording_id: String,
        source: String,
        completion: DenoiseCompletion,
    ) -> Result<(), String>;
    fn export_edit(
        &self,
        source: &str,
        deleted_regions: &[Region],
        envelope_points: &[EnvelopePoint],
        destination: &str,
    ) -> Result<String, String>;
}
