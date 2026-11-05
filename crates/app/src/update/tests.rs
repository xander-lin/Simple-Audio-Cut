//! Unit tests for message handling and state transitions.

use super::App;
use crate::events::{AppEvent, DecodedClip};
use crate::message::Message;
use crate::model::{MediaId, Track};
use crate::services::Services;
use simple_audio_cut_core::audio::ExportResult;

fn app() -> App {
    let (mut app, _) = App::boot(Services::mock());
    // Settle the boot status job, then turn automatic denoising off. The
    // tests below exercise editing and export; a take that silently starts
    // a denoise job would add unrelated state to every assertion.
    let _ = app.update(Message::Tick);
    let mut status = app.services.denoise.status();
    status.effective.enabled = false;
    status.configured.enabled = false;
    app.session.denoise_status = Some(status);
    app
}

fn add_ready_media(app: &mut App, id: MediaId) {
    use simple_audio_cut_core::audio::RecordingInfo;
    // Denoising reads the normalized file for real, so the fixture must
    // exist on disk. A unique name keeps parallel tests independent, and
    // the folder is emptied once per test process.
    static FIXTURES: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    let dir = FIXTURES.get_or_init(|| {
        let dir = std::env::temp_dir().join("simple-audio-cut-unit-fixtures");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture folder");
        dir
    });
    let path = dir.join(format!("voice-{id}-{}.wav", crate::model::new_id()));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 1_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).expect("create fixture wav");
    for _ in 0..1_000 {
        writer.write_sample(1_600_i16).expect("write sample");
    }
    writer.finalize().expect("finish fixture wav");

    app.session.media.push(crate::model::MediaClip::placeholder(
        id,
        path.display().to_string(),
    ));
    let audio = std::sync::Arc::new(
        simple_audio_cut_core::buffer::AudioBuffer::decode_wav(&path).expect("decode fixture"),
    );
    let rms = std::sync::Arc::new(simple_audio_cut_core::analysis::analyze_rms(
        &audio,
        Default::default(),
    ));
    let info = RecordingInfo {
        id: id.to_string(),
        name: "Voice".into(),
        path: path.display().to_string(),
        duration_seconds: 1.0,
        integrated_lufs: Some(-14.0),
    };
    app.services
        .events
        .emit(crate::events::AppEvent::ImportCompleted {
            id,
            info,
            audio,
            rms,
        });
    let _ = app.update(Message::Tick);
}

#[test]
fn selecting_a_pool_item_closes_the_track_editor_and_resets_the_playhead() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    add_ready_media(&mut app, 2);
    let _ = app.update(Message::AddToTimeline(1));
    app.session.playhead = 0.8;

    let _ = app.update(Message::SelectMedia(2));

    assert_eq!(app.session.selected_media, Some(2));
    assert_eq!(app.session.selected_track, None);
    assert_eq!(app.session.playhead, 0.0);
}

#[test]
fn imported_media_becomes_ready_and_selectable() {
    let mut app = app();
    add_ready_media(&mut app, 7);
    let clip = app.session.media(7).expect("media");
    assert!(clip.ready());
    let _ = app.update(Message::AddToTimeline(7));
    assert!(app.session.media(7).is_none());
    assert_eq!(app.session.selected_track, Some(7));
    assert_eq!(app.session.tracks.len(), 1);
}

#[test]
fn muting_a_range_marks_the_track_for_export() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    let track = app.session.track(1).expect("track");
    assert_eq!(track.id(), 1);
    app.session.track_mut(1).unwrap().last_export_signature =
        Some(app.session.track(1).unwrap().signature());
    assert!(!app.session.track(1).unwrap().needs_export());

    let _ = app.update(Message::SelectRange(Some(
        simple_audio_cut_core::region::Region::new(0.1, 0.2),
    )));
    let _ = app.update(Message::MuteRange);

    let track: &Track = app.session.track(1).unwrap();
    assert_eq!(track.muted.len(), 1);
    assert!(track.needs_export());
}

