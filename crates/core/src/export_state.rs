//! Incremental export bookkeeping: "Export changed" only exports tracks whose
//! output-affecting edits differ from their last successful export.

use crate::region::{combine_regions, EnvelopePoint, Region};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq)]
pub struct ExportableTrack {
    pub name: String,
    pub path: String,
    pub deleted_regions: Vec<Region>,
    pub muted_regions: Vec<Region>,
    pub envelope_points: Vec<EnvelopePoint>,
}

#[derive(Serialize)]
struct SignaturePayload<'a> {
    name: &'a str,
    path: &'a str,
    #[serde(rename = "deletedRegions")]
    deleted_regions: Vec<Region>,
    #[serde(rename = "mutedRegions")]
    muted_regions: Vec<Region>,
    #[serde(rename = "envelopePoints")]
    envelope_points: Vec<SignaturePoint>,
}

#[derive(Serialize)]
struct SignaturePoint {
    time: f64,
    gain: f64,
}

/// A stable signature of everything that changes exported audio.
pub fn export_signature(track: &ExportableTrack) -> String {
    let deleted_regions = normalize_regions(&[&track.deleted_regions]);
    let muted_regions = normalize_regions(&[&track.muted_regions]);
    let mut envelope_points: Vec<&EnvelopePoint> = track.envelope_points.iter().collect();
    envelope_points.sort_by(|left, right| left.time.total_cmp(&right.time));
    let payload = SignaturePayload {
        name: &track.name,
        path: &track.path,
        deleted_regions,
        muted_regions,
        envelope_points: envelope_points
            .into_iter()
            .map(|point| SignaturePoint {
                time: point.time,
                gain: point.gain,
            })
            .collect(),
    };
    serde_json::to_string(&payload).unwrap_or_default()
}

pub fn needs_export(track: &ExportableTrack, last_export_signature: Option<&str>) -> bool {
    last_export_signature != Some(export_signature(track).as_str())
}

fn normalize_regions(groups: &[&[Region]]) -> Vec<Region> {
    combine_regions(groups)
}

/// Persistence contract for the last export directory.
pub trait ExportDirectoryStore: Send + Sync {
    fn last_directory(&self) -> Option<String>;
    fn remember(&self, directory: &str);
}

/// Stores the directory in a small file inside the application config folder.
pub struct FileExportDirectoryStore {
    path: PathBuf,
}

impl FileExportDirectoryStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ExportDirectoryStore for FileExportDirectoryStore {
    fn last_directory(&self) -> Option<String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn remember(&self, directory: &str) {
        let trimmed = directory.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&self.path, trimmed);
    }
}

/// In-memory store used by tests and by headless builds.
#[derive(Default)]
pub struct MemoryExportDirectoryStore {
    value: Mutex<Option<String>>,
}

impl MemoryExportDirectoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ExportDirectoryStore for MemoryExportDirectoryStore {
    fn last_directory(&self) -> Option<String> {
        self.value.lock().ok().and_then(|value| value.clone())
    }

    fn remember(&self, directory: &str) {
        let trimmed = directory.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Ok(mut value) = self.value.lock() {
            *value = Some(trimmed.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        export_signature, needs_export, ExportableTrack, MemoryExportDirectoryStore,
        ExportDirectoryStore,
    };
    use crate::region::{EnvelopePoint, Region};

    fn track() -> ExportableTrack {
        ExportableTrack {
            name: "voice".into(),
            path: "/tmp/voice.wav".into(),
            deleted_regions: vec![Region::new(1.0, 2.0)],
            muted_regions: Vec::new(),
            envelope_points: vec![EnvelopePoint::new("point-1", 3.0, 0.5)],
        }
    }

    #[test]
    fn only_tracks_changed_since_their_successful_export_need_batch_export() {
        let signature = export_signature(&track());
        assert!(needs_export(&track(), None));
        assert!(!needs_export(&track(), Some(&signature)));

        let mut renamed = track();
        renamed.name = "renamed".into();
        assert!(needs_export(&renamed, Some(&signature)));

        let mut extended = track();
        extended.deleted_regions.push(Region::new(4.0, 5.0));
        assert!(needs_export(&extended, Some(&signature)));
    }

    #[test]
    fn muted_ranges_are_output_affecting_edits() {
        let mut muted = track();
        muted.muted_regions = vec![Region::new(3.0, 4.0)];
        assert!(needs_export(&muted, Some(&export_signature(&track()))));
    }

    #[test]
    fn equivalent_muted_ranges_do_not_create_false_changes() {
        let mut split = track();
        split.muted_regions = vec![Region::new(4.0, 5.0), Region::new(3.0, 4.0)];
        let mut merged = track();
        merged.muted_regions = vec![Region::new(3.0, 5.0)];
        assert_eq!(export_signature(&split), export_signature(&merged));
    }

    #[test]
    fn envelope_point_identity_and_ordering_do_not_create_false_changes() {
        let first = ExportableTrack {
            envelope_points: vec![
                EnvelopePoint::new("later", 4.0, 0.8),
                EnvelopePoint::new("earlier", 2.0, 0.3),
            ],
            ..track()
        };
        let second = ExportableTrack {
            envelope_points: vec![
                EnvelopePoint::new("new-earlier-id", 2.0, 0.3),
                EnvelopePoint::new("new-later-id", 4.0, 0.8),
            ],
            ..track()
        };
        assert_eq!(export_signature(&first), export_signature(&second));
    }

    #[test]
    fn equivalent_deleted_regions_do_not_create_false_changes() {
        let mut split = track();
        split.deleted_regions = vec![
            Region::new(1.5, 2.0),
            Region::new(1.0, 1.5),
        ];
        assert_eq!(export_signature(&split), export_signature(&track()));
    }

    #[test]
    fn remembers_the_last_export_directory() {
        let store = MemoryExportDirectoryStore::new();
        assert_eq!(store.last_directory(), None);
        store.remember("  /tmp/exports ");
        assert_eq!(store.last_directory().as_deref(), Some("/tmp/exports"));
        store.remember("   ");
        assert_eq!(store.last_directory().as_deref(), Some("/tmp/exports"));
    }
}
