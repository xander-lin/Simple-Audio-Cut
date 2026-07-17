import { useEffect, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import EditorTrack from "./components/EditorTrack";
import { combineRegions, mergeRegions, subtractRegion, type EnvelopePoint, type Region } from "./utils/regionUtils";
import { formatTimeStandard } from "./utils/timeUtils";
import { dbfsToAmplitude, detectSilence } from "./utils/audioAnalysis";
import { createEditedBuffer, getKeptRegions } from "./utils/exportUtils";
import { appendUniqueTrack } from "./utils/trackUtils";
import "./App.css";

interface RecordingInfo {
  id: string;
  name: string;
  path: string;
  durationSeconds: number;
  integratedLufs: number | null;
}

interface Recording extends RecordingInfo {
  buffer: AudioBuffer;
  pixelsPerSecond: number;
  manualDeletedRegions: Region[];
  silenceRegions: Region[];
  silenceDetectionEnabled: boolean;
  silenceThresholdDb: number;
  minimumSilenceDurationMs: number;
  envelopePoints: EnvelopePoint[];
  collapsed: boolean;
  denoiseStatus: "processing" | "complete" | "failed";
}

interface DenoiseEvent {
  status: "complete" | "failed";
  result?: { recordingId: string; path: string; integratedLufs: number | null };
  recordingId?: string;
  error?: string;
}

function keptDuration(regions: Region[]) {
  return regions.reduce((duration, region) => duration + region.end - region.start, 0);
}

function editedOffsetAtOriginalTime(time: number, regions: Region[]) {
  let editedOffset = 0;
  for (const region of regions) {
    if (time <= region.start) return editedOffset;
    if (time < region.end) return editedOffset + time - region.start;
    editedOffset += region.end - region.start;
  }
  return editedOffset;
}

function originalTimeAtEditedOffset(offset: number, regions: Region[]) {
  let remaining = offset;
  for (const region of regions) {
    const duration = region.end - region.start;
    if (remaining <= duration) return region.start + remaining;
    remaining -= duration;
  }
  return regions.length ? regions[regions.length - 1].end : 0;
}

function App() {
  const [recordings, setRecordings] = useState<Recording[]>([]);
  const [editorTracks, setEditorTracks] = useState<Recording[]>([]);
  const [selectedTrackId, setSelectedTrackId] = useState<string | null>(null);
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [recordingSeconds, setRecordingSeconds] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [message, setMessage] = useState("Ready to record");
  const [targetLufs, setTargetLufs] = useState(-15);
  const [silenceControlOpen, setSilenceControlOpen] = useState(false);
  const [silenceDurationControlOpen, setSilenceDurationControlOpen] = useState(false);
  const [silencePrecision, setSilencePrecision] = useState<0 | 1 | 2>(0);

  const audioContextRef = useRef<AudioContext | null>(null);
  const sourceNodeRef = useRef<AudioBufferSourceNode | null>(null);
  const startedAtRef = useRef(0);
  const playbackOffsetRef = useRef(0);
  const editedPlaybackOffsetRef = useRef(0);
  const animationFrameRef = useRef<number | null>(null);
  const recordingStartedAtRef = useRef(0);
  const selectedTrack = editorTracks.find((track) => track.id === selectedTrackId) ?? null;
  const selectedDeletedRegions = selectedTrack
    ? combineRegions(selectedTrack.manualDeletedRegions, selectedTrack.silenceRegions)
    : [];
  const selectedTrackBuffer = selectedTrack?.buffer;
  const selectedSilenceThresholdDb = selectedTrack?.silenceThresholdDb ?? -36;
  const selectedMinimumSilenceDurationMs = selectedTrack?.minimumSilenceDurationMs ?? 200;

  useEffect(() => {
    audioContextRef.current = new AudioContext();
    const suppressWebViewContextMenu = (event: MouseEvent) => event.preventDefault();
    document.addEventListener("contextmenu", suppressWebViewContextMenu, true);
    return () => {
      document.removeEventListener("contextmenu", suppressWebViewContextMenu, true);
      if (animationFrameRef.current) cancelAnimationFrame(animationFrameRef.current);
      sourceNodeRef.current?.stop();
      void audioContextRef.current?.close();
    };
  }, []);

  useEffect(() => {
    if (!isRecording) return;
    const interval = window.setInterval(() => {
      setRecordingSeconds((Date.now() - recordingStartedAtRef.current) / 1000);
    }, 100);
    return () => window.clearInterval(interval);
  }, [isRecording]);

  useEffect(() => {
    if ((!silenceControlOpen && !silenceDurationControlOpen) || !selectedTrackId || !selectedTrackBuffer) return;
    const silenceRegions = detectSilence(selectedTrackBuffer, {
      threshold: dbfsToAmplitude(selectedSilenceThresholdDb),
      minDuration: selectedMinimumSilenceDurationMs / 1_000,
    });
    setEditorTracks((current) => current.map((track) => track.id === selectedTrackId
      ? { ...track, silenceRegions, silenceDetectionEnabled: true }
      : track));
  }, [silenceControlOpen, silenceDurationControlOpen, selectedSilenceThresholdDb, selectedMinimumSilenceDurationMs, selectedTrackId, selectedTrackBuffer]);

  const decodeRecording = async (info: RecordingInfo) => {
    const context = audioContextRef.current;
    if (!context) throw new Error("Audio system is not ready.");
    const response = await fetch(convertFileSrc(info.path));
    if (!response.ok) throw new Error("Unable to load the completed recording.");
    const buffer = await context.decodeAudioData(await response.arrayBuffer());
    return {
      ...info,
      buffer,
      pixelsPerSecond: 48,
      durationSeconds: info.durationSeconds || buffer.duration,
      manualDeletedRegions: [],
      silenceRegions: [],
      silenceDetectionEnabled: false,
      silenceThresholdDb: -36,
      minimumSilenceDurationMs: 200,
      envelopePoints: [],
      collapsed: false,
      denoiseStatus: "processing" as const,
    };
  };

  const loadRecording = async (info: RecordingInfo) => {
    const recording = await decodeRecording(info);
    setRecordings((current) => [...current, recording]);
    return recording;
  };

  const queueDenoise = async (recording: Recording) => {
    try {
      await invoke("start_denoise", { recordingId: recording.id, sourcePath: recording.path });
    } catch (error) {
      const update = (current: Recording) => current.id === recording.id ? { ...current, denoiseStatus: "failed" as const } : current;
      setRecordings((current) => current.map(update));
      setEditorTracks((current) => current.map(update));
      setMessage(String(error));
    }
  };

  const startRecording = async () => {
    try {
      await invoke("start_recording");
      recordingStartedAtRef.current = Date.now();
      setRecordingSeconds(0);
      setIsRecording(true);
      setMessage("Recording from the default microphone");
    } catch (error) {
      setMessage(String(error));
    }
  };

  const stopRecording = async () => {
    setIsRecording(false);
    setIsProcessing(true);
    setMessage(`Measuring loudness and normalizing to ${targetLufs} LUFS`);
    try {
      const recording = await loadRecording(await invoke<RecordingInfo>("stop_recording", { targetLufs }));
      void queueDenoise(recording);
      setMessage("Recording is ready. Drag it into the editor.");
    } catch (error) {
      setMessage(String(error));
    } finally {
      setIsProcessing(false);
    }
  };

  const importAudio = async () => {
    const sourcePath = await open({
      multiple: false,
      filters: [{ name: "Audio", extensions: ["wav", "mp3", "m4a", "aac", "flac", "ogg", "opus", "aiff"] }],
    });
    if (!sourcePath || Array.isArray(sourcePath)) return;
    setIsProcessing(true);
    setMessage(`Importing and normalizing audio to ${targetLufs} LUFS`);
    try {
      const recording = await loadRecording(await invoke<RecordingInfo>("import_audio", { sourcePath, targetLufs }));
      void queueDenoise(recording);
      setMessage("Imported audio is ready. Drag it into the editor.");
    } catch (error) {
      setMessage(String(error));
    } finally {
      setIsProcessing(false);
    }
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<DenoiseEvent>("denoise-status", async ({ payload }) => {
      if (payload.status === "failed") {
        const markFailed = (recording: Recording) => recording.id === payload.recordingId ? { ...recording, denoiseStatus: "failed" as const } : recording;
        setRecordings((current) => current.map(markFailed));
        setEditorTracks((current) => current.map(markFailed));
        setMessage(payload.error ?? "ClearVoice denoising failed.");
        return;
      }
      if (!payload.result) return;
      try {
        if (selectedTrackId === payload.result.recordingId) {
          sourceNodeRef.current?.stop();
          sourceNodeRef.current = null;
          if (animationFrameRef.current) cancelAnimationFrame(animationFrameRef.current);
          setIsPlaying(false);
        }
        const context = audioContextRef.current;
        if (!context) throw new Error("Audio system is not ready.");
        const response = await fetch(convertFileSrc(payload.result.path));
        if (!response.ok) throw new Error("Unable to load ClearVoice output.");
        const buffer = await context.decodeAudioData(await response.arrayBuffer());
        const replace = (recording: Recording): Recording => {
          if (recording.id !== payload.result?.recordingId) return recording;
          return {
            ...recording,
            path: payload.result.path,
            buffer,
            durationSeconds: buffer.duration,
            integratedLufs: payload.result.integratedLufs ?? recording.integratedLufs,
            silenceRegions: recording.silenceDetectionEnabled
              ? detectSilence(buffer, {
                threshold: dbfsToAmplitude(recording.silenceThresholdDb),
                minDuration: recording.minimumSilenceDurationMs / 1_000,
              })
              : [],
            denoiseStatus: "complete",
          };
        };
        setRecordings((current) => current.map(replace));
        setEditorTracks((current) => current.map(replace));
      } catch (error) {
        setMessage(String(error));
      }
    }).then((dispose) => { unlisten = dispose; });
    return () => unlisten?.();
  }, [selectedTrackId]);

  const stopPlayback = (savePosition: boolean) => {
    if (sourceNodeRef.current) {
      sourceNodeRef.current.onended = null;
      try { sourceNodeRef.current.stop(); } catch { /* Already stopped. */ }
      sourceNodeRef.current = null;
    }
    if (animationFrameRef.current) cancelAnimationFrame(animationFrameRef.current);
    if (savePosition && audioContextRef.current && selectedTrack) {
      const keptRegions = getKeptRegions(selectedDeletedRegions, selectedTrack.buffer.duration);
      const editedTime = editedPlaybackOffsetRef.current + audioContextRef.current.currentTime - startedAtRef.current;
      playbackOffsetRef.current = originalTimeAtEditedOffset(
        Math.min(editedTime, keptDuration(keptRegions)),
        keptRegions,
      );
      setCurrentTime(playbackOffsetRef.current);
    }
    setIsPlaying(false);
  };

  const startPlayback = async (offset: number) => {
    const context = audioContextRef.current;
    if (!context || !selectedTrack) return;
    if (context.state === "suspended") await context.resume();
    stopPlayback(false);
    const keptRegions = getKeptRegions(selectedDeletedRegions, selectedTrack.buffer.duration);
    const editedBuffer = createEditedBuffer(selectedTrack.buffer, selectedDeletedRegions, selectedTrack.envelopePoints);
    if (editedBuffer.duration === 0) {
      setMessage("All audio has been removed from this recording.");
      return;
    }
    const editedOffset = editedOffsetAtOriginalTime(offset, keptRegions);
    const source = context.createBufferSource();
    source.buffer = editedBuffer;
    source.connect(context.destination);
    source.onended = () => {
      setIsPlaying(false);
      playbackOffsetRef.current = 0;
      setCurrentTime(0);
    };
    startedAtRef.current = context.currentTime;
    playbackOffsetRef.current = offset;
    editedPlaybackOffsetRef.current = editedOffset;
    source.start(0, editedOffset);
    sourceNodeRef.current = source;
    setIsPlaying(true);
    const tick = () => {
      const editedTime = editedPlaybackOffsetRef.current + context.currentTime - startedAtRef.current;
      if (editedTime < editedBuffer.duration) {
        setCurrentTime(originalTimeAtEditedOffset(editedTime, keptRegions));
        animationFrameRef.current = requestAnimationFrame(tick);
      }
    };
    animationFrameRef.current = requestAnimationFrame(tick);
  };

  const updateSelectedTrack = (update: (track: Recording) => Recording) => {
    if (!selectedTrackId) return;
    setEditorTracks((current) => current.map((track) => track.id === selectedTrackId ? update(track) : track));
  };

  const updateTrack = (id: string, update: (track: Recording) => Recording) => {
    setEditorTracks((current) => current.map((track) => track.id === id ? update(track) : track));
  };

  const moveToEditor = (id: string) => {
    const recording = recordings.find((item) => item.id === id);
    if (!recording) return;
    stopPlayback(false);
    setEditorTracks((current) => appendUniqueTrack(current, recording));
    setSelectedTrackId(recording.id);
    setRecordings((current) => current.filter((item) => item.id !== id));
    setCurrentTime(0);
    playbackOffsetRef.current = 0;
    editedPlaybackOffsetRef.current = 0;
    setMessage("Editing selected recording");
  };

  const selectAfterTrackRemoval = (removedId: string) => {
    const nextTrack = editorTracks.find((track) => track.id !== removedId) ?? null;
    setEditorTracks((current) => current.filter((track) => track.id !== removedId));
    setSelectedTrackId(nextTrack?.id ?? null);
    setCurrentTime(0);
    playbackOffsetRef.current = 0;
    editedPlaybackOffsetRef.current = 0;
  };

  const returnToLibrary = () => {
    if (!selectedTrack) return;
    stopPlayback(false);
    setRecordings((current) => appendUniqueTrack(current, selectedTrack));
    selectAfterTrackRemoval(selectedTrack.id);
    setMessage("Recording returned to the library");
  };

  const removeTrack = () => {
    if (!selectedTrack) return;
    stopPlayback(false);
    selectAfterTrackRemoval(selectedTrack.id);
    setMessage("Track removed from the editor");
  };

  const handleDrop = (event: React.DragEvent<HTMLElement>) => {
    event.preventDefault();
    const id = event.dataTransfer.getData("application/simple-audio-cut-recording");
    moveToEditor(id);
  };

  const renameRecording = (id: string, name: string) => {
    const trimmedName = name.trim();
    if (!trimmedName) return;
    setRecordings((current) => current.map((recording) => recording.id === id ? { ...recording, name: trimmedName } : recording));
  };

  const exportEdit = async () => {
    if (!selectedTrack) return;
    const destination = await save({
      defaultPath: `${selectedTrack.name}-edited.wav`,
      filters: [{ name: "WAV audio", extensions: ["wav"] }],
    });
    if (!destination) {
      setMessage("Export cancelled");
      return;
    }
    setIsProcessing(true);
    try {
      const outputPath = await invoke<string>("export_edit", {
        sourcePath: selectedTrack.path,
        deletedRegions: selectedDeletedRegions,
        envelopePoints: selectedTrack.envelopePoints,
        destination,
      });
      setMessage(`Edited WAV exported to ${outputPath}`);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setIsProcessing(false);
    }
  };

  const markSilence = () => {
    if (!selectedTrack) return;
    stopPlayback(false);
    const silenceRegions = detectSilence(selectedTrack.buffer, {
      threshold: dbfsToAmplitude(selectedTrack.silenceThresholdDb),
      minDuration: selectedTrack.minimumSilenceDurationMs / 1_000,
    });
    updateSelectedTrack((track) => ({ ...track, silenceRegions, silenceDetectionEnabled: true }));
  };

  return (
    <main className="app-shell">
      <section className={`recording-panel ${recordings.length || isRecording || isProcessing ? "has-content" : "is-empty"}`} aria-label="Recording">
        <header className="panel-header">
          <div className="window-drag-area" data-tauri-drag-region><p className="eyebrow">Recording</p><h1>Simple Audio Cut</h1></div>
          <div className="recording-actions">
            <button type="button" className={isRecording ? "record-button is-active" : "record-button"} onClick={isRecording ? stopRecording : startRecording} disabled={isProcessing} aria-label={isRecording ? "Stop recording" : "Start recording"}>
              <span className="record-symbol" />{isRecording ? formatTimeStandard(recordingSeconds) : "Record"}
            </button>
            <button type="button" onClick={importAudio} disabled={isRecording || isProcessing}>Import</button>
            <button type="button" className="lufs-target-button" aria-label="Loudness normalization target" disabled={isRecording || isProcessing} onWheel={(event) => {
              if (event.deltaY === 0) return;
              event.preventDefault();
              setTargetLufs((current) => Math.max(-70, Math.min(-5, current + (event.deltaY < 0 ? 1 : -1))));
            }}>{targetLufs} LUFS</button>
            <div className="window-controls">
              <button type="button" className="window-control minimize-control" aria-label="Minimize window" onClick={() => void getCurrentWindow().minimize()} />
              <button type="button" className="window-control maximize-control" aria-label="Maximize window" onClick={() => void getCurrentWindow().toggleMaximize()} />
              <button type="button" className="window-control close-control" aria-label="Close window" onClick={() => void getCurrentWindow().close()} />
            </div>
          </div>
        </header>
        {(recordings.length > 0 || isRecording || isProcessing) && <div className="recording-content">
          <p className="status-line">{isProcessing ? "Processing audio locally" : message}</p>
          <div className="recording-library">
            {recordings.map((recording) => (
              <article key={recording.id} className="recording-item" draggable onDragStart={(event) => event.dataTransfer.setData("application/simple-audio-cut-recording", recording.id)}>
                <div className="recording-details"><input aria-label="Recording name" className="recording-name" defaultValue={recording.name} onBlur={(event) => renameRecording(recording.id, event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }} onDragStart={(event) => event.stopPropagation()} /><span>{formatTimeStandard(recording.durationSeconds)}</span></div>
                <div className="recording-meta"><span className="lufs-badge">{recording.integratedLufs?.toFixed(1) ?? "--"} LUFS</span><span className={recording.denoiseStatus === "processing" ? "denoise-processing" : ""}>{recording.denoiseStatus === "processing" ? "Denoising" : recording.denoiseStatus === "complete" ? "ClearVoice" : "Denoise failed"}</span></div>
              </article>
            ))}
          </div>
        </div>}
      </section>

      <div className="panel-divider" />

      <section className="editor-panel" aria-label="Editor" onDragOver={(event) => event.preventDefault()} onDrop={handleDrop}>
        <header className="panel-header editor-header">
          <div className="editor-heading"><p className="eyebrow">Editor</p><h2>{selectedTrack ? selectedTrack.name : "Drop a recording here"}</h2></div>
          {selectedTrack && <div className="editor-actions">
            <button type="button" className="play-button" onClick={() => isPlaying ? stopPlayback(true) : startPlayback(playbackOffsetRef.current)}>{isPlaying ? formatTimeStandard(currentTime) : "Play"}</button>
            <fieldset className="silence-control" aria-label="Silence threshold" onMouseLeave={() => {
              setSilenceControlOpen(false);
              setSilencePrecision(0);
            }}>
              {silenceControlOpen ? <div className="silence-wheel" onWheel={(event) => {
                if (event.deltaY === 0) return;
                event.preventDefault();
                const step = 10 ** -silencePrecision;
                const direction = event.deltaY < 0 ? 1 : -1;
                updateSelectedTrack((track) => {
                  const next = Math.round(track.silenceThresholdDb / step) * step + direction * step;
                  return {
                    ...track,
                    silenceThresholdDb: Math.max(-70, Math.min(-12, Number(next.toFixed(silencePrecision)))),
                  };
                });
              }}>
                <span>{Math.min(-12, selectedSilenceThresholdDb + 10 ** -silencePrecision).toFixed(silencePrecision)} dBFS</span>
                <button type="button" className="silence-button silence-wheel-center" onClick={() => setSilencePrecision((current) => Math.max(0, current - 1) as 0 | 1 | 2)} onContextMenu={(event) => {
                  event.preventDefault();
                  setSilencePrecision((current) => Math.min(2, current + 1) as 0 | 1 | 2);
                }}><span className="silence-label is-threshold">{selectedSilenceThresholdDb.toFixed(silencePrecision)} dBFS</span></button>
                <span>{Math.max(-70, selectedSilenceThresholdDb - 10 ** -silencePrecision).toFixed(silencePrecision)} dBFS</span>
              </div> : <button type="button" className="silence-button" onClick={markSilence} onContextMenu={(event) => {
                event.preventDefault();
                setSilencePrecision(0);
                setSilenceControlOpen(true);
              }}><span className="silence-label">Mark silence</span></button>}
            </fieldset>
            <fieldset className="silence-duration-control" aria-label="Minimum silence duration" onMouseLeave={() => setSilenceDurationControlOpen(false)}>
              {silenceDurationControlOpen ? <div className="silence-duration-wheel" onWheel={(event) => {
                if (event.deltaY === 0) return;
                event.preventDefault();
                const direction = event.deltaY < 0 ? 10 : -10;
                updateSelectedTrack((track) => ({
                  ...track,
                  minimumSilenceDurationMs: Math.max(0, Math.min(5_000, track.minimumSilenceDurationMs + direction)),
                }));
              }}>
                <span>{Math.min(5_000, selectedMinimumSilenceDurationMs + 10)} ms</span>
                <button type="button" className="silence-duration-button" onClick={() => setSilenceDurationControlOpen(false)} onContextMenu={(event) => {
                  event.preventDefault();
                  setSilenceDurationControlOpen(false);
                }}>{selectedMinimumSilenceDurationMs} ms</button>
                <span>{Math.max(0, selectedMinimumSilenceDurationMs - 10)} ms</span>
              </div> : <button type="button" className="silence-duration-button" onClick={() => setSilenceDurationControlOpen(true)} onContextMenu={(event) => {
                event.preventDefault();
                setSilenceDurationControlOpen(true);
              }}>{selectedMinimumSilenceDurationMs} ms</button>}
            </fieldset>
            <button type="button" className={selectedTrack.collapsed ? "collapse-button is-active" : "collapse-button"} onClick={() => updateSelectedTrack((track) => ({ ...track, collapsed: !track.collapsed }))} disabled={selectedDeletedRegions.length === 0}>{selectedTrack.collapsed ? "Show cuts" : "Collapse cuts"}</button>
            <button type="button" className="return-button" onClick={returnToLibrary} disabled={isProcessing}>Return</button>
            <button type="button" className="remove-track-button" onClick={removeTrack} disabled={isProcessing}>Remove</button>
            <button type="button" className="export-button" onClick={exportEdit} disabled={isProcessing}>Export</button>
          </div>}
        </header>
        <div className="editor-content">
          {editorTracks.length > 0 ? <div className="editor-tracks">{editorTracks.map((track) => {
            const deletedRegions = combineRegions(track.manualDeletedRegions, track.silenceRegions);
            const isSelected = track.id === selectedTrackId;
            return <EditorTrack key={track.id} name={track.name} buffer={track.buffer} selected={isSelected} collapsed={track.collapsed} currentTime={isSelected ? currentTime : 0} pixelsPerSecond={track.pixelsPerSecond} deletedRegions={deletedRegions} envelopePoints={track.envelopePoints} silenceThresholdDb={track.silenceThresholdDb} onSelect={() => {
              if (track.id === selectedTrackId) return;
              stopPlayback(false);
              setSelectedTrackId(track.id);
              setCurrentTime(0);
              playbackOffsetRef.current = 0;
              editedPlaybackOffsetRef.current = 0;
            }} onScale={(deltaY) => updateTrack(track.id, (current) => ({
              ...current,
              pixelsPerSecond: Math.max(0.25, current.pixelsPerSecond * (deltaY < 0 ? 1.15 : 1 / 1.15)),
            }))} onSeek={(time) => {
              setCurrentTime(time);
              playbackOffsetRef.current = time;
              if (isPlaying && isSelected) void startPlayback(time);
            }} onRegionAdd={(start, end) => {
              stopPlayback(false);
              updateTrack(track.id, (current) => ({ ...current, manualDeletedRegions: mergeRegions(current.manualDeletedRegions, { start, end }) }));
            }} onRegionRemove={(start, end) => {
              stopPlayback(false);
              updateTrack(track.id, (current) => ({
                ...current,
                manualDeletedRegions: subtractRegion(current.manualDeletedRegions, { start, end }),
                silenceRegions: subtractRegion(current.silenceRegions, { start, end }),
              }));
            }} onEnvelopePointAdd={(point) => {
              stopPlayback(false);
              updateTrack(track.id, (current) => ({ ...current, envelopePoints: [...current.envelopePoints, point].sort((left, right) => left.time - right.time) }));
            }} onEnvelopePointMove={(id, time, gain) => {
              stopPlayback(false);
              updateTrack(track.id, (current) => ({
                ...current,
                envelopePoints: current.envelopePoints
                  .map((point) => point.id === id ? { ...point, time: Math.max(0, Math.min(current.buffer.duration, time)), gain: Math.max(0, Math.min(2, gain)) } : point)
                  .sort((left, right) => left.time - right.time),
              }));
            }} onEnvelopePointRemove={(id) => {
              stopPlayback(false);
              updateTrack(track.id, (current) => ({
                ...current,
                envelopePoints: current.envelopePoints.filter((point) => point.id !== id),
              }));
            }} />;
          })}</div> : <div className="editor-empty">Drag completed recordings from above into this area.</div>}
        </div>
      </section>
    </main>
  );
}

export default App;