#[test]
fn collapsed_tracks_map_display_time_back_to_source_time() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    {
        let track = app.session.track_mut(1).unwrap();
        track.manual_deleted = vec![simple_audio_cut_core::region::Region::new(0.2, 0.4)];
        track.touch();
        track.collapsed = true;
        track.rebuild_collapsed();
    }
    let _ = app.update(Message::Waveform {
        track: 1,
        event: crate::message::WaveformEvent::Seek(0.5),
    });
    let playhead = app.session.playhead;
    assert!((playhead - 0.7).abs() < 1e-6, "playhead {playhead}");
}

#[test]
fn export_without_changes_is_reported_instead_of_opening_a_dialog() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    app.session.track_mut(1).unwrap().mark_exported();
    let _ = app.update(Message::ExportChanged);
    assert_eq!(app.ui.message, "All editor tracks are already exported.");
}

fn decoded_clip(id: MediaId) -> DecodedClip {
    use simple_audio_cut_core::audio::RecordingInfo;
    let audio = std::sync::Arc::new(simple_audio_cut_core::buffer::AudioBuffer::new(
        1_000,
        vec![vec![0.1; 1_000]],
    ));
    let rms = std::sync::Arc::new(simple_audio_cut_core::analysis::analyze_rms(
        &audio,
        Default::default(),
    ));
    DecodedClip {
        info: RecordingInfo {
            id: id.to_string(),
            name: "Voice".into(),
            path: "/tmp/voice.wav".into(),
            duration_seconds: 1.0,
            integrated_lufs: Some(-14.0),
        },
        audio,
        rms,
    }
}

#[test]
fn escape_closes_the_settings_drawer() {
    let mut app = app();
    let _ = app.update(Message::OpenSettings);
    assert!(app.ui.settings_open);
    let _ = app.update(Message::Dismiss);
    assert!(!app.ui.settings_open);
    assert!(!app.ui.settings.busy);
}

#[test]
fn escape_closes_the_context_menu_before_the_drawer() {
    let mut app = app();
    app.ui.settings_open = true;
    app.ui.context_menu = Some(crate::model::ContextMenu {
        track_id: 1,
        kind: crate::model::ContextMenuKind::Actions,
        x: 10.0,
        y: 10.0,
    });
    let _ = app.update(Message::Dismiss);
    assert!(app.ui.context_menu.is_none());
    assert!(app.ui.settings_open);
    let _ = app.update(Message::Dismiss);
    assert!(!app.ui.settings_open);
}

#[test]
fn rename_commits_on_submit_and_ignores_an_empty_draft() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::RenameMedia(1, "Take two".into()));
    let _ = app.update(Message::RenameCommit);
    assert_eq!(app.session.clip_by_id(1).unwrap().name, "Take two");
    assert!(app.ui.rename.is_none());

    let _ = app.update(Message::RenameMedia(1, "   ".into()));
    let _ = app.update(Message::RenameCommit);
    assert_eq!(app.session.clip_by_id(1).unwrap().name, "Take two");
}

#[test]
fn rename_commits_when_another_interaction_starts() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    add_ready_media(&mut app, 2);
    let _ = app.update(Message::RenameMedia(1, "Renamed".into()));
    let _ = app.update(Message::SelectMedia(2));
    assert_eq!(app.session.clip_by_id(1).unwrap().name, "Renamed");
    assert!(app.ui.rename.is_none());
}

#[test]
fn starting_a_second_rename_commits_the_first_draft() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    add_ready_media(&mut app, 2);
    let _ = app.update(Message::RenameMedia(1, "First".into()));
    let _ = app.update(Message::RenameMedia(2, "Second".into()));
    let _ = app.update(Message::RenameCommit);

    assert_eq!(app.session.clip_by_id(1).unwrap().name, "First");
    assert_eq!(app.session.clip_by_id(2).unwrap().name, "Second");
}

#[test]
fn escape_discards_an_uncommitted_rename() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let original = app.session.clip_by_id(1).unwrap().name.clone();
    let _ = app.update(Message::RenameMedia(1, "Draft".into()));
    let _ = app.update(Message::Dismiss);
    assert!(app.ui.rename.is_none());
    assert_eq!(app.session.clip_by_id(1).unwrap().name, original);
}

