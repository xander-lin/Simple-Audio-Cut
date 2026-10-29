//! Application state: media pool, timeline tracks and transient UI state.
//!
//! Everything here is plain data so the model can be exercised in tests
//! without a window, a device or a GUI toolkit.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use simple_audio_cut_core::analysis::RmsFrame;
use simple_audio_cut_core::buffer::AudioBuffer;
use simple_audio_cut_core::denoise::{DenoiseAvailability, DenoiseConfig, DenoiseProviderStatus};
use simple_audio_cut_core::export_state::{export_signature, ExportableTrack};
use simple_audio_cut_core::region::{combine_regions, EnvelopePoint, Region};
use simple_audio_cut_core::timeline::{create_edited_buffer, edited_duration, kept_regions};

pub type MediaId = u64;

pub const DEFAULT_TARGET_LUFS: f64 = -14.0;
pub const DEFAULT_SILENCE_THRESHOLD_DB: f64 = -36.0;
pub const DEFAULT_MIN_SILENCE_MS: f64 = 200.0;
pub const DEFAULT_PIXELS_PER_SECOND: f32 = 48.0;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Creates a process-unique identifier for media and tasks.
pub fn new_id() -> MediaId {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    (stamp << 16) | (sequence & 0xffff)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DenoiseStatus {
    #[default]
    Unavailable,
    Queued,
    Processing,
    Complete,
    Failed,
}

impl DenoiseStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unavailable => "No denoise",
            Self::Queued => "Denoise queued",
            Self::Processing => "Denoising",
            Self::Complete => "ClearVoice",
            Self::Failed => "Denoise failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct DenoiseState {
    pub status: DenoiseStatus,
    pub task_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClipStatus {
    Normalizing,
    Ready,
    Failed(String),
}

/// A decoded (or still processing) piece of source media.
#[derive(Clone)]
pub struct MediaClip {
    pub id: MediaId,
    pub name: String,
    pub path: String,
    pub source_path: Option<String>,
    pub duration: f64,
    pub lufs: Option<f64>,
    pub audio: Option<Arc<AudioBuffer>>,
    pub rms: Option<Arc<Vec<RmsFrame>>>,
    pub status: ClipStatus,
    pub denoise: DenoiseState,
}

impl MediaClip {
    pub fn placeholder(id: MediaId, source_path: impl Into<String>) -> Self {
        let source_path = source_path.into();
        let name = simple_audio_cut_core::import_names::file_stem(&source_path);
        Self {
            id,
            name: if name.is_empty() {
                simple_audio_cut_core::import_names::file_basename(&source_path)
            } else {
                name
            },
            path: source_path.clone(),
            source_path: Some(source_path),
            duration: 0.0,
            lufs: None,
            audio: None,
            rms: None,
            status: ClipStatus::Normalizing,
            denoise: DenoiseState::default(),
        }
    }

    pub fn recording_placeholder(id: MediaId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            path: "recording.wav".into(),
            source_path: None,
            duration: 0.0,
            lufs: None,
            audio: None,
            rms: None,
            status: ClipStatus::Normalizing,
            denoise: DenoiseState::default(),
        }
    }

    pub fn ready(&self) -> bool {
        matches!(self.status, ClipStatus::Ready) && self.audio.is_some()
    }

    pub fn processing(&self) -> bool {
        matches!(self.status, ClipStatus::Normalizing)
            || matches!(
                self.denoise.status,
                DenoiseStatus::Queued | DenoiseStatus::Processing
            )
    }

    pub fn status_label(&self) -> &str {
        match &self.status {
            ClipStatus::Normalizing => "Normalizing",
            ClipStatus::Failed(_) => "Import failed",
            ClipStatus::Ready => self.denoise.status.label(),
        }
    }

    pub fn error(&self) -> Option<&str> {
        match &self.status {
            ClipStatus::Failed(error) => Some(error),
            _ => None,
        }
    }

    pub fn audio(&self) -> Option<&Arc<AudioBuffer>> {
        self.audio.as_ref()
    }

    pub fn rms(&self) -> Option<&Arc<Vec<RmsFrame>>> {
        self.rms.as_ref()
    }

    pub fn apply_audio(&mut self, path: String, duration: f64, lufs: Option<f64>, audio: Arc<AudioBuffer>, rms: Arc<Vec<RmsFrame>>) {
        self.path = path;
        self.duration = duration;
        self.lufs = lufs;
        self.audio = Some(audio);
        self.rms = Some(rms);
        self.status = ClipStatus::Ready;
    }
}

