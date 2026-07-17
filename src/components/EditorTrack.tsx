import { useMemo } from "react";
import WaveformScore from "./WaveformScore";
import {
  createEditedBuffer,
  editedOffsetAtOriginalTime,
  getKeptRegions,
  originalTimeAtEditedOffset,
} from "../utils/exportUtils";
import { analyzeRms } from "../utils/audioAnalysis";
import { type EnvelopePoint, type Region } from "../utils/regionUtils";

interface EditorTrackProps {
  name: string;
  buffer: AudioBuffer;
  selected: boolean;
  collapsed: boolean;
  currentTime: number;
  pixelsPerSecond: number;
  deletedRegions: Region[];
  envelopePoints: EnvelopePoint[];
  silenceThresholdDb: number;
  onSelect: () => void;
  onScale: (deltaY: number) => void;
  onSeek: (time: number) => void;
  onRegionAdd: (start: number, end: number) => void;
  onRegionRemove: (start: number, end: number) => void;
  onEnvelopePointAdd: (point: EnvelopePoint) => void;
  onEnvelopePointMove: (id: string, time: number, gain: number) => void;
  onEnvelopePointRemove: (id: string) => void;
}

export default function EditorTrack({
  name,
  buffer,
  selected,
  collapsed,
  currentTime,
  pixelsPerSecond,
  deletedRegions,
  envelopePoints,
  silenceThresholdDb,
  onSelect,
  onScale,
  onSeek,
  onRegionAdd,
  onRegionRemove,
  onEnvelopePointAdd,
  onEnvelopePointMove,
  onEnvelopePointRemove,
}: EditorTrackProps) {
  const keptRegions = useMemo(
    () => getKeptRegions(deletedRegions, buffer.duration),
    [buffer.duration, deletedRegions],
  );
  const displayBuffer = useMemo(
    () => collapsed ? createEditedBuffer(buffer, deletedRegions) : buffer,
    [buffer, collapsed, deletedRegions],
  );
  const displayPoints = useMemo(() => envelopePoints
    .filter((point) => !collapsed || keptRegions.some((region) => point.time >= region.start && point.time <= region.end))
    .map((point) => ({
      ...point,
      time: collapsed ? editedOffsetAtOriginalTime(point.time, keptRegions) : point.time,
    })), [collapsed, envelopePoints, keptRegions]);
  const rmsFrames = useMemo(() => analyzeRms(displayBuffer), [displayBuffer]);

  const toSourceTime = (time: number) => collapsed
    ? originalTimeAtEditedOffset(time, keptRegions)
    : time;

  return <article className={selected ? "editor-track is-selected" : "editor-track"} onMouseDownCapture={onSelect} onContextMenu={(event) => event.preventDefault()}>
    <div className="track-label"><strong>{name}</strong>{collapsed && <span>Collapsed</span>}</div>
    <WaveformScore
      buffer={displayBuffer}
      pixelsPerSecond={pixelsPerSecond}
      onScale={selected ? onScale : undefined}
      currentTime={selected ? (collapsed ? editedOffsetAtOriginalTime(currentTime, keptRegions) : currentTime) : -1}
      onSeek={(time) => onSeek(toSourceTime(time))}
      regions={collapsed ? [] : deletedRegions}
      envelopePoints={displayPoints}
      rmsFrames={rmsFrames}
      silenceThresholdDb={silenceThresholdDb}
      showSilenceThreshold={selected}
      onRegionAdd={(start, end) => { if (!collapsed) onRegionAdd(start, end); }}
      onRegionRemove={(start, end) => { if (!collapsed) onRegionRemove(start, end); }}
      onEnvelopePointAdd={(point) => onEnvelopePointAdd({ ...point, time: toSourceTime(point.time) })}
      onEnvelopePointMove={(id, time, gain) => onEnvelopePointMove(id, toSourceTime(time), gain)}
      onEnvelopePointRemove={onEnvelopePointRemove}
    />
  </article>;
}