#[test]
fn renaming_a_collapsed_track_does_not_rebuild_the_collapsed_audio() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    {
        let track = app.session.track_mut(1).unwrap();
        track.manual_deleted = vec![simple_audio_cut_core::region::Region::new(0.2, 0.4)];
        track.touch();
        track.collapsed = true;
        track.rebuild_collapsed();
    }
    let before = app
        .session
        .track(1)
        .unwrap()
        .collapsed_audio
        .clone()
        .expect("collapsed audio");
    let revision = app.session.track(1).unwrap().revision;

    let _ = app.update(Message::RenameMedia(1, "Renamed".into()));
    let _ = app.update(Message::RenameCommit);

    let track = app.session.track(1).unwrap();
    assert_eq!(track.revision, revision);
    let after = track.collapsed_audio.clone().expect("collapsed audio");
    assert!(std::sync::Arc::ptr_eq(&before, &after));
}

#[test]
fn a_recording_deleted_while_finalizing_does_not_report_success() {
    let mut app = app();
    let placeholder = 7;
    app.session
        .media
        .push(crate::model::MediaClip::recording_placeholder(
            placeholder,
            "Recording",
        ));
    app.session.recording = Some(crate::model::ActiveRecording {
        placeholder,
        started: std::time::Instant::now(),
    });
    let _ = app.update(Message::DeleteMedia(placeholder));
    app.services.events.emit(AppEvent::RecordingStopped {
        placeholder,
        result: Ok(decoded_clip(placeholder)),
    });
    let _ = app.update(Message::Tick);

    assert_eq!(
        app.ui.message,
        "The recording was discarded before it finished."
    );
    assert!(app.session.clip_by_id(placeholder).is_none());
}

#[test]
fn an_import_removed_before_completion_does_not_claim_success() {
    let mut app = app();
    let clip = decoded_clip(404);
    app.services.events.emit(AppEvent::ImportCompleted {
        id: 404,
        info: clip.info,
        audio: clip.audio,
        rms: clip.rms,
    });
    let _ = app.update(Message::Tick);

    assert_eq!(
        app.ui.message,
        "The imported media was removed before it finished."
    );
    assert!(app.session.clip_by_id(404).is_none());
}

fn add_track_for_export(app: &mut App, id: MediaId) {
    add_ready_media(app, id);
    let _ = app.update(Message::AddToTimeline(id));
}

fn completed_export(id: MediaId, signature: String) -> AppEvent {
    AppEvent::ExportCompleted {
        results: vec![ExportResult {
            recording_id: id.to_string(),
            path: Some("/tmp/export/voice.wav".into()),
            error: None,
        }],
        signatures: vec![(id, signature)],
    }
}

#[test]
fn a_track_that_changed_during_export_stays_queued() {
    let mut app = app();
    add_track_for_export(&mut app, 1);
    let signature = app.session.track(1).unwrap().signature();
    app.session.exporting = vec![1];
    app.session.track_mut(1).unwrap().clip.name = "Renamed mid-export".into();

    app.services.events.emit(completed_export(1, signature));
    let _ = app.update(Message::Tick);

    let track = app.session.track(1).unwrap();
    assert!(track.needs_export());
    assert!(app.ui.message.contains("changed during export"));
}

#[test]
fn an_unchanged_export_marks_the_track_exported() {
    let mut app = app();
    add_track_for_export(&mut app, 1);
    let signature = app.session.track(1).unwrap().signature();
    app.session.exporting = vec![1];

    app.services.events.emit(completed_export(1, signature));
    let _ = app.update(Message::Tick);

    assert!(!app.session.track(1).unwrap().needs_export());
    assert!(app.session.failed_exports.is_empty());
}

#[test]
fn a_completed_export_keeps_failures_from_other_batches() {
    let mut app = app();
    add_track_for_export(&mut app, 1);
    let signature = app.session.track(1).unwrap().signature();
    app.session.exporting = vec![1];
    app.session.failed_exports = vec![99];

    app.services.events.emit(completed_export(1, signature));
    let _ = app.update(Message::Tick);

    assert_eq!(app.session.failed_exports, vec![99]);
}

