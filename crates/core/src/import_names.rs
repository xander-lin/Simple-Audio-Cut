//! File naming and status summaries for imports.

use std::path::Path;

pub fn file_basename(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(path)
        .to_string()
}

pub fn file_stem(path: &str) -> String {
    let basename = file_basename(path);
    match basename.rfind('.') {
        Some(index) if index > 0 => basename[..index].to_string(),
        _ => basename,
    }
}

pub fn display_name(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| file_basename(&path.display().to_string()))
}

pub fn import_summary(imported: usize, failed: usize, first_failure: Option<(&str, &str)>) -> String {
    let failure_detail = first_failure
        .map(|(path, error)| format!(" First failure: {} - {error}", file_basename(path)))
        .unwrap_or_default();
    if failed == 0 {
        return if imported == 1 {
            "Imported audio is ready. Drag it into the editor.".into()
        } else {
            format!("Imported {imported} audio files. Drag them into the editor.")
        };
    }
    if imported == 0 {
        return format!(
            "Import failed for {failed} file{}.{failure_detail}",
            if failed == 1 { "" } else { "s" }
        );
    }
    format!(
        "Imported {imported} audio file{}; {failed} failed.{failure_detail}",
        if imported == 1 { "" } else { "s" }
    )
}

pub fn import_queued_summary(count: usize) -> String {
    if count == 1 {
        "Import queued. Normalizing loudness before editing.".into()
    } else {
        format!("Queued {count} imports. Normalizing loudness before editing.")
    }
}

#[cfg(test)]
mod tests {
    use super::{file_basename, file_stem, import_queued_summary, import_summary};

    #[test]
    fn extracts_display_names_from_selected_paths() {
        assert_eq!(file_basename("/tmp/voice/take 1.wav"), "take 1.wav");
        assert_eq!(file_stem("/tmp/voice/take 1.wav"), "take 1");
        assert_eq!(file_stem("C:\\audio\\voice.mp3"), "voice");
        assert_eq!(file_stem("/tmp/.hidden"), ".hidden");
    }

    #[test]
    fn summarizes_multi_file_imports() {
        assert_eq!(
            import_summary(1, 0, None),
            "Imported audio is ready. Drag it into the editor."
        );
        assert_eq!(
            import_summary(2, 0, None),
            "Imported 2 audio files. Drag them into the editor."
        );
        assert_eq!(
            import_summary(0, 1, Some(("/tmp/a.mp4", "No audio stream"))),
            "Import failed for 1 file. First failure: a.mp4 - No audio stream"
        );
        assert_eq!(
            import_summary(2, 1, Some(("/tmp/a.mp4", "No audio stream"))),
            "Imported 2 audio files; 1 failed. First failure: a.mp4 - No audio stream"
        );
    }

    #[test]
    fn summarizes_queued_imports() {
        assert_eq!(
            import_queued_summary(1),
            "Import queued. Normalizing loudness before editing."
        );
        assert_eq!(
            import_queued_summary(3),
            "Queued 3 imports. Normalizing loudness before editing."
        );
    }
}
