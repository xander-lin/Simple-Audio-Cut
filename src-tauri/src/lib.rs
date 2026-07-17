mod audio;

#[cfg(feature = "native-audio")]
use audio::NativeAudioEngine;
use audio::{AudioEngine, DenoiseResult, EnvelopePoint, RecordingInfo, Region};
use serde::Serialize;
use tauri::{Emitter, Manager};

#[cfg(feature = "native-audio")]
type AppAudioEngine = NativeAudioEngine;
#[cfg(not(feature = "native-audio"))]
type AppAudioEngine = audio::MockAudioEngine;

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum DenoiseEvent {
    Complete { result: DenoiseResult },
    Failed { recording_id: String, error: String },
}

#[tauri::command]
fn start_recording(engine: tauri::State<'_, AppAudioEngine>) -> Result<(), String> {
    engine.inner().start_recording()
}

#[tauri::command]
fn stop_recording(
    engine: tauri::State<'_, AppAudioEngine>,
    target_lufs: f64,
) -> Result<RecordingInfo, String> {
    engine.inner().stop_recording(target_lufs)
}

#[tauri::command]
fn import_audio(
    engine: tauri::State<'_, AppAudioEngine>,
    source_path: String,
    target_lufs: f64,
) -> Result<RecordingInfo, String> {
    engine.inner().import_audio(&source_path, target_lufs)
}

#[tauri::command]
fn start_denoise(
    app: tauri::AppHandle,
    engine: tauri::State<'_, AppAudioEngine>,
    recording_id: String,
    source_path: String,
) -> Result<(), String> {
    let event_id = recording_id.clone();
    engine.inner().start_denoise(
        recording_id,
        source_path,
        Box::new(move |outcome| {
            let payload = match outcome {
                Ok(result) => DenoiseEvent::Complete { result },
                Err(error) => DenoiseEvent::Failed {
                    recording_id: event_id,
                    error,
                },
            };
            let _ = app.emit("denoise-status", payload);
        }),
    )
}

#[tauri::command]
fn export_edit(
    engine: tauri::State<'_, AppAudioEngine>,
    source_path: String,
    deleted_regions: Vec<Region>,
    envelope_points: Vec<EnvelopePoint>,
) -> Result<String, String> {
    engine
        .inner()
        .export_edit(&source_path, &deleted_regions, &envelope_points)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let recording_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("Unable to locate application data directory: {error}"))?
                .join("recordings");
            #[cfg(feature = "native-audio")]
            app.manage(
                NativeAudioEngine::new(recording_dir)
                    .map_err(|error| format!("Unable to initialize audio engine: {error}"))?,
            );
            #[cfg(not(feature = "native-audio"))]
            {
                let _ = recording_dir;
                app.manage(audio::MockAudioEngine::new());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            import_audio,
            start_denoise,
            export_edit
        ]);
    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