#[test]
fn starting_a_second_export_reports_that_one_is_already_running() {
    let mut app = app();
    add_track_for_export(&mut app, 1);
    app.session.exporting = vec![1];

    let _ = app.update(Message::ExportChanged);

    assert_eq!(app.ui.message, "An export is already running.");
}

#[test]
fn checking_the_setup_does_not_replace_the_draft() {
    let mut app = app();
    let mut status = app.services.denoise.status();
    status.configured.executable_path = "/different/python".into();
    app.ui.settings.draft = Some(app.services.denoise.status().configured);
    app.ui.settings.busy = true;

    app.services
        .events
        .emit(AppEvent::SettingsInspected(Ok(status)));
    let _ = app.update(Message::Tick);

    assert_eq!(
        app.ui.settings.draft.as_ref().unwrap().executable_path,
        app.services.denoise.status().configured.executable_path
    );
    assert!(!app.ui.settings.busy);
}

#[test]
fn stopping_when_idle_is_a_no_op() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    app.session.playhead = 0.3;

    let _ = app.update(Message::StopPlayback);

    assert!(app.session.playback.is_none());
    assert!((app.session.playhead - 0.3).abs() < 1e-9);
}

#[test]
fn home_and_end_move_the_playhead_to_the_track_boundaries() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));

    let _ = app.update(Message::JumpToEnd);
    assert!((app.session.playhead - 1.0).abs() < 1e-9);
    let _ = app.update(Message::JumpToStart);
    assert_eq!(app.session.playhead, 0.0);
}

#[test]
fn clear_envelope_removes_every_keyframe_and_requeues_export() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    app.session.track_mut(1).unwrap().envelope = vec![
        simple_audio_cut_core::region::EnvelopePoint::new("a", 0.2, 0.5),
        simple_audio_cut_core::region::EnvelopePoint::new("b", 0.6, 1.4),
    ];
    app.session.track_mut(1).unwrap().mark_exported();
    assert!(!app.session.track(1).unwrap().needs_export());

    let _ = app.update(Message::ClearEnvelope);

    let track = app.session.track(1).unwrap();
    assert!(track.envelope.is_empty());
    assert_eq!(app.ui.message, "Cleared the volume envelope.");
}

#[test]
fn zooming_changes_only_the_selected_track_scale() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    add_ready_media(&mut app, 2);
    let _ = app.update(Message::AddToTimeline(1));
    let _ = app.update(Message::AddToTimeline(2));
    let _ = app.update(Message::SelectTrack(1));
    let other = app.session.track(2).unwrap().pixels_per_second;
    assert!(!app.session.track(1).unwrap().zoom_locked);

    // The first zoom pins the current on-screen "fit" scale.
    let _ = app.update(Message::ZoomSelected(true));
    let pinned = app.session.track(1).unwrap().pixels_per_second;
    assert!(app.session.track(1).unwrap().zoom_locked);
    assert_eq!(app.session.track(2).unwrap().pixels_per_second, other);
    assert!(!app.session.track(2).unwrap().zoom_locked);

    let _ = app.update(Message::ZoomSelected(false));
    let zoomed_out = app.session.track(1).unwrap().pixels_per_second;
    assert!(zoomed_out < pinned);
    let _ = app.update(Message::ZoomSelected(true));
    assert!((app.session.track(1).unwrap().pixels_per_second - pinned).abs() < 1e-3);
}

#[test]
fn enter_moves_the_selected_library_item_onto_the_timeline() {
    let mut app = app();
    add_ready_media(&mut app, 5);
    let _ = app.update(Message::SelectMedia(5));

    let _ = app.update(Message::Shortcut(crate::message::Shortcut::AddSelected));

    assert!(app.session.media(5).is_none());
    assert!(app.session.track(5).is_some());
}

