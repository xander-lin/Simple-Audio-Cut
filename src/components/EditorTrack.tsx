import { useEffect, useMemo, useRef } from "react";
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
  status: string;
  statusKind: "queued" | "processing" | "complete" | "failed" | "unavailable";
  exportState: "pending" | "exporting" | "exported" | "failed";
  buffer: AudioBuffer;
  selected: boolean;
  collapsed: boolean;
  currentTime: number;
  bladeActive: boolean;
  pixelsPerSecond: number;
  deletedRegions: Region[];
  mutedRegions: Region[];
  editPoints: number[];
  selectedRange: Region | null;
  envelopePoints: EnvelopePoint[];
  silenceThresholdDb: number;
  onSelect: () => void;
  onScale: (deltaY: number) => void;
  onScaleContextMenu: (x: number, y: number) => void;
  onSeek: (time: number) => void;
  onRangeSelect: (start: number, end: number) => void;
  onDirectRegionEdit: (start: number, end: number, operation: "delete" | "restore") => void;
  onRegionRestore: (region: Region) => void;
  onRangeMuteToggle: () => void;
  onBlade: (time: number) => void;
  onEditPointRemove: (time: number) => void;
  onEnvelopePointAdd: (point: EnvelopePoint) => void;
  onEnvelopePointMove: (id: string, time: number, gain: number) => void;
  onEnvelopePointRemove: (id: string) => void;
}

export default function EditorTrack({
  name,
  status,
  statusKind,
  exportState,
  buffer,
  selected,
  collapsed,
  currentTime,
  bladeActive,
  pixelsPerSecond,
  deletedRegions,
  mutedRegions,
  editPoints,
  selectedRange,
  envelopePoints,
  silenceThresholdDb,
  onSelect,
  onScale,
  onScaleContextMenu,
  onSeek,
  onRangeSelect,
  onDirectRegionEdit,
  onRegionRestore,
  onRangeMuteToggle,
  onBlade,
  onEditPointRemove,
  onEnvelopePointAdd,
  onEnvelopePointMove,
  onEnvelopePointRemove,
}: EditorTrackProps) {
  const scaleHandleRef = useRef<HTMLButtonElement>(null);
  const onScaleRef = useRef(onScale);
  const onSelectRef = useRef(onSelect);
  onScaleRef.current = onScale;
  onSelectRef.current = onSelect;

  useEffect(() => {
    const scaleHandle = scaleHandleRef.current;
    if (!scaleHandle) return;
    const scaleTrack = (event: WheelEvent) => {
      if (event.deltaY === 0) return;
      event.preventDefault();
      event.stopPropagation();
      onSelectRef.current();
      onScaleRef.current(event.deltaY);
    };
    scaleHandle.addEventListener("wheel", scaleTrack, { passive: false });
    return () => scaleHandle.removeEventListener("wheel", scaleTrack);
  }, []);

  const keptRegions = useMemo(
    () => getKeptRegions(deletedRegions, buffer.duration),
    [buffer.duration, deletedRegions],
  );
  const displayBuffer = useMemo(
    () => collapsed ? createEditedBuffer(buffer, deletedRegions, [], mutedRegions) : buffer,
    [buffer, collapsed, deletedRegions, mutedRegions],
  );
  const displayPoints = useMemo(() => envelopePoints
    .filter((point) => !collapsed || keptRegions.some((region) => point.time >= region.start && point.time <= region.end))
    .map((point) => ({
      ...point,
      time: collapsed ? editedOffsetAtOriginalTime(point.time, keptRegions) : point.time,
    })), [collapsed, envelopePoints, keptRegions]);
  const displayEditPoints = useMemo(() => editPoints
    .filter((time) => !collapsed || keptRegions.some((region) => time >= region.start && time <= region.end))
    .map((time) => collapsed ? editedOffsetAtOriginalTime(time, keptRegions) : time), [collapsed, editPoints, keptRegions]);
  const rmsFrames = useMemo(() => analyzeRms(displayBuffer), [displayBuffer]);

  const toSourceTime = (time: number) => collapsed
    ? originalTimeAtEditedOffset(time, keptRegions)
    : time;

  return <article className={selected ? "editor-track is-selected" : "editor-track"} aria-current={selected ? "true" : undefined} onMouseDownCapture={onSelect} onFocusCapture={onSelect} onContextMenu={(event) => event.preventDefault()}>
    <button ref={scaleHandleRef} type="button" className={`track-scale-handle is-export-${exportState}`} aria-label={`${name}: export state ${exportState}`} title={exportState === "pending" ? "Included in the next changed-track export" : exportState === "exporting" ? "Exporting" : exportState === "failed" ? "Export failed; right-click to try this track again" : "Already exported; right-click to export again"} onContextMenu={(event) => {
      event.preventDefault();
      event.stopPropagation();
      onSelect();
      onScaleContextMenu(event.clientX, event.clientY);
    }} />
    <div className="track-label"><strong>{name}</strong><span className={`track-denoise-status is-${statusKind}`}>{status}</span>{collapsed && <span>Collapsed</span>}</div>
    <WaveformScore
      buffer={displayBuffer}
      pixelsPerSecond={pixelsPerSecond}
      currentTime={selected ? (collapsed ? editedOffsetAtOriginalTime(currentTime, keptRegions) : currentTime) : -1}
      bladeActive={bladeActive && !collapsed}
      showEnvelope={selected}
      onSeek={(time) => onSeek(toSourceTime(time))}
      regions={collapsed ? [] : deletedRegions}
      mutedRegions={collapsed ? [] : mutedRegions}
      editPoints={displayEditPoints}
      selectedRange={collapsed ? null : selectedRange}
      envelopePoints={displayPoints}
      rmsFrames={rmsFrames}
      silenceThresholdDb={silenceThresholdDb}
      showSilenceThreshold={selected}
      onRangeSelect={(start, end) => { if (!collapsed) onRangeSelect(start, end); }}
      onDirectRegionEdit={(start, end, operation) => { if (!collapsed) onDirectRegionEdit(start, end, operation); }}
      onRegionRestore={onRegionRestore}
      onRangeMuteToggle={onRangeMuteToggle}
      onBlade={(time) => { if (!collapsed) onBlade(toSourceTime(time)); }}
      onEditPointRemove={(time) => onEditPointRemove(toSourceTime(time))}
      onEnvelopePointAdd={(point) => onEnvelopePointAdd({ ...point, time: toSourceTime(point.time) })}
      onEnvelopePointMove={(id, time, gain) => onEnvelopePointMove(id, toSourceTime(time), gain)}
      onEnvelopePointRemove={onEnvelopePointRemove}
    />
  </article>;
}
