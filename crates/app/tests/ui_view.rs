//! Layout smoke tests.
//!
//! Building an `Element` is pure: it exercises every view branch (empty
//! states, cards, track rows, drawers, menus) without a window, a GPU or an
//! audio device. This catches panics and type-level layout regressions that
//! the state tests cannot see.

use std::sync::Arc;

use simple_audio_cut_core::analysis::{analyze_rms, RmsOptions};
use simple_audio_cut_core::audio::RecordingInfo;
use simple_audio_cut_core::buffer::AudioBuffer;
use simple_audio_cut_core::region::{EnvelopePoint, Region};

use simple_audio_cut_iced::events::AppEvent;
use simple_audio_cut_iced::message::Message;
use simple_audio_cut_iced::model::{ContextMenu, ContextMenuKind, MediaClip, MediaId};
use simple_audio_cut_iced::services::Services;
use simple_audio_cut_iced::update::App;

fn ready_media(app: &mut App, id: MediaId, name: &str) {
    app.session
        .media
        .push(MediaClip::placeholder(id, format!("/tmp/{name}.wav")));
    let audio = Arc::new(AudioBuffer::new(
        48_000,
        vec![vec![0.1; 48_000]],
    ));
    let rms = Arc::new(analyze_rms(&audio, RmsOptions::default()));
    app.services.events.emit(AppEvent::ImportCompleted {
        id,
        info: RecordingInfo {
            id: id.to_string(),
            name: name.into(),
            path: format!("/tmp/{name}.wav"),
            duration_seconds: 1.0,
            integrated_lufs: Some(-14.0),
        },
        audio,
        rms,
    });
    let _ = app.update(Message::Tick);
}

fn boot() -> App {
    let (app, _) = App::boot(Services::mock());
    app
}

#[test]
fn empty_session_builds() {
    let app = boot();
    let _ = app.view();
}

#[test]
fn library_rows_and_track_rows_build() {
    let mut app = boot();
    ready_media(&mut app, 1, "take one");
    ready_media(&mut app, 2, "take two");
    let _ = app.update(Message::AddToTimeline(1));
    let _ = app.update(Message::AddToTimeline(2));

    {
        let track = app.session.track_mut(1).expect("track");
        track.manual_deleted = vec![Region::new(0.1, 0.2)];
        track.muted = vec![Region::new(0.3, 0.4)];
        track.envelope = vec![EnvelopePoint::new("a", 0.5, 0.6)];
        track.edit_points = vec![0.7];
        track.selected_range = Some(Region::new(0.15, 0.35));
        track.touch();
    }
    let _ = app.update(Message::SelectTrack(1));

    let _ = app.view();
}

#[test]
fn inline_rename_view_builds_without_a_field_box() {
    let mut app = boot();
    ready_media(&mut app, 6, "voice");
    let _ = app.update(Message::AddToTimeline(6));
    let _ = app.update(Message::BeginRename(6));

    // The row must remember the draft even before the window exists; the
    // focus task is a no-op in this headless test.
    assert!(app.ui.rename.is_some());
    let _ = app.view();
}

#[test]
fn collapsed_track_builds() {
    let mut app = boot();
    ready_media(&mut app, 3, "voice");
    let _ = app.update(Message::AddToTimeline(3));
    {
        let track = app.session.track_mut(3).expect("track");
        track.manual_deleted = vec![Region::new(0.2, 0.4)];
        track.touch();
    }
    let _ = app.update(Message::ToggleCollapse(3));

    let _ = app.view();
}

#[test]
fn failed_import_still_builds_a_card() {
    let mut app = boot();
    app.session
        .media
        .push(MediaClip::placeholder(9, "/tmp/broken.mp4"));
    app.services.events.emit(AppEvent::ImportFailed {
        id: 9,
        error: "No audio stream".into(),
    });
    let _ = app.update(Message::Tick);

    let _ = app.view();
}

#[test]
fn transient_layers_build() {
    let mut app = boot();
    ready_media(&mut app, 4, "voice");
    let _ = app.update(Message::AddToTimeline(4));

    app.ui.context_menu = Some(ContextMenu {
        track_id: 4,
        kind: ContextMenuKind::Actions,
        x: 40.0,
        y: 90.0,
    });
    let _ = app.view();

    app.ui.context_menu = Some(ContextMenu {
        track_id: 4,
        kind: ContextMenuKind::Export,
        x: 40.0,
        y: 90.0,
    });
    let _ = app.view();

    app.ui.context_menu = None;
    let _ = app.update(Message::OpenSettings);
    let _ = app.update(Message::Tick);
    let _ = app.view();

    app.ui.settings_open = false;
    app.ui.confirm_quit = true;
    let _ = app.view();
}

#[test]
fn renaming_draft_is_rendered_instead_of_the_committed_name() {
    let mut app = boot();
    ready_media(&mut app, 5, "voice");
    let _ = app.update(Message::RenameMedia(5, "draft name".into()));

    assert_eq!(
        app.ui.rename.as_ref().map(|draft| draft.value.as_str()),
        Some("draft name")
    );
    assert_eq!(app.session.clip_by_id(5).unwrap().name, "voice");
    let _ = app.view();
}