#[test]
fn dragging_a_library_take_onto_the_timeline_adds_a_track() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    assert!(app.session.tracks.is_empty());

    let _ = app.update(Message::DragItem(1));
    assert_eq!(app.ui.dragging, Some(1));
    assert_eq!(app.session.selected_media, Some(1));

    let _ = app.update(Message::DropItem);

    assert!(app.ui.dragging.is_none());
    assert!(app.session.track(1).is_some());
    assert!(app.session.media(1).is_none());
    assert_eq!(app.session.selected_track, Some(1));
}

#[test]
fn releasing_a_drag_away_from_the_timeline_cancels_it() {
    let mut app = app();
    add_ready_media(&mut app, 1);

    let _ = app.update(Message::DragItem(1));
    let _ = app.update(Message::DragCancel);

    assert!(app.ui.dragging.is_none());
    assert!(app.session.media(1).is_some());
    assert!(app.session.tracks.is_empty());
}

#[test]
fn a_pending_import_cannot_be_dragged_onto_the_timeline() {
    let mut app = app();
    app.session
        .media
        .push(crate::model::MediaClip::placeholder(2, "/tmp/pending.wav"));

    let _ = app.update(Message::DragItem(2));

    assert!(app.ui.dragging.is_none());
    let _ = app.update(Message::DropItem);
    assert!(app.session.tracks.is_empty());
    assert!(app.session.media(2).is_some());
}

#[test]
fn dropping_files_imports_the_accepted_ones_only() {
    let mut app = app();
    let before = app.session.media.len();

    let _ = app.update(Message::FilesDropped(vec![
        std::path::PathBuf::from("/tmp/voice.WAV"),
        std::path::PathBuf::from("/tmp/clip.mp4"),
        std::path::PathBuf::from("/tmp/notes.txt"),
    ]));

    // Only the two audio/video files are queued.
    assert_eq!(app.session.media.len(), before + 2);
    assert!(app.ui.message.contains('2'));
}

#[test]
fn dropping_unsupported_files_explains_why_nothing_happened() {
    let mut app = app();
    let _ = app.update(Message::FilesDropped(vec![std::path::PathBuf::from(
        "/tmp/notes.txt",
    )]));

    assert_eq!(
        app.ui.message,
        "Only audio or video files can be dropped here."
    );
    assert!(app.session.media.is_empty());
}

#[test]
fn escape_cancels_a_drag_before_closing_anything_else() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    app.ui.settings_open = true;

    let _ = app.update(Message::DragItem(1));
    let _ = app.update(Message::Dismiss);

    assert!(app.ui.dragging.is_none());
    assert!(app.ui.settings_open, "the drawer should stay open");
}

#[test]
fn an_unresolved_provider_status_does_not_start_denoising() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    // Simulate the race: the take exists while the provider status is
    // still unknown (the boot job has not reported yet).
    app.session.denoise_status = None;
    if let Some(clip) = app.session.editing_mut(1) {
        clip.denoise = crate::model::DenoiseState::default();
    }

    app.queue_denoise(1);

    let status = app.session.clip_by_id(1).unwrap().denoise.status;
    assert!(
        !status.is_busy(),
        "denoising started before the provider was known ({status:?})"
    );
}

#[test]
fn left_drag_mute_keeps_the_selection_so_it_can_be_restored_or_cut() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));

    let range = simple_audio_cut_core::region::Region::new(0.2, 0.6);
    let _ = app.update(Message::Waveform {
        track: 1,
        event: crate::message::WaveformEvent::MuteRange(range),
    });

    let track = app.session.track(1).expect("track");
    assert_eq!(track.muted, vec![range]);
    assert_eq!(track.selected_range, Some(range));
    assert!(track.needs_export());
    assert!(app.ui.message.starts_with("Muted "), "{}", app.ui.message);
    assert!(
        app.ui.message.contains("Press M to restore"),
        "{}",
        app.ui.message
    );

    // The same selection can be restored with M, without re-selecting.
    let _ = app.update(Message::MuteRange);
    let track = app.session.track(1).expect("track");
    assert!(track.muted.is_empty());
    assert_eq!(track.selected_range, Some(range));
    assert!(
        app.ui.message.starts_with("Restored sound"),
        "{}",
        app.ui.message
    );
}

