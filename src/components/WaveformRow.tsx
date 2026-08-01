import React, { memo, useEffect, useRef, useState } from "react";
import {
  amplitudeToDbfs,
  type RmsFrame,
} from "../utils/audioAnalysis";
import { envelopeGainAtTime, type EnvelopePoint, type Region } from "../utils/regionUtils";
import { formatTimeCompact } from "../utils/timeUtils";

interface WaveformRowProps {
  buffer: AudioBuffer;
  startTime: number;
  endTime: number;
  width: number;
  height: number;
  leadingPadding: number;
  trailingPadding: number;
  currentTime: number;
  onSeek: (time: number) => void;
  regions: Region[];
  envelopePoints: EnvelopePoint[];
  rmsFrames: readonly RmsFrame[];
  silenceThresholdDb: number;
  showSilenceThreshold: boolean;
  onRegionAdd: (start: number, end: number) => void;
  onRegionRemove: (start: number, end: number) => void;
  onRegionDragStart: (time: number) => void;
  onRegionDragMove: (time: number) => void;
  onRegionDragEnd: (time: number) => void;
  onEnvelopePointAdd: (point: EnvelopePoint) => void;
  onEnvelopePointMove: (id: string, time: number, gain: number) => void;
  onEnvelopePointRemove: (id: string) => void;
}

function drawWaveform(
  context: CanvasRenderingContext2D,
  buffer: AudioBuffer,
  startTime: number,
  endTime: number,
  width: number,
  height: number,
  leadingPadding: number,
  trailingPadding: number,
  rmsFrames: readonly RmsFrame[],
  silenceThresholdDb: number,
  showSilenceThreshold: boolean,
) {
  context.fillStyle = "#1a211f";
  context.fillRect(0, 0, width, height);
  const timelineWidth = Math.max(1, width - leadingPadding - trailingPadding);
  context.strokeStyle = "#384540";
  context.beginPath();
  context.moveTo(leadingPadding, height / 2);
  context.lineTo(leadingPadding + timelineWidth, height / 2);
  context.stroke();

  const rmsRadius = (amplitude: number) => {
    const normalized = (amplitudeToDbfs(amplitude) + 70) / 70;
    return Math.max(0, Math.min(1, normalized)) * (height / 2 - 4);
  };
  const lowerBound = (time: number) => {
    let low = 0;
    let high = rmsFrames.length;
    while (low < high) {
      const middle = Math.floor((low + high) / 2);
      if (rmsFrames[middle].time < time) low = middle + 1;
      else high = middle;
    }
    return low;
  };
  const firstRmsIndex = Math.max(0, lowerBound(startTime) - 1);
  const lastRmsIndex = Math.min(rmsFrames.length, lowerBound(endTime) + 1);
  if (firstRmsIndex < lastRmsIndex) {
    const maximumLevels = new Float32Array(Math.max(1, Math.ceil(timelineWidth)));
    const minimumLevels = new Float32Array(maximumLevels.length);
    let frameIndex = firstRmsIndex;
    let previousAmplitude = rmsFrames[firstRmsIndex].amplitude;
    for (let x = 0; x < maximumLevels.length; x++) {
      const binEndTime = startTime + ((x + 1) / maximumLevels.length) * (endTime - startTime);
      let minimumAmplitude = Number.POSITIVE_INFINITY;
      let maximumAmplitude = 0;
      while (frameIndex < lastRmsIndex && rmsFrames[frameIndex].time < binEndTime) {
        minimumAmplitude = Math.min(minimumAmplitude, rmsFrames[frameIndex].amplitude);
        maximumAmplitude = Math.max(maximumAmplitude, rmsFrames[frameIndex].amplitude);
        previousAmplitude = rmsFrames[frameIndex].amplitude;
        frameIndex += 1;
      }
      if (!Number.isFinite(minimumAmplitude)) minimumAmplitude = previousAmplitude;
      if (maximumAmplitude === 0) maximumAmplitude = previousAmplitude;
      minimumLevels[x] = rmsRadius(minimumAmplitude);
      maximumLevels[x] = rmsRadius(maximumAmplitude);
    }

    context.fillStyle = "rgba(104, 147, 170, 0.24)";
    context.strokeStyle = "rgba(133, 177, 198, 0.72)";
    context.lineWidth = 1;
    context.beginPath();
    context.moveTo(leadingPadding, height / 2 - maximumLevels[0]);
    for (let x = 1; x < maximumLevels.length; x++) {
      context.lineTo(leadingPadding + x, height / 2 - maximumLevels[x]);
    }
    for (let x = maximumLevels.length - 1; x >= 0; x--) {
      context.lineTo(leadingPadding + x, height / 2 + maximumLevels[x]);
    }
    context.closePath();
    context.fill();
    context.stroke();

    context.strokeStyle = "rgba(181, 211, 220, 0.88)";
    context.beginPath();
    context.moveTo(leadingPadding, height / 2 - minimumLevels[0]);
    for (let x = 1; x < minimumLevels.length; x++) {
      context.lineTo(leadingPadding + x, height / 2 - minimumLevels[x]);
    }
    context.moveTo(leadingPadding, height / 2 + minimumLevels[0]);
    for (let x = 1; x < minimumLevels.length; x++) {
      context.lineTo(leadingPadding + x, height / 2 + minimumLevels[x]);
    }
    context.stroke();
  }

  if (showSilenceThreshold) {
    const thresholdRadius = rmsRadius(10 ** (silenceThresholdDb / 20));
    context.save();
    context.strokeStyle = "rgba(224, 190, 100, 0.82)";
    context.lineWidth = 1;
    context.setLineDash([5, 4]);
    context.beginPath();
    context.moveTo(leadingPadding, height / 2 - thresholdRadius);
    context.lineTo(leadingPadding + timelineWidth, height / 2 - thresholdRadius);
    context.moveTo(leadingPadding, height / 2 + thresholdRadius);
    context.lineTo(leadingPadding + timelineWidth, height / 2 + thresholdRadius);
    context.stroke();
    context.restore();
  }

  const first = Math.floor(startTime * buffer.sampleRate);
  const last = Math.floor(endTime * buffer.sampleRate);
  const sampleCount = Math.max(1, last - first);
  const amplitude = height / 2;
  context.strokeStyle = "#72b993";
  context.beginPath();
  for (let x = 0; x < timelineWidth; x++) {
    let min = 1;
    let max = -1;
    const binStart = first + Math.floor(x * sampleCount / timelineWidth);
    const binEnd = Math.max(binStart + 1, first + Math.floor((x + 1) * sampleCount / timelineWidth));
    for (let channelIndex = 0; channelIndex < buffer.numberOfChannels; channelIndex++) {
      const samples = buffer.getChannelData(channelIndex);
      for (let sample = binStart; sample < binEnd && sample < last && sample < samples.length; sample++) {
        min = Math.min(min, samples[sample]);
        max = Math.max(max, samples[sample]);
      }
    }
    if (max < min) min = max = 0;
    context.moveTo(leadingPadding + x, (1 + min) * amplitude);
    context.lineTo(leadingPadding + x, (1 + max) * amplitude);
  }
  context.stroke();
}

