import React, { memo, useEffect, useRef, useState } from "react";
import {
  amplitudeToDbfs,
  type RmsFrame,
} from "../utils/audioAnalysis";
import { envelopeGainAtTime, regionIsCovered, type EnvelopePoint, type Region } from "../utils/regionUtils";
import { formatTimeCompact } from "../utils/timeUtils";
import Icon from "./Icon";

interface WaveformRowProps {
  buffer: AudioBuffer;
  startTime: number;
  endTime: number;
  width: number;
  height: number;
  leadingPadding: number;
  trailingPadding: number;
  currentTime: number;
  bladeActive: boolean;
  showEnvelope: boolean;
  onSeek: (time: number) => void;
  regions: Region[];
  mutedRegions: Region[];
  editPoints: number[];
  selectedRange: Region | null;
  envelopePoints: EnvelopePoint[];
  rmsFrames: readonly RmsFrame[];
  silenceThresholdDb: number;
  showSilenceThreshold: boolean;
  onRegionDragStart: (time: number) => void;
  onRegionDragMove: (time: number) => void;
  onRegionDragEnd: (time: number) => void;
  onDirectEditDragStart: (time: number) => void;
  onDirectEditDragMove: (time: number) => void;
  onDirectEditDragEnd: (time: number) => void;
  onRegionRestore: (region: Region) => void;
  onRangeMuteToggle: () => void;
  onBlade: (time: number) => void;
  onEditPointRemove: (time: number) => void;
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

