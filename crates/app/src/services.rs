//! Replaceable services: audio engine, playback, dialogs, export memory.
//!
//! Every trait has a production implementation and a deterministic mock, so
//! the whole application can run headless in tests and in CI.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use simple_audio_cut_core::analysis::{analyze_rms, RmsFrame, RmsOptions};
use simple_audio_cut_core::audio::{AudioEngine, AudioPlayer, MockAudioEngine, MockAudioPlayer};
use simple_audio_cut_core::buffer::AudioBuffer;
use simple_audio_cut_core::denoise::{DenoiseProvider, MockDenoiseProvider};
#[cfg(feature = "native-audio")]
use simple_audio_cut_core::export_state::FileExportDirectoryStore;
use simple_audio_cut_core::export_state::{ExportDirectoryStore, MemoryExportDirectoryStore};

use crate::events::SharedSink;

/// Runs blocking work away from the UI thread.
///
/// Production spawns a thread per job. Tests can plug in [`InlineJobRunner`]
/// and every job completes before the message handler returns, which removes
/// the sleeps and timeouts that used to make the workflow tests flaky.
pub trait JobRunner: Send + Sync {
    fn spawn(&self, job: Box<dyn FnOnce() + Send + 'static>);
}

/// Production runner: one detached worker thread per job.
pub struct ThreadJobRunner;

impl JobRunner for ThreadJobRunner {
    fn spawn(&self, job: Box<dyn FnOnce() + Send + 'static>) {
        let _ = std::thread::Builder::new()
            .name("simple-audio-cut-job".into())
            .spawn(job);
    }
}

/// Test runner: executes the job immediately on the calling thread.
pub struct InlineJobRunner;

impl JobRunner for InlineJobRunner {
    fn spawn(&self, job: Box<dyn FnOnce() + Send + 'static>) {
        job();
    }
}

/// Native file dialogs, always executed worker threads.
pub trait DialogService: Send + Sync {
    fn pick_files(&self, title: &str, extensions: &[&str]) -> Result<Vec<PathBuf>, String>;
    fn pick_folder(
        &self,
        title: &str,
        default_path: Option<&str>,
    ) -> Result<Option<PathBuf>, String>;
    fn pick_file(&self, title: &str) -> Result<Option<PathBuf>, String>;
}

/// Mock dialogs returning whatever the test queued, in order.
#[derive(Default)]
pub struct MockDialogService {
    files: Mutex<Vec<Vec<PathBuf>>>,
    folders: Mutex<Vec<Option<PathBuf>>>,
    single_files: Mutex<Vec<Option<PathBuf>>>,
    error: Mutex<Option<String>>,
}

impl MockDialogService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue_files(&self, paths: Vec<PathBuf>) {
        if let Ok(mut files) = self.files.lock() {
            files.push(paths);
        }
    }

    pub fn queue_folder(&self, path: Option<PathBuf>) {
        if let Ok(mut folders) = self.folders.lock() {
            folders.push(path);
        }
    }

    pub fn queue_single_file(&self, path: Option<PathBuf>) {
        if let Ok(mut single_files) = self.single_files.lock() {
            single_files.push(path);
        }
    }

    pub fn fail(&self, message: impl Into<String>) {
        if let Ok(mut error) = self.error.lock() {
            *error = Some(message.into());
        }
    }

    fn check_error(&self) -> Result<(), String> {
        match self.error.lock() {
            Ok(error) => match error.as_ref() {
                Some(message) => Err(message.clone()),
                None => Ok(()),
            },
            Err(_) => Err("Dialog error state is unavailable.".into()),
        }
    }
}

impl DialogService for MockDialogService {
    fn pick_files(&self, _title: &str, _extensions: &[&str]) -> Result<Vec<PathBuf>, String> {
        self.check_error()?;
        let next = self
            .files
            .lock()
            .ok()
            .and_then(|mut files| {
                if files.is_empty() {
                    None
                } else {
                    Some(files.remove(0))
                }
            })
            .unwrap_or_default();
        Ok(next)
    }

