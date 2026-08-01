import { useEffect, useRef, useState } from "react";
import WaveformRow from "./WaveformRow";
import { type RmsFrame } from "../utils/audioAnalysis";
import { type EnvelopePoint, type Region } from "../utils/regionUtils";

interface WaveformScoreProps {
  buffer: AudioBuffer;
  currentTime: number;
  onSeek: (time: number) => void;
  pixelsPerSecond?: number;
  regions: Region[];
  envelopePoints: EnvelopePoint[];
  rmsFrames: readonly RmsFrame[];
  silenceThresholdDb: number;
  showSilenceThreshold: boolean;
  onRegionAdd: (start: number, end: number) => void;
  onRegionRemove: (start: number, end: number) => void;
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

const ROW_HEIGHT = 96;
const ROW_GAP = 8;

const WaveformScore = ({
  buffer, currentTime, onSeek, pixelsPerSecond = 48, regions, envelopePoints,
  rmsFrames, silenceThresholdDb, showSilenceThreshold, onRegionAdd, onRegionRemove, onEnvelopePointAdd, onEnvelopePointMove, onEnvelopePointRemove,
}: WaveformScoreProps) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [rowWidth, setRowWidth] = useState(0);
  const [regionDrag, setRegionDrag] = useState<{ start: number; current: number } | null>(null);

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
    if (!regionDrag) return;
    const cancelDrag = () => setRegionDrag(null);
    const finishDrag = () => {
      const delta = regionDrag.current - regionDrag.start;
      if (Math.abs(delta) > 0.01) {
        const start = Math.min(regionDrag.start, regionDrag.current);
        const end = Math.max(regionDrag.start, regionDrag.current);
        if (delta > 0) onRegionAdd(start, end);
        else onRegionRemove(start, end);
      }
      setRegionDrag(null);
    };
    window.addEventListener("blur", cancelDrag);
    window.addEventListener("mouseup", finishDrag);
    return () => {
      window.removeEventListener("blur", cancelDrag);
      window.removeEventListener("mouseup", finishDrag);
    };
  }, [regionDrag, onRegionAdd, onRegionRemove]);

  const finishRegionDrag = (time: number) => {
    setRegionDrag((current) => {
      if (!current) return null;
      const delta = time - current.start;
      if (Math.abs(delta) > 0.01) {
        const start = Math.min(current.start, time);
        const end = Math.max(current.start, time);
        if (delta > 0) onRegionAdd(start, end);
        else onRegionRemove(start, end);
      }
      return null;
    });
  };

  const preview = regionDrag
    ? {
      start: Math.min(regionDrag.start, regionDrag.current),
      end: Math.max(regionDrag.start, regionDrag.current),
      direction: regionDrag.current >= regionDrag.start ? "delete" : "restore",
    }
    : null;

  return <div ref={containerRef} className="waveform-score-container">
    {rowMetas.map((row) => <WaveformRow key={row.index} buffer={buffer} startTime={row.startTime} endTime={row.endTime} width={row.renderedWidth} height={ROW_HEIGHT} leadingPadding={row.leadingPadding} trailingPadding={row.trailingPadding} currentTime={currentTime} onSeek={onSeek} regions={regions} envelopePoints={envelopePoints} rmsFrames={rmsFrames} silenceThresholdDb={silenceThresholdDb} showSilenceThreshold={showSilenceThreshold} onRegionAdd={onRegionAdd} onRegionRemove={onRegionRemove} onRegionDragStart={(time) => setRegionDrag({ start: time, current: time })} onRegionDragMove={(time) => setRegionDrag((current) => current ? { ...current, current: time } : current)} onRegionDragEnd={finishRegionDrag} onEnvelopePointAdd={onEnvelopePointAdd} onEnvelopePointMove={onEnvelopePointMove} onEnvelopePointRemove={onEnvelopePointRemove} />)}
    {preview && <div className="score-region-preview-layer">{rowMetas.map((row) => {
      const start = Math.max(preview.start, row.startTime);
      const end = Math.min(preview.end, row.endTime);
      if (start >= end) return null;
      return <span key={row.index} className={`region-preview ${preview.direction}`} style={{ top: row.index * (ROW_HEIGHT + ROW_GAP), left: row.leadingPadding + (start - row.startTime) * pixelsPerSecond, width: (end - start) * pixelsPerSecond }} />;
    })}</div>}
  </div>;
};

export default WaveformScore;
