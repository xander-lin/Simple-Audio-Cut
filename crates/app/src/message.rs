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
    /// Enter: move the selected library item onto the timeline.
    AddSelected,
    /// Delete: cut the selected range out of the timeline.
    DeleteSelection,
    JumpStart,
    JumpEnd,
    ZoomIn,
    ZoomOut,
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
    RenameCommit,
    /// Escape: dismiss the innermost transient layer (rename, menu, drawer).
    Dismiss,
    AddToTimeline(MediaId),
    /// Starts dragging a library item.
    DragItem(MediaId),
    /// Released while hovering the timeline.
    DropItem,
    /// Released anywhere else: the drag is abandoned.
    DragCancel,
    /// Files dragged in from the file manager.
    FilesHovered,
    FilesHoverLeft,
    FilesDropped(Vec<std::path::PathBuf>),
    DeleteMedia(MediaId),
    RetryImport(MediaId),
    /// Turn denoising on or off for one take.
    SetDenoise {
        id: MediaId,
        enabled: bool,
    },
    RemoveTrack(MediaId),
    ReturnTrackToPool(MediaId),
    ToggleCollapse(MediaId),
    PlayPause,
    StopPlayback,
    Seek(f64),
    JumpToStart,
    JumpToEnd,
    SelectRange(Option<Region>),
    MuteRange,
    DeleteRange,
    ClearEnvelope,
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
    ZoomSelected(bool),
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
    /// Logical size of the monitor hosting the window, resolved once at boot.
    MonitorResolved(Option<iced::Size>),
}
