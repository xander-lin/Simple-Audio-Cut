mod audio;

#[cfg(feature = "native-audio")]
use audio::NativeAudioEngine;
use audio::{
    AudioEngine, DenoiseAvailability, DenoiseResult, DenoiseUpdate, ExportEdit, ExportResult,
    RecordingInfo,
};
use tauri::{Emitter, Manager};

#[cfg(feature = "native-audio")]
type AppAudioEngine = NativeAudioEngine;
#[cfg(not(feature = "native-audio"))]
type AppAudioEngine = audio::MockAudioEngine;

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
fn denoise_availability(
    engine: tauri::State<'_, AppAudioEngine>,
    sample_rate: u32,
) -> DenoiseAvailability {
    engine.inner().denoise_availability(sample_rate)
}

#[tauri::command]
async fn start_denoise(
    app: tauri::AppHandle,
    engine: tauri::State<'_, AppAudioEngine>,
    recording_id: String,
    task_id: String,
    source_path: String,
    sample_rate: u32,
    target_lufs: f64,
) -> Result<DenoiseResult, String> {
    let (completion_tx, completion_rx) = std::sync::mpsc::channel();
    engine.inner().start_denoise(
        recording_id,
        task_id,
        source_path,
        sample_rate,
        target_lufs,
        Box::new(move |update: DenoiseUpdate| match update {
            update @ DenoiseUpdate::Processing { .. } => {
                let _ = app.emit("denoise-status", update);
            }
            DenoiseUpdate::Complete { result } => {
                let _ = completion_tx.send(Ok(result));
            }
            DenoiseUpdate::Failed { error, .. } => {
                let _ = completion_tx.send(Err(error));
            }
        }),
    )?;
    tauri::async_runtime::spawn_blocking(move || {
        completion_rx
            .recv()
            .map_err(|_| "Denoising stopped without returning a result.".to_string())?
    })
    .await
    .map_err(|error| format!("Unable to wait for denoising: {error}"))?
}

#[tauri::command]
fn export_edits(
    engine: tauri::State<'_, AppAudioEngine>,
    edits: Vec<ExportEdit>,
    destination_dir: String,
) -> Vec<ExportResult> {
    engine.inner().export_edits(&edits, &destination_dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
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
            denoise_availability,
            start_denoise,
            export_edits
        ]);
    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