/// A timeline track: a media clip plus its non-destructive edit state.
#[derive(Clone)]
pub struct Track {
    pub clip: MediaClip,
    pub pixels_per_second: f32,
    pub manual_deleted: Vec<Region>,
    pub silence_regions: Vec<Region>,
    pub muted: Vec<Region>,
    pub silence_threshold_db: f64,
    pub min_silence_ms: f64,
    pub silence_detection: bool,
    pub envelope: Vec<EnvelopePoint>,
    pub edit_points: Vec<f64>,
    pub selected_range: Option<Region>,
    pub collapsed: bool,
    pub collapsed_audio: Option<Arc<AudioBuffer>>,
    pub last_export_signature: Option<String>,
    pub revision: u64,
}

impl Track {
    pub fn from_clip(clip: MediaClip) -> Self {
        Self {
            clip,
            pixels_per_second: DEFAULT_PIXELS_PER_SECOND,
            manual_deleted: Vec::new(),
            silence_regions: Vec::new(),
            muted: Vec::new(),
            silence_threshold_db: DEFAULT_SILENCE_THRESHOLD_DB,
            min_silence_ms: DEFAULT_MIN_SILENCE_MS,
            silence_detection: false,
            envelope: Vec::new(),
            edit_points: Vec::new(),
            selected_range: None,
            collapsed: false,
            collapsed_audio: None,
            last_export_signature: None,
            revision: 1,
        }
    }

    pub fn id(&self) -> MediaId {
        self.clip.id
    }

    pub fn audio(&self) -> &Arc<AudioBuffer> {
        self.clip
            .audio
            .as_ref()
            .expect("timeline tracks always hold decoded audio")
    }

    pub fn rms(&self) -> &Arc<Vec<RmsFrame>> {
        self.clip
            .rms
            .as_ref()
            .expect("timeline tracks always hold RMS analysis")
    }

    pub fn duration(&self) -> f64 {
        self.audio().duration()
    }

    pub fn deleted(&self) -> Vec<Region> {
        combine_regions(&[&self.manual_deleted, &self.silence_regions])
    }

    pub fn kept(&self) -> Vec<Region> {
        kept_regions(&self.deleted(), self.duration())
    }

    pub fn edited_duration(&self) -> f64 {
        edited_duration(&self.kept())
    }

    pub fn exportable(&self) -> ExportableTrack {
        ExportableTrack {
            name: self.clip.name.clone(),
            path: self.clip.path.clone(),
            deleted_regions: self.deleted(),
            muted_regions: self.muted.clone(),
            envelope_points: self.envelope.clone(),
        }
    }

    pub fn needs_export(&self) -> bool {
        simple_audio_cut_core::export_state::needs_export(
            &self.exportable(),
            self.last_export_signature.as_deref(),
        )
    }

    pub fn signature(&self) -> String {
        export_signature(&self.exportable())
    }