    fn pick_folder(
        &self,
        _title: &str,
        _default_path: Option<&str>,
    ) -> Result<Option<PathBuf>, String> {
        self.check_error()?;
        Ok(self.folders.lock().ok().and_then(|mut folders| {
            if folders.is_empty() {
                None
            } else {
                folders.remove(0)
            }
        }))
    }

    fn pick_file(&self, _title: &str) -> Result<Option<PathBuf>, String> {
        self.check_error()?;
        Ok(self.single_files.lock().ok().and_then(|mut files| {
            if files.is_empty() {
                None
            } else {
                files.remove(0)
            }
        }))
    }
}

/// Real dialog implementation backed by the system portal or GTK dialog.
pub struct RfdDialogService;

impl DialogService for RfdDialogService {
    fn pick_files(&self, title: &str, extensions: &[&str]) -> Result<Vec<PathBuf>, String> {
        let files = rfd::FileDialog::new()
            .set_title(title)
            .add_filter("Audio or video", extensions)
            .pick_files()
            .unwrap_or_default();
        Ok(files)
    }

    fn pick_folder(
        &self,
        title: &str,
        default_path: Option<&str>,
    ) -> Result<Option<PathBuf>, String> {
        let mut dialog = rfd::FileDialog::new().set_title(title);
        if let Some(path) = default_path.filter(|path| !path.is_empty()) {
            dialog = dialog.set_directory(path);
        }
        Ok(dialog.pick_folder())
    }

    fn pick_file(&self, title: &str) -> Result<Option<PathBuf>, String> {
        Ok(rfd::FileDialog::new().set_title(title).pick_file())
    }
}

pub const IMPORT_EXTENSIONS: &[&str] = &[
    "wav", "mp3", "m4a", "aac", "flac", "ogg", "opus", "aiff", "mp4", "mov", "mkv", "webm", "m4v",
    "avi",
];

/// Everything the application needs from the outside world.
#[derive(Clone)]
pub struct Services {
    pub engine: Arc<dyn AudioEngine>,
    pub denoise: Arc<dyn DenoiseProvider>,
    pub player: Arc<dyn AudioPlayer>,
    pub dialogs: Arc<dyn DialogService>,
    pub export_dirs: Arc<dyn ExportDirectoryStore>,
    pub events: SharedSink,
    /// Executes the blocking jobs dispatched by [`crate::jobs`].
    pub jobs: Arc<dyn JobRunner>,
}

impl Services {
    /// Builds the real services: native engine, CPAL playback and portal dialogs.
    #[cfg(feature = "native-audio")]
    pub fn production() -> Result<Self, String> {
        use config_paths::config_dir;
        use simple_audio_cut_core::audio::CpalAudioPlayer;

        let config = config_dir();
        let denoise_config_path = config.join("denoise.json");
        #[cfg(feature = "clearvoice-denoise")]
        let denoise: Arc<dyn DenoiseProvider> = Arc::new(
            simple_audio_cut_core::denoise::ClearVoiceProvider::new(denoise_config_path),
        );
        #[cfg(not(feature = "clearvoice-denoise"))]
        let denoise: Arc<dyn DenoiseProvider> = {
            let _ = denoise_config_path;
            Arc::new(simple_audio_cut_core::denoise::DisabledDenoiseProvider::new())
        };
        let engine = simple_audio_cut_core::audio::NativeAudioEngine::new_in_temp(denoise.clone())?;
        let bus: SharedSink = Arc::new(crate::events::EventBus::new());
        Ok(Self {
            engine: Arc::new(engine),
            denoise,
            player: Arc::new(CpalAudioPlayer::new()),
            dialogs: Arc::new(RfdDialogService),
            export_dirs: Arc::new(FileExportDirectoryStore::new(
                config.join("last-export-directory"),
            )),
            events: bus,
            jobs: Arc::new(ThreadJobRunner),
        })
    }

