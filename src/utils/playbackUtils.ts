import type { EnvelopePoint, Region } from "./regionUtils";
import { originalTimeAtEditedOffset } from "./exportUtils.ts";

export interface PlaybackSourceSnapshot {
  buffer: AudioBuffer;
  manualDeletedRegions: Region[];
  silenceRegions: Region[];
  mutedRegions: Region[];
  envelopePoints: EnvelopePoint[];
}

export function playbackSourceMatches(
  candidate: PlaybackSourceSnapshot | undefined,
  snapshot: PlaybackSourceSnapshot,
) {
  return candidate !== undefined
    && candidate.buffer === snapshot.buffer
    && candidate.manualDeletedRegions === snapshot.manualDeletedRegions
    && candidate.silenceRegions === snapshot.silenceRegions
    && candidate.mutedRegions === snapshot.mutedRegions
    && candidate.envelopePoints === snapshot.envelopePoints;
}

interface PlaybackTiming {
  contextTime: number;
  startedAt: number;
  editedOffset: number;
  keptRegions: Region[];
}

export function savedPlaybackPosition(hasActiveSource: boolean, timing: PlaybackTiming) {
  if (!hasActiveSource) return null;
  const duration = timing.keptRegions.reduce(
    (total, region) => total + region.end - region.start,
    0,
  );
  const elapsed = Math.max(0, timing.contextTime - timing.startedAt);
  const editedTime = Math.max(0, Math.min(timing.editedOffset + elapsed, duration));
  return originalTimeAtEditedOffset(editedTime, timing.keptRegions);
}
