//! End-to-end state tests with mock services: no window, no audio device.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use simple_audio_cut_core::analysis::{analyze_rms, RmsOptions};
use simple_audio_cut_core::audio::{AudioPlayer, MockAudioEngine, RecordingInfo};
use simple_audio_cut_core::buffer::AudioBuffer;
use simple_audio_cut_core::denoise::{DenoiseProvider, MockDenoiseProvider};

use simple_audio_cut_iced::events::{AppEvent, EventBus, SharedSink};
use simple_audio_cut_iced::message::Message;
use simple_audio_cut_iced::model::MediaId;
use simple_audio_cut_iced::services::{MockDialogService, Services};
use simple_audio_cut_iced::update::App;

struct Harness {
    app: App,
    player: Arc<simple_audio_cut_core::audio::MockAudioPlayer>,
    dialogs: Arc<MockDialogService>,
    root: PathBuf,
}

fn unique_root() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "simple-audio-cut-app-test-{}-{stamp}",
        std::process::id()
    ))
}

fn write_wav(path: &PathBuf, frames: usize) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 1_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    for _ in 0..frames {
        writer.write_sample(1_000_i16).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

fn harness() -> Harness {
    let root = unique_root();
    let denoise = Arc::new(MockDenoiseProvider::new());
    let mut config = denoise.status().configured;
    config.enabled = false;
    denoise.save_config(config).expect("disable mock denoise");

    let engine = Arc::new(MockAudioEngine::new(denoise.clone()));
    let player = Arc::new(simple_audio_cut_core::audio::MockAudioPlayer::deterministic());
    let dialogs = Arc::new(MockDialogService::new());
    let events: SharedSink = Arc::new(EventBus::new());
    let services = Services::headless_with(
        engine,
        denoise,
        player.clone(),
        dialogs.clone(),
        events,
    );
    let (app, _) = App::boot(services);
    Harness {
        app,
        player,
        dialogs,
        root,
    }
}

impl Harness {
    fn pump(&mut self) {
        let _ = self.app.update(Message::Tick);
    }

    fn pump_until(&mut self, mut condition: impl FnMut(&App) -> bool, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            self.pump();
            if condition(&self.app) {
                return true;
            }
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn import_ready(&mut self, id: MediaId, name: &str) -> PathBuf {
        let path = self.root.join(format!("{name}.wav"));
        write_wav(&path, 1_000);
        self.app
            .session
            .media
            .push(simple_audio_cut_iced::model::MediaClip::placeholder(
                id,
                path.display().to_string(),
            ));
        let audio = AudioBuffer::decode_wav(&path).expect("decode test wav");
        let rms = analyze_rms(&audio, RmsOptions::default());
        self.app.services.events.emit(AppEvent::ImportCompleted {
            id,
            info: RecordingInfo {
                id: id.to_string(),
                name: name.into(),
                path: path.display().to_string(),
                duration_seconds: audio.duration(),
                integrated_lufs: Some(-14.0),
            },
            audio: Arc::new(audio),
            rms: Arc::new(rms),
        });
        self.pump();
        path
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.player.stop();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn importing_a_file_makes_it_ready_in_the_pool() {
    let mut harness = harness();
    let id = 11;
    harness.import_ready(id, "take-one");
    let clip = harness.app.session.media(id).expect("media clip");
    assert!(clip.ready());
    assert_eq!(clip.name, "take-one");
    assert!((clip.duration - 1.0).abs() < 1e-9);
}

#[test]
fn playback_advances_the_playhead_in_source_time() {
    let mut harness = harness();
    let id = 21;
    harness.import_ready(id, "voice");
    let _ = harness.app.update(Message::AddToTimeline(id));
    assert_eq!(harness.app.session.selected_track, Some(id));

    let _ = harness.app.update(Message::PlayPause);
    assert!(harness.app.session.playback.is_some());
    assert_eq!(harness.player.position(), Some(0.0));

    harness.player.advance(0.25);
    harness.pump();
    assert!((harness.app.session.playhead - 0.25).abs() < 1e-6);

    let _ = harness.app.update(Message::PlayPause);
    assert!(harness.app.session.playback.is_none());
    assert!((harness.app.session.playhead - 0.25).abs() < 1e-6);
    assert_eq!(harness.player.position(), None);
}

#[test]
fn automatic_denoising_off_leaves_the_take_usable_and_exportable() {
    // The harness turns automatic denoising off, which is the same end state as
    // an unconfigured ClearVoice install: no denoising, everything else works.
    let mut harness = harness();
    let id = 27;
    harness.import_ready(id, "voice");
    let _ = harness.app.update(Message::AddToTimeline(id));
    assert!(harness.pump_until(
        |app| !app.session.clip_by_id(id).unwrap().denoise.status.is_busy(),
        Duration::from_secs(5)
    ));
    let clip = harness.app.session.clip_by_id(id).unwrap();
    // Either the provider reports itself unusable, or automatic runs are off:
    // both must leave the take untouched and explain why.
    assert!(
        matches!(
            clip.denoise.status,
            simple_audio_cut_iced::model::DenoiseStatus::Skipped
                | simple_audio_cut_iced::model::DenoiseStatus::Unavailable
        ),
        "unexpected status {:?}",
        clip.denoise.status
    );
    assert!(clip.denoise.reason.is_some());
    assert!(clip.ready());

    // Editing and playback still work.
    let _ = harness.app.update(Message::PlayPause);
    assert!(harness.app.session.playback.is_some());
    let _ = harness.app.update(Message::StopPlayback);
    let _ = harness.app.update(Message::Waveform {
        track: id,
        event: simple_audio_cut_iced::message::WaveformEvent::DirectEdit {
            region: simple_audio_cut_core::region::Region::new(0.1, 0.2),
            operation: simple_audio_cut_core::direct_edit::DirectRegionOperation::Delete,
        },
    });
    assert!(harness.app.session.track(id).unwrap().needs_export());

    // And the changed track still exports.
    let export_dir = harness.root.join("exports-without-denoise");
    harness.dialogs.queue_folder(Some(export_dir.clone()));
    let _ = harness.app.update(Message::ExportChanged);
    assert!(
        harness.pump_until(
            |app| app.session.exporting.is_empty() && !app.ui.message.starts_with("Choose"),
            Duration::from_secs(30)
        ),
        "export did not finish: {}",
        harness.app.ui.message
    );
    assert!(harness.app.ui.message.starts_with("Exported"));
    assert!(std::fs::read_dir(&export_dir).unwrap().next().is_some());
}

#[test]
fn a_take_can_opt_out_of_denoising_and_still_export() {
    let mut harness = harness();
    let id = 28;
    harness.import_ready(id, "voice");
    let _ = harness.app.update(Message::AddToTimeline(id));
    let _ = harness.app.update(Message::SetDenoise {
        id,
        enabled: false,
    });
    assert_eq!(
        harness.app.session.clip_by_id(id).unwrap().denoise.status,
        simple_audio_cut_iced::model::DenoiseStatus::Skipped
    );

    let export_dir = harness.root.join("exports-opted-out");
    harness.dialogs.queue_folder(Some(export_dir.clone()));
    let _ = harness.app.update(Message::ExportChanged);
    assert!(
        harness.pump_until(
            |app| app.session.exporting.is_empty() && !app.ui.message.starts_with("Choose"),
            Duration::from_secs(30)
        ),
        "export did not finish: {}",
        harness.app.ui.message
    );
    assert!(harness.app.ui.message.starts_with("Exported"));
}

#[test]
fn stop_halts_playback_and_holds_the_current_position() {
    let mut harness = harness();
    let id = 25;
    harness.import_ready(id, "voice");
    let _ = harness.app.update(Message::AddToTimeline(id));
    let _ = harness.app.update(Message::PlayPause);
    harness.player.advance(0.4);
    harness.pump();
    assert!((harness.app.session.playhead - 0.4).abs() < 1e-6);

    let _ = harness.app.update(Message::StopPlayback);

    assert!(harness.app.session.playback.is_none());
    assert!((harness.app.session.playhead - 0.4).abs() < 1e-6);
    assert_eq!(harness.player.position(), None);

    // Pressing stop again must not disturb the held position.
    let _ = harness.app.update(Message::StopPlayback);
    assert!((harness.app.session.playhead - 0.4).abs() < 1e-6);
}

#[test]
fn deleting_a_range_changes_the_exported_audio_and_survives_collapse() {
    let mut harness = harness();
    let id = 31;
    harness.import_ready(id, "voice");
    let _ = harness.app.update(Message::AddToTimeline(id));
    let _ = harness.app.update(Message::Waveform {
        track: id,
        event: simple_audio_cut_iced::message::WaveformEvent::DirectEdit {
            region: simple_audio_cut_core::region::Region::new(0.2, 0.4),
            operation: simple_audio_cut_core::direct_edit::DirectRegionOperation::Delete,
        },
    });
    let track = harness.app.session.track(id).expect("track");
    assert_eq!(track.manual_deleted.len(), 1);
    assert!((track.edited_duration() - 0.8).abs() < 1e-9);
    assert!(track.needs_export());

    let _ = harness.app.update(Message::ToggleCollapse(id));
    let track = harness.app.session.track(id).expect("track");
    assert!(track.collapsed);
    assert!((track.edited_duration() - 0.8).abs() < 1e-9);
    assert!(track.collapsed_audio.is_some());
}

#[test]
fn export_changed_writes_only_changed_tracks_and_marks_them_exported() {
    let mut harness = harness();
    let id = 41;
    harness.import_ready(id, "voice");
    let _ = harness.app.update(Message::AddToTimeline(id));
    let export_dir = harness.root.join("exports");
    harness.dialogs.queue_folder(Some(export_dir.clone()));

    let _ = harness.app.update(Message::ExportChanged);
    let finished = harness.pump_until(
        |app| app.session.exporting.is_empty() && !app.ui.message.starts_with("Choose"),
        Duration::from_secs(30),
    );
    assert!(finished, "export did not finish: {}", harness.app.ui.message);
    assert!(harness.app.ui.message.starts_with("Exported"), "{}", harness.app.ui.message);

    let exported: Vec<_> = std::fs::read_dir(&export_dir)
        .expect("export directory")
        .filter_map(Result::ok)
        .collect();
    assert_eq!(exported.len(), 1);
    let track = harness.app.session.track(id).expect("track");
    assert!(!track.needs_export());
    assert!(!harness.app.session.changed_tracks().contains(&id));
}

#[test]
fn cancelling_the_export_dialog_leaves_tracks_pending() {
    let mut harness = harness();
    let id = 51;
    harness.import_ready(id, "voice");
    let _ = harness.app.update(Message::AddToTimeline(id));
    harness.dialogs.queue_folder(None);

    let _ = harness.app.update(Message::ExportChanged);
    let finished = harness
        .pump_until(|app| app.session.exporting.is_empty(), Duration::from_secs(5));
    assert!(finished);
    assert_eq!(harness.app.ui.message, "Export cancelled");
    assert!(harness.app.session.track(id).expect("track").needs_export());
}

#[test]
fn moving_a_track_back_to_the_pool_keeps_the_clip_selectable() {
    let mut harness = harness();
    let id = 61;
    harness.import_ready(id, "voice");
    let _ = harness.app.update(Message::AddToTimeline(id));
    let _ = harness.app.update(Message::ReturnTrackToPool(id));
    assert!(harness.app.session.track(id).is_none());
    assert!(harness.app.session.media(id).is_some());
    assert_eq!(harness.app.session.selected_media, Some(id));
    assert_eq!(harness.app.session.selected_track, None);
}

#[test]
fn shortcuts_are_ignored_while_the_settings_drawer_is_open() {
    let mut harness = harness();
    let id = 71;
    harness.import_ready(id, "voice");
    let _ = harness.app.update(Message::AddToTimeline(id));
    let _ = harness.app.update(Message::OpenSettings);
    harness.pump();
    let _ = harness.app.update(Message::Shortcut(
        simple_audio_cut_iced::message::Shortcut::TogglePlayback,
    ));
    assert!(harness.app.session.playback.is_none());
}

#[test]
fn recording_cannot_be_toggled_twice_while_the_microphone_is_opening() {
    let mut harness = harness();
    let _ = harness.app.update(Message::RecordToggle);
    assert!(harness.app.ui.recording_busy);
    assert!(harness.app.session.recording.is_none());
    let _ = harness.app.update(Message::RecordToggle);
    assert!(harness.app.ui.recording_busy);
    assert!(harness.app.session.recording.is_none());
}
