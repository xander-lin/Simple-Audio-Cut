//! Message handling: the only place application state is mutated.

use std::sync::Arc;

use iced::{Element, Size, Task};
use simple_audio_cut_core::analysis::{
    dbfs_to_amplitude, detect_silence_from_analysis, SilenceOptions,
};
use simple_audio_cut_core::audio::ExportEdit;
use simple_audio_cut_core::direct_edit::{DirectRegionOperation, MuteRegionOperation};
use simple_audio_cut_core::region::{merge_regions, subtract_region, EnvelopePoint, Region};
use simple_audio_cut_core::timeline::{
    create_edited_buffer, edited_offset_at_original_time, kept_regions,
    original_time_at_edited_offset,
};

use crate::events::{AppEvent, FileRequest};
use crate::jobs;
use crate::message::{Message, Shortcut, WaveformEvent};
use crate::model::{
    new_id, ClipStatus, MediaClip, MediaId, Session, Track, UiState, DEFAULT_MIN_SILENCE_MS,
    DEFAULT_SILENCE_THRESHOLD_DB,
};
use crate::services::Services;
use crate::ui::format_time_standard;

#[derive(Clone)]
struct PendingExport {
    edits: Vec<ExportEdit>,
    signatures: Vec<(MediaId, String)>,
}

