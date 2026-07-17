import type { Region } from "./regionUtils";

export interface SilenceDetectionOptions {
  threshold?: number;
  minDuration?: number;
  padding?: number;
  windowDuration?: number;
  hopDuration?: number;
}

export interface RmsFrame {
  time: number;
  amplitude: number;
}

export interface RmsAnalysisOptions {
  windowDuration?: number;
  hopDuration?: number;
}

export const DEFAULT_RMS_WINDOW_DURATION = 0.005;
export const DEFAULT_RMS_HOP_DURATION = 0.0025;

const rmsCache = new WeakMap<AudioBuffer, Map<string, RmsFrame[]>>();

export function dbfsToAmplitude(dbfs: number) {
  return 10 ** (dbfs / 20);
}

export function amplitudeToDbfs(amplitude: number, floorDb = -70) {
  if (amplitude <= 0 || !Number.isFinite(amplitude)) return floorDb;
  return Math.max(floorDb, 20 * Math.log10(amplitude));
}

export function analyzeRms(
  buffer: AudioBuffer,
  options: RmsAnalysisOptions = {},
): RmsFrame[] {
  const windowDuration = options.windowDuration ?? DEFAULT_RMS_WINDOW_DURATION;
  const hopDuration = options.hopDuration ?? DEFAULT_RMS_HOP_DURATION;
  const cacheKey = `${windowDuration}:${hopDuration}`;
  let analyses = rmsCache.get(buffer);
  if (!analyses) {
    analyses = new Map();
    rmsCache.set(buffer, analyses);
  }
  const cached = analyses.get(cacheKey);
  if (cached) return cached;

  const channels = Array.from(
    { length: buffer.numberOfChannels },
    (_, channel) => buffer.getChannelData(channel),
  );
  const frames = analyzeRmsFromChannels(channels, buffer.sampleRate, {
    windowDuration,
    hopDuration,
  });
  analyses.set(cacheKey, frames);
  return frames;
}

export function analyzeRmsFromChannels(
  channels: readonly Float32Array[],
  sampleRate: number,
  options: RmsAnalysisOptions = {},
): RmsFrame[] {
  if (channels.length === 0 || !Number.isFinite(sampleRate) || sampleRate <= 0) return [];

  const frameCount = Math.min(...channels.map((channel) => channel.length));
  if (frameCount === 0) return [];

  const windowFrames = Math.max(1, Math.round(sampleRate * (options.windowDuration ?? DEFAULT_RMS_WINDOW_DURATION)));
  const hopFrames = Math.max(1, Math.round(sampleRate * (options.hopDuration ?? DEFAULT_RMS_HOP_DURATION)));
  const frames: RmsFrame[] = [];

  for (let frameStart = 0; frameStart < frameCount; frameStart += hopFrames) {
    const frameEnd = Math.min(frameCount, frameStart + windowFrames);
    const samplesInWindow = frameEnd - frameStart;
    let loudestChannelRms = 0;

    for (const channel of channels) {
      let sumSquares = 0;
      for (let frame = frameStart; frame < frameEnd; frame++) {
        const sample = channel[frame];
        sumSquares += sample * sample;
      }
      loudestChannelRms = Math.max(loudestChannelRms, Math.sqrt(sumSquares / samplesInWindow));
    }

    frames.push({
      time: (frameStart + samplesInWindow / 2) / sampleRate,
      amplitude: loudestChannelRms,
    });
  }

  return frames;
}

export function detectSilence(
  buffer: AudioBuffer,
  options: SilenceDetectionOptions = {},
): Region[] {
  return detectSilenceFromAnalysis(
    analyzeRms(buffer, options),
    buffer.duration,
    options,
  );
}

export function detectSilenceFromChannels(
  channels: readonly Float32Array[],
  sampleRate: number,
  options: SilenceDetectionOptions = {},
): Region[] {
  if (channels.length === 0 || !Number.isFinite(sampleRate) || sampleRate <= 0) return [];
  const frameCount = Math.min(...channels.map((channel) => channel.length));
  return detectSilenceFromAnalysis(
    analyzeRmsFromChannels(channels, sampleRate, options),
    frameCount / sampleRate,
    options,
  );
}

function detectSilenceFromAnalysis(
  frames: readonly RmsFrame[],
  duration: number,
  options: SilenceDetectionOptions,
): Region[] {
  if (frames.length === 0 || duration <= 0) return [];
  const threshold = Math.max(0, options.threshold ?? 0.015);
  const minDuration = Math.max(0, options.minDuration ?? 0.2);
  const padding = Math.max(0, options.padding ?? 0);
  const regions: Region[] = [];
  let silenceStart: number | null = null;
  let previousFrame: RmsFrame | null = null;

  const thresholdCrossing = (left: RmsFrame, right: RmsFrame) => {
    const amplitudeDelta = right.amplitude - left.amplitude;
    if (Math.abs(amplitudeDelta) < Number.EPSILON) return (left.time + right.time) / 2;
    const progress = Math.max(0, Math.min(1, (threshold - left.amplitude) / amplitudeDelta));
    return left.time + (right.time - left.time) * progress;
  };

  const finishSilence = (end: number) => {
    if (silenceStart === null) return;
    if (end - silenceStart >= minDuration) {
      const start = silenceStart + padding;
      const paddedEnd = end - padding;
      if (paddedEnd > start) regions.push({ start, end: paddedEnd });
    }
    silenceStart = null;
  };

  for (const frame of frames) {
    if (frame.amplitude < threshold) {
      if (silenceStart === null) {
        silenceStart = previousFrame && previousFrame.amplitude >= threshold
          ? thresholdCrossing(previousFrame, frame)
          : 0;
      }
    } else {
      const end = previousFrame && previousFrame.amplitude < threshold
        ? thresholdCrossing(previousFrame, frame)
        : frame.time;
      finishSilence(end);
    }
    previousFrame = frame;
  }

  finishSilence(duration);
  return regions;
}