    /// Headless service set used by tests: no audio device, no dialogs.
    pub fn mock() -> Self {
        let denoise: Arc<dyn DenoiseProvider> = Arc::new(MockDenoiseProvider::new());
        let bus: SharedSink = Arc::new(crate::events::EventBus::new());
        Self {
            engine: Arc::new(MockAudioEngine::new(denoise.clone())),
            denoise,
            player: Arc::new(MockAudioPlayer::deterministic()),
            dialogs: Arc::new(MockDialogService::new()),
            export_dirs: Arc::new(MemoryExportDirectoryStore::new()),
            events: bus,
            jobs: Arc::new(InlineJobRunner),
        }
    }

    /// Headless set with caller-supplied mocks, used by integration tests.
    pub fn headless(
        engine: Arc<dyn AudioEngine>,
        player: Arc<dyn AudioPlayer>,
        dialogs: Arc<dyn DialogService>,
        events: SharedSink,
    ) -> Self {
        Self::headless_with(
            engine,
            Arc::new(MockDenoiseProvider::new()),
            player,
            dialogs,
            events,
        )
    }

    /// Like [`Services::headless`], but shares one denoise provider with the
    /// engine — the production wiring. Tests that configure the provider must
    /// use this, or the engine and the UI would disagree about availability.
    pub fn headless_with(
        engine: Arc<dyn AudioEngine>,
        denoise: Arc<dyn DenoiseProvider>,
        player: Arc<dyn AudioPlayer>,
        dialogs: Arc<dyn DialogService>,
        events: SharedSink,
    ) -> Self {
        Self {
            engine,
            denoise,
            player,
            dialogs,
            export_dirs: Arc::new(MemoryExportDirectoryStore::new()),
            events,
            jobs: Arc::new(InlineJobRunner),
        }
    }

    /// Replaces the job runner. The headless constructors default to
    /// [`InlineJobRunner`]; real-engine tests opt back into threads.
    pub fn with_jobs(mut self, jobs: Arc<dyn JobRunner>) -> Self {
        self.jobs = jobs;
        self
    }
}

/// Decodes a normalized WAV and computes the RMS frames used by the waveform.
pub fn load_audio(
    path: impl AsRef<Path>,
) -> Result<(Arc<AudioBuffer>, Arc<Vec<RmsFrame>>), String> {
    let buffer = AudioBuffer::decode_wav(path)?;
    let rms = analyze_rms(&buffer, RmsOptions::default());
    Ok((Arc::new(buffer), Arc::new(rms)))
}

pub mod config_paths {
    use std::path::PathBuf;

    pub const APP_ID: &str = "io.github.xander-lin.simple-audio-cut";

    pub fn config_dir() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(APP_ID)
    }
}

#[cfg(test)]
mod tests {
    use super::{load_audio, DialogService, MockDialogService};
    use std::path::PathBuf;

    #[test]
    fn mock_dialogs_return_queued_paths_in_order() {
        let dialogs = MockDialogService::new();
        dialogs.queue_files(vec![PathBuf::from("/tmp/a.wav")]);
        dialogs.queue_folder(Some(PathBuf::from("/tmp/exports")));
        dialogs.queue_single_file(Some(PathBuf::from("/usr/bin/python3")));

        assert_eq!(
            dialogs.pick_files("Import", &[]).expect("files"),
            vec![PathBuf::from("/tmp/a.wav")]
        );
        assert_eq!(
            dialogs.pick_folder("Export", None).expect("folder"),
            Some(PathBuf::from("/tmp/exports"))
        );
        assert_eq!(
            dialogs.pick_file("Python").expect("file"),
            Some(PathBuf::from("/usr/bin/python3"))
        );
        assert!(dialogs.pick_files("Import", &[]).expect("files").is_empty());
    }

    #[test]
    fn mock_dialogs_can_fail() {
        let dialogs = MockDialogService::new();
        dialogs.fail("portal unavailable");
        assert_eq!(
            dialogs.pick_files("Import", &[]).unwrap_err(),
            "portal unavailable"
        );
    }

    #[test]
    fn loading_audio_reports_missing_files() {
        assert!(load_audio("/nonexistent/simple-audio-cut.wav").is_err());
    }
}
