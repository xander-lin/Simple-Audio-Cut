export interface Region {
  start: number;
  end: number;
}

export interface EnvelopePoint {
  id: string;
  time: number;
  gain: number;
}

export function envelopeGainAtTime(points: EnvelopePoint[], duration: number, time: number) {
  if (points.length === 0 || duration <= 0) return 1;
  const anchors = points
    .map((point) => ({ time: Math.max(0, Math.min(duration, point.time)), gain: Math.max(0, Math.min(2, point.gain)) }))
    .sort((left, right) => left.time - right.time);
  if (anchors[0].time > 0) anchors.unshift({ time: 0, gain: 1 });
  if (anchors[anchors.length - 1].time < duration) anchors.push({ time: duration, gain: 1 });

  for (let index = 1; index < anchors.length; index++) {
    const left = anchors[index - 1];
    const right = anchors[index];
    if (time > right.time && index < anchors.length - 1) continue;
    if (right.time <= left.time) return right.gain;
    const progress = Math.max(0, Math.min(1, (time - left.time) / (right.time - left.time)));
    const smooth = progress * progress * (3 - 2 * progress);
    return left.gain + (right.gain - left.gain) * smooth;
  }
  return 1;
}

/**
 * Merges a list of regions with a new region.
 * Overlapping adjacent regions are combined.
 */
export function mergeRegions(regions: Region[], newRegion: Region): Region[] {
  // 1. Add new region and sort by start time
  const sorted = [...regions, newRegion].map((region) => ({ ...region })).sort((a, b) => a.start - b.start);

  if (sorted.length === 0) return [];

  // 2. Merge overlapping intervals
  const merged: Region[] = [sorted[0]];

  for (let i = 1; i < sorted.length; i++) {
    const current = sorted[i];
    const last = merged[merged.length - 1];

    if (current.start <= last.end) {
      // Overlap or adjacency: extend the last region
      last.end = Math.max(last.end, current.end);
    } else {
      // No overlap: add as new region
      merged.push(current);
    }
  }

  return merged;
}

export function combineRegions(...groups: Region[][]): Region[] {
  return groups.flat().reduce<Region[]>((regions, region) => mergeRegions(regions, region), []);
}

export function regionIsCovered(regions: Region[], target: Region) {
  if (target.end <= target.start) return false;
  return combineRegions(regions).some((region) => region.start <= target.start && region.end >= target.end);
}

/**
 * Removes a region from a list of regions.
 * Can split existing regions or shorten them.
 */
export function subtractRegion(regions: Region[], subtract: Region): Region[] {
  const result: Region[] = [];

  for (const r of regions) {
    // Case 1: No overlap (Subtract region is fully before or after)
    if (subtract.end <= r.start || subtract.start >= r.end) {
      result.push(r);
      continue;
    }

    // Case 2: Overlap
    
    // Left part remains?
    if (r.start < subtract.start) {
      result.push({ start: r.start, end: subtract.start });
    }

    // Right part remains?
    if (r.end > subtract.end) {
      result.push({ start: subtract.end, end: r.end });
    }
  }

  return result;
}
