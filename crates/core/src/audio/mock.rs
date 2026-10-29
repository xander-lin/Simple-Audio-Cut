use super::{
    reserve_export_path, AudioEngine, DenoiseCompletion, DenoiseResult, DenoiseUpdate, ExportEdit,
    ExportResult, ImportCompletion, ImportUpdate, RecordingInfo,
};
use crate::denoise::{DenoiseAvailability, DenoiseProvider};
use std::sync::Arc;

pub struct MockAudioEngine {
    denoise_provider: Arc<dyn DenoiseProvider>,
}

impl MockAudioEngine {
    pub fn new(denoise_provider: Arc<dyn DenoiseProvider>) -> Self {
        Self { denoise_provider }
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

    fn start_import_audio(
        &self,
        recording_id: String,
        source: String,
        target_lufs: f64,
        completion: ImportCompletion,
    ) -> Result<(), String> {
        completion(ImportUpdate::Normalizing {
            recording_id: recording_id.clone(),
        });
        match self.normalize_to_lufs(&source, target_lufs) {
            Ok(mut info) => {
                info.id = recording_id;
                completion(ImportUpdate::Complete { info });
            }
            Err(error) => completion(ImportUpdate::Failed {
                recording_id,
                source_path: source,
                error,
            }),
        }
        Ok(())
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

    fn denoise_availability(&self, sample_rate: u32) -> DenoiseAvailability {
        self.denoise_provider.availability(sample_rate)
    }

    fn start_denoise(
        &self,
        recording_id: String,
        task_id: String,
        source: String,
        sample_rate: u32,
        target_lufs: f64,
        completion: DenoiseCompletion,
    ) -> Result<(), String> {
        let session = self.denoise_provider.prepare(sample_rate)?;
        completion(DenoiseUpdate::Processing {
            recording_id: recording_id.clone(),
            task_id: task_id.clone(),
        });
        std::thread::spawn(move || {
            let output = std::path::Path::new(&source)
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(format!("simple-audio-cut-{task_id}-mock-denoise.wav"));
            match session.enhance(std::path::Path::new(&source), &output) {
                Ok(()) => completion(DenoiseUpdate::Complete {
                    result: DenoiseResult {
                        recording_id,
                        task_id,
                        path: output.display().to_string(),
                        integrated_lufs: Some(target_lufs),
                    },
                }),
                Err(error) => completion(DenoiseUpdate::Failed {
                    recording_id,
                    task_id,
                    error,
                }),
            }
        });
        Ok(())
    }

    fn export_edit(
        &self,
        source: &str,
        _deleted_regions: &[super::engine::Region],
        _muted_regions: &[super::engine::Region],
        _envelope_points: &[super::engine::EnvelopePoint],
        destination: &str,
    ) -> Result<String, String> {
        std::fs::copy(source, destination)
            .map_err(|error| format!("Unable to create mock export: {error}"))?;
        Ok(destination.into())
    }

    fn export_edits(&self, edits: &[ExportEdit], destination_dir: &str) -> Vec<ExportResult> {
        let destination_dir = std::path::Path::new(destination_dir);
        if let Err(error) = std::fs::create_dir_all(destination_dir) {
            return edits
                .iter()
                .map(|edit| ExportResult {
                    recording_id: edit.recording_id.clone(),
                    path: None,
                    error: Some(format!("Unable to create the export directory: {error}")),
                })
                .collect();
        }
        edits
            .iter()
            .map(|edit| {
                let destination = match reserve_export_path(destination_dir, &edit.name) {
                    Ok(destination) => destination,
                    Err(error) => {
                        return ExportResult {
                            recording_id: edit.recording_id.clone(),
                            path: None,
                            error: Some(error),
                        };
                    }
                };
                let result = self.export_edit(
                    &edit.source_path,
                    &edit.deleted_regions,
                    &edit.muted_regions,
                    &edit.envelope_points,
                    &destination.display().to_string(),
                );
                match result {
                    Ok(path) => ExportResult {
                        recording_id: edit.recording_id.clone(),
                        path: Some(path),
                        error: None,
                    },
                    Err(error) => {
                        let _ = std::fs::remove_file(destination);
                        ExportResult {
                            recording_id: edit.recording_id.clone(),
                            path: None,
                            error: Some(error),
                        }
                    }
                }
            })
            .collect()
    }
}