const WaveformCanvas = memo(({ buffer, startTime, endTime, width, height, leadingPadding, trailingPadding, rmsFrames, silenceThresholdDb, showSilenceThreshold }: Pick<WaveformRowProps, "buffer" | "startTime" | "endTime" | "width" | "height" | "leadingPadding" | "trailingPadding" | "rmsFrames" | "silenceThresholdDb" | "showSilenceThreshold">) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const context = canvas.getContext("2d", { alpha: false });
    if (!context) return;
    const ratio = window.devicePixelRatio || 1;
    canvas.width = width * ratio;
    canvas.height = height * ratio;
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    context.scale(ratio, ratio);
    drawWaveform(context, buffer, startTime, endTime, width, height, leadingPadding, trailingPadding, rmsFrames, silenceThresholdDb, showSilenceThreshold);
  }, [buffer, startTime, endTime, width, height, leadingPadding, trailingPadding, rmsFrames, silenceThresholdDb, showSilenceThreshold]);
  return <canvas ref={canvasRef} className="waveform-canvas" />;
});

type DragState =
  | { kind: "point"; point: EnvelopePoint; currentX: number; currentY: number }
  | null;

const WaveformRow = ({
  buffer, startTime, endTime, width, height, leadingPadding, trailingPadding, currentTime,
  onSeek, regions, envelopePoints, rmsFrames, silenceThresholdDb, showSilenceThreshold, onRegionDragStart, onRegionDragMove, onRegionDragEnd, onEnvelopePointAdd, onEnvelopePointMove, onEnvelopePointRemove,
}: WaveformRowProps) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<DragState>(null);
  const duration = endTime - startTime;
  const timelineWidth = Math.max(1, width - leadingPadding - trailingPadding);
  const timeAtX = (x: number) => startTime + (Math.max(0, Math.min(timelineWidth, x - leadingPadding)) / timelineWidth) * duration;
  const xAtTime = (time: number) => leadingPadding + ((time - startTime) / duration) * timelineWidth;
  const gainAtY = (y: number) => Math.max(0, Math.min(2, 2 * (1 - y / height)));
  const yAtGain = (gain: number) => height * (1 - gain / 2);
  const visiblePoints = envelopePoints.filter((point) => point.time >= startTime && point.time <= endTime);

  const coordinates = (event: React.MouseEvent<HTMLDivElement>) => {
    const rect = containerRef.current?.getBoundingClientRect();
    return {
      x: rect ? Math.max(0, Math.min(width, event.clientX - rect.left)) : 0,
      y: rect ? Math.max(0, Math.min(height, event.clientY - rect.top)) : height / 2,
    };
  };

  const handleMouseDown = (event: React.MouseEvent<HTMLDivElement>) => {
    event.preventDefault();
    const { x, y } = coordinates(event);
    if (event.button === 0) {
      onSeek(timeAtX(x));
      return;
    }
    if (event.button === 2) {
      const hit = visiblePoints.find((point) => Math.hypot(xAtTime(point.time) - x, yAtGain(point.gain) - y) <= 14);
      if (hit) {
        onEnvelopePointRemove(hit.id);
        return;
      }
      onRegionDragStart(timeAtX(x));
      return;
    }
    if (event.button === 1) {
      const hit = visiblePoints.find((point) => Math.hypot(xAtTime(point.time) - x, yAtGain(point.gain) - y) <= 14);
      if (hit) setDrag({ kind: "point", point: hit, currentX: x, currentY: y });
      else onEnvelopePointAdd({ id: crypto.randomUUID(), time: timeAtX(x), gain: gainAtY(y) });
    }
  };

  const handleMouseMove = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!drag) return;
    const { x, y } = coordinates(event);
    setDrag({ ...drag, currentX: x, currentY: y });
    onEnvelopePointMove(drag.point.id, timeAtX(x), gainAtY(y));
  };

  const finishDrag = () => {
    if (!drag) return;
    setDrag(null);
  };

  const moveRegionDrag = (event: React.MouseEvent<HTMLDivElement>) => {
    if (event.buttons & 2) onRegionDragMove(timeAtX(coordinates(event).x));
  };

  const finishRegionDrag = (event: React.MouseEvent<HTMLDivElement>) => {
    if (event.button !== 2) return;
    event.stopPropagation();
    onRegionDragEnd(timeAtX(coordinates(event).x));
  };

  const playhead = currentTime >= startTime && currentTime <= endTime ? xAtTime(currentTime) : null;
  const envelopeSampleCount = Math.max(16, Math.ceil(timelineWidth / 12)) + 1;
  const envelopePath = Array.from({ length: envelopeSampleCount }, (_, index) => {
    const time = startTime + duration * index / (envelopeSampleCount - 1);
    const gain = envelopeGainAtTime(envelopePoints, buffer.duration, time);
    return `${index === 0 ? "M" : "L"} ${xAtTime(time)} ${yAtGain(gain)}`;
  }).join(" ");

  return <div ref={containerRef} className="waveform-row-container" role="application" onMouseDown={handleMouseDown} onMouseMove={(event) => { handleMouseMove(event); moveRegionDrag(event); }} onMouseUp={(event) => { finishDrag(); finishRegionDrag(event); }} onMouseLeave={() => finishDrag()} onContextMenu={(event) => event.preventDefault()} style={{ width, height }}>
    <div className="waveform-row-clip">
      <span className="row-time-label">{formatTimeCompact(endTime)}</span>
      <WaveformCanvas buffer={buffer} startTime={startTime} endTime={endTime} width={width} height={height} leadingPadding={leadingPadding} trailingPadding={trailingPadding} rmsFrames={rmsFrames} silenceThresholdDb={silenceThresholdDb} showSilenceThreshold={showSilenceThreshold} />
      {regions.map((region) => {
        const start = Math.max(region.start, startTime);
        const end = Math.min(region.end, endTime);
        return start < end ? <span key={`${region.start}-${region.end}`} className="deleted-region" style={{ left: xAtTime(start), width: xAtTime(end) - xAtTime(start) }} /> : null;
      })}
      {envelopePoints.length > 0 && <svg className="fade-envelope" role="img" aria-label="Volume envelope" viewBox={`0 0 ${width} ${height}`}><path d={envelopePath} />{visiblePoints.map((point) => <circle key={point.id} cx={xAtTime(point.time)} cy={yAtGain(point.gain)} r="5" />)}</svg>}
      {playhead !== null && <span className="playhead" style={{ transform: `translateX(${playhead}px)` }} />}
    </div>
  </div>;
};

export default WaveformRow;