pub struct App {
    pub session: Session,
    pub ui: UiState,
    pub services: Services,
    pub window_size: Size,
    /// Size of the monitor the window lives on. Used to decide whether the
    /// window is effectively full screen (columns) or half screen (stacked).
    pub monitor_size: Option<Size>,
    /// Set by the preview binary when the simulated monitor size must win over
    /// the real one.
    pub monitor_size_forced: bool,
    pending_export: Option<PendingExport>,
    /// Whether the in-flight export was started from a single track. Kept
    /// apart from [`PendingExport`] so the dispatched request never has to
    /// carry placeholder data just to remember the completion wording.
    export_single: bool,
}

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        crate::ui::view(self)
    }

    /// Boots and immediately imports the given files, so the app can be opened
    /// from a file manager or the command line. Unsupported paths are ignored.
    pub fn boot_with_files(
        services: Services,
        paths: Vec<std::path::PathBuf>,
    ) -> (Self, Task<Message>) {
        let (app, _) = Self::boot(services);
        let task = if paths.is_empty() {
            Task::none()
        } else {
            Task::done(Message::FilesDropped(paths))
        };
        (app, task)
    }

    pub fn boot(services: Services) -> (Self, Task<Message>) {
        let app = Self {
            session: Session::default(),
            ui: UiState::default(),
            services,
            window_size: crate::app::DEFAULT_WINDOW_SIZE,
            monitor_size: None,
            monitor_size_forced: false,
            pending_export: None,
            export_single: false,
        };
        let denoise = app.services.denoise.clone();
        let events = app.services.events.clone();
        let jobs = app.services.jobs.clone();
        jobs::provider_status(jobs, denoise, events);
        // The monitor size arrives asynchronously; until then the layout falls
        // back to an absolute width threshold.
        let monitor = iced::window::latest()
            .and_then(iced::window::monitor_size)
            .map(Message::MonitorResolved);
        (app, monitor)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        self.commit_rename_before(&message);
        match message {
            Message::Tick => self.tick(),
            Message::RecordToggle => self.toggle_recording(),
            Message::TargetLufsInput(value) => {
                self.ui.target_lufs_input = value;
                if let Ok(parsed) = self.ui.target_lufs_input.trim().parse::<f64>() {
                    if parsed.is_finite() {
                        self.session.target_lufs = parsed.clamp(-70.0, -5.0);
                    }
                }
            }
            Message::TargetLufsSubmit => {
                let value = self.session.target_lufs;
                self.ui.target_lufs_input = format!("{value}");
            }
            Message::ImportPressed => self.import_audio(),
            Message::SelectMedia(id) => self.select_media(id),
            Message::SelectTrack(id) => self.select_track(id),
            Message::RenameMedia(id, name) => self.edit_rename(id, name),
            Message::RenameCommit => self.commit_rename(),
            Message::BeginRename(id) => {
                // Rename a take or a track in place, opened by right-clicking
                // its name (library row, library chip, track label) or from the
                // ••• menu. The name becomes an inline field; no dialog.
                if self.session.clip_by_id(id).is_none() {
                    return Task::none();
                }
                self.ui.context_menu = None;
                if self.session.media(id).is_some() {
                    self.select_media(id);
                } else {
                    self.select_track(id);
                }
                // A second right-click inside an already open field should not
                // restart the draft (it would discard typing).
                if self.ui.rename.as_ref().is_some_and(|draft| draft.id == id) {
                    return Task::none();
                }
                self.begin_rename(id);
                // Focus and pre-select the whole name, so typing replaces it
                // the way a normal "rename" gesture is expected to behave.
                let field = crate::ui::name_input_id(id);
                return Task::batch([
                    iced::widget::operation::focus(field.clone()),
                    iced::widget::operation::select_all(field),
                ]);
            }
            Message::Dismiss => self.dismiss(),
            Message::AddToTimeline(id) => self.add_to_timeline(id),
            Message::DragItem(id) => self.begin_drag(id),
            Message::DropItem => self.drop_item(),
            Message::DragCancel => self.ui.dragging = None,
            Message::FilesHovered => self.ui.files_hovering = true,
            Message::FilesHoverLeft => self.ui.files_hovering = false,
            Message::FilesDropped(paths) => self.import_dropped(paths),
            Message::DeleteMedia(id) => self.delete_media(id),
            Message::RetryImport(id) => self.retry_import(id),
            Message::SetDenoise { id, enabled } => self.set_denoise(id, enabled),
            Message::RemoveTrack(id) => self.remove_track(id),
            Message::ReturnTrackToPool(id) => self.return_track_to_pool(id),
            Message::ToggleCollapse(id) => self.toggle_collapse(id),
            Message::PlayPause => self.toggle_playback(),
            Message::StopPlayback => {
                if self.session.playback.is_some() {
                    self.stop_playback(true);
                }
            }
            Message::Seek(time) => self.seek(time),
            Message::JumpToStart => self.seek(0.0),
            Message::JumpToEnd => {
                let duration = self.session.selected_track().map(Track::duration);
                if let Some(duration) = duration {
                    self.seek(duration);
                }
            }
            Message::SelectRange(range) => self.set_selected_range(range),
            Message::MuteRange => self.toggle_mute_range(),
            Message::DeleteRange => self.delete_selected_range(),
            Message::ClearEnvelope => self.clear_envelope(),
            Message::ZoomSelected(closer) => {
                if let Some(id) = self.session.selected_track {
                    let delta = if closer { -1.0 } else { 1.0 };
                    self.scale_track(id, delta);
                }
            }
            Message::SetBlade(active) => {
                self.ui.blade_mode = active;
                self.ui.message = if active {
                    "Blade active. Click the waveform to add an edit point; press A to return."
                        .into()
                } else {
                    "Direct editing active. Left-drag forward mutes, backward restores; right-drag deletes or restores."
                        .into()
                };
            }
            Message::Waveform { track, event } => self.waveform_event(track, event),
            Message::ScaleTrack(id, delta) => self.scale_track(id, delta),
            Message::SilenceThresholdInput(value) => {
                self.ui.silence_threshold_input = value;
                self.apply_silence_inputs();
            }
            Message::MinSilenceInput(value) => {
                self.ui.min_silence_input = value;
                self.apply_silence_inputs();
            }
            Message::SilenceSettingsSubmit => self.refresh_silence_inputs(),
            Message::DetectSilence => self.detect_silence(),
            Message::ClearSilence => self.clear_silence(),
            Message::ExportChanged => {
                let ids = self.session.changed_tracks();
                if ids.is_empty() {
                    self.ui.message = "All editor tracks are already exported.".into();
                } else {
                    self.start_export(ids, false);
                }
            }
            Message::ExportTrack(id) => {
                self.ui.context_menu = None;
                self.start_export(vec![id], true);
            }
            Message::ContextMenu { track, kind, x, y } => {
                self.select_track(track);
                self.ui.context_menu = Some(crate::model::ContextMenu {
                    track_id: track,
                    kind,
                    x,
                    y,
                });
            }
            Message::CloseContextMenu => self.ui.context_menu = None,
            Message::OpenSettings => {
                self.stop_playback(false);
                self.ui.settings_open = true;
                self.ui.settings.busy = true;
                self.ui.settings.error = None;
                // Re-seed from the saved configuration; unsaved edits are not
                // carried across drawer visits.
                self.ui.settings.draft = None;
                jobs::provider_status(
                    self.services.jobs.clone(),
                    self.services.denoise.clone(),
                    self.services.events.clone(),
                );
            }
            Message::CloseSettings => {
                self.ui.settings_open = false;
                // The note is a one-drawer ambient report; it must not survive
                // into the next visit or leak into the status bar forever.
                self.session.denoise_note = None;
                self.stop_playback(false);
            }
            Message::SettingsEnabled(enabled) => {
                if let Some(draft) = &mut self.ui.settings.draft {
                    draft.enabled = enabled;
                }
            }
            Message::SettingsExecutable(value) => {
                if let Some(draft) = &mut self.ui.settings.draft {
                    draft.executable_path = value;
                }
            }
            Message::SettingsModelRoot(value) => {
                if let Some(draft) = &mut self.ui.settings.draft {
                    draft.model_root = value;
                }
            }
            Message::SettingsDevice(value) => {
                if let Some(draft) = &mut self.ui.settings.draft {
                    draft.device = value;
                }
            }
            Message::SettingsHighModel(value) => {
                if let Some(draft) = &mut self.ui.settings.draft {
                    draft.high_sample_rate_model = value;
                }
            }
            Message::SettingsLowModel(value) => {
                if let Some(draft) = &mut self.ui.settings.draft {
                    draft.low_sample_rate_model = value;
                }
            }
            Message::SettingsCheck => {
                if let Some(draft) = self.ui.settings.draft.clone() {
                    self.ui.settings.busy = true;
                    self.ui.settings.error = None;
                    jobs::inspect_config(
                        self.services.jobs.clone(),
                        self.services.denoise.clone(),
                        self.services.events.clone(),
                        draft,
                    );
                }
            }
            Message::SettingsSave => {
                if let Some(draft) = self.ui.settings.draft.clone() {
                    self.ui.settings.busy = true;
                    self.ui.settings.error = None;
                    jobs::save_config(
                        self.services.jobs.clone(),
                        self.services.denoise.clone(),
                        self.services.events.clone(),
                        draft,
                    );
                }
            }
            Message::PickExecutable => jobs::choose_file(
                self.services.jobs.clone(),
                self.services.dialogs.clone(),
                self.services.events.clone(),
                FileRequest::PythonExecutable,
                "Choose the Python executable from your ClearVoice environment",
            ),
            Message::PickModelRoot => jobs::choose_folder(
                self.services.jobs.clone(),
                self.services.dialogs.clone(),
                self.services.events.clone(),
                FileRequest::ModelRoot,
                "Choose your ClearVoice model library",
                self.ui
                    .settings
                    .draft
                    .as_ref()
                    .map(|draft| draft.model_root.clone())
                    .filter(|path| !path.is_empty()),
            ),
            Message::CloseRequested => {
                if self.session.has_unsaved_work() {
                    self.ui.confirm_quit = true;
                } else {
                    return iced::exit();
                }
            }
            Message::ConfirmQuit => {
                self.ui.confirm_quit = false;
                return iced::exit();
            }
            Message::CancelQuit => self.ui.confirm_quit = false,
            Message::Shortcut(shortcut) => self.shortcut(shortcut),
            Message::WindowResized(size) => self.window_size = size,
            Message::MonitorResolved(size) => {
                if !self.monitor_size_forced {
                    self.monitor_size = size;
                }
            }
        }
        Task::none()
    }

    /// Rename is committed like the old editor's `onBlur`: typing updates a
    /// draft, any other interaction commits it, and Escape discards it.
    fn commit_rename_before(&mut self, message: &Message) {
        if self.ui.rename.is_none() {
            return;
        }
        if matches!(
            message,
            Message::Tick
                | Message::RenameMedia(..)
                | Message::RenameCommit
                | Message::BeginRename(..)
                | Message::Dismiss
        ) {
            return;
        }
        self.commit_rename();
    }

    fn edit_rename(&mut self, id: MediaId, value: String) {
        let Some(original) = self.session.clip_by_id(id).map(|clip| clip.name.clone()) else {
            return;
        };
        let restart = self.ui.rename.as_ref().is_some_and(|draft| draft.id != id);
        if restart {
            // Moving to another field commits the previous draft instead of
            // silently dropping the edit.
            self.commit_rename();
        }
        if self.ui.rename.as_ref().is_none_or(|draft| draft.id != id) {
            self.ui.rename = Some(crate::model::RenameDraft {
                id,
                original,
                value: value.clone(),
            });
        }
        if let Some(draft) = &mut self.ui.rename {
            draft.value = value;
        }
    }

    /// Opens a rename draft without typing anything yet. Used by the
    /// double-click gesture on a track name.
    fn begin_rename(&mut self, id: MediaId) {
        let Some(original) = self.session.clip_by_id(id).map(|clip| clip.name.clone()) else {
            return;
        };
        if self.ui.rename.as_ref().is_some_and(|draft| draft.id == id) {
            return;
        }
        self.commit_rename();
        self.ui.rename = Some(crate::model::RenameDraft {
            id,
            original: original.clone(),
            value: original,
        });
    }

    fn commit_rename(&mut self) {
        let Some(draft) = self.ui.rename.take() else {
            return;
        };
        let name = draft.value.trim();
        if name.is_empty() || name == draft.original.trim() {
            return;
        }
        let name = name.to_string();
        // Renaming changes the export file name only; it must not rebuild the
        // collapsed waveform or invalidate the rendered canvas.
        if let Some(clip) = self.session.editing_mut(draft.id) {
            clip.name = name;
        }
    }

    /// Escape closes the innermost transient layer.
    fn dismiss(&mut self) {
        if self.ui.dragging.is_some() {
            self.ui.dragging = None;
            return;
        }
        if self.ui.rename.is_some() {
            self.ui.rename = None;
            return;
        }
        if self.ui.confirm_quit {
            self.ui.confirm_quit = false;
            return;
        }
        if self.ui.context_menu.is_some() {
            self.ui.context_menu = None;
            return;
        }
        if self.ui.settings_open {
            self.ui.settings_open = false;
            self.ui.settings.busy = false;
            self.session.denoise_note = None;
            self.stop_playback(false);
        }
    }

    fn tick(&mut self) {
        // Drain follow-up events in the same tick. A dialog chain or an
        // inline job emits more events while they are being applied; waiting
        // for the next frame would add latency in production and turn every
        // such chain into a polling loop in tests. The cap only protects
        // against a handler that re-emits forever.
        const MAX_EVENT_BATCHES: usize = 256;
        for _ in 0..MAX_EVENT_BATCHES {
            let events = self.services.events.drain();
            if events.is_empty() {
                break;
            }
            for event in events {
                self.apply_event(event);
            }
        }
        if self.session.recording.is_some() {
            let seconds = self
                .session
                .recording
                .as_ref()
                .map(|recording| recording.started.elapsed().as_secs_f64())
                .unwrap_or(0.0);
            self.session.recording_seconds = seconds;
        }
        if let Some(playback) = self.session.playback.clone() {
            match self.services.player.position() {
                Some(position) => {
                    let clamped = position.clamp(0.0, playback.duration);
                    self.session.playhead = original_time_at_edited_offset(clamped, &playback.kept);
                }
                None => {
                    self.services.player.stop();
                    self.session.playback = None;
                    self.session.playhead = 0.0;
                }
            }
        }
    }

    fn apply_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::RecordingStarted => {
                self.ui.recording_busy = false;
                let placeholder = new_id();
                let name = format!("Recording {}", chrono_like_clock());
                self.session
                    .media
                    .push(MediaClip::recording_placeholder(placeholder, name));
                self.session.recording = Some(crate::model::ActiveRecording {
                    placeholder,
                    started: std::time::Instant::now(),
                });
                self.session.recording_seconds = 0.0;
                self.ui.message = "Recording from the default microphone".into();
            }
            AppEvent::RecordingStartFailed(error) => {
                self.ui.recording_busy = false;
                self.session.recording = None;
                self.session.recording_seconds = 0.0;
                self.ui.message = error;
            }
            AppEvent::RecordingStopped {
                placeholder,
                result,
            } => {
                self.ui.recording_busy = false;
                self.session.recording = None;
                self.session.recording_seconds = 0.0;
                match result {
                    Ok(decoded) => {
                        let id = placeholder;
                        let name = decoded.info.name.clone();
                        let existed = self.session.clip_by_id(id).is_some();
                        self.apply_decoded(
                            id,
                            decoded.info.path.clone(),
                            decoded.audio,
                            decoded.rms,
                            decoded.info.integrated_lufs,
                        );
                        if !existed {
                            // The placeholder was deleted while the take was
                            // being finalized; do not claim a success the user
                            // cannot see.
                            self.ui.message =
                                "The recording was discarded before it finished.".into();
                            return;
                        }
                        if let Some(clip) = self.session.editing_mut(id) {
                            if clip.name.trim().is_empty() {
                                clip.name = name.clone();
                            }
                        }
                        self.ui.message = "Recording is ready in the media pool.".into();
                        self.queue_denoise(id);
                    }
                    Err(error) => {
                        if let Some(clip) = self.session.editing_mut(placeholder) {
                            clip.status = ClipStatus::Failed(error.clone());
                        }
                        self.ui.message = error;
                    }
                }
            }
            AppEvent::ImportNormalizing { id } => {
                if let Some(clip) = self.session.editing_mut(id) {
                    clip.status = ClipStatus::Normalizing;
                }
            }
            AppEvent::ImportCompleted {
                id,
                info,
                audio,
                rms,
            } => {
                let existed = self.session.clip_by_id(id).is_some();
                self.apply_decoded(id, info.path.clone(), audio, rms, info.integrated_lufs);
                if !existed {
                    self.ui.message = "The imported media was removed before it finished.".into();
                    return;
                }
                if let Some(clip) = self.session.editing_mut(id) {
                    if clip.name.trim().is_empty() {
                        clip.name = info.name;
                    }
                }
                let display = self
                    .session
                    .clip_by_id(id)
                    .map(|clip| clip.name.clone())
                    .unwrap_or_default();
                self.ui.message = format!("{display} is ready in the media pool.");
                self.queue_denoise(id);
            }
            AppEvent::ImportFailed { id, error } => {
                if let Some(clip) = self.session.editing_mut(id) {
                    clip.status = ClipStatus::Failed(error.clone());
                    clip.denoise.status = crate::model::DenoiseStatus::Failed;
                }
                let name = self
                    .session
                    .clip_by_id(id)
                    .map(|clip| clip.name.clone())
                    .unwrap_or_else(|| "Media".into());
                self.ui.message = format!("Import failed for {name}: {error}");
            }
            AppEvent::DenoiseUnavailable { id, reason } => {
                if let Some(clip) = self.session.editing_mut(id) {
                    // A missing/broken provider only means "no denoise"; the
                    // take stays editable and exportable.
                    clip.denoise.status = crate::model::DenoiseStatus::Unavailable;
                    clip.denoise.task_id.clear();
                    clip.denoise.reason = reason.clone();
                }
                // Ambient state, not an action result: surface it in the
                // settings drawer instead of overwriting the status bar.
                if reason.is_some() {
                    self.session.denoise_note = reason;
                }
            }
            AppEvent::DenoiseStarted { id, task_id } => {
                self.session.denoise_note = None;
                self.session
                    .mark_denoise(id, &task_id, crate::model::DenoiseStatus::Processing);
            }
            AppEvent::DenoiseCompleted {
                id,
                task_id,
                path,
                lufs,
                audio,
                rms,
            } => {
                let current = self
                    .session
                    .clip_by_id(id)
                    .map(|clip| clip.denoise.task_id.clone());
                if current.as_deref() != Some(task_id.as_str()) {
                    return;
                }
                if self
                    .session
                    .playback
                    .as_ref()
                    .is_some_and(|playback| playback.track_id == id)
                {
                    self.stop_playback(true);
                }
                let was_selected = self.session.selected_track == Some(id);
                self.apply_decoded(id, path, audio, rms, lufs);
                if let Some(clip) = self.session.editing_mut(id) {
                    clip.denoise.status = crate::model::DenoiseStatus::Complete;
                    clip.denoise.reason = None;
                }
                self.session.denoise_note = None;
                if was_selected {
                    self.refresh_silence_inputs();
                }
                let name = self
                    .session
                    .clip_by_id(id)
                    .map(|clip| clip.name.clone())
                    .unwrap_or_default();
                self.session.denoise_note = Some(format!("{name}: denoising complete."));
            }
            AppEvent::DenoiseFailed { id, task_id, error } => {
                let current = self
                    .session
                    .clip_by_id(id)
                    .map(|clip| clip.denoise.task_id.clone());
                if current.as_deref() != Some(task_id.as_str()) {
                    return;
                }
                if let Some(clip) = self.session.editing_mut(id) {
                    clip.denoise.status = crate::model::DenoiseStatus::Failed;
                    clip.denoise.task_id.clear();
                    clip.denoise.reason = Some(error.clone());
                }
                // Background denoise failures are ambient: the row badge and
                // the drawer already show them, so do not overwrite the status
                // bar result of whatever the user just did.
                self.session.denoise_note = Some(error);
            }
            AppEvent::OriginalAudioRestored {
                id,
                path,
                audio,
                rms,
            } => {
                let was_selected = self.session.selected_track == Some(id);
                if self
                    .session
                    .playback
                    .as_ref()
                    .is_some_and(|playback| playback.track_id == id)
                {
                    self.stop_playback(true);
                }
                self.apply_decoded(id, path, audio, rms, None);
                if let Some(clip) = self.session.editing_mut(id) {
                    // Restoring the source is a user choice: mark it as an
                    // opt-out so an automatic policy cannot queue denoising
                    // again behind their back.
                    clip.denoise = crate::model::DenoiseState::skipped_because(Some(
                        "Using the normalized original audio.".into(),
                    ));
                    clip.denoise.opted_out = true;
                }
                if was_selected {
                    self.refresh_silence_inputs();
                }
                let name = self
                    .session
                    .clip_by_id(id)
                    .map(|clip| clip.name.clone())
                    .unwrap_or_default();
                self.ui.message = format!("{name} now plays its original audio.");
            }
            AppEvent::OriginalAudioRestoreFailed { id, error } => {
                self.ui.message = format!("Unable to restore the original audio: {error}");
                if let Some(clip) = self.session.editing_mut(id) {
                    clip.denoise.reason = Some(error);
                }
            }
            AppEvent::ExportCompleted {
                results,
                signatures,
            } => {
                self.session.exporting.clear();
                let failures: Vec<_> = results
                    .iter()
                    .filter(|result| result.error.is_some())
                    .collect();
                let failed_ids: Vec<MediaId> = results
                    .iter()
                    .filter(|result| result.error.is_some())
                    .filter_map(|result| result.recording_id.parse().ok())
                    .collect();
                // Keep failures from earlier batches, but replace the state of
                // every track that took part in this one.
                let batch_ids: Vec<MediaId> = signatures.iter().map(|(id, _)| *id).collect();
                self.session
                    .failed_exports
                    .retain(|id| !batch_ids.contains(id));
                for id in failed_ids {
                    if !self.session.failed_exports.contains(&id) {
                        self.session.failed_exports.push(id);
                    }
                }
                let mut changed_during_export = false;
                for (id, signature) in signatures {
                    let succeeded = results.iter().any(|result| {
                        result.recording_id == id.to_string() && result.error.is_none()
                    });
                    if !succeeded {
                        continue;
                    }
                    // Only claim the export if the track still produces the
                    // audio that was sent to FFmpeg. A denoise pass finishing
                    // mid-export replaces the buffer, so the track must stay
                    // queued instead of being marked with a stale signature.
                    let unchanged = self
                        .session
                        .track(id)
                        .is_some_and(|track| track.signature() == signature);
                    if unchanged {
                        self.session.mark_exported(id, signature);
                    } else {
                        changed_during_export = true;
                    }
                }
                let single = self.export_single;
                self.pending_export = None;
                if let Some(failure) = failures.first() {
                    let name = results
                        .iter()
                        .find(|result| result.error.is_some())
                        .and_then(|result| result.recording_id.parse::<MediaId>().ok())
                        .and_then(|id| self.session.clip_by_id(id))
                        .map(|clip| clip.name.clone())
                        .unwrap_or_else(|| "A track".into());
                    self.ui.message = format!(
                        "Exported {}/{} tracks. {name} failed: {}",
                        results.len() - failures.len(),
                        results.len(),
                        failure.error.clone().unwrap_or_default()
                    );
                } else {
                    let directory = self
                        .services
                        .export_dirs
                        .last_directory()
                        .unwrap_or_else(|| "the chosen folder".into());
                    if single {
                        let name = results
                            .first()
                            .and_then(|result| result.recording_id.parse::<MediaId>().ok())
                            .and_then(|id| self.session.clip_by_id(id))
                            .map(|clip| clip.name.clone())
                            .unwrap_or_else(|| "Track".into());
                        self.ui.message = format!("Exported {name} to {directory}");
                    } else {
                        self.ui.message = format!(
                            "Exported {} changed timeline tracks to {directory}",
                            results.len()
                        );
                    }
                }
                if changed_during_export {
                    self.ui.message.push_str(
                        " A track changed during export and stays queued for the next export.",
                    );
                }
            }
            AppEvent::ExportFailed { error } => {
                self.session.exporting.clear();
                self.pending_export = None;
                self.export_single = false;
                self.ui.message = error;
            }
            AppEvent::SettingsStatus(result) => {
                self.ui.settings.busy = false;
                match result {
                    Ok(status) => {
                        // Opening the drawer seeds the draft from the saved
                        // configuration.
                        if self.ui.settings.draft.is_none() {
                            self.ui.settings.draft = Some(status.configured.clone());
                        }
                        self.ui.settings.status = Some(status.clone());
                        self.session.denoise_status = Some(status);
                        self.apply_denoise_policy();
                    }
                    Err(error) => self.ui.settings.error = Some(error),
                }
            }
            AppEvent::SettingsInspected(result) => {
                self.ui.settings.busy = false;
                match result {
                    Ok(status) => {
                        // Never touch the draft: the user may be editing it
                        // while the check runs.
                        if self.ui.settings.draft.is_none() {
                            self.ui.settings.draft = Some(status.configured.clone());
                        }
                        self.ui.settings.status = Some(status);
                        // Do not touch `session.denoise_status`: that one is
                        // the *saved* configuration the denoising policy
                        // runs against. Applying an unsaved draft here made
                        // "Check setup" silently change how new takes are
                        // handled.
                    }
                    Err(error) => self.ui.settings.error = Some(error),
                }
            }
            AppEvent::SettingsSaved(result) => {
                self.ui.settings.busy = false;
                match result {
                    Ok(status) => {
                        self.session.denoise_status = Some(status.clone());
                        self.ui.settings.draft = Some(status.configured.clone());
                        self.ui.settings.status = Some(status.clone());
                        // Keep the drawer open so the user can read the
                        // summary of what was saved.
                        self.ui.settings.error = None;
                        self.ui.message = if status.ready {
                            "Denoising setup saved. Queuing eligible media.".into()
                        } else {
                            status.summary.clone()
                        };
                        if status.ready {
                            self.apply_denoise_policy();
                        }
                    }
                    Err(error) => self.ui.settings.error = Some(error),
                }
            }
            AppEvent::PathsChosen { request, paths } => match request {
                FileRequest::ImportMedia => self.import_paths(paths),
                FileRequest::ExportFolder => self.export_to_folder(paths.into_iter().next()),
                FileRequest::PythonExecutable => {
                    if let (Some(path), Some(draft)) =
                        (paths.into_iter().next(), self.ui.settings.draft.as_mut())
                    {
                        draft.executable_path = path.display().to_string();
                    }
                }
                FileRequest::ModelRoot => {
                    if let (Some(path), Some(draft)) =
                        (paths.into_iter().next(), self.ui.settings.draft.as_mut())
                    {
                        draft.model_root = path.display().to_string();
                    }
                }
            },
            AppEvent::PathChoiceCancelled { request } => {
                if request == FileRequest::ExportFolder {
                    self.session.exporting.clear();
                    self.pending_export = None;
                    self.export_single = false;
                    self.ui.message = "Export cancelled".into();
                }
            }
            AppEvent::DialogFailed { request, error } => {
                if request == FileRequest::ExportFolder {
                    self.session.exporting.clear();
                    self.pending_export = None;
                    self.export_single = false;
                } else {
                    self.ui.settings.busy = false;
                    self.ui.settings.error = Some(error.clone());
                }
                self.ui.message = error;
            }
        }
    }

    fn apply_decoded(
        &mut self,
        id: MediaId,
        path: String,
        audio: Arc<simple_audio_cut_core::buffer::AudioBuffer>,
        rms: Arc<Vec<simple_audio_cut_core::analysis::RmsFrame>>,
        lufs: Option<f64>,
    ) {
        let duration = audio.duration();
        let silence_settings = self.session.track(id).map(|track| {
            (
                track.silence_detection,
                track.silence_threshold_db,
                track.min_silence_ms,
            )
        });
        if let Some(clip) = self.session.editing_mut(id) {
            clip.apply_audio(path, duration, lufs, audio, rms);
        }
        if let Some((enabled, threshold, min_ms)) = silence_settings {
            let silence = if enabled {
                self.session
                    .track(id)
                    .map(|track| {
                        detect_silence_from_analysis(
                            track.rms(),
                            track.duration(),
                            SilenceOptions {
                                threshold: dbfs_to_amplitude(threshold),
                                min_duration: min_ms / 1_000.0,
                                padding: 0.0,
                            },
                        )
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            if let Some(track) = self.session.track_mut(id) {
                track.silence_regions = silence;
                track.touch();
                track.rebuild_collapsed();
            }
        } else if let Some(track) = self.session.track_mut(id) {
            track.touch();
            track.rebuild_collapsed();
        }
    }

    fn apply_silence_inputs(&mut self) {
        let threshold = self
            .ui
            .silence_threshold_input
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite());
        let minimum = self
            .ui
            .min_silence_input
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite());
        let Some(track) = self.session.selected_track_mut() else {
            return;
        };
        if let Some(value) = threshold {
            track.silence_threshold_db = value.clamp(-70.0, -12.0);
        }
        if let Some(value) = minimum {
            track.min_silence_ms = value.clamp(0.0, 5_000.0);
        }
        let id = track.id();
        let enabled = track.silence_detection;
        let threshold = track.silence_threshold_db;
        let min_ms = track.min_silence_ms;
        if !enabled {
            return;
        }
        if let Some(track) = self.session.track(id) {
            let silence = detect_silence_from_analysis(
                track.rms(),
                track.duration(),
                SilenceOptions {
                    threshold: dbfs_to_amplitude(threshold),
                    min_duration: min_ms / 1_000.0,
                    padding: 0.0,
                },
            );
            if let Some(track) = self.session.track_mut(id) {
                track.silence_regions = silence;
                track.touch();
            }
        }
    }

    fn refresh_silence_inputs(&mut self) {
        if let Some(track) = self.session.selected_track() {
            self.ui.silence_threshold_input = format!("{}", track.silence_threshold_db.round());
            self.ui.min_silence_input = format!("{}", track.min_silence_ms.round());
        } else {
            self.ui.silence_threshold_input = format!("{DEFAULT_SILENCE_THRESHOLD_DB}");
            self.ui.min_silence_input = format!("{DEFAULT_MIN_SILENCE_MS}");
        }
    }

    fn toggle_recording(&mut self) {
        if self.ui.recording_busy {
            return;
        }
        if let Some(recording) = self.session.recording.take() {
            self.session.recording_seconds = 0.0;
            self.ui.recording_busy = true;
            self.ui.message = format!(
                "Measuring loudness and normalizing to {} LUFS",
                self.session.target_lufs
            );
            jobs::stop_recording(
                self.services.jobs.clone(),
                self.services.engine.clone(),
                self.services.events.clone(),
                recording.placeholder,
                self.session.target_lufs,
            );
            return;
        }
        self.stop_playback(false);
        self.ui.recording_busy = true;
        self.ui.message = "Starting the microphone…".into();
        jobs::start_recording(
            self.services.jobs.clone(),
            self.services.engine.clone(),
            self.services.events.clone(),
        );
    }

    fn import_audio(&mut self) {
        self.stop_playback(true);
        jobs::choose_import_files(
            self.services.jobs.clone(),
            self.services.dialogs.clone(),
            self.services.events.clone(),
        );
    }

    fn import_paths(&mut self, paths: Vec<std::path::PathBuf>) {
        let paths: Vec<String> = jobs::import_paths(paths);
        if paths.is_empty() {
            return;
        }
        self.stop_playback(false);
        let mut ids = Vec::new();
        for path in &paths {
            let id = new_id();
            ids.push(id);
            self.session
                .media
                .push(MediaClip::placeholder(id, path.clone()));
        }
        self.ui.message = simple_audio_cut_core::import_names::import_queued_summary(paths.len());
        for (id, path) in ids.into_iter().zip(paths) {
            jobs::import_media(
                self.services.jobs.clone(),
                self.services.engine.clone(),
                self.services.events.clone(),
                id,
                path,
                self.session.target_lufs,
            );
        }
    }

    fn retry_import(&mut self, id: MediaId) {
        let Some(clip) = self.session.media(id) else {
            return;
        };
        let Some(source_path) = clip.source_path.clone() else {
            return;
        };
        let name = clip.name.clone();
        if let Some(clip) = self.session.media_mut(id) {
            clip.status = ClipStatus::Normalizing;
            clip.denoise.status = crate::model::DenoiseStatus::Unavailable;
        }
        self.ui.message = format!("Retrying {name}");
        jobs::import_media(
            self.services.jobs.clone(),
            self.services.engine.clone(),
            self.services.events.clone(),
            id,
            source_path,
            self.session.target_lufs,
        );
    }

    /// Queues denoising for `id` unless the user opted out or a task is
    /// already running. Manual requests (`force`) bypass the automatic switch.
    fn queue_denoise(&mut self, id: MediaId) {
        self.queue_denoise_inner(id, false)
    }

    fn queue_denoise_forced(&mut self, id: MediaId) {
        self.queue_denoise_inner(id, true)
    }

    fn queue_denoise_inner(&mut self, id: MediaId, force: bool) {
        let Some(clip) = self.session.clip_by_id(id) else {
            return;
        };
        if clip.denoise.status.is_busy() || (!force && clip.denoise.opted_out) {
            return;
        }
        if !force {
            // The global switch means "denoise new media automatically". When
            // it is off the take keeps its normalized audio and the inspector
            // can still trigger it explicitly. Until the provider status has
            // been resolved we do not know the switch, so the take is left
            // alone; `apply_denoise_policy` runs once the status arrives.
            let Some(automatic) = self
                .session
                .denoise_status
                .as_ref()
                .map(|status| status.effective.enabled)
            else {
                return;
            };
            if !automatic {
                // A build without a provider cannot denoise at all; anything
                // else is a user preference that can be overridden per take.
                let (provider_usable, provider_summary) = self
                    .session
                    .denoise_status
                    .as_ref()
                    .map(|status| (status.has_provider(), status.summary.clone()))
                    .unwrap_or((false, String::new()));
                if let Some(clip) = self.session.editing_mut(id) {
                    clip.denoise = if provider_usable {
                        crate::model::DenoiseState::skipped_because(Some(
                            "Automatic denoising is off. You can denoise this take manually."
                                .into(),
                        ))
                    } else {
                        crate::model::DenoiseState::unavailable(Some(
                            if provider_summary.is_empty() {
                                "Denoising is not available.".into()
                            } else {
                                provider_summary
                            },
                        ))
                    };
                }
                return;
            }
        }
        let Some(audio) = clip.audio.clone() else {
            return;
        };
        let path = clip.denoise_source().to_string();
        let task_id = new_id().to_string();
        if let Some(clip) = self.session.editing_mut(id) {
            clip.denoise = crate::model::DenoiseState {
                status: crate::model::DenoiseStatus::Queued,
                task_id: task_id.clone(),
                reason: None,
                opted_out: false,
            };
        }
        if let Some(track) = self.session.track_mut(id) {
            track.touch();
        }
        jobs::denoise_media(
            self.services.jobs.clone(),
            self.services.engine.clone(),
            self.services.denoise.clone(),
            self.services.events.clone(),
            id,
            path,
            audio.sample_rate(),
            self.session.target_lufs,
            task_id,
        );
    }

    /// Per-take denoising switch. Turning it off keeps the normalized source
    /// (the take stays fully editable and exportable); turning it back on
    /// queues a new task unless one is already running.
    fn set_denoise(&mut self, id: MediaId, enabled: bool) {
        let Some(clip) = self.session.clip_by_id(id) else {
            return;
        };
        let name = clip.name.clone();
        let status = clip.denoise.status;
        let restore_path = clip
            .has_original_audio()
            .then(|| clip.normalized_path.clone());
        if !enabled {
            // A take skipped by the automatic policy still needs the explicit
            // marker: otherwise a later policy run could turn denoising back
            // on against the user's choice.
            if status == crate::model::DenoiseStatus::Skipped && clip.denoise.opted_out {
                return;
            }
            let was_denoised = status == crate::model::DenoiseStatus::Complete;
            if let Some(clip) = self.session.editing_mut(id) {
                let mut skipped = crate::model::DenoiseState::skipped_because(Some(
                    "Denoising is off for this take.".into(),
                ));
                skipped.opted_out = true;
                clip.denoise = skipped;
            }
            if let Some(track) = self.session.track_mut(id) {
                track.touch();
            }
            match restore_path {
                Some(path) if was_denoised => {
                    // Swap the decoded buffer back to the normalized source in
                    // the background; the take stays visible meanwhile.
                    self.ui.message = format!("Restoring the original audio of {name}…");
                    jobs::restore_original_audio(
                        self.services.jobs.clone(),
                        self.services.events.clone(),
                        id,
                        path,
                    );
                }
                _ => {
                    self.ui.message = format!("{name} keeps its original audio.");
                }
            }
            return;
        }
        if status.is_busy() {
            return;
        }
        if status == crate::model::DenoiseStatus::Complete {
            self.ui.message = format!("{name} is already denoised.");
            return;
        }
        if let Some(clip) = self.session.editing_mut(id) {
            // Clear the opt-out/failure marker so `queue_denoise` accepts the
            // take again; it re-checks provider availability itself.
            clip.denoise.status = crate::model::DenoiseStatus::Unavailable;
            clip.denoise.task_id.clear();
            clip.denoise.reason = None;
        }
        self.ui.message = format!("Queuing denoising for {name}.");
        self.queue_denoise_forced(id);
    }

    /// Applies the global denoising policy to every ready take.
    ///
    /// Runs whenever the provider status is (re)resolved, so the outcome no
    /// longer depends on whether the import finished before or after the
    /// status arrived. An explicit per-take opt-out is never overridden.
    fn apply_denoise_policy(&mut self) {
        let Some(status) = self.session.denoise_status.clone() else {
            return;
        };
        let automatic = status.can_denoise_automatically();
        for id in self.session.denoise_policy_candidates() {
            if automatic {
                self.queue_denoise(id);
                continue;
            }
            // Explain the specific blocker (missing Python, model, device…)
            // instead of a generic "not available".
            let reason = status.blocking_reason();
            if let Some(clip) = self.session.editing_mut(id) {
                clip.denoise = if status.has_provider() && !status.effective.enabled {
                    crate::model::DenoiseState::skipped_because(Some(reason))
                } else {
                    crate::model::DenoiseState::unavailable(Some(reason))
                };
            }
        }
    }

    fn select_media(&mut self, id: MediaId) {
        if self.session.selected_track.is_some() {
            self.stop_playback(false);
        }
        self.session.selected_media = Some(id);
        self.session.selected_track = None;
        self.session.playhead = 0.0;
        self.refresh_silence_inputs();
    }

    fn select_track(&mut self, id: MediaId) {
        if self.session.selected_track == Some(id) {
            return;
        }
        self.stop_playback(false);
        self.session.selected_track = Some(id);
        self.session.selected_media = None;
        self.session.playhead = 0.0;
        self.refresh_silence_inputs();
    }

    /// Picks a library take up for dragging. Selecting it first matches what a
    /// plain click used to do, so a cancelled drag leaves no odd state behind.
    fn begin_drag(&mut self, id: MediaId) {
        // Selecting is unconditional: a press on a card must still behave like
        // a click even when the take is not ready to be dragged.
        self.select_media(id);
        if self.session.media(id).is_some_and(MediaClip::ready) {
            self.ui.dragging = Some(id);
        }
    }

    /// Finishes a library drag: the take becomes its own timeline track.
    fn drop_item(&mut self) {
        let Some(id) = self.ui.dragging.take() else {
            return;
        };
        self.add_to_timeline(id);
    }

    /// Imports files dropped from the file manager, filtering by extension the
    /// same way the import dialog does.
    fn import_dropped(&mut self, paths: Vec<std::path::PathBuf>) {
        self.ui.files_hovering = false;
        let accepted: Vec<std::path::PathBuf> = paths
            .into_iter()
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .map(|extension| extension.to_ascii_lowercase())
                    .is_some_and(|extension| {
                        crate::services::IMPORT_EXTENSIONS
                            .iter()
                            .any(|candidate| *candidate == extension)
                    })
            })
            .collect();
        if accepted.is_empty() {
            self.ui.message = "Only audio or video files can be dropped here.".into();
            return;
        }
        self.import_paths(accepted);
    }

    fn add_to_timeline(&mut self, id: MediaId) {
        let Some(clip) = self.session.media(id).cloned() else {
            return;
        };
        if !clip.ready() {
            self.ui.message = if matches!(clip.status, ClipStatus::Failed(_)) {
                format!("{} failed to import.", clip.name)
            } else {
                format!("{} is still normalizing.", clip.name)
            };
            return;
        }
        self.stop_playback(false);
        self.session.remove_media(id);
        let track = Track::from_clip(clip);
        self.session.add_track(track);
        self.session.selected_media = None;
        self.session.playhead = 0.0;
        self.refresh_silence_inputs();
        self.ui.message = "Editing selected recording".into();
    }

    fn delete_media(&mut self, id: MediaId) {
        let Some(clip) = self.session.remove_media(id) else {
            return;
        };
        self.ui.message = format!("{} deleted from the source shelf", clip.name);
    }

    fn remove_track(&mut self, id: MediaId) {
        self.stop_playback(false);
        let Some(track) = self.session.remove_track(id) else {
            return;
        };
        self.ui.message = format!("{} deleted from the timeline", track.clip.name);
        self.refresh_silence_inputs();
    }

    fn return_track_to_pool(&mut self, id: MediaId) {
        self.stop_playback(false);
        let Some(track) = self.session.remove_track(id) else {
            return;
        };
        let mut clip = track.clip;
        if let Some(audio) = clip.audio.as_ref() {
            clip.duration = audio.duration();
        }
        let clip_id = clip.id;
        self.session.media.push(clip);
        self.session.selected_media = Some(clip_id);
        self.session.selected_track = None;
        self.session.playhead = 0.0;
        self.refresh_silence_inputs();
        self.ui.message = "Recording returned to the library".into();
    }

    fn toggle_collapse(&mut self, id: MediaId) {
        self.stop_playback(false);
        let Some(track) = self.session.track_mut(id) else {
            return;
        };
        track.collapsed = !track.collapsed;
        track.selected_range = None;
        track.touch();
        track.rebuild_collapsed();
        let collapsed = track.collapsed;
        self.ui.message = if collapsed {
            "Deleted audio collapsed in the view.".into()
        } else {
            "Showing the complete original timeline.".into()
        };
    }

    fn scale_track(&mut self, id: MediaId, delta: f32) {
        if self.session.selected_track != Some(id) {
            self.select_track(id);
        }
        if let Some(track) = self.session.track_mut(id) {
            // Zooming from "fit" starts at the scale currently on screen, so
            // the first wheel tick feels continuous instead of snapping.
            if !track.zoom_locked {
                let fit = track
                    .effective_pixels_per_second(crate::ui::canvas_width(self.window_size.width));
                track.pixels_per_second = fit;
                track.zoom_locked = true;
            }
            let factor = if delta < 0.0 { 1.15 } else { 1.0 / 1.15 };
            track.pixels_per_second = (track.pixels_per_second * factor).clamp(0.25, 400.0);
            track.bump_revision();
        }
    }

    fn toggle_playback(&mut self) {
        if self.session.playback.is_some() {
            self.stop_playback(true);
        } else {
            let offset = self.session.playhead;
            self.start_playback(offset);
        }
    }

    fn start_playback(&mut self, offset: f64) {
        let Some(track) = self.session.selected_track() else {
            return;
        };
        if !track.clip.ready() {
            self.ui.message = "The selected track is not ready to play.".into();
            return;
        }
        let duration = track.duration();
        let deleted = track.deleted();
        let edited = create_edited_buffer(track.audio(), &deleted, &track.envelope, &track.muted);
        if edited.is_empty() {
            self.ui.message = "All audio has been removed from this recording.".into();
            return;
        }
        let kept = kept_regions(&deleted, duration);
        let safe_offset = if offset >= duration {
            0.0
        } else {
            offset.max(0.0)
        };
        let mut edited_offset = edited_offset_at_original_time(safe_offset, &kept);
        if edited_offset >= edited.duration() {
            edited_offset = 0.0;
        }
        match self
            .services
            .player
            .play(Arc::new(edited.clone()), edited_offset)
        {
            Ok(()) => {
                let id = track.id();
                self.session.playback = Some(crate::model::PlaybackState {
                    track_id: id,
                    kept,
                    duration: edited.duration(),
                });
                self.session.playhead = safe_offset;
            }
            Err(error) => {
                self.ui.message = format!("Unable to play audio: {error}");
            }
        }
    }

    fn stop_playback(&mut self, save_position: bool) {
        if self.session.playback.is_none() {
            self.services.player.stop();
            return;
        }
        if save_position {
            if let Some(position) = self.services.player.position() {
                if let Some(playback) = &self.session.playback {
                    self.session.playhead =
                        original_time_at_edited_offset(position, &playback.kept);
                }
            }
        }
        self.services.player.stop();
        self.session.playback = None;
    }

    fn seek(&mut self, time: f64) {
        let Some(track) = self.session.selected_track() else {
            return;
        };
        let was_playing = self
            .session
            .playback
            .as_ref()
            .is_some_and(|playback| playback.track_id == track.id());
        let safe = time.clamp(0.0, track.duration());
        self.stop_playback(false);
        self.session.playhead = safe;
        if let Some(track) = self.session.selected_track_mut() {
            track.selected_range = None;
        }
        if was_playing {
            self.start_playback(safe);
        }
    }

    fn set_selected_range(&mut self, range: Option<Region>) {
        if let Some(track) = self.session.selected_track_mut() {
            track.selected_range = range.map(Region::normalized);
        }
        if let Some(range) = range {
            self.ui.message = format!(
                "Selected {}. Delete cuts it out.",
                format_time_standard(range.duration())
            );
        }
    }

    fn toggle_mute_range(&mut self) {
        let Some((id, range)) = self
            .session
            .selected_track()
            .and_then(|track| track.selected_range.map(|range| (track.id(), range)))
        else {
            self.ui.message = "Drag across the waveform to select a range first.".into();
            return;
        };
        self.mute_range(id, range);
    }

    /// Toggles silence for `range` and keeps it selected, so pressing M or
    /// Delete can act on the same range immediately.
    fn mute_range(&mut self, id: MediaId, range: Region) {
        let Some(track) = self.session.track(id) else {
            return;
        };
        let mute = !simple_audio_cut_core::region::region_is_covered(&track.muted, range);
        self.apply_mute(id, range, mute);
    }

    /// Applies `mute` to `range` unconditionally. The left-drag passes its
    /// direction here (forward = mute, backward = restore). Selection is the
    /// caller's business: the drag clears it, the inspector button keeps it.
    fn apply_mute(&mut self, id: MediaId, range: Region, mute: bool) {
        if self.session.track(id).is_none() {
            return;
        }
        if self.session.track(id).is_some_and(|track| track.collapsed) {
            self.ui.message = "Show all edits to change a muted range.".into();
            return;
        }
        self.stop_playback(false);
        let range = range.normalized();
        if let Some(track) = self.session.track_mut(id) {
            track.set_mute(range, mute);
            track.touch();
        }
        self.ui.message = if mute {
            format!("Muted {}", format_time_standard(range.duration()))
        } else {
            format!(
                "Restored sound to {}",
                format_time_standard(range.duration())
            )
        };
    }

    /// Cuts the selected range out of the timeline (unlike mute, the result is
    /// shorter). Reversible with the region's Restore action or a left-drag
    /// while holding the right button.
    fn delete_selected_range(&mut self) {
        let Some((id, range)) = self
            .session
            .selected_track()
            .and_then(|track| track.selected_range.map(|range| (track.id(), range)))
        else {
            return;
        };
        if self
            .session
            .selected_track()
            .is_some_and(|track| track.collapsed)
        {
            return;
        }
        self.stop_playback(false);
        let mut removed_duration = range.duration();
        if let Some(track) = self.session.track_mut(id) {
            let before = track.edited_duration();
            track.manual_deleted = merge_regions(&track.manual_deleted, range);
            track.selected_range = None;
            track.touch();
            removed_duration = (before - track.edited_duration()).max(0.0);
        }
        self.ui.message = if removed_duration > 0.001 {
            format!(
                "Cut {} out of the timeline",
                format_time_standard(removed_duration)
            )
        } else {
            "That range was already removed.".into()
        };
    }

    fn waveform_event(&mut self, id: MediaId, event: WaveformEvent) {
        let Some(track) = self.session.track(id) else {
            return;
        };
        let collapsed = track.collapsed;
        let kept = if collapsed { track.kept() } else { Vec::new() };
        let to_source = |time: f64| {
            if collapsed {
                original_time_at_edited_offset(time, &kept)
            } else {
                time
            }
        };
        self.select_track(id);
        match event {
            WaveformEvent::Seek(time) => {
                let time = to_source(time);
                self.seek(time);
                // A plain click inside a blade segment selects that segment, the
                // same way the Tauri editor did.
                if let Some(track) = self.session.selected_track() {
                    if let Some(segment) = simple_audio_cut_core::direct_edit::segment_at_time(
                        &track.edit_points,
                        time,
                        track.duration(),
                    ) {
                        self.set_selected_range(Some(segment));
                    }
                }
            }
            WaveformEvent::MuteEdit { region, operation } => {
                let region = Region::new(to_source(region.start), to_source(region.end));
                let mute = matches!(operation, MuteRegionOperation::Mute);
                self.apply_mute(id, region, mute);
                // Same as the right-drag: the drag is the whole interaction, so
                // no selection marquee stays behind on the waveform.
                if let Some(track) = self.session.track_mut(id) {
                    track.selected_range = None;
                }
            }
            WaveformEvent::DirectEdit { region, operation } => {
                self.stop_playback(false);
                let region = Region::new(to_source(region.start), to_source(region.end));
                if self.session.track(id).is_some_and(|track| track.collapsed) {
                    self.ui.message = "Show all edits to delete or restore audio.".into();
                    return;
                }
                if let Some(track) = self.session.track_mut(id) {
                    match operation {
                        DirectRegionOperation::Delete => {
                            track.manual_deleted = merge_regions(&track.manual_deleted, region);
                        }
                        DirectRegionOperation::Restore => {
                            track.manual_deleted = subtract_region(&track.manual_deleted, region);
                            track.silence_regions = subtract_region(&track.silence_regions, region);
                        }
                    }
                    track.selected_range = None;
                    track.touch();
                }
                self.ui.message = format!(
                    "{} {} with right-drag",
                    if matches!(operation, DirectRegionOperation::Delete) {
                        "Deleted"
                    } else {
                        "Restored"
                    },
                    format_time_standard(region.duration())
                );
            }
            WaveformEvent::Restore(region) => {
                self.stop_playback(false);
                let region = Region::new(to_source(region.start), to_source(region.end));
                if let Some(track) = self.session.track_mut(id) {
                    track.manual_deleted = subtract_region(&track.manual_deleted, region);
                    track.silence_regions = subtract_region(&track.silence_regions, region);
                    track.selected_range = None;
                    track.touch();
                }
                self.ui.message = format!(
                    "Restored {} of deleted audio",
                    format_time_standard(region.duration())
                );
            }
            WaveformEvent::Blade(time) => {
                let time = to_source(time);
                self.stop_playback(false);
                if let Some(track) = self.session.track_mut(id) {
                    let duration = track.duration();
                    track.edit_points = simple_audio_cut_core::direct_edit::add_edit_point(
                        &track.edit_points,
                        time,
                        duration,
                    );
                    track.selected_range = None;
                    track.touch();
                }
                self.session.playhead = time;
                self.ui.message = format!("Edit point added at {}", format_time_standard(time));
            }
            WaveformEvent::RemoveEditPoint(time) => {
                let time = to_source(time);
                if let Some(track) = self.session.track_mut(id) {
                    track
                        .edit_points
                        .retain(|point| (point - time).abs() >= 0.01);
                    track.touch();
                }
            }
            WaveformEvent::EnvelopeAdd(point) => {
                self.stop_playback(false);
                let point = EnvelopePoint::new(point.id, to_source(point.time), point.gain);
                if let Some(track) = self.session.track_mut(id) {
                    track.envelope.push(point);
                    track
                        .envelope
                        .sort_by(|left, right| left.time.total_cmp(&right.time));
                    track.touch();
                }
            }
            WaveformEvent::EnvelopeMove {
                id: point_id,
                time,
                gain,
            } => {
                let time = to_source(time);
                if let Some(track) = self.session.track_mut(id) {
                    let duration = track.duration();
                    for point in &mut track.envelope {
                        if point.id == point_id {
                            point.time = time.clamp(0.0, duration);
                            point.gain = gain.clamp(0.0, 2.0);
                        }
                    }
                    track
                        .envelope
                        .sort_by(|left, right| left.time.total_cmp(&right.time));
                    track.touch();
                }
            }
            WaveformEvent::EnvelopeRemove(point_id) => {
                self.stop_playback(false);
                if let Some(track) = self.session.track_mut(id) {
                    track.envelope.retain(|point| point.id != point_id);
                    track.touch();
                }
            }
        }
    }

    fn detect_silence(&mut self) {
        self.stop_playback(false);
        let Some(track) = self.session.selected_track() else {
            return;
        };
        let id = track.id();
        let regions = detect_silence_from_analysis(
            track.rms(),
            track.duration(),
            SilenceOptions {
                threshold: dbfs_to_amplitude(track.silence_threshold_db),
                min_duration: track.min_silence_ms / 1_000.0,
                padding: 0.0,
            },
        );
        let count = regions.len();
        if let Some(track) = self.session.track_mut(id) {
            track.silence_regions = regions;
            track.silence_detection = true;
            track.selected_range = None;
            track.touch();
        }
        self.ui.message = format!("Detected {count} silence region(s).");
    }

    fn clear_silence(&mut self) {
        let Some(track) = self.session.selected_track_mut() else {
            return;
        };
        track.silence_regions.clear();
        track.silence_detection = false;
        track.touch();
        self.ui.message = "Cleared automatic silence cuts.".into();
    }

    fn clear_envelope(&mut self) {
        let Some(track) = self.session.selected_track_mut() else {
            return;
        };
        if track.envelope.is_empty() {
            return;
        }
        track.envelope.clear();
        track.touch();
        self.ui.message = "Cleared the volume envelope.".into();
    }

    fn start_export(&mut self, ids: Vec<MediaId>, single: bool) {
        if !self.session.exporting.is_empty() {
            self.ui.message = "An export is already running.".into();
            return;
        }
        self.stop_playback(true);
        let mut edits = Vec::new();
        let mut signatures = Vec::new();
        for id in &ids {
            let Some(track) = self.session.track(*id) else {
                continue;
            };
            edits.push(ExportEdit {
                recording_id: id.to_string(),
                name: track.clip.name.clone(),
                source_path: track.clip.path.clone(),
                deleted_regions: track
                    .deleted()
                    .into_iter()
                    .map(|region| simple_audio_cut_core::audio::Region {
                        start: region.start,
                        end: region.end,
                    })
                    .collect(),
                muted_regions: track
                    .muted
                    .iter()
                    .map(|region| simple_audio_cut_core::audio::Region {
                        start: region.start,
                        end: region.end,
                    })
                    .collect(),
                envelope_points: track
                    .envelope
                    .iter()
                    .map(|point| simple_audio_cut_core::audio::EnvelopePoint {
                        time: point.time,
                        gain: point.gain,
                    })
                    .collect(),
            });
            signatures.push((*id, track.signature()));
        }
        if edits.is_empty() {
            self.ui.message = "There are no timeline tracks to export.".into();
            return;
        }
        self.session.exporting = ids;
        self.export_single = single;
        self.session
            .failed_exports
            .retain(|id| !signatures.iter().any(|(candidate, _)| candidate == id));
        self.pending_export = Some(PendingExport { edits, signatures });
        self.ui.message = "Choose a folder for the exported audio.".into();
        jobs::choose_folder(
            self.services.jobs.clone(),
            self.services.dialogs.clone(),
            self.services.events.clone(),
            FileRequest::ExportFolder,
            "Choose a folder for exported audio",
            self.services.export_dirs.last_directory(),
        );
    }

    fn export_to_folder(&mut self, destination: Option<std::path::PathBuf>) {
        let Some(pending) = self.pending_export.take() else {
            self.session.exporting.clear();
            self.export_single = false;
            return;
        };
        let Some(destination) = destination else {
            self.session.exporting.clear();
            self.export_single = false;
            self.ui.message = "Export cancelled".into();
            return;
        };
        let directory = destination.display().to_string();
        self.services.export_dirs.remember(&directory);
        let PendingExport { edits, signatures } = pending;
        jobs::export_tracks(
            self.services.jobs.clone(),
            self.services.engine.clone(),
            self.services.events.clone(),
            edits,
            signatures,
            directory,
        );
    }

    fn shortcut(&mut self, shortcut: Shortcut) {
        match shortcut {
            Shortcut::TogglePlayback => {
                if self.session.selected_track.is_some() && !self.ui.settings_open {
                    self.toggle_playback();
                }
            }
            Shortcut::Back => {
                if self.session.selected_track.is_some() {
                    let next = (self.session.playhead - 5.0).max(0.0);
                    let playing = self.session.playback.is_some();
                    self.stop_playback(false);
                    self.session.playhead = next;
                    if playing {
                        self.start_playback(next);
                    }
                }
            }
            Shortcut::Forward => {
                if let Some(duration) = self.session.selected_track().map(Track::duration) {
                    let next = (self.session.playhead + 5.0).min(duration);
                    let playing = self.session.playback.is_some();
                    self.stop_playback(false);
                    self.session.playhead = next;
                    if playing {
                        self.start_playback(next);
                    }
                }
            }
            Shortcut::Blade => {
                let _ = self.update(Message::SetBlade(true));
            }
            Shortcut::DirectEdit => {
                let _ = self.update(Message::SetBlade(false));
            }
            Shortcut::Settings => {
                let _ = self.update(Message::OpenSettings);
            }
            Shortcut::Import => {
                let _ = self.update(Message::ImportPressed);
            }
            Shortcut::DeleteSelection => {
                if !self.ui.settings_open {
                    let _ = self.update(Message::DeleteRange);
                }
            }
            Shortcut::AddSelected => {
                if let Some(id) = self.session.selected_media {
                    self.add_to_timeline(id);
                }
            }
            Shortcut::JumpStart => {
                let _ = self.update(Message::JumpToStart);
            }
            Shortcut::JumpEnd => {
                let _ = self.update(Message::JumpToEnd);
            }
            Shortcut::ZoomIn => {
                let _ = self.update(Message::ZoomSelected(true));
            }
            Shortcut::ZoomOut => {
                let _ = self.update(Message::ZoomSelected(false));
            }
        }
    }
}

/// Formats the current wall clock as `HH:MM` without pulling in a date crate.
fn chrono_like_clock() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let hours = (secs / 3600) % 24;
    let minutes = (secs / 60) % 60;
    format!("{hours:02}:{minutes:02}")
}

/// Builds an export signature for the current track state.
pub fn track_signature(track: &Track) -> String {
    track.signature()
}

#[cfg(test)]
mod tests;
