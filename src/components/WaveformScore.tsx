import { useCallback, useEffect, useRef, useState } from "react";
import WaveformRow from "./WaveformRow";
import { type RmsFrame } from "../utils/audioAnalysis";
import { type EnvelopePoint, type Region } from "../utils/regionUtils";
import { directRegionEdit, segmentAtTime } from "../utils/directEdit";

interface WaveformScoreProps {
  buffer: AudioBuffer;
  currentTime: number;
  bladeActive: boolean;
  showEnvelope: boolean;
  onSeek: (time: number) => void;
  pixelsPerSecond?: number;
  regions: Region[];
  mutedRegions: Region[];
  editPoints: number[];
  selectedRange: Region | null;
  envelopePoints: EnvelopePoint[];
  rmsFrames: readonly RmsFrame[];
  silenceThresholdDb: number;
  showSilenceThreshold: boolean;
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

interface RowMeta {
  index: number;
  startTime: number;
  endTime: number;
  renderedWidth: number;
  leadingPadding: number;
  trailingPadding: number;
}

const ROW_HEIGHT = 120;
const ROW_GAP = 10;

const WaveformScore = ({
  buffer, currentTime, bladeActive, showEnvelope, onSeek, pixelsPerSecond = 48, regions, mutedRegions,
  editPoints, selectedRange, envelopePoints, rmsFrames, silenceThresholdDb,
  showSilenceThreshold, onRangeSelect, onRegionRestore, onRangeMuteToggle,
  onDirectRegionEdit,
  onBlade, onEditPointRemove, onEnvelopePointAdd, onEnvelopePointMove,
  onEnvelopePointRemove,
}: WaveformScoreProps) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [rowWidth, setRowWidth] = useState(0);
  const [regionDrag, setRegionDrag] = useState<{ start: number; current: number } | null>(null);
  const [directEditDrag, setDirectEditDrag] = useState<{ start: number; current: number } | null>(null);
  const regionDragRef = useRef(regionDrag);
  const directEditDragRef = useRef(directEditDrag);
  const draggingRegion = regionDrag !== null;
  const minimumDragDuration = Math.max(0.01, 3 / Math.max(0.25, pixelsPerSecond));
  regionDragRef.current = regionDrag;
  directEditDragRef.current = directEditDrag;

  const finishRegionDrag = useCallback((endTime?: number) => {
    const activeDrag = regionDragRef.current;
    if (!activeDrag) return;
    const finishedDrag = endTime === undefined ? activeDrag : { ...activeDrag, current: endTime };
    regionDragRef.current = null;
    setRegionDrag(null);
    const delta = finishedDrag.current - finishedDrag.start;
    if (Math.abs(delta) > minimumDragDuration) {
      onRangeSelect(
        Math.min(finishedDrag.start, finishedDrag.current),
        Math.max(finishedDrag.start, finishedDrag.current),
      );
      return;
    }
    onSeek(finishedDrag.start);
    const segment = segmentAtTime(editPoints, finishedDrag.start, buffer.duration);
    if (segment) onRangeSelect(segment.start, segment.end);
  }, [buffer.duration, editPoints, minimumDragDuration, onRangeSelect, onSeek]);

