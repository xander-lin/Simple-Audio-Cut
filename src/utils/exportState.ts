interface Region {
  start: number;
  end: number;
}

interface EnvelopePoint {
  id: string;
  time: number;
  gain: number;
}

interface ExportDirectoryStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const LAST_EXPORT_DIRECTORY_KEY = "simple-audio-cut:last-export-directory";

function defaultExportDirectoryStorage(): ExportDirectoryStorage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

export interface ExportableTrack {
  name: string;
  path: string;
  manualDeletedRegions: Region[];
  silenceRegions: Region[];
  envelopePoints: EnvelopePoint[];
  lastExportSignature: string | null;
}

export function exportSignature(track: Omit<ExportableTrack, "lastExportSignature">) {
  const deletedRegions = [...track.manualDeletedRegions, ...track.silenceRegions]
    .sort((left, right) => left.start - right.start)
    .reduce<Region[]>((regions, region) => {
      const previous = regions[regions.length - 1];
      if (previous && region.start <= previous.end) {
        previous.end = Math.max(previous.end, region.end);
      } else {
        regions.push({ ...region });
      }
      return regions;
    }, []);
  return JSON.stringify({
    name: track.name,
    path: track.path,
    deletedRegions,
    envelopePoints: [...track.envelopePoints]
      .sort((left, right) => left.time - right.time)
      .map(({ time, gain }) => ({ time, gain })),
  });
}

export function needsExport(track: ExportableTrack) {
  return track.lastExportSignature !== exportSignature(track);
}

export function getLastExportDirectory(storage = defaultExportDirectoryStorage()) {
  try {
    return storage?.getItem(LAST_EXPORT_DIRECTORY_KEY)?.trim() || null;
  } catch {
    return null;
  }
}

export function rememberLastExportDirectory(directory: string, storage = defaultExportDirectoryStorage()) {
  const trimmedDirectory = directory.trim();
  if (!trimmedDirectory) return;
  try {
    storage?.setItem(LAST_EXPORT_DIRECTORY_KEY, trimmedDirectory);
  } catch {
    // Export still succeeds if browser storage is unavailable.
  }
}
