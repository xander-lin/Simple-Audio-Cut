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

const WaveformScore = ({
  buffer, currentTime, onSeek, pixelsPerSecond = 48, regions, envelopePoints,
  rmsFrames, silenceThresholdDb, showSilenceThreshold, onRegionAdd, onRegionRemove, onEnvelopePointAdd, onEnvelopePointMove, onEnvelopePointRemove,
}: WaveformScoreProps) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [rowWidth, setRowWidth] = useState(0);

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
  const rows = [];
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
    rows.push(<WaveformRow key={index} buffer={buffer} startTime={startTime} endTime={endTime} width={renderedWidth} height={96} leadingPadding={leadingPadding} trailingPadding={trailingPadding} currentTime={currentTime} onSeek={onSeek} regions={regions} envelopePoints={envelopePoints} rmsFrames={rmsFrames} silenceThresholdDb={silenceThresholdDb} showSilenceThreshold={showSilenceThreshold} onRegionAdd={onRegionAdd} onRegionRemove={onRegionRemove} onEnvelopePointAdd={onEnvelopePointAdd} onEnvelopePointMove={onEnvelopePointMove} onEnvelopePointRemove={onEnvelopePointRemove} />);
    startTime = endTime;
    index += 1;
  }

  return <div ref={containerRef} className="waveform-score-container">{rows}</div>;
};

export default WaveformScore;
