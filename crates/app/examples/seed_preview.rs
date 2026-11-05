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
    // Screenshots often run on a different monitor than the one being
    // simulated; allow forcing the monitor size.
    let read = |key: &str| {
        std::env::var(key)
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value >= 320.0)
    };
    if let (Some(width), Some(height)) = (
        read("PREVIEW_MONITOR_WIDTH"),
        read("PREVIEW_MONITOR_HEIGHT"),
    ) {
        app.monitor_size = Some(iced::Size::new(width, height));
        app.monitor_size_forced = true;
    }
    if std::env::var_os("PREVIEW_EMPTY").is_some() {
        return app;
    }

    let root = std::env::temp_dir().join("simple-audio-cut-preview");
    std::fs::create_dir_all(&root).expect("preview directory");

    let tracks = [
        ("narration take 1", 6.0_f32, 220.0_f32),
        ("narration take 2", 26.0, 330.0),
        ("pickup line", 3.2, 260.0),
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
    // Placeholders already carry the file stem as their name; use the nicer
    // labels so the preview reads like a real session.
    for (index, (name, _, _)) in tracks.into_iter().enumerate() {
        let _ = app.update(Message::RenameMedia(index as u64 + 1, name.into()));
    }
    let _ = app.update(Message::RenameCommit);
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
    // Leave the pickup line in the library so the sidebar design is visible.
    let _ = app.update(Message::SelectTrack(1));
    let _ = app.update(Message::Seek(1.2));
    if std::env::var_os("PREVIEW_DRAG").is_some() {
        let _ = app.update(Message::DragItem(3));
    }
    if std::env::var_os("PREVIEW_SELECTION").is_some() {
        let _ = app.update(Message::SelectRange(Some(Region::new(1.0, 2.2))));
    }
    if std::env::var_os("PREVIEW_MENU").is_some() {
        app.ui.context_menu = Some(simple_audio_cut_iced::model::ContextMenu {
            track_id: 1,
            kind: simple_audio_cut_iced::model::ContextMenuKind::Actions,
            x: 620.0,
            y: 150.0,
        });
    }
    if std::env::var_os("PREVIEW_RENAME").is_some() {
        // Inline rename field on the library take, as opened by a right click.
        // Set the draft directly: the preview boots before the window exists,
        // so the "focus the field" task would have nothing to focus yet.
        let name = app
            .session
            .clip_by_id(3)
            .map(|clip| clip.name.clone())
            .unwrap_or_default();
        app.ui.rename = Some(simple_audio_cut_iced::model::RenameDraft {
            id: 3,
            original: name.clone(),
            value: name,
        });
    }
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
    // Deterministic pseudo-noise so the preview looks like speech instead of a
    // pure tone: bursts of syllables separated by real pauses.
    let mut noise: u32 = 0x1234_5678;
    let mut next_noise = || {
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        (noise as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    let mut filtered = 0.0_f32;
    for frame in 0..frames {
        let time = frame as f32 / sample_rate as f32;
        // 2.5 syllables per second, each ~0.28s, with 0.12s pauses.
        let beat = time * 2.5;
        let phase = beat.fract();
        let syllable = if phase < 0.72 {
            let attack = (phase / 0.06).min(1.0);
            let release = ((0.72 - phase) / 0.16).clamp(0.0, 1.0);
            attack * release
        } else {
            0.0
        };
        let variation = 0.55 + 0.45 * ((beat.floor() * 1.7).sin() * 0.5 + 0.5);
        let tilt = ((time * frequency * std::f32::consts::TAU) * 0.02).sin();
        let voice = next_noise() * 0.6 + tilt * 0.4;
        filtered = filtered * 0.72 + voice * 0.28;
        let sample = filtered * syllable * variation * 0.42;
        writer
            .write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}
