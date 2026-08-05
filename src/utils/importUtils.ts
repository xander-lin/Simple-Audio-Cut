export function selectedPaths(selection: string | string[] | null) {
  if (!selection) return [];
  return Array.isArray(selection) ? selection : [selection];
}

export function fileBasename(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export function fileStem(path: string) {
  const basename = fileBasename(path);
  const dotIndex = basename.lastIndexOf(".");
  return dotIndex > 0 ? basename.slice(0, dotIndex) : basename;
}

export function importSummary(imported: number, failed: number, firstFailure?: { path: string; error: string }) {
  const failureDetail = firstFailure ? ` First failure: ${fileBasename(firstFailure.path)} - ${firstFailure.error}` : "";
  if (failed === 0) {
    return imported === 1
      ? "Imported audio is ready. Drag it into the editor."
      : `Imported ${imported} audio files. Drag them into the editor.`;
  }
  if (imported === 0) return `Import failed for ${failed} file${failed === 1 ? "" : "s"}.${failureDetail}`;
  return `Imported ${imported} audio file${imported === 1 ? "" : "s"}; ${failed} failed.${failureDetail}`;
}

export function importQueuedSummary(count: number) {
  return count === 1
    ? "Import queued. Normalizing loudness before editing."
    : `Queued ${count} imports. Normalizing loudness before editing.`;
}