  const finishDirectEditDrag = useCallback((endTime?: number) => {
    const activeDrag = directEditDragRef.current;
    if (!activeDrag) return;
    const finishedDrag = endTime === undefined ? activeDrag : { ...activeDrag, current: endTime };
    directEditDragRef.current = null;
    setDirectEditDrag(null);
    const edit = directRegionEdit(finishedDrag.start, finishedDrag.current, minimumDragDuration);
    if (edit) onDirectRegionEdit(edit.start, edit.end, edit.operation);
  }, [minimumDragDuration, onDirectRegionEdit]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const updateWidth = () => setRowWidth(Math.max(1, Math.floor(container.clientWidth)));
    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  const edgePadding = Math.min(
    Math.max(24, pixelsPerSecond * 0.3),
    Math.max(24, rowWidth * 0.25),
  );
  const rowMetas: RowMeta[] = [];
  let startTime = 0;
  let index = 0;
  while (rowWidth > edgePadding && startTime < buffer.duration) {
    const leadingPadding = index === 0 ? edgePadding : 0;
    const remainingDuration = buffer.duration - startTime;
    const finalWidth = leadingPadding + remainingDuration * pixelsPerSecond + edgePadding;
    const isFinalRow = finalWidth <= rowWidth;
    const trailingPadding = isFinalRow ? edgePadding : 0;
    const availableAudioWidth = Math.max(1, rowWidth - leadingPadding - trailingPadding);
    const rowDuration = isFinalRow ? remainingDuration : availableAudioWidth / pixelsPerSecond;
    const endTime = Math.min(buffer.duration, startTime + rowDuration);
    const renderedWidth = isFinalRow
      ? leadingPadding + (endTime - startTime) * pixelsPerSecond + trailingPadding
      : rowWidth;
    rowMetas.push({ index, startTime, endTime, renderedWidth, leadingPadding, trailingPadding });
    startTime = endTime;
    index += 1;
  }

  useEffect(() => {
    if (!draggingRegion && !directEditDrag) return;
    const cancelDrag = () => {
      regionDragRef.current = null;
      setRegionDrag(null);
      directEditDragRef.current = null;
      setDirectEditDrag(null);
    };
    const finishDrag = () => {
      finishRegionDrag();
      finishDirectEditDrag();
    };
    window.addEventListener("blur", cancelDrag);
    window.addEventListener("mouseup", finishDrag);
    return () => {
      window.removeEventListener("blur", cancelDrag);
      window.removeEventListener("mouseup", finishDrag);
    };
  }, [directEditDrag, draggingRegion, finishDirectEditDrag, finishRegionDrag]);

  const preview = directEditDrag
    ? {
      start: Math.min(directEditDrag.start, directEditDrag.current),
      end: Math.max(directEditDrag.start, directEditDrag.current),
      direction: directEditDrag.current >= directEditDrag.start ? "delete" : "restore",
    }
    : regionDrag
    ? {
      start: Math.min(regionDrag.start, regionDrag.current),
      end: Math.max(regionDrag.start, regionDrag.current),
      direction: "range",
    }
    : null;

  return <div ref={containerRef} className="waveform-score-container">
    {rowMetas.map((row) => <WaveformRow key={row.index} buffer={buffer} startTime={row.startTime} endTime={row.endTime} width={row.renderedWidth} height={ROW_HEIGHT} leadingPadding={row.leadingPadding} trailingPadding={row.trailingPadding} currentTime={currentTime} bladeActive={bladeActive} showEnvelope={showEnvelope} onSeek={onSeek} regions={regions} mutedRegions={mutedRegions} editPoints={editPoints} selectedRange={selectedRange} envelopePoints={envelopePoints} rmsFrames={rmsFrames} silenceThresholdDb={silenceThresholdDb} showSilenceThreshold={showSilenceThreshold} onRegionDragStart={(time) => {
      const next = { start: time, current: time };
      regionDragRef.current = next;
      setRegionDrag(next);
    }} onRegionDragMove={(time) => setRegionDrag((current) => {
      if (!current) return current;
      const next = { ...current, current: time };
      regionDragRef.current = next;
      return next;
    })} onRegionDragEnd={finishRegionDrag} onDirectEditDragStart={(time) => {
      const next = { start: time, current: time };
      directEditDragRef.current = next;
      setDirectEditDrag(next);
    }} onDirectEditDragMove={(time) => setDirectEditDrag((current) => {
      if (!current) return current;
      const next = { ...current, current: time };
      directEditDragRef.current = next;
      return next;
    })} onDirectEditDragEnd={finishDirectEditDrag} onRegionRestore={onRegionRestore} onRangeMuteToggle={onRangeMuteToggle} onBlade={onBlade} onEditPointRemove={onEditPointRemove} onEnvelopePointAdd={onEnvelopePointAdd} onEnvelopePointMove={onEnvelopePointMove} onEnvelopePointRemove={onEnvelopePointRemove} />)}
    {preview && <div className="score-region-preview-layer">{rowMetas.map((row) => {
      const start = Math.max(preview.start, row.startTime);
      const end = Math.min(preview.end, row.endTime);
      if (start >= end) return null;
      return <span key={row.index} className={`region-preview ${preview.direction}`} style={{ top: row.index * (ROW_HEIGHT + ROW_GAP), left: row.leadingPadding + (start - row.startTime) * pixelsPerSecond, width: (end - start) * pixelsPerSecond }} />;
    })}</div>}
  </div>;
};

export default WaveformScore;