    pub fn mark_exported(&mut self) {
        self.last_export_signature = Some(self.signature());
    }

    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.collapsed_audio = None;
    }

    pub fn display_audio(&self) -> Arc<AudioBuffer> {
        if self.collapsed {
            if let Some(audio) = &self.collapsed_audio {
                return audio.clone();
            }
            self.audio().clone()
        } else {
            self.audio().clone()
        }
    }

    /// Recomputes the collapsed (deleted audio removed) view buffer.
    pub fn rebuild_collapsed(&mut self) {
        if !self.collapsed {
            self.collapsed_audio = None;
            return;
        }
        self.collapsed_audio = Some(Arc::new(create_edited_buffer(
            self.audio(),
            &self.deleted(),
            &[],
            &self.muted,
        )));
    }

    pub fn display_audio_cached(&self) -> Arc<AudioBuffer> {
        if self.collapsed {
            self.collapsed_audio
                .clone()
                .unwrap_or_else(|| self.audio().clone())
        } else {
            self.audio().clone()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMenuKind {
    Export,
    Actions,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenu {
    pub track_id: MediaId,
    pub kind: ContextMenuKind,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug)]
pub struct PlaybackState {
    pub track_id: MediaId,
    pub kept: Vec<Region>,
    pub duration: f64,
}

#[derive(Clone, Debug)]
pub struct ActiveRecording {
    pub placeholder: MediaId,
    pub started: Instant,
}

#[derive(Clone, Debug, Default)]
pub struct SettingsUi {
    pub status: Option<DenoiseProviderStatus>,
    pub draft: Option<DenoiseConfig>,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct UiState {
    pub message: String,
    pub settings_open: bool,
    pub settings: SettingsUi,
    pub context_menu: Option<ContextMenu>,
    pub confirm_quit: bool,
    pub blade_mode: bool,
    pub target_lufs_input: String,
    pub silence_threshold_input: String,
    pub min_silence_input: String,
    pub renaming: Option<MediaId>,
    /// True while the microphone is being opened or the take is being
    /// finalized; the record button is inert meanwhile.
    pub recording_busy: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            message: "Ready to record".into(),
            settings_open: false,
            settings: SettingsUi::default(),
            context_menu: None,
            confirm_quit: false,
            blade_mode: false,
            target_lufs_input: DEFAULT_TARGET_LUFS.to_string(),
            silence_threshold_input: DEFAULT_SILENCE_THRESHOLD_DB.to_string(),
            min_silence_input: DEFAULT_MIN_SILENCE_MS.to_string(),
            renaming: None,
            recording_busy: false,
        }
    }
}

pub struct Session {
    pub media: Vec<MediaClip>,
    pub tracks: Vec<Track>,
    pub selected_media: Option<MediaId>,
    pub selected_track: Option<MediaId>,
    pub target_lufs: f64,
    pub recording: Option<ActiveRecording>,
    pub recording_seconds: f64,
    pub playhead: f64,
    pub playback: Option<PlaybackState>,
    pub exporting: Vec<MediaId>,
    pub failed_exports: Vec<MediaId>,
    pub denoise_status: Option<DenoiseProviderStatus>,
    pub waveform_caches: HashMap<MediaId, crate::waveform::WaveformCache>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            media: Vec::new(),
            tracks: Vec::new(),
            selected_media: None,
            selected_track: None,
            target_lufs: DEFAULT_TARGET_LUFS,
            recording: None,
            recording_seconds: 0.0,
            playhead: 0.0,
            playback: None,
            exporting: Vec::new(),
            failed_exports: Vec::new(),
            denoise_status: None,
            waveform_caches: HashMap::new(),
        }
    }
}

impl Session {
    pub fn selected_track(&self) -> Option<&Track> {
        self.selected_track
            .and_then(|id| self.tracks.iter().find(|track| track.id() == id))
    }

    pub fn selected_track_mut(&mut self) -> Option<&mut Track> {
        let id = self.selected_track?;
        self.tracks.iter_mut().find(|track| track.id() == id)
    }

    pub fn track(&self, id: MediaId) -> Option<&Track> {
        self.tracks.iter().find(|track| track.id() == id)
    }

