mod audio;
mod denoise;

#[cfg(feature = "native-audio")]
use audio::NativeAudioEngine;
use audio::{
    AudioEngine, DenoiseResult, DenoiseUpdate, ExportEdit, ExportResult, ImportUpdate,
    RecordingInfo,
};
use denoise::{DenoiseAvailability, DenoiseConfig, DenoiseProvider, DenoiseProviderStatus};
use std::sync::Arc;
use tauri::{Emitter, Manager};

#[cfg(feature = "native-audio")]
type AppAudioEngine = NativeAudioEngine;
#[cfg(not(feature = "native-audio"))]
type AppAudioEngine = audio::MockAudioEngine;

struct DenoiseProviderState(Arc<dyn DenoiseProvider>);

#[tauri::command]
async fn start_recording(engine: tauri::State<'_, Arc<AppAudioEngine>>) -> Result<(), String> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || engine.start_recording())
        .await
        .map_err(|error| format!("Unable to start recording: {error}"))?
}

#[tauri::command]
async fn stop_recording(
    engine: tauri::State<'_, Arc<AppAudioEngine>>,
    target_lufs: f64,
) -> Result<RecordingInfo, String> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || engine.stop_recording(target_lufs))
        .await
        .map_err(|error| format!("Unable to finish recording: {error}"))?
}

#[tauri::command]
fn start_import_audio(
    app: tauri::AppHandle,
    engine: tauri::State<'_, Arc<AppAudioEngine>>,
    recording_id: String,
    source_path: String,
    target_lufs: f64,
) -> Result<(), String> {
    engine.inner().start_import_audio(
        recording_id,
        source_path,
        target_lufs,
        Box::new(move |update: ImportUpdate| {
            let _ = app.emit("import-status", update);
        }),
    )
}

#[tauri::command]
async fn denoise_availability(
    engine: tauri::State<'_, Arc<AppAudioEngine>>,
    sample_rate: u32,
) -> Result<DenoiseAvailability, String> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || engine.denoise_availability(sample_rate))
        .await
        .map_err(|error| format!("Unable to inspect denoising availability: {error}"))
}

#[tauri::command]
async fn denoise_provider_status(
    provider: tauri::State<'_, DenoiseProviderState>,
) -> Result<DenoiseProviderStatus, String> {
    let provider = provider.0.clone();
    tauri::async_runtime::spawn_blocking(move || provider.status())
        .await
        .map_err(|error| format!("Unable to inspect denoising settings: {error}"))
}

#[tauri::command]
async fn inspect_denoise_config(
    provider: tauri::State<'_, DenoiseProviderState>,
    config: DenoiseConfig,
) -> Result<DenoiseProviderStatus, String> {
    let provider = provider.0.clone();
    tauri::async_runtime::spawn_blocking(move || provider.inspect_config(config))
        .await
        .map_err(|error| format!("Unable to inspect denoising settings: {error}"))
}

#[tauri::command]
async fn save_denoise_config(
    provider: tauri::State<'_, DenoiseProviderState>,
    config: DenoiseConfig,
) -> Result<DenoiseProviderStatus, String> {
    let provider = provider.0.clone();
    tauri::async_runtime::spawn_blocking(move || provider.save_config(config))
        .await
        .map_err(|error| format!("Unable to save denoising settings: {error}"))?
}

#[tauri::command]
async fn start_denoise(
    app: tauri::AppHandle,
    engine: tauri::State<'_, Arc<AppAudioEngine>>,
    recording_id: String,
    task_id: String,
    source_path: String,
    sample_rate: u32,
    target_lufs: f64,
) -> Result<DenoiseResult, String> {
    let (completion_tx, completion_rx) = std::sync::mpsc::channel();
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        engine.start_denoise(
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
        )
    })
    .await
    .map_err(|error| format!("Unable to start denoising: {error}"))??;
    tauri::async_runtime::spawn_blocking(move || {
        completion_rx
            .recv()
            .map_err(|_| "Denoising stopped without returning a result.".to_string())?
    })
    .await
    .map_err(|error| format!("Unable to wait for denoising: {error}"))?
}

#[tauri::command]
async fn export_edits(
    engine: tauri::State<'_, Arc<AppAudioEngine>>,
    edits: Vec<ExportEdit>,
    destination_dir: String,
) -> Result<Vec<ExportResult>, String> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || engine.export_edits(&edits, &destination_dir))
        .await
        .map_err(|error| format!("Unable to export audio: {error}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let denoise_config_path = app
                .path()
                .app_config_dir()
                .map_err(|error| format!("Unable to locate the settings directory: {error}"))?
                .join("denoise.json");
            #[cfg(all(feature = "native-audio", feature = "clearvoice-denoise"))]
            let denoise_provider: Arc<dyn DenoiseProvider> =
                Arc::new(denoise::ClearVoiceProvider::new(denoise_config_path));
            #[cfg(all(feature = "native-audio", not(feature = "clearvoice-denoise")))]
            let denoise_provider: Arc<dyn DenoiseProvider> = {
                let _ = denoise_config_path;
                Arc::new(denoise::DisabledDenoiseProvider::new())
            };
            #[cfg(not(feature = "native-audio"))]
            let denoise_provider: Arc<dyn DenoiseProvider> = {
                let _ = denoise_config_path;
                Arc::new(denoise::MockDenoiseProvider::new())
            };
            let legacy_recording_dir = app
                .path()
                .app_data_dir()
                .map(|path| path.join("recordings"));
            #[cfg(feature = "native-audio")]
            {
                if let Ok(legacy_recording_dir) = legacy_recording_dir {
                    let _ = std::fs::remove_dir_all(legacy_recording_dir);
                }
                if let Ok(data_dir) = app.path().data_dir() {
                    let _ = std::fs::remove_dir_all(
                        data_dir.join("io.github.xander_lin.simple_audio_cut/recordings"),
                    );
                }
                let engine = NativeAudioEngine::new_in_temp(denoise_provider.clone())
                    .map_err(|error| format!("Unable to initialize audio engine: {error}"))?;
                app.asset_protocol_scope()
                    .allow_directory(engine.session_dir(), true)
                    .map_err(|error| format!("Unable to expose session audio: {error}"))?;
                app.manage(Arc::new(engine));
            }
            #[cfg(not(feature = "native-audio"))]
            {
                let _ = legacy_recording_dir;
                app.manage(Arc::new(audio::MockAudioEngine::new(
                    denoise_provider.clone(),
                )));
            }
            app.manage(DenoiseProviderState(denoise_provider));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            start_import_audio,
            denoise_availability,
            denoise_provider_status,
            inspect_denoise_config,
            save_denoise_config,
            start_denoise,
            export_edits
        ]);
    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
