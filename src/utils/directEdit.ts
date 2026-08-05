export type DirectRegionOperation = "delete" | "restore";

export function directRegionEdit(start: number, current: number, minimumDuration = 0.01) {
  const delta = current - start;
  if (!Number.isFinite(delta) || Math.abs(delta) <= minimumDuration) return null;
  return {
    start: Math.min(start, current),
    end: Math.max(start, current),
    operation: (delta > 0 ? "delete" : "restore") as DirectRegionOperation,
  };
}

export function segmentAtTime(points: number[], time: number, duration: number) {
  if (!points.length || duration <= 0) return null;
  const cuts = [0, ...points.filter((point) => point > 0 && point < duration), duration]
    .sort((left, right) => left - right)
    .filter((point, index, sorted) => index === 0 || Math.abs(point - sorted[index - 1]) >= 0.01);
  const safeTime = Math.max(0, Math.min(duration, time));
  const endIndex = cuts.findIndex((point) => point > safeTime);
  const safeEndIndex = endIndex < 0 ? cuts.length - 1 : endIndex;
  return { start: cuts[Math.max(0, safeEndIndex - 1)], end: cuts[safeEndIndex] };
}

export function addEditPoint(points: number[], time: number, duration: number) {
  const safeTime = Math.max(0, Math.min(duration, time));
  if (safeTime <= 0.001 || safeTime >= duration - 0.001) return [...points];
  return [...points, safeTime]
    .sort((left, right) => left - right)
    .filter((point, index, sorted) => index === 0 || Math.abs(point - sorted[index - 1]) >= 0.01);
}
