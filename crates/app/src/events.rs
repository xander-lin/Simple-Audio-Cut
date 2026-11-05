//! Background job results.
//!
//! Worker threads never touch application state directly. They publish
//! [`AppEvent`] values through an [`EventSink`], and the UI thread drains the
//! bus on its animation tick. This keeps every state mutation on one thread
//! and makes the workers replaceable in tests.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use simple_audio_cut_core::analysis::RmsFrame;
use simple_audio_cut_core::audio::{ExportResult, RecordingInfo};
use simple_audio_cut_core::buffer::AudioBuffer;
use simple_audio_cut_core::denoise::DenoiseProviderStatus;

use crate::model::MediaId;

/// Identifies which dialog a result belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileRequest {
    ImportMedia,
    ExportFolder,
    PythonExecutable,
    ModelRoot,
}

/// A finished recording or import, already decoded for the editor.
#[derive(Clone)]
pub struct DecodedClip {
    pub info: RecordingInfo,
    pub audio: Arc<AudioBuffer>,
    pub rms: Arc<Vec<RmsFrame>>,
}

impl std::fmt::Debug for DecodedClip {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DecodedClip")
            .field("info", &self.info)
            .field("duration", &self.audio.duration())
            .finish()
    }
}

#[derive(Clone, Debug)]
pub enum AppEvent {
    RecordingStarted,
    RecordingStartFailed(String),
    RecordingStopped {
        placeholder: MediaId,
        result: Result<DecodedClip, String>,
    },
    ImportNormalizing {
        id: MediaId,
    },
    ImportCompleted {
        id: MediaId,
        info: RecordingInfo,
        audio: Arc<AudioBuffer>,
        rms: Arc<Vec<RmsFrame>>,
    },
    ImportFailed {
        id: MediaId,
        error: String,
    },
    DenoiseUnavailable {
        id: MediaId,
        reason: Option<String>,
    },
    DenoiseStarted {
        id: MediaId,
        task_id: String,
    },
    DenoiseCompleted {
        id: MediaId,
        task_id: String,
        path: String,
        lufs: Option<f64>,
        audio: Arc<AudioBuffer>,
        rms: Arc<Vec<RmsFrame>>,
    },
    DenoiseFailed {
        id: MediaId,
        task_id: String,
        error: String,
    },
    /// Result of loading the pre-denoise audio when the user turns denoising
    /// off for a take that was already denoised.
    OriginalAudioRestored {
        id: MediaId,
        path: String,
        audio: Arc<AudioBuffer>,
        rms: Arc<Vec<RmsFrame>>,
    },
    OriginalAudioRestoreFailed {
        id: MediaId,
        error: String,
    },
    ExportCompleted {
        results: Vec<ExportResult>,
        signatures: Vec<(MediaId, String)>,
    },
    ExportFailed {
        error: String,
    },
    SettingsStatus(Result<DenoiseProviderStatus, String>),
    /// Result of an explicit "Check setup" run. Unlike [`AppEvent::SettingsStatus`]
    /// it must never replace a draft the user is editing.
    SettingsInspected(Result<DenoiseProviderStatus, String>),
    SettingsSaved(Result<DenoiseProviderStatus, String>),
    PathsChosen {
        request: FileRequest,
        paths: Vec<PathBuf>,
    },
    PathChoiceCancelled {
        request: FileRequest,
    },
    DialogFailed {
        request: FileRequest,
        error: String,
    },
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: AppEvent);

    /// Removes and returns every queued event. Queue-less sinks keep the
    /// default empty implementation.
    fn drain(&self) -> Vec<AppEvent> {
        Vec::new()
    }
}

/// Queue drained by the UI thread.
#[derive(Default)]
pub struct EventBus {
    queue: Mutex<VecDeque<AppEvent>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.queue
            .lock()
            .map(|queue| queue.is_empty())
            .unwrap_or(true)
    }
}

impl EventSink for EventBus {
    fn emit(&self, event: AppEvent) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push_back(event);
        }
    }

    fn drain(&self) -> Vec<AppEvent> {
        match self.queue.lock() {
            Ok(mut queue) => queue.drain(..).collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Event sink for tests: records every event instead of queueing for a UI.
#[derive(Default)]
pub struct RecordingEventSink {
    events: Mutex<Vec<AppEvent>>,
}

impl RecordingEventSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<AppEvent> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.events.lock().map(|events| events.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl EventSink for RecordingEventSink {
    fn emit(&self, event: AppEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }

    fn drain(&self) -> Vec<AppEvent> {
        match self.events.lock() {
            Ok(mut events) => events.drain(..).collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Shared handle that can be cloned into worker closures.
pub type SharedSink = Arc<dyn EventSink>;

#[cfg(test)]
mod tests {
    use super::{AppEvent, EventBus, EventSink, RecordingEventSink};
    use crate::model::new_id;

    #[test]
    fn bus_drains_events_in_order() {
        let bus = EventBus::new();
        let id = new_id();
        bus.emit(AppEvent::ImportFailed {
            id,
            error: "first".into(),
        });
        bus.emit(AppEvent::ImportFailed {
            id,
            error: "second".into(),
        });
        assert!(!bus.is_empty());
        let events = bus.drain();
        assert_eq!(events.len(), 2);
        assert!(bus.is_empty());
    }

    #[test]
    fn recording_sink_keeps_every_event_for_assertions() {
        let sink = RecordingEventSink::new();
        sink.emit(AppEvent::DenoiseUnavailable {
            id: 1,
            reason: Some("not configured".into()),
        });
        assert_eq!(sink.len(), 1);
    }
}
