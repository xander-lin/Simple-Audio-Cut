//! Real pipeline smoke tests: FFmpeg normalization, WAV decoding and CPAL
//! playback. They need `ffmpeg` and an audio output device; playback is
//! skipped gracefully when the host exposes none.

#![cfg(feature = "native-audio")]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use simple_audio_cut_core::analysis::{analyze_rms, RmsOptions};
use simple_audio_cut_core::audio::{AudioEngine, AudioPlayer, CpalAudioPlayer, NativeAudioEngine};
use simple_audio_cut_core::buffer::AudioBuffer;
use simple_audio_cut_core::denoise::{DenoiseProvider, MockDenoiseProvider};

fn unique_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "simple-audio-cut-native-{}-{stamp}",
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
            "sine=frequency=440:sample_rate=44100:duration=1.5",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(path)
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "ffmpeg could not create the test source");
}

#[test]
fn imports_normalizes_and_decodes_a_real_file() {
    let root = unique_dir();
    let source = root.join("spoken.wav");
    generate_source(&source);

    let denoise: Arc<dyn DenoiseProvider> = Arc::new(MockDenoiseProvider::new());
    let engine = NativeAudioEngine::new(root.join("session"), denoise).expect("engine");

    let results = Arc::new(Mutex::new(Vec::new()));
    let sink = results.clone();
    engine
        .start_import_audio(
            "1".into(),
            source.display().to_string(),
            -14.0,
            Box::new(move |update| {
                if let simple_audio_cut_core::audio::ImportUpdate::Complete { info } = update {
                    sink.lock().expect("results").push(info);
                }
            }),
        )
        .expect("start import");

    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        if !results.lock().expect("results").is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "import did not finish in time"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let info = results
        .lock()
        .expect("results")
        .first()
        .cloned()
        .expect("imported info");

    let buffer = AudioBuffer::decode_wav(&info.path).expect("decode normalized output");
    assert!(
        (buffer.duration() - 1.5).abs() < 0.05,
        "duration {}",
        buffer.duration()
    );
    assert_eq!(buffer.sample_rate(), 44_100);
    assert!(buffer.channel_count() >= 1);
    let lufs = info.integrated_lufs.expect("measured loudness");
    assert!((lufs + 14.0).abs() < 2.0, "lufs {lufs}");

    let rms = analyze_rms(&buffer, RmsOptions::default());
    assert!(!rms.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cpal_player_streams_a_decoded_buffer_when_a_device_exists() {
    let audio = Arc::new(AudioBuffer::new(
        48_000,
        vec![(0..48_000)
            .map(|index| {
                let time = index as f32 / 48_000.0;
                (time * 440.0 * std::f32::consts::TAU).sin() * 0.1
            })
            .collect()],
    ));
    let player = CpalAudioPlayer::new();
    match player.play(audio, 0.0) {
        Ok(()) => {
            std::thread::sleep(Duration::from_millis(120));
            let position = player.position();
            assert!(
                position.is_some_and(|position| position > 0.0),
                "playhead did not advance: {position:?}"
            );
            player.stop();
            assert_eq!(player.position(), None);
        }
        Err(error) => {
            eprintln!("Skipping CPAL playback check: {error}");
        }
    }
}
