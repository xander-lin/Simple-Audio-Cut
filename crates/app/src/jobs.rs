//! Worker threads for every blocking operation.
//!
//! Jobs own cloned service handles, never the application state, and report
//! exclusively through the event sink.

use std::path::PathBuf;
use std::sync::Arc;

use simple_audio_cut_core::audio::{
    AudioEngine, DenoiseUpdate, ExportEdit, ImportUpdate,
};
use simple_audio_cut_core::denoise::{DenoiseConfig, DenoiseProvider};

use crate::events::{AppEvent, DecodedClip, FileRequest, SharedSink};
use crate::model::MediaId;
use crate::services::{load_audio, DialogService, IMPORT_EXTENSIONS};

fn spawn(job: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name("simple-audio-cut-job".into())
        .spawn(job);
}

pub fn start_recording(engine: Arc<dyn AudioEngine>, events: SharedSink) {
    spawn(move || match engine.start_recording() {
        Ok(()) => events.emit(AppEvent::RecordingStarted),
        Err(error) => events.emit(AppEvent::RecordingStartFailed(error)),
    });
}

pub fn stop_recording(
    engine: Arc<dyn AudioEngine>,
    events: SharedSink,
    placeholder: MediaId,
    target_lufs: f64,
) {
    spawn(move || {
        let result = engine
            .stop_recording(target_lufs)
            .and_then(|info| {
                load_audio(&info.path).map(|(audio, rms)| DecodedClip { info, audio, rms })
            });
        events.emit(AppEvent::RecordingStopped { placeholder, result });
    });
}