#[test]
fn muting_without_a_selection_explains_the_gesture() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));

    let _ = app.update(Message::MuteRange);

    assert!(
        app.ui.message.contains("Drag across the waveform"),
        "{}",
        app.ui.message
    );
}

#[test]
fn range_edits_on_a_collapsed_track_explain_why_they_do_nothing() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    let _ = app.update(Message::ToggleCollapse(1));

    let _ = app.update(Message::Waveform {
        track: 1,
        event: crate::message::WaveformEvent::MuteRange(
            simple_audio_cut_core::region::Region::new(0.2, 0.6),
        ),
    });
    assert!(app.session.track(1).unwrap().muted.is_empty());
    assert!(
        app.ui.message.contains("Show all edits"),
        "{}",
        app.ui.message
    );

    let _ = app.update(Message::Waveform {
        track: 1,
        event: crate::message::WaveformEvent::DirectEdit {
            region: simple_audio_cut_core::region::Region::new(0.2, 0.6),
            operation: simple_audio_cut_core::direct_edit::DirectRegionOperation::Delete,
        },
    });
    assert!(app.session.track(1).unwrap().manual_deleted.is_empty());
    assert!(
        app.ui.message.contains("Show all edits"),
        "{}",
        app.ui.message
    );
}

#[test]
fn checking_an_unsaved_draft_does_not_change_the_operational_provider() {
    let mut app = app();
    let saved = app.session.denoise_status.clone().expect("boot status");
    let mut inspected = saved.clone();
    inspected.provider_id = "draft-provider".into();
    inspected.ready = false;
    inspected.summary = "Draft is broken".into();

    app.services
        .events
        .emit(AppEvent::SettingsInspected(Ok(inspected)));
    let _ = app.update(Message::Tick);

    // The drawer shows the check result…
    assert_eq!(
        app.ui.settings.status.as_ref().unwrap().provider_id,
        "draft-provider"
    );
    // …but new takes still follow the saved configuration.
    assert_eq!(
        app.session.denoise_status.as_ref().unwrap().provider_id,
        saved.provider_id
    );
}

#[test]
fn resolving_the_status_applies_the_policy_to_already_imported_takes() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    // The take is sitting untouched, waiting for the provider status.
    if let Some(clip) = app.session.editing_mut(1) {
        clip.denoise = crate::model::DenoiseState::default();
    }
    let mut status = app.services.denoise.status();
    status.effective.enabled = false;
    status.configured.enabled = false;
    app.session.denoise_status = Some(status);

    app.apply_denoise_policy();

    let clip = app.session.clip_by_id(1).unwrap();
    assert_eq!(clip.denoise.status, crate::model::DenoiseStatus::Skipped);
    assert!(clip.denoise.reason.is_some());
    assert!(!clip.denoise.opted_out);
}

#[test]
fn a_broken_provider_is_explained_instead_of_starting_doomed_jobs() {
    use simple_audio_cut_core::denoise::{DenoiseCheck, DenoiseCheckState};

    let mut app = app();
    add_ready_media(&mut app, 1);
    let mut status = app.services.denoise.status();
    status.effective.enabled = true;
    status.configured.enabled = true;
    status.ready = false;
    status.checks = vec![DenoiseCheck {
        id: "python".into(),
        label: "Python environment".into(),
        state: DenoiseCheckState::Error,
        detail: "The selected executable does not exist.".into(),
    }];
    app.session.denoise_status = Some(status);

    app.apply_denoise_policy();

    let clip = app.session.clip_by_id(1).unwrap();
    assert_eq!(
        clip.denoise.status,
        crate::model::DenoiseStatus::Unavailable
    );
    assert!(!clip.denoise.status.is_busy(), "a job must not start");
    let reason = clip.denoise.reason.as_deref().unwrap_or_default();
    assert!(reason.contains("Python environment"), "{reason}");
    assert!(reason.contains("does not exist"), "{reason}");
}

