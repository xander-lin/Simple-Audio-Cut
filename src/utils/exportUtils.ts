import { envelopeGainAtTime, type EnvelopePoint, type Region } from "./regionUtils.ts";
export function getKeptRegions(deletedRegions: Region[], totalDuration: number): Region[] {
  if (!Number.isFinite(totalDuration) || totalDuration <= 0) return [];
  const sorted = deletedRegions
    .filter((region) => Number.isFinite(region.start) && Number.isFinite(region.end))
    .map((region) => ({
      start: Math.max(0, Math.min(totalDuration, region.start)),
      end: Math.max(0, Math.min(totalDuration, region.end)),
    }))
    .filter((region) => region.end > region.start)
    .sort((left, right) => left.start - right.start);
  const kept: Region[] = [];
  let currentTime = 0;

  for (const region of sorted) {
    if (currentTime < region.start) {
      kept.push({ start: currentTime, end: region.start });
    }
    currentTime = Math.max(currentTime, region.end);
  }

  if (currentTime < totalDuration) {
    kept.push({ start: currentTime, end: totalDuration });
  }

  return kept;
}

export function editedOffsetAtOriginalTime(time: number, keptRegions: Region[]) {
  let offset = 0;
  for (const region of keptRegions) {
    if (time <= region.start) return offset;
    if (time < region.end) return offset + time - region.start;
    offset += region.end - region.start;
  }
  return offset;
}

export function originalTimeAtEditedOffset(offset: number, keptRegions: Region[]) {
  let remaining = offset;
  for (const region of keptRegions) {
    const duration = region.end - region.start;
    if (remaining <= duration) return region.start + remaining;
    remaining -= duration;
  }
  return keptRegions.length ? keptRegions[keptRegions.length - 1].end : 0;
}

export function createEditedBuffer(
  buffer: AudioBuffer,
  deletedRegions: Region[],
  envelopePoints: EnvelopePoint[] = [],
  mutedRegions: Region[] = [],
): AudioBuffer {
  const keptRegions = getKeptRegions(deletedRegions, buffer.duration);
  const sampleRate = buffer.sampleRate;
  const numberOfChannels = buffer.numberOfChannels;
  const sampleRanges = keptRegions.map((region) => ({
    start: Math.max(0, Math.min(buffer.length, Math.floor(region.start * sampleRate))),
    end: Math.max(0, Math.min(buffer.length, Math.floor(region.end * sampleRate))),
  }));
  const totalSamples = sampleRanges.reduce((total, region) => total + Math.max(0, region.end - region.start), 0);
  const outputBuffer = new AudioBuffer({
    length: totalSamples,
    numberOfChannels,
    sampleRate,
  });

  for (let channel = 0; channel < numberOfChannels; channel++) {
    const outputData = outputBuffer.getChannelData(channel);
    const inputData = buffer.getChannelData(channel);
    let offset = 0;

    for (const region of sampleRanges) {
      const chunk = inputData.slice(region.start, region.end);
      applyEnvelope(chunk, region.start, sampleRate, buffer.duration, envelopePoints);
      applyMutedRegions(chunk, region.start, sampleRate, buffer.duration, mutedRegions);
      outputData.set(chunk, offset);
      offset += chunk.length;
    }
  }

  return outputBuffer;
}

function applyMutedRegions(
  samples: Float32Array,
  sourceStartSample: number,
  sampleRate: number,
  duration: number,
  regions: Region[],
) {
  for (const region of regions) {
    if (!Number.isFinite(region.start) || !Number.isFinite(region.end)) continue;
    const start = Math.max(0, Math.min(duration, region.start));
    const end = Math.max(0, Math.min(duration, region.end));
    if (end <= start) continue;
    const localStart = Math.max(0, Math.floor(start * sampleRate) - sourceStartSample);
    const localEnd = Math.min(samples.length, Math.ceil(end * sampleRate) - sourceStartSample);
    if (localEnd > localStart) samples.fill(0, localStart, localEnd);
  }
}

function applyEnvelope(
  samples: Float32Array,
  sourceStartSample: number,
  sampleRate: number,
  duration: number,
  points: EnvelopePoint[],
) {
  if (points.length === 0) return;
  for (let index = 0; index < samples.length; index++) {
    const time = (sourceStartSample + index) / sampleRate;
    samples[index] *= envelopeGainAtTime(points, duration, time);
  }
}