pub fn import_media(
    engine: Arc<dyn AudioEngine>,
    events: SharedSink,
    id: MediaId,
    source_path: String,
    target_lufs: f64,
) {
    spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel::<Result<simple_audio_cut_core::audio::RecordingInfo, String>>();
        let sink = events.clone();
        let completion = Box::new(move |update: ImportUpdate| match update {
            ImportUpdate::Normalizing { .. } => {
                sink.emit(AppEvent::ImportNormalizing { id });
            }
            ImportUpdate::Complete { info } => {
                let _ = tx.send(Ok(info));
            }
            ImportUpdate::Failed { error, .. } => {
                let _ = tx.send(Err(error));
            }
        });
        if let Err(error) = engine.start_import_audio(
            id.to_string(),
            source_path,
            target_lufs,
            completion,
        ) {
            events.emit(AppEvent::ImportFailed { id, error });
            return;
        }
        match rx.recv() {
            Ok(Ok(info)) => match load_audio(&info.path) {
                Ok((audio, rms)) => events.emit(AppEvent::ImportCompleted {
                    id,
                    info,
                    audio,
                    rms,
                }),
                Err(error) => events.emit(AppEvent::ImportFailed { id, error }),
            },
            Ok(Err(error)) => events.emit(AppEvent::ImportFailed { id, error }),
            Err(_) => events.emit(AppEvent::ImportFailed {
                id,
                error: "The import stopped without returning a result.".into(),
            }),
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub fn denoise_media(
    engine: Arc<dyn AudioEngine>,
    denoise: Arc<dyn DenoiseProvider>,
    events: SharedSink,
    id: MediaId,
    path: String,
    sample_rate: u32,
    target_lufs: f64,
    task_id: String,
) {
    spawn(move || {
        let availability = denoise.availability(sample_rate);
        if !availability.available {
            events.emit(AppEvent::DenoiseUnavailable {
                id,
                reason: availability.reason,
            });
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel::<
            Result<simple_audio_cut_core::audio::DenoiseResult, String>,
        >();
        let sink = events.clone();
        let completion = Box::new(move |update: DenoiseUpdate| match update {
            DenoiseUpdate::Processing { task_id, .. } => {
                sink.emit(AppEvent::DenoiseStarted { id, task_id });
            }
            DenoiseUpdate::Complete { result } => {
                let _ = tx.send(Ok(result));
            }
            DenoiseUpdate::Failed { error, .. } => {
                let _ = tx.send(Err(error));
            }
        });
        let id_string = id.to_string();
        if let Err(error) = engine.start_denoise(
            id_string,
            task_id.clone(),
            path,
            sample_rate,
            target_lufs,
            completion,
        ) {
            events.emit(AppEvent::DenoiseFailed {
                id,
                task_id,
                error,
            });
            return;
        }
        match rx.recv() {
            Ok(Ok(result)) => match load_audio(&result.path) {
                Ok((audio, rms)) => events.emit(AppEvent::DenoiseCompleted {
                    id,
                    task_id: result.task_id,
                    path: result.path,
                    lufs: result.integrated_lufs,
                    audio,
                    rms,
                }),
                Err(error) => events.emit(AppEvent::DenoiseFailed {
                    id,
                    task_id,
                    error,
                }),
            },
            Ok(Err(error)) => events.emit(AppEvent::DenoiseFailed {
                id,
                task_id,
                error,
            }),
            Err(_) => events.emit(AppEvent::DenoiseFailed {
                id,
                task_id,
                error: "Denoising stopped without returning a result.".into(),
            }),
        }
    });
}

pub fn export_tracks(
    engine: Arc<dyn AudioEngine>,
    events: SharedSink,
    edits: Vec<ExportEdit>,
    signatures: Vec<(MediaId, String)>,
    destination: String,
) {
    spawn(move || {
        let results = engine.export_edits(&edits, &destination);
        events.emit(AppEvent::ExportCompleted {
            results,
            signatures,
        });
    });
}

pub fn provider_status(denoise: Arc<dyn DenoiseProvider>, events: SharedSink) {
    spawn(move || {
        events.emit(AppEvent::SettingsStatus(Ok(denoise.status())));
    });
}

pub fn inspect_config(
    denoise: Arc<dyn DenoiseProvider>,
    events: SharedSink,
    config: DenoiseConfig,
) {
    spawn(move || {
        events.emit(AppEvent::SettingsInspected(Ok(denoise.inspect_config(config))));
    });
}

pub fn save_config(
    denoise: Arc<dyn DenoiseProvider>,
    events: SharedSink,
    config: DenoiseConfig,
) {
    spawn(move || {
        events.emit(AppEvent::SettingsSaved(denoise.save_config(config)));
    });
}

pub fn choose_import_files(dialogs: Arc<dyn DialogService>, events: SharedSink) {
    choose_files(
        dialogs,
        events,
        FileRequest::ImportMedia,
        "Import audio or video",
        IMPORT_EXTENSIONS,
    );
}

pub fn choose_files(
    dialogs: Arc<dyn DialogService>,
    events: SharedSink,
    request: FileRequest,
    title: &'static str,
    extensions: &'static [&'static str],
) {
    spawn(move || match dialogs.pick_files(title, extensions) {
        Ok(paths) if paths.is_empty() => events.emit(AppEvent::PathChoiceCancelled { request }),
        Ok(paths) => events.emit(AppEvent::PathsChosen { request, paths }),
        Err(error) => events.emit(AppEvent::DialogFailed { request, error }),
    });
}

pub fn choose_file(
    dialogs: Arc<dyn DialogService>,
    events: SharedSink,
    request: FileRequest,
    title: &'static str,
) {
    spawn(move || match dialogs.pick_file(title) {
        Ok(Some(path)) => events.emit(AppEvent::PathsChosen {
            request,
            paths: vec![path],
        }),
        Ok(None) => events.emit(AppEvent::PathChoiceCancelled { request }),
        Err(error) => events.emit(AppEvent::DialogFailed { request, error }),
    });
}

pub fn choose_folder(
    dialogs: Arc<dyn DialogService>,
    events: SharedSink,
    request: FileRequest,
    title: &'static str,
    default_path: Option<String>,
) {
    spawn(move || {
        let default = default_path.unwrap_or_default();
        match dialogs.pick_folder(title, Some(default.as_str()).filter(|path| !path.is_empty())) {
            Ok(Some(path)) => events.emit(AppEvent::PathsChosen {
                request,
                paths: vec![path],
            }),
            Ok(None) => events.emit(AppEvent::PathChoiceCancelled { request }),
            Err(error) => events.emit(AppEvent::DialogFailed { request, error }),
        }
    });
}

/// Convenience for tests and for validating imports into a path list.
pub fn import_paths(paths: Vec<PathBuf>) -> Vec<String> {
    paths
        .into_iter()
        .filter_map(|path| path.to_str().map(str::to_string))
        .collect()
}
