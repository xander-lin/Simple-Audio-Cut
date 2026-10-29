//! Development preview with synthetic tracks and no audio device.
//!
//! Run `cargo run -p simple-audio-cut-iced --example seed_preview` to inspect
//! the editor layout, waveform rendering and overlays without recording
//! anything. Real audio, dialogs and playback are replaced by mocks.

use std::path::PathBuf;
use std::sync::Arc;

use simple_audio_cut_core::analysis::{analyze_rms, RmsOptions};
use simple_audio_cut_core::audio::RecordingInfo;
use simple_audio_cut_core::buffer::AudioBuffer;
use simple_audio_cut_core::region::{EnvelopePoint, Region};
use simple_audio_cut_iced::app::preview;
use simple_audio_cut_iced::events::AppEvent;
use simple_audio_cut_iced::message::Message;
use simple_audio_cut_iced::model::MediaClip;
use simple_audio_cut_iced::services::Services;
use simple_audio_cut_iced::update::App;

fn main() -> iced::Result {
    preview(seed).run()
}

fn seed() -> App {
    let services = Services::mock();
    let (mut app, _) = App::boot(services);

    let root = std::env::temp_dir().join("simple-audio-cut-preview");
    std::fs::create_dir_all(&root).expect("preview directory");

    let tracks = [
        ("narration take 1", 6.0_f32, 220.0_f32),
        ("narration take 2", 26.0, 330.0),
    ];
    for (index, (name, seconds, frequency)) in tracks.into_iter().enumerate() {
        let path: PathBuf = root.join(format!("take-{index}.wav"));
        write_preview_wav(&path, seconds as f64, frequency);
        let audio = AudioBuffer::decode_wav(&path).expect("decode preview wav");
        let rms = analyze_rms(&audio, RmsOptions::default());
        let id = index as u64 + 1;
        app.session
            .media
            .push(MediaClip::placeholder(id, path.display().to_string()));
        app.services.events.emit(AppEvent::ImportCompleted {
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
    }

    let _ = app.update(Message::Tick);
    let _ = app.update(Message::AddToTimeline(1));
    if let Some(first) = app.session.track_mut(1) {
        first.manual_deleted = vec![Region::new(0.8, 1.4), Region::new(3.2, 3.9)];
        first.muted = vec![Region::new(2.0, 2.6)];
        first.envelope = vec![
            EnvelopePoint::new("a", 1.6, 1.0),
            EnvelopePoint::new("b", 2.8, 0.35),
            EnvelopePoint::new("c", 4.6, 1.5),
        ];
        first.edit_points = vec![2.4, 4.2];
        first.touch();
    }
    let _ = app.update(Message::AddToTimeline(2));
    let _ = app.update(Message::Seek(1.2));
    if std::env::var_os("PREVIEW_SETTINGS").is_some() {
        let _ = app.update(Message::OpenSettings);
        let _ = app.update(Message::Tick);
    }
    app
}

fn write_preview_wav(path: &PathBuf, seconds: f64, frequency: f32) {
    let sample_rate = 48_000;
    let frames = (seconds * sample_rate as f64) as usize;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    for frame in 0..frames {
        let time = frame as f32 / sample_rate as f32;
        let phrase = ((time * 1.4).sin() * 0.5 + 0.5).powf(2.0);
        let pause = if (time % 5.0) > 4.2 { 0.0 } else { 1.0 };
        let sample = (time * frequency * std::f32::consts::TAU).sin() * phrase * pause * 0.35;
        writer
            .write_sample((sample * i16::MAX as f32) as i16)
            .expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}
