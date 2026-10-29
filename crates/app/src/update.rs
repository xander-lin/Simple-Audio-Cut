//! Message handling: the only place application state is mutated.

use std::sync::Arc;

use iced::{Element, Size, Task};
use simple_audio_cut_core::analysis::{
    dbfs_to_amplitude, detect_silence_from_analysis, SilenceOptions,
};
use simple_audio_cut_core::audio::ExportEdit;
use simple_audio_cut_core::direct_edit::DirectRegionOperation;
use simple_audio_cut_core::region::{
    merge_regions, region_is_covered, subtract_region, EnvelopePoint, Region,
};
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
    single: bool,
}

pub struct App {
    pub session: Session,
    pub ui: UiState,
    pub services: Services,
    pub window_size: Size,
    pending_export: Option<PendingExport>,
}

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        crate::ui::view(self)
    }

    pub fn boot(services: Services) -> (Self, Task<Message>) {
        let app = Self {
            session: Session::default(),
            ui: UiState::default(),
            services,
            window_size: Size::new(1100.0, 760.0),
            pending_export: None,
        };
        let denoise = app.services.denoise.clone();
        let events = app.services.events.clone();
        jobs::provider_status(denoise, events);
        (app, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
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
            Message::RenameMedia(id, name) => {
                if let Some(clip) = self.session.editing_mut(id) {
                    clip.name = name;
                }
                if let Some(track) = self.session.track_mut(id) {
                    track.touch();
                }
            }
            Message::AddToTimeline(id) => self.add_to_timeline(id),
            Message::DeleteMedia(id) => self.delete_media(id),
            Message::RetryImport(id) => self.retry_import(id),
            Message::RemoveTrack(id) => self.remove_track(id),
            Message::ReturnTrackToPool(id) => self.return_track_to_pool(id),
            Message::ToggleCollapse(id) => self.toggle_collapse(id),
            Message::PlayPause => self.toggle_playback(),
            Message::Seek(time) => self.seek(time),
            Message::SelectRange(range) => self.set_selected_range(range),
            Message::MuteRange => self.toggle_mute_range(),
            Message::SetBlade(active) => {
                self.ui.blade_mode = active;
                self.ui.message = if active {
                    "Blade active. Click the waveform to add an edit point; press A to return."
                        .into()
                } else {
                    "Direct editing active. Drag the waveform to select a range.".into()
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
            Message::ContextMenu {
                track,
                kind,
                x,
                y,
            } => {
                self.select_track_for_interaction(track);
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
                jobs::provider_status(self.services.denoise.clone(), self.services.events.clone());
            }
            Message::CloseSettings => {
                self.ui.settings_open = false;
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
                        self.services.denoise.clone(),
                        self.services.events.clone(),
                        draft,
                    );
                }
            }
            Message::PickExecutable => jobs::choose_file(
                self.services.dialogs.clone(),
                self.services.events.clone(),
                FileRequest::PythonExecutable,
                "Choose the Python executable from your ClearVoice environment",
            ),
            Message::PickModelRoot => jobs::choose_folder(
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
            Message::Noop => {}
        }
        Task::none()
    }

    fn tick(&mut self) {
        for event in self.services.events.drain() {
            self.apply_event(event);
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
            AppEvent::RecordingStopped { placeholder, result } => {
                self.ui.recording_busy = false;
                self.session.recording = None;
                self.session.recording_seconds = 0.0;
                match result {
                    Ok(decoded) => {
                        let id = placeholder;
                        let name = decoded.info.name.clone();
                        self.apply_decoded(
                            id,
                            decoded.info.path.clone(),
                            decoded.audio,
                            decoded.rms,
                            decoded.info.integrated_lufs,
                        );
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
                self.apply_decoded(
                    id,
                    info.path.clone(),
                    audio,
                    rms,
                    info.integrated_lufs,
                );
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
                    clip.denoise.status = crate::model::DenoiseStatus::Unavailable;
                }
                if let Some(reason) = reason {
                    let name = self
                        .session
                        .clip_by_id(id)
                        .map(|clip| clip.name.clone())
                        .unwrap_or_default();
                    self.ui.message = format!("{name}: {reason}");
                }
            }
            AppEvent::DenoiseStarted { id, task_id } => {
                self.session.mark_denoise(id, &task_id, crate::model::DenoiseStatus::Processing);
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
                }
                if was_selected {
                    self.refresh_silence_inputs();
                }
                let name = self
                    .session
                    .clip_by_id(id)
                    .map(|clip| clip.name.clone())
                    .unwrap_or_default();
                self.ui.message = format!("{name}: denoising complete.");
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
                }
                self.ui.message = error;
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
                self.session.failed_exports = failed_ids;
                for (id, signature) in signatures {
                    if results.iter().any(|result| {
                        result.recording_id == id.to_string() && result.error.is_none()
                    }) {
                        self.session.mark_exported(id, signature);
                    }
                }
                let single = self
                    .pending_export
                    .as_ref()
                    .map(|export| export.single)
                    .unwrap_or(false);
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
            }
            AppEvent::ExportFailed { error } => {
                self.session.exporting.clear();
                self.pending_export = None;
                self.ui.message = error;
            }
            AppEvent::SettingsStatus(result) => {
                self.ui.settings.busy = false;
                match result {
                    Ok(status) => {
                        self.ui.settings.draft = Some(status.configured.clone());
                        self.ui.settings.status = Some(status.clone());
                        self.session.denoise_status = Some(status);
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
                        self.ui.settings_open = false;
                        self.ui.message = if status.ready {
                            "Denoising setup saved. Queuing eligible media.".into()
                        } else {
                            status.summary.clone()
                        };
                        if status.ready {
                            self.requeue_denoise_candidates();
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
                    self.ui.message = "Export cancelled".into();
                }
            }
            AppEvent::DialogFailed { request, error } => {
                if request == FileRequest::ExportFolder {
                    self.session.exporting.clear();
                    self.pending_export = None;
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
            self.services.engine.clone(),
            self.services.events.clone(),
        );
    }

    fn import_audio(&mut self) {
        self.stop_playback(true);
        jobs::choose_import_files(
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
            self.session.media.push(MediaClip::placeholder(id, path.clone()));
        }
        self.ui.message = simple_audio_cut_core::import_names::import_queued_summary(paths.len());
        for (id, path) in ids.into_iter().zip(paths) {
            jobs::import_media(
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
            self.services.engine.clone(),
            self.services.events.clone(),
            id,
            source_path,
            self.session.target_lufs,
        );
    }

    fn queue_denoise(&mut self, id: MediaId) {
        let Some(clip) = self.session.clip_by_id(id) else {
            return;
        };
        if matches!(
            clip.denoise.status,
            crate::model::DenoiseStatus::Queued | crate::model::DenoiseStatus::Processing
        ) {
            return;
        }
        let Some(audio) = clip.audio.clone() else {
            return;
        };
        let path = clip.path.clone();
        let task_id = new_id().to_string();
        if let Some(clip) = self.session.editing_mut(id) {
            clip.denoise = crate::model::DenoiseState {
                status: crate::model::DenoiseStatus::Queued,
                task_id: task_id.clone(),
            };
        }
        if let Some(track) = self.session.track_mut(id) {
            track.touch();
        }
        jobs::denoise_media(
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

    fn requeue_denoise_candidates(&mut self) {
        let candidates: Vec<MediaId> = self
            .session
            .media
            .iter()
            .chain(self.session.tracks.iter().map(|track| &track.clip))
            .filter(|clip| clip.ready())
            .filter(|clip| {
                matches!(
                    clip.denoise.status,
                    crate::model::DenoiseStatus::Unavailable
                        | crate::model::DenoiseStatus::Failed
                )
            })
            .map(|clip| clip.id)
            .collect();
        for id in candidates {
            self.queue_denoise(id);
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

    fn select_track_for_interaction(&mut self, id: MediaId) {
        if self.session.selected_track == Some(id) {
            return;
        }
        self.stop_playback(false);
        self.session.selected_track = Some(id);
        self.session.selected_media = None;
        self.session.playhead = 0.0;
        self.refresh_silence_inputs();
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
            self.select_track_for_interaction(id);
        }
        if let Some(track) = self.session.track_mut(id) {
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
        let edited = create_edited_buffer(
            track.audio(),
            &deleted,
            &track.envelope,
            &track.muted,
        );
        if edited.is_empty() {
            self.ui.message = "All audio has been removed from this recording.".into();
            return;
        }
        let kept = kept_regions(&deleted, duration);
        let safe_offset = if offset >= duration { 0.0 } else { offset.max(0.0) };
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
                "Range selected: {}. Press M to mute or restore sound.",
                format_time_standard(range.duration())
            );
        }
    }

    fn toggle_mute_range(&mut self) {
        let Some((id, range, unmute)) = self.session.selected_track().and_then(|track| {
            let range = track.selected_range?;
            if track.collapsed {
                return None;
            }
            Some((track.id(), range, region_is_covered(&track.muted, range)))
        }) else {
            return;
        };
        self.stop_playback(false);
        if let Some(track) = self.session.track_mut(id) {
            track.muted = if unmute {
                subtract_region(&track.muted, range)
            } else {
                merge_regions(&track.muted, range)
            };
            track.selected_range = None;
            track.touch();
        }
        self.ui.message = format!(
            "{} {}",
            if unmute {
                "Restored sound to"
            } else {
                "Muted"
            },
            format_time_standard(range.duration())
        );
    }

    fn waveform_event(&mut self, id: MediaId, event: WaveformEvent) {
        let Some(track) = self.session.track(id) else {
            return;
        };
        let collapsed = track.collapsed;
        let kept = if collapsed {
            track.kept()
        } else {
            Vec::new()
        };
        let to_source = |time: f64| {
            if collapsed {
                original_time_at_edited_offset(time, &kept)
            } else {
                time
            }
        };
        self.select_track_for_interaction(id);
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
            WaveformEvent::Select(range) => self.set_selected_range(
                range.map(|region| Region::new(to_source(region.start), to_source(region.end))),
            ),
            WaveformEvent::DirectEdit { region, operation } => {
                self.stop_playback(false);
                let region = Region::new(to_source(region.start), to_source(region.end));
                if let Some(track) = self.session.track_mut(id) {
                    if track.collapsed {
                        return;
                    }
                    match operation {
                        DirectRegionOperation::Delete => {
                            track.manual_deleted =
                                merge_regions(&track.manual_deleted, region);
                        }
                        DirectRegionOperation::Restore => {
                            track.manual_deleted =
                                subtract_region(&track.manual_deleted, region);
                            track.silence_regions =
                                subtract_region(&track.silence_regions, region);
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
                    track.edit_points =
                        simple_audio_cut_core::direct_edit::add_edit_point(
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
                let point = EnvelopePoint::new(
                    point.id,
                    to_source(point.time),
                    point.gain,
                );
                if let Some(track) = self.session.track_mut(id) {
                    track.envelope.push(point);
                    track
                        .envelope
                        .sort_by(|left, right| left.time.total_cmp(&right.time));
                    track.touch();
                }
            }
            WaveformEvent::EnvelopeMove { id: point_id, time, gain } => {
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

    fn start_export(&mut self, ids: Vec<MediaId>, single: bool) {
        if !self.session.exporting.is_empty() {
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
        self.session
            .failed_exports
            .retain(|id| !signatures.iter().any(|(candidate, _)| candidate == id));
        self.pending_export = Some(PendingExport {
            edits,
            signatures,
            single,
        });
        self.ui.message = "Choose a folder for the exported audio.".into();
        jobs::choose_folder(
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
            return;
        };
        let Some(destination) = destination else {
            self.session.exporting.clear();
            self.ui.message = "Export cancelled".into();
            return;
        };
        let directory = destination.display().to_string();
        self.services.export_dirs.remember(&directory);
        let PendingExport {
            edits,
            signatures,
            single,
        } = pending;
        jobs::export_tracks(
            self.services.engine.clone(),
            self.services.events.clone(),
            edits,
            signatures,
            directory,
        );
        // Keep `single` for the completion message.
        self.pending_export = Some(PendingExport {
            edits: Vec::new(),
            signatures: Vec::new(),
            single,
        });
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
            Shortcut::MuteRange => {
                if !self.ui.settings_open {
                    self.toggle_mute_range();
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
mod tests {
    use super::App;
    use crate::message::Message;
    use crate::model::{MediaId, Track};
    use crate::services::Services;

    fn app() -> App {
        let (app, _) = App::boot(Services::mock());
        app
    }

    fn add_ready_media(app: &mut App, id: MediaId) {
        use simple_audio_cut_core::audio::RecordingInfo;
        app.session
            .media
            .push(crate::model::MediaClip::placeholder(id, "/tmp/voice.wav"));
        let audio = std::sync::Arc::new(simple_audio_cut_core::buffer::AudioBuffer::new(
            1_000,
            vec![vec![0.1; 1_000]],
        ));
        let rms = std::sync::Arc::new(simple_audio_cut_core::analysis::analyze_rms(
            &audio,
            Default::default(),
        ));
        let info = RecordingInfo {
            id: id.to_string(),
            name: "Voice".into(),
            path: "/tmp/voice.wav".into(),
            duration_seconds: 1.0,
            integrated_lufs: Some(-14.0),
        };
        app.services.events.emit(crate::events::AppEvent::ImportCompleted {
            id,
            info,
            audio,
            rms,
        });
        let _ = app.update(Message::Tick);
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
        assert_eq!(
            app.ui.message,
            "All editor tracks are already exported."
        );
    }
}
