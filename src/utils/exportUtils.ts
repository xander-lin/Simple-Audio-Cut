import { envelopeGainAtTime, type EnvelopePoint, type Region } from "./regionUtils.ts";
export function getKeptRegions(deletedRegions: Region[], totalDuration: number): Region[] {
  const sorted = [...deletedRegions].sort((a, b) => a.start - b.start);
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
): AudioBuffer {
  const keptRegions = getKeptRegions(deletedRegions, buffer.duration);
  const sampleRate = buffer.sampleRate;
  const numberOfChannels = buffer.numberOfChannels;

  // Calculate total duration in samples consistently
  let totalSamples = 0;
  for (const region of keptRegions) {
    const startSample = Math.floor(region.start * sampleRate);
    const endSample = Math.floor(region.end * sampleRate);
    totalSamples += (endSample - startSample);
  }

  // Create new buffer data
  const outputBuffer = new AudioBuffer({
    length: totalSamples,
    numberOfChannels: numberOfChannels,
    sampleRate: sampleRate,
  });

  for (let channel = 0; channel < numberOfChannels; channel++) {
    const outputData = outputBuffer.getChannelData(channel);
    const inputData = buffer.getChannelData(channel);
    let offset = 0;

    for (const region of keptRegions) {
      const startSample = Math.floor(region.start * sampleRate);
      const endSample = Math.floor(region.end * sampleRate);
      const length = endSample - startSample;
      
      // Safety check boundaries
      if (startSample < inputData.length) {
          // slice handles end > length automatically, but we want exact length match with offset logic
          // However, if inputData is shorter than expected endSample, we might have issues.
          // But startSample/endSample are derived from region which is constrained by duration.
          
      const chunk = inputData.slice(startSample, startSample + length);
      applyEnvelope(chunk, startSample, sampleRate, buffer.duration, envelopePoints);
          
          // Double check target fit
          if (offset + chunk.length <= outputData.length) {
      outputData.set(chunk, offset);
          } else {
             // If rounding error caused overflow, truncate
             // This effectively solves the RangeError
      const fitLength = outputData.length - offset;
      outputData.set(chunk.slice(0, fitLength), offset);
          }
      offset += chunk.length;
      }
    }
  }

  return outputBuffer;
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
