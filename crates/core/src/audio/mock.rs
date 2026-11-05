use super::{
    reserve_export_path, AudioEngine, DenoiseCompletion, DenoiseResult, DenoiseUpdate, ExportEdit,
    ExportResult, ImportCompletion, ImportUpdate, RecordingInfo,
};
use crate::denoise::{DenoiseAvailability, DenoiseProvider};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Writes a short WAV that the rest of the pipeline can decode. The mock must
/// return a real file, otherwise "record → stop" would look broken in every
/// test that only swaps the audio engine.
fn write_mock_wav(path: &std::path::Path, seconds: f64) -> Result<RecordingInfo, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Unable to create the mock recording folder: {error}"))?;
    }
    let sample_rate = 48_000_u32;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|error| format!("Unable to create the mock recording: {error}"))?;
    let frames = (seconds * sample_rate as f64) as usize;
    for index in 0..frames {
        let phase = index as f32 / sample_rate as f32 * 220.0 * std::f32::consts::TAU;
        writer
            .write_sample((phase.sin() * 0.2 * i16::MAX as f32) as i16)
            .map_err(|error| format!("Unable to write the mock recording: {error}"))?;
    }
    writer
        .finalize()
        .map_err(|error| format!("Unable to finish the mock recording: {error}"))?;
    Ok(RecordingInfo {
        id: "mock-recording".into(),
        name: "Mock recording".into(),
        path: path.display().to_string(),
        duration_seconds: frames as f64 / sample_rate as f64,
        integrated_lufs: Some(-14.0),
    })
}

/// Mock artefacts live in one temp folder that is emptied once per process,
/// so a test run does not leave a growing pile of WAV files behind.
fn mock_dir() -> &'static std::path::Path {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join("simple-audio-cut-mock");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    })
}

fn unique_mock_path(label: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    mock_dir().join(format!("{label}-{stamp}.wav"))
}

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
        let mut info = write_mock_wav(&unique_mock_path("recording"), 0.5)?;
        info.integrated_lufs = Some(target_lufs);
        Ok(info)
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
        // The caller already runs this on a worker thread; a second thread
        // only makes tests non-deterministic without adding realism.
        let output = unique_mock_path(&format!("{task_id}-mock-denoise"));
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

#[cfg(test)]
mod tests {
    use super::{write_mock_wav, MockAudioEngine};
    use crate::audio::{AudioEngine, DenoiseUpdate};
    use crate::denoise::MockDenoiseProvider;
    use std::sync::Arc;

    fn engine() -> MockAudioEngine {
        MockAudioEngine::new(Arc::new(MockDenoiseProvider::new()))
    }

    #[test]
    fn stopping_a_mock_recording_returns_a_decodable_file() {
        let info = engine().stop_recording(-14.0).expect("stop recording");
        assert!(info.duration_seconds > 0.1, "{info:?}");
        let buffer = crate::buffer::AudioBuffer::decode_wav(&info.path).expect("decode mock");
        assert!((buffer.duration() - info.duration_seconds).abs() < 0.01);
        let _ = std::fs::remove_file(&info.path);
    }

    #[test]
    fn mock_denoising_reports_processing_then_completion_for_existing_input() {
        let source = super::unique_mock_path("denoise-source");
        write_mock_wav(&source, 0.2).expect("write source");
        let updates = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = updates.clone();
        let completion = Box::new(move |update: DenoiseUpdate| {
            sink.lock().unwrap().push(update);
        });

        engine()
            .start_denoise(
                "take".into(),
                "task".into(),
                source.display().to_string(),
                48_000,
                -14.0,
                completion,
            )
            .expect("start denoise");

        let updates = updates.lock().unwrap();
        assert!(matches!(updates[0], DenoiseUpdate::Processing { .. }));
        match &updates[1] {
            DenoiseUpdate::Complete { result } => {
                assert!(std::path::Path::new(&result.path).is_file());
                let _ = std::fs::remove_file(&result.path);
            }
            other => panic!("unexpected update: {other:?}"),
        }
        drop(updates);
        let _ = std::fs::remove_file(&source);
    }
}
