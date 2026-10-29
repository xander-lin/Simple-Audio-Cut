//! End-to-end workflow against the real audio engine and CPAL playback.
//!
//! Only the file dialogs are mocked: FFmpeg normalization, WAV decoding, RMS
//! analysis, incremental export and CPAL output all run for real.

#![cfg(feature = "native-audio")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use simple_audio_cut_core::audio::{AudioEngine, AudioPlayer, CpalAudioPlayer, NativeAudioEngine};
use simple_audio_cut_core::denoise::{DenoiseProvider, MockDenoiseProvider};
use simple_audio_cut_iced::events::{EventBus, SharedSink};
use simple_audio_cut_iced::message::Message;
use simple_audio_cut_iced::services::{MockDialogService, Services};
use simple_audio_cut_iced::update::App;

fn unique_root() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "simple-audio-cut-workflow-{}-{stamp}",
        std::process::id()
    ))
}

fn generate_source(path: &PathBuf) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=300:sample_rate=48000:duration=2",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(path)
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "ffmpeg could not create the test source");
}

#[test]
fn imports_edits_plays_and_exports_with_the_production_services() {
    let root = unique_root();
    let source = root.join("take.wav");
    generate_source(&source);

    let denoise: Arc<dyn DenoiseProvider> = Arc::new(MockDenoiseProvider::new());
    let engine = NativeAudioEngine::new(root.join("session"), denoise.clone()).expect("engine");
    let engine: Arc<dyn AudioEngine> = Arc::new(engine);
    let player = Arc::new(CpalAudioPlayer::new());
    let dialogs = Arc::new(MockDialogService::new());
    let events: SharedSink = Arc::new(EventBus::new());
    let services = Services::headless(
        engine,
        player.clone(),
        dialogs.clone(),
        events.clone(),
    );
    let services = Services {
        denoise,
        ..services
    };
    let (mut app, _) = App::boot(services);

    let exports = root.join("exports");
    dialogs.queue_files(vec![source.clone()]);
    dialogs.queue_folder(Some(exports.clone()));

    let _ = app.update(Message::ImportPressed);

    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        let _ = app.update(Message::Tick);
        if let Some(clip) = app.session.media.first() {
            if clip.ready() {
                break;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "import never became ready: {}",
            app.ui.message
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let id = app.session.media[0].id;
    let duration = app.session.media[0].duration;
    assert!((duration - 2.0).abs() < 0.05, "duration {duration}");
    let _ = app.update(Message::AddToTimeline(id));
    assert_eq!(app.session.selected_track, Some(id));

    // Right-drag delete of the middle second, then mute a range.
    let _ = app.update(Message::Waveform {
        track: id,
        event: simple_audio_cut_iced::message::WaveformEvent::DirectEdit {
            region: simple_audio_cut_core::region::Region::new(0.5, 1.5),
            operation: simple_audio_cut_core::direct_edit::DirectRegionOperation::Delete,
        },
    });
    let track = app.session.track(id).expect("track");
    assert!((track.edited_duration() - 1.0).abs() < 0.05);

    let _ = app.update(Message::Seek(0.1));
    let _ = app.update(Message::SelectRange(Some(
        simple_audio_cut_core::region::Region::new(1.6, 1.9),
    )));
    let _ = app.update(Message::MuteRange);

    // Playback: started through the app, so this also exercises the shared
    // edited-buffer path. A missing output device must not panic the app.
    let _ = app.update(Message::PlayPause);
    match player.position() {
        Some(position) => {
            std::thread::sleep(Duration::from_millis(120));
            let _ = app.update(Message::Tick);
            assert!(
                player.position().is_some_and(|next| next >= position),
                "playhead did not advance"
            );
            let _ = app.update(Message::PlayPause);
            assert!(app.session.playback.is_none());
        }
        None => {
            eprintln!("Playback unavailable on this host: {}", app.ui.message);
        }
    }

    let _ = app.update(Message::ExportChanged);
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        let _ = app.update(Message::Tick);
        if app.session.exporting.is_empty() && !app.ui.message.starts_with("Choose") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "export never finished: {}",
            app.ui.message
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let exported: Vec<PathBuf> = std::fs::read_dir(&exports)
        .expect("export directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    assert_eq!(exported.len(), 1, "{}", app.ui.message);
    let buffer = simple_audio_cut_core::buffer::AudioBuffer::decode_wav(&exported[0])
        .expect("decode export");
    assert!(
        (buffer.duration() - 1.0).abs() < 0.06,
        "exported duration {}",
        buffer.duration()
    );
    assert!(!app.session.track(id).expect("track").needs_export());

    let _ = std::fs::remove_dir_all(root);
}