    pub fn track_mut(&mut self, id: MediaId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|track| track.id() == id)
    }

    pub fn media(&self, id: MediaId) -> Option<&MediaClip> {
        self.media.iter().find(|clip| clip.id == id)
    }

    pub fn media_mut(&mut self, id: MediaId) -> Option<&mut MediaClip> {
        self.media.iter_mut().find(|clip| clip.id == id)
    }

    /// Applies a change received for either a pool clip or a timeline track.
    pub fn clip_by_id(&self, id: MediaId) -> Option<&MediaClip> {
        self.media(id).or_else(|| self.track(id).map(|track| &track.clip))
    }

    pub fn editing_mut(&mut self, id: MediaId) -> Option<&mut MediaClip> {
        if let Some(clip) = self.media.iter_mut().find(|clip| clip.id == id) {
            return Some(clip);
        }
        self.tracks
            .iter_mut()
            .find(|track| track.id() == id)
            .map(|track| &mut track.clip)
    }

    pub fn remove_media(&mut self, id: MediaId) -> Option<MediaClip> {
        let index = self.media.iter().position(|clip| clip.id == id)?;
        let clip = self.media.remove(index);
        if self.selected_media == Some(id) {
            self.selected_media = None;
        }
        Some(clip)
    }

    pub fn remove_track(&mut self, id: MediaId) -> Option<Track> {
        let index = self.tracks.iter().position(|track| track.id() == id)?;
        let track = self.tracks.remove(index);
        self.waveform_caches.remove(&id);
        if self.selected_track == Some(id) {
            self.selected_track = self.tracks.first().map(|track| track.id());
            self.playhead = 0.0;
        }
        Some(track)
    }

    pub fn add_track(&mut self, track: Track) {
        let id = track.id();
        if self.tracks.iter().any(|existing| existing.id() == id) {
            if let Some(existing) = self.track_mut(id) {
                *existing = track;
            }
            self.selected_track = Some(id);
            return;
        }
        self.tracks.push(track);
        self.waveform_caches
            .entry(id)
            .or_default();
        self.selected_track = Some(id);
    }

    pub fn cache(&self, id: MediaId) -> Option<&crate::waveform::WaveformCache> {
        self.waveform_caches.get(&id)
    }

    pub fn pending_imports(&self) -> usize {
        self.media
            .iter()
            .chain(self.tracks.iter().map(|track| &track.clip))
            .filter(|clip| matches!(clip.status, ClipStatus::Normalizing))
            .count()
    }

    pub fn pending_denoise(&self) -> usize {
        self.media
            .iter()
            .chain(self.tracks.iter().map(|track| &track.clip))
            .filter(|clip| {
                matches!(
                    clip.denoise.status,
                    DenoiseStatus::Queued | DenoiseStatus::Processing
                )
            })
            .count()
    }

    pub fn changed_tracks(&self) -> Vec<MediaId> {
        self.tracks
            .iter()
            .filter(|track| track.needs_export())
            .map(|track| track.id())
            .collect()
    }

    pub fn has_unsaved_work(&self) -> bool {
        self.recording.is_some()
            || !self.exporting.is_empty()
            || self.pending_imports() > 0
            || self.pending_denoise() > 0
            || !self.changed_tracks().is_empty()
    }

    pub fn apply_denoise_availability(&mut self, id: MediaId, availability: &DenoiseAvailability) {
        if let Some(clip) = self.editing_mut(id) {
            if !availability.available {
                clip.denoise.status = DenoiseStatus::Unavailable;
            }
        }
    }

    pub fn mark_denoise(&mut self, id: MediaId, task_id: &str, status: DenoiseStatus) {
        if let Some(clip) = self.editing_mut(id) {
            if clip.denoise.task_id == task_id {
                clip.denoise.status = status;
            }
        }
    }

    pub fn mark_exported(&mut self, id: MediaId, signature: String) {
        if let Some(track) = self.track_mut(id) {
            track.last_export_signature = Some(signature);
        }
    }

    pub fn reset_playhead(&mut self) {
        self.playhead = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip_with_audio(id: MediaId, frames: usize) -> MediaClip {
        let mut clip = MediaClip::placeholder(id, "/tmp/voice.wav");
        clip.apply_audio(
            "/tmp/voice.wav".into(),
            frames as f64 / 1_000.0,
            Some(-14.0),
            Arc::new(AudioBuffer::new(1_000, vec![vec![0.0; frames]])),
            Arc::new(Vec::new()),
        );
        clip
    }

    #[test]
    fn new_media_has_no_output_affecting_edits_but_still_exports_once() {
        let track = Track::from_clip(clip_with_audio(1, 1_000));
        assert!(track.needs_export());
        assert_eq!(track.edited_duration(), 1.0);
    }

    #[test]
    fn marking_exported_clears_pending_state_until_the_next_edit() {
        let mut track = Track::from_clip(clip_with_audio(1, 1_000));
        track.mark_exported();
        assert!(!track.needs_export());

        track.muted.push(Region::new(0.1, 0.2));
        track.touch();
        assert!(track.needs_export());
    }

    #[test]
    fn removing_the_selected_track_selects_the_next_available_one() {
        let mut session = Session::default();
        session.add_track(Track::from_clip(clip_with_audio(1, 100)));
        session.add_track(Track::from_clip(clip_with_audio(2, 100)));
        session.selected_track = Some(2);
        session.remove_track(2);
        assert_eq!(session.selected_track, Some(1));
        session.remove_track(1);
        assert_eq!(session.selected_track, None);
    }
}