    context.strokeStyle = "rgba(215, 230, 223, 0.88)";
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
    context.strokeStyle = "rgba(215, 189, 88, 0.82)";
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
  context.strokeStyle = "#75ad93";
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

type DragState = { kind: "point"; point: EnvelopePoint; currentX: number; currentY: number } | null;

interface EnvelopeHover {
  x: number;
  y: number;
  gain: number;
}

const WaveformRow = ({
  buffer, startTime, endTime, width, height, leadingPadding, trailingPadding, currentTime,
  bladeActive, showEnvelope, onSeek, regions, mutedRegions, editPoints, selectedRange, envelopePoints,
  rmsFrames, silenceThresholdDb, showSilenceThreshold, onRegionDragStart, onRegionDragMove,
  onRegionDragEnd, onDirectEditDragStart, onDirectEditDragMove, onDirectEditDragEnd,
  onRegionRestore, onRangeMuteToggle, onBlade, onEditPointRemove,
  onEnvelopePointAdd, onEnvelopePointMove, onEnvelopePointRemove,
}: WaveformRowProps) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<DragState>(null);
  const [envelopeHover, setEnvelopeHover] = useState<EnvelopeHover | null>(null);
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
    const time = timeAtX(x);
    if (event.button === 2 && !bladeActive) {
      setEnvelopeHover(null);
      onDirectEditDragStart(time);
      return;
    }
    if (event.button !== 0) return;
    if (bladeActive) {
      onBlade(time);
      onSeek(time);
      return;
    }
    if (event.detail > 1) return;
    if (showEnvelope) {
      const hit = visiblePoints.find((point) => Math.hypot(xAtTime(point.time) - x, yAtGain(point.gain) - y) <= 14);
      if (hit && event.altKey) {
        onEnvelopePointRemove(hit.id);
      } else if (hit) {
        setDrag({ kind: "point", point: hit, currentX: x, currentY: y });
      } else {
        const lineGain = envelopeGainAtTime(envelopePoints, buffer.duration, time);
        if (Math.abs(yAtGain(lineGain) - y) <= 8) {
          const point = { id: crypto.randomUUID(), time, gain: lineGain };
          onEnvelopePointAdd(point);
          setDrag({ kind: "point", point, currentX: x, currentY: y });
          return;
        }
      }
      if (hit) return;
    }
    onRegionDragStart(time);
  };

  const handleMouseMove = (event: React.MouseEvent<HTMLDivElement>) => {
    const { x, y } = coordinates(event);
    if (!drag && !bladeActive && event.buttons & 2) {
      setEnvelopeHover(null);
      onDirectEditDragMove(timeAtX(x));
      return;
    }
    if (!drag) return;
    setDrag({ ...drag, currentX: x, currentY: y });
    onEnvelopePointMove(drag.point.id, timeAtX(x), gainAtY(y));
    setEnvelopeHover({ x, y, gain: gainAtY(y) });
  };

  const finishDrag = (event: React.MouseEvent<HTMLDivElement>) => {
    if (drag) {
      setDrag(null);
      return;
    }
    if (!bladeActive && event.button === 0) {
      onRegionDragEnd(timeAtX(coordinates(event).x));
    }
  };

  const moveRegionDrag = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!drag && !bladeActive && event.buttons & 1) {
      onRegionDragMove(timeAtX(coordinates(event).x));
    }
  };

  const updateEnvelopeHover = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!showEnvelope || bladeActive || drag || event.buttons !== 0) return;
    const { x } = coordinates(event);
    const gain = envelopeGainAtTime(envelopePoints, buffer.duration, timeAtX(x));
    const y = yAtGain(gain);
    setEnvelopeHover(Math.abs(coordinates(event).y - y) <= 12 ? { x, y, gain } : null);
  };

  const finishDirectEditDrag = (event: React.MouseEvent<HTMLDivElement>) => {
    if (event.button !== 2 || bladeActive) return;
    event.stopPropagation();
    onDirectEditDragEnd(timeAtX(coordinates(event).x));
  };

  const playhead = currentTime >= startTime && currentTime <= endTime ? xAtTime(currentTime) : null;
  const envelopeSampleCount = Math.max(16, Math.ceil(timelineWidth / 12)) + 1;
  const envelopePath = Array.from({ length: envelopeSampleCount }, (_, index) => {
    const time = startTime + duration * index / (envelopeSampleCount - 1);
    const gain = envelopeGainAtTime(envelopePoints, buffer.duration, time);
    return `${index === 0 ? "M" : "L"} ${xAtTime(time)} ${yAtGain(gain)}`;
  }).join(" ");

  const visibleSelection = selectedRange
    ? { start: Math.max(selectedRange.start, startTime), end: Math.min(selectedRange.end, endTime) }
    : null;
  const visibleEditPoints = editPoints.filter((time) => time >= startTime && time <= endTime);
  const selectionMuted = selectedRange ? regionIsCovered(mutedRegions, selectedRange) : false;
  const gainLabel = (gain: number) => gain <= 0.000_1 ? "−∞ dB" : `${20 * Math.log10(gain) >= 0 ? "+" : ""}${(20 * Math.log10(gain)).toFixed(1)} dB`;

  return <div ref={containerRef} className={`waveform-row-container ${bladeActive ? "is-blade-active" : "is-direct-edit"} ${envelopeHover ? "is-over-envelope" : ""}`} role="application" aria-label={`Audio waveform${bladeActive ? ", blade active" : ". Right-drag to delete; click the yellow loudness curve to add and drag a keyframe"}`} onMouseDown={handleMouseDown} onMouseMove={(event) => { handleMouseMove(event); moveRegionDrag(event); updateEnvelopeHover(event); }} onMouseUp={(event) => { finishDrag(event); finishDirectEditDrag(event); }} onMouseLeave={(event) => { setEnvelopeHover(null); if (drag) finishDrag(event); }} onContextMenu={(event) => event.preventDefault()} style={{ width, height }}>
    <div className="waveform-row-clip">
      <span className="row-time-label">{formatTimeCompact(endTime)}</span>
      <WaveformCanvas buffer={buffer} startTime={startTime} endTime={endTime} width={width} height={height} leadingPadding={leadingPadding} trailingPadding={trailingPadding} rmsFrames={rmsFrames} silenceThresholdDb={silenceThresholdDb} showSilenceThreshold={showSilenceThreshold} />
      {regions.map((region) => {
        const start = Math.max(region.start, startTime);
        const end = Math.min(region.end, endTime);
        return start < end ? <button type="button" key={`${region.start}-${region.end}`} className="deleted-region" title="Restore this removed audio" aria-label={`Restore removed audio from ${formatTimeCompact(region.start)} to ${formatTimeCompact(region.end)}`} style={{ left: xAtTime(start), width: xAtTime(end) - xAtTime(start) }} onMouseDown={(event) => { if (event.button === 0) event.stopPropagation(); }} onClick={(event) => { event.stopPropagation(); onRegionRestore(region); }}><Icon name="restore" size={12} /><span>Restore</span></button> : null;
      })}
      {mutedRegions.map((region) => {
        const start = Math.max(region.start, startTime);
        const end = Math.min(region.end, endTime);
        return start < end ? <span key={`muted-${region.start}-${region.end}`} className="muted-region" style={{ left: xAtTime(start), width: xAtTime(end) - xAtTime(start) }}>Muted</span> : null;
      })}
      {visibleSelection && visibleSelection.start < visibleSelection.end && <span className="selected-range" style={{ left: xAtTime(visibleSelection.start), width: xAtTime(visibleSelection.end) - xAtTime(visibleSelection.start) }} />}
      {selectedRange && selectedRange.start >= startTime && selectedRange.start <= endTime && <button type="button" className="range-process-action" aria-label={selectionMuted ? "Restore sound to selected audio" : "Mute selected audio"} style={{ left: xAtTime(selectedRange.start) + 5 }} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onRangeMuteToggle(); }} title={selectionMuted ? "Restore sound without changing duration" : "Replace this range with silence without changing duration"}><Icon name="volume" size={12} />{selectionMuted ? "Restore sound" : "Mute range"}</button>}
      {visibleEditPoints.map((time) => <button type="button" key={time} className="edit-point" style={{ transform: `translateX(${xAtTime(time)}px)` }} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onEditPointRemove(time); }} title="Remove edit point" aria-label={`Remove edit point at ${formatTimeCompact(time)}`} />)}
      {(showEnvelope || envelopePoints.length > 0) && <svg className={`loudness-envelope ${showEnvelope ? "is-editable" : ""}`} role="img" aria-label="Loudness envelope" viewBox={`0 0 ${width} ${height}`}><path d={envelopePath} />{visiblePoints.map((point) => <circle key={point.id} cx={xAtTime(point.time)} cy={yAtGain(point.gain)} r="5" />)}</svg>}
      {envelopeHover && <span className="envelope-value" style={{ left: Math.max(8, Math.min(width - 74, envelopeHover.x + 10)), top: Math.max(5, Math.min(height - 29, envelopeHover.y - 31)) }}>{gainLabel(envelopeHover.gain)}</span>}
      {playhead !== null && <span className="playhead" style={{ transform: `translateX(${playhead}px)` }} />}
    </div>
  </div>;
};

export default WaveformRow;
