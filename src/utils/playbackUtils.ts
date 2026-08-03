import type { Region } from "./regionUtils";
import { originalTimeAtEditedOffset } from "./exportUtils.ts";

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
