//! Every interaction the UI can produce.

use simple_audio_cut_core::region::{EnvelopePoint, Region};

use crate::model::{ContextMenuKind, MediaId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    TogglePlayback,
    Back,
    Forward,
    MuteRange,
    Blade,
    DirectEdit,
    Settings,
    Import,
}

/// Canvas-level gestures, in display time (collapsed or original timeline).
#[derive(Clone, Debug, PartialEq)]
pub enum WaveformEvent {
    Seek(f64),
    Select(Option<Region>),
    DirectEdit {
        region: Region,
        operation: simple_audio_cut_core::direct_edit::DirectRegionOperation,
    },
    Restore(Region),
    Blade(f64),
    RemoveEditPoint(f64),
    EnvelopeAdd(EnvelopePoint),
    EnvelopeMove {
        id: String,
        time: f64,
        gain: f64,
    },
    EnvelopeRemove(String),
}

#[derive(Clone, Debug)]
pub enum Message {
    Tick,
    RecordToggle,
    TargetLufsInput(String),
    TargetLufsSubmit,
    ImportPressed,
    SelectMedia(MediaId),
    SelectTrack(MediaId),
    RenameMedia(MediaId, String),
    AddToTimeline(MediaId),
    DeleteMedia(MediaId),
    RetryImport(MediaId),
    RemoveTrack(MediaId),
    ReturnTrackToPool(MediaId),
    ToggleCollapse(MediaId),
    PlayPause,
    Seek(f64),
    SelectRange(Option<Region>),
    MuteRange,
    SetBlade(bool),
    Waveform {
        track: MediaId,
        event: WaveformEvent,
    },
    ScaleTrack(MediaId, f32),
    SilenceThresholdInput(String),
    MinSilenceInput(String),
    SilenceSettingsSubmit,
    DetectSilence,
    ClearSilence,
    ExportChanged,
    ExportTrack(MediaId),
    ContextMenu {
        track: MediaId,
        kind: ContextMenuKind,
        x: f32,
        y: f32,
    },
    CloseContextMenu,
    OpenSettings,
    CloseSettings,
    SettingsEnabled(bool),
    SettingsExecutable(String),
    SettingsModelRoot(String),
    SettingsDevice(String),
    SettingsHighModel(String),
    SettingsLowModel(String),
    SettingsCheck,
    SettingsSave,
    PickExecutable,
    PickModelRoot,
    CloseRequested,
    ConfirmQuit,
    CancelQuit,
    Shortcut(Shortcut),
    WindowResized(iced::Size),
    Noop,
}