#[test]
fn a_failed_run_waits_for_an_explicit_retry() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    if let Some(clip) = app.session.editing_mut(1) {
        clip.denoise.status = crate::model::DenoiseStatus::Failed;
        clip.denoise.reason = Some("model crashed".into());
    }
    let mut status = app.services.denoise.status();
    status.effective.enabled = true;
    status.configured.enabled = true;
    status.ready = true;
    app.session.denoise_status = Some(status);

    app.apply_denoise_policy();
    assert_eq!(
        app.session.clip_by_id(1).unwrap().denoise.status,
        crate::model::DenoiseStatus::Failed,
        "reopening settings must not respawn a failed job"
    );

    // The inspector's retry action starts a fresh run.
    let _ = app.update(Message::SetDenoise {
        id: 1,
        enabled: true,
    });
    let _ = app.update(Message::Tick);
    assert_ne!(
        app.session.clip_by_id(1).unwrap().denoise.status,
        crate::model::DenoiseStatus::Failed
    );
}

#[test]
fn the_policy_never_overrides_an_explicit_opt_out() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::SetDenoise {
        id: 1,
        enabled: false,
    });
    assert!(app.session.clip_by_id(1).unwrap().denoise.opted_out);
    let mut status = app.services.denoise.status();
    status.effective.enabled = true;
    status.configured.enabled = true;
    app.session.denoise_status = Some(status);

    app.apply_denoise_policy();

    let clip = app.session.clip_by_id(1).unwrap();
    assert_eq!(clip.denoise.status, crate::model::DenoiseStatus::Skipped);
    assert!(clip.denoise.opted_out);
}

#[test]
fn explicit_denoising_round_trips_back_to_the_normalized_source() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    let source = app.session.clip_by_id(1).unwrap().path.clone();

    let _ = app.update(Message::SetDenoise {
        id: 1,
        enabled: true,
    });
    let _ = app.update(Message::Tick);
    let clip = app.session.clip_by_id(1).unwrap();
    assert_eq!(clip.denoise.status, crate::model::DenoiseStatus::Complete);
    assert!(clip.has_original_audio());
    let denoised = clip.path.clone();
    assert_ne!(denoised, source);

    let _ = app.update(Message::SetDenoise {
        id: 1,
        enabled: false,
    });
    let _ = app.update(Message::Tick);
    let clip = app.session.clip_by_id(1).unwrap();
    assert_eq!(clip.denoise.status, crate::model::DenoiseStatus::Skipped);
    assert!(clip.denoise.opted_out);
    assert!(!clip.has_original_audio());
    assert_eq!(clip.path, source);

    // Even an enabled automatic policy must not undo the explicit choice.
    let mut status = app.services.denoise.status();
    status.effective.enabled = true;
    app.session.denoise_status = Some(status);
    app.apply_denoise_policy();
    assert_eq!(
        app.session.clip_by_id(1).unwrap().denoise.status,
        crate::model::DenoiseStatus::Skipped
    );

    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_file(denoised);
}

#[test]
fn turning_denoising_off_keeps_the_take_editable_and_exportable() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));

    let _ = app.update(Message::SetDenoise {
        id: 1,
        enabled: false,
    });

    let clip = app.session.clip_by_id(1).unwrap();
    assert_eq!(clip.denoise.status, crate::model::DenoiseStatus::Skipped);
    assert!(clip.ready());
    assert!(app.ui.message.contains("original audio"));
    // The take is still a normal timeline track that needs export.
    assert!(app.session.track(1).unwrap().needs_export());
    assert!(app.session.changed_tracks().contains(&1));
}

