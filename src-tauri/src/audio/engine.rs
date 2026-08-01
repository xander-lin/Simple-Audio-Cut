use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

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
    #[cfg_attr(not(feature = "native-audio"), allow(dead_code))]
    pub start: f64,
    #[cfg_attr(not(feature = "native-audio"), allow(dead_code))]
    pub end: f64,
}

#[derive(Clone, Deserialize)]
pub struct EnvelopePoint {
    #[cfg_attr(not(feature = "native-audio"), allow(dead_code))]
    pub time: f64,
    #[cfg_attr(not(feature = "native-audio"), allow(dead_code))]
    pub gain: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DenoiseResult {
    pub recording_id: String,
    pub task_id: String,
    pub path: String,
    pub integrated_lufs: Option<f64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DenoiseAvailability {
    pub available: bool,
}

#[derive(Clone, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DenoiseUpdate {
    Processing {
        recording_id: String,
        task_id: String,
    },
    Complete {
        result: DenoiseResult,
    },
    #[cfg_attr(not(feature = "native-audio"), allow(dead_code))]
    Failed {
        recording_id: String,
        task_id: String,
        error: String,
    },
}

pub type DenoiseCompletion = Box<dyn Fn(DenoiseUpdate) + Send>;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportEdit {
    pub recording_id: String,
    pub name: String,
    pub source_path: String,
    pub deleted_regions: Vec<Region>,
    pub envelope_points: Vec<EnvelopePoint>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub recording_id: String,
    pub path: Option<String>,
    pub error: Option<String>,
}

pub trait AudioEngine: Send + Sync {
    fn start_recording(&self) -> Result<(), String>;
    fn stop_recording(&self, target_lufs: f64) -> Result<RecordingInfo, String>;
    fn import_audio(&self, source: &str, target_lufs: f64) -> Result<RecordingInfo, String>;
    fn normalize_to_lufs(&self, source: &str, target_lufs: f64) -> Result<RecordingInfo, String>;
    fn denoise_availability(&self, sample_rate: u32) -> DenoiseAvailability;
    fn start_denoise(
        &self,
        recording_id: String,
        task_id: String,
        source: String,
        sample_rate: u32,
        target_lufs: f64,
        completion: DenoiseCompletion,
    ) -> Result<(), String>;
    fn export_edit(
        &self,
        source: &str,
        deleted_regions: &[Region],
        envelope_points: &[EnvelopePoint],
        destination: &str,
    ) -> Result<String, String>;
    fn export_edits(&self, edits: &[ExportEdit], destination_dir: &str) -> Vec<ExportResult>;
}

pub fn reserve_export_path(destination_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let sanitized = name
        .trim()
        .chars()
        .map(|character| match character {
            '/' | '\\' | '\0' => '_',
            _ => character,
        })
        .collect::<String>();
    let stem = format!(
        "{}-edited",
        if sanitized.is_empty() {
            "audio"
        } else {
            &sanitized
        }
    );
    for suffix in 1.. {
        let filename = if suffix == 1 {
            format!("{stem}.wav")
        } else {
            format!("{stem}-{suffix}.wav")
        };
        let candidate = destination_dir.join(filename);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!("Unable to reserve an export filename: {error}"));
            }
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::{reserve_export_path, DenoiseUpdate};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn serializes_denoise_progress_for_the_frontend_contract() {
        let update = DenoiseUpdate::Processing {
            recording_id: "recording".into(),
            task_id: "task".into(),
        };

        assert_eq!(
            serde_json::to_value(update).expect("serialize progress"),
            serde_json::json!({
                "status": "processing",
                "recordingId": "recording",
                "taskId": "task",
            })
        );
    }

    #[test]
    fn reserves_sanitized_non_overwriting_export_paths() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("simple-audio-cut-path-{stamp}"));
        fs::create_dir_all(&root).expect("create test directory");

        let first = reserve_export_path(&root, "folder/voice").expect("reserve first path");
        let second = reserve_export_path(&root, "folder/voice").expect("reserve second path");

        assert_eq!(first, root.join("folder_voice-edited.wav"));
        assert_eq!(second, root.join("folder_voice-edited-2.wav"));
        assert!(first.is_file());
        assert!(second.is_file());
        fs::remove_dir_all(root).expect("remove test directory");
    }
}