#[test]
fn automatic_off_marks_new_takes_as_original_audio() {
    let mut app = app();
    // Settle the status job started by `boot` before overriding it;
    // otherwise its event would arrive after the override.
    let _ = app.update(Message::Tick);
    let mut config = app.services.denoise.status().configured;
    config.enabled = false;
    let status = app
        .services
        .denoise
        .save_config(config)
        .expect("disable automatic denoising");
    app.session.denoise_status = Some(status);
    add_ready_media(&mut app, 1);

    let clip = app.session.clip_by_id(1).unwrap();
    assert_eq!(clip.denoise.status, crate::model::DenoiseStatus::Skipped);
    assert!(clip
        .denoise
        .reason
        .as_deref()
        .unwrap_or_default()
        .contains("Automatic denoising is off"));

    // The take can still be denoised explicitly.
    let _ = app.update(Message::SetDenoise {
        id: 1,
        enabled: true,
    });
    assert!(matches!(
        app.session.clip_by_id(1).unwrap().denoise.status,
        crate::model::DenoiseStatus::Queued | crate::model::DenoiseStatus::Unavailable
    ));
}

#[test]
fn opting_out_of_denoising_is_not_overridden_by_the_requeue_sweep() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    let _ = app.update(Message::SetDenoise {
        id: 1,
        enabled: false,
    });

    app.apply_denoise_policy();

    assert_eq!(
        app.session.clip_by_id(1).unwrap().denoise.status,
        crate::model::DenoiseStatus::Skipped
    );
}

#[test]
fn re_enabling_denoising_clears_the_opt_out() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    let _ = app.update(Message::SetDenoise {
        id: 1,
        enabled: false,
    });

    let _ = app.update(Message::SetDenoise {
        id: 1,
        enabled: true,
    });

    let status = app.session.clip_by_id(1).unwrap().denoise.status;
    assert!(matches!(
        status,
        crate::model::DenoiseStatus::Queued | crate::model::DenoiseStatus::Unavailable
    ));
}

#[test]
fn an_unavailable_provider_records_the_reason_without_blocking_export() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    app.services.events.emit(AppEvent::DenoiseUnavailable {
        id: 1,
        reason: Some("ClearVoice is not configured.".into()),
    });
    let _ = app.update(Message::Tick);

    let clip = app.session.clip_by_id(1).unwrap();
    assert_eq!(
        clip.denoise.status,
        crate::model::DenoiseStatus::Unavailable
    );
    assert_eq!(
        clip.denoise.reason.as_deref(),
        Some("ClearVoice is not configured.")
    );
    assert!(clip.ready());
    assert!(app.session.track(1).unwrap().needs_export());
    // Nothing is left "processing", so quitting is not blocked by it.
    assert_eq!(app.session.pending_denoise(), 0);
}

#[test]
fn a_denoised_take_can_be_switched_back_to_its_normalized_audio() {
    let mut app = app();
    add_ready_media(&mut app, 1);
    let _ = app.update(Message::AddToTimeline(1));
    {
        let clip = app.session.editing_mut(1).unwrap();
        clip.denoise.status = crate::model::DenoiseStatus::Complete;
        clip.path = "/tmp/denoised.wav".into();
    }
    assert!(app.session.clip_by_id(1).unwrap().has_original_audio());

    let _ = app.update(Message::SetDenoise {
        id: 1,
        enabled: false,
    });
    // The swap itself happens on a worker thread; simulate its result.
    let clip = app.session.clip_by_id(1).unwrap();
    let path = clip.normalized_path.clone();
    let audio = clip.audio.clone().unwrap();
    let rms = clip.rms.clone().unwrap();
    app.services.events.emit(AppEvent::OriginalAudioRestored {
        id: 1,
        path: path.clone(),
        audio,
        rms,
    });
    let _ = app.update(Message::Tick);

    let clip = app.session.clip_by_id(1).unwrap();
    assert_eq!(clip.path, path);
    assert_eq!(clip.denoise.status, crate::model::DenoiseStatus::Skipped);
    assert!(!clip.has_original_audio());
}

#[test]
fn saving_settings_keeps_the_drawer_open() {
    let mut app = app();
    let _ = app.update(Message::OpenSettings);
    let status = app.services.denoise.status();
    app.ui.settings.draft = Some(status.configured.clone());

    app.services
        .events
        .emit(AppEvent::SettingsSaved(Ok(status)));
    let _ = app.update(Message::Tick);

    assert!(app.ui.settings_open);
    assert!(app.ui.settings.status.is_some());
}
