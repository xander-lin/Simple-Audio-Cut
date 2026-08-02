import { useEffect, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { confirm as confirmDialog, open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import EditorTrack from "./components/EditorTrack";
import { combineRegions, mergeRegions, subtractRegion, type EnvelopePoint, type Region } from "./utils/regionUtils";
import { formatTimeStandard } from "./utils/timeUtils";
import { dbfsToAmplitude, detectSilence } from "./utils/audioAnalysis";
import { createEditedBuffer, getKeptRegions } from "./utils/exportUtils";
import { appendUniqueTrack } from "./utils/trackUtils";
import { applyDenoiseUpdate, canExportTracks, type DenoiseState } from "./utils/denoiseUtils";
import { exportSignature, getLastExportDirectory, needsExport, rememberLastExportDirectory } from "./utils/exportState";
import { importSummary, selectedPaths } from "./utils/importUtils";
import "./App.css";

interface RecordingInfo {
  id: string;
  name: string;
  path: string;
  durationSeconds: number;
  integratedLufs: number | null;
}

interface Recording extends RecordingInfo, DenoiseState {
  buffer: AudioBuffer;
  pixelsPerSecond: number;
  manualDeletedRegions: Region[];
  silenceRegions: Region[];
  silenceDetectionEnabled: boolean;
  silenceThresholdDb: number;
  minimumSilenceDurationMs: number;
  envelopePoints: EnvelopePoint[];
  collapsed: boolean;
  lastExportSignature: string | null;
}

interface DenoiseResult {
  recordingId: string;
  taskId: string;
  path: string;
  integratedLufs: number | null;
}

interface DenoiseAvailability {
  available: boolean;
}

interface DenoiseEvent {
  status: "processing";
  recordingId: string;
  taskId: string;
}

interface ExportResult {
  recordingId: string;
  path: string | null;
  error: string | null;
}

interface RecordingContextMenu {
  recordingId: string;
  x: number;
  y: number;
}

interface TrackContextMenu {
  trackId: string;
  x: number;
  y: number;
}

function createDeferred() {
  let resolve!: () => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<void>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
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

function denoiseLabel(recording: Recording) {
  if (recording.denoiseStatus === "queued") return "Denoise queued";
  if (recording.denoiseStatus === "processing") return "Denoising";
  if (recording.denoiseStatus === "unavailable") return "No denoise";
  return recording.denoiseStatus === "complete" ? "ClearVoice" : "Denoise failed";
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
  const [targetLufs, setTargetLufs] = useState(-14);
  const [silenceControlOpen, setSilenceControlOpen] = useState(false);
  const [silenceDurationControlOpen, setSilenceDurationControlOpen] = useState(false);
  const [silencePrecision, setSilencePrecision] = useState<0 | 1 | 2>(0);
  const [recordingContextMenu, setRecordingContextMenu] = useState<RecordingContextMenu | null>(null);
  const [trackContextMenu, setTrackContextMenu] = useState<TrackContextMenu | null>(null);

  const audioContextRef = useRef<AudioContext | null>(null);
  const audioContextNeedsResetRef = useRef(false);
  const sourceNodeRef = useRef<AudioBufferSourceNode | null>(null);
  const startedAtRef = useRef(0);
  const playbackOffsetRef = useRef(0);
  const editedPlaybackOffsetRef = useRef(0);
  const animationFrameRef = useRef<number | null>(null);
  const recordingStartedAtRef = useRef(0);
  const recordingContextMenuRef = useRef<HTMLDivElement>(null);
  const trackContextMenuRef = useRef<HTMLDivElement>(null);
  const selectedTrackIdRef = useRef<string | null>(null);
  const denoiseListenerReadyRef = useRef<ReturnType<typeof createDeferred> | null>(null);
  const currentDenoiseTasksRef = useRef(new Map<string, string>());
  const activeDenoiseRecordingsRef = useRef(new Set<string>());
  const exportInProgressRef = useRef(false);
  const closePromptOpenRef = useRef(false);
  const hasUnsavedWorkRef = useRef(false);
  if (!denoiseListenerReadyRef.current) denoiseListenerReadyRef.current = createDeferred();
  selectedTrackIdRef.current = selectedTrackId;
  const selectedTrack = editorTracks.find((track) => track.id === selectedTrackId) ?? null;
  const selectedDeletedRegions = selectedTrack
    ? combineRegions(selectedTrack.manualDeletedRegions, selectedTrack.silenceRegions)
    : [];
  const selectedTrackBuffer = selectedTrack?.buffer;
  const selectedSilenceThresholdDb = selectedTrack?.silenceThresholdDb ?? -36;
  const selectedMinimumSilenceDurationMs = selectedTrack?.minimumSilenceDurationMs ?? 200;
  const pendingExportCount = editorTracks.filter(needsExport).length;
  const hasUnsavedWork = isRecording || isProcessing || pendingExportCount > 0;
  hasUnsavedWorkRef.current = hasUnsavedWork;

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

  const getAudioContext = () => {
    let context = audioContextRef.current;
    if (!context || context.state === "closed" || audioContextNeedsResetRef.current) {
      if (sourceNodeRef.current) {
        sourceNodeRef.current.onended = null;
        try { sourceNodeRef.current.stop(); } catch { /* Already stopped. */ }
        sourceNodeRef.current = null;
      }
      if (animationFrameRef.current) cancelAnimationFrame(animationFrameRef.current);
      setIsPlaying(false);
      if (context && context.state !== "closed") void context.close().catch(() => undefined);
      context = new AudioContext();
      audioContextRef.current = context;
      audioContextNeedsResetRef.current = false;
    }
    return context;
  };

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow().onCloseRequested(async (event) => {
      if (!hasUnsavedWorkRef.current) return;
      event.preventDefault();
      if (closePromptOpenRef.current) return;
      closePromptOpenRef.current = true;
      try {
        const confirmed = await confirmDialog("There are recording or export changes that have not been saved. Quit anyway?", {
          title: "Quit Simple Audio Cut?",
          kind: "warning",
        });
        if (confirmed) await getCurrentWindow().destroy();
      } finally {
        closePromptOpenRef.current = false;
      }
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!recordingContextMenu) return;
    const dismiss = (event: MouseEvent) => {
      if (!recordingContextMenuRef.current?.contains(event.target as Node)) {
        setRecordingContextMenu(null);
      }
    };
    const dismissOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setRecordingContextMenu(null);
    };
    const dismissOnBlur = () => setRecordingContextMenu(null);
    window.addEventListener("mousedown", dismiss);
    window.addEventListener("keydown", dismissOnEscape);
    window.addEventListener("blur", dismissOnBlur);
    return () => {
      window.removeEventListener("mousedown", dismiss);
      window.removeEventListener("keydown", dismissOnEscape);
      window.removeEventListener("blur", dismissOnBlur);
    };
  }, [recordingContextMenu]);

  useEffect(() => {
    if (!trackContextMenu) return;
    const dismiss = (event: MouseEvent) => {
      if (!trackContextMenuRef.current?.contains(event.target as Node)) {
        setTrackContextMenu(null);
      }
    };
    const dismissOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setTrackContextMenu(null);
    };
    const dismissOnBlur = () => setTrackContextMenu(null);
    window.addEventListener("mousedown", dismiss);
    window.addEventListener("keydown", dismissOnEscape);
    window.addEventListener("blur", dismissOnBlur);
    return () => {
      window.removeEventListener("mousedown", dismiss);
      window.removeEventListener("keydown", dismissOnEscape);
      window.removeEventListener("blur", dismissOnBlur);
    };
  }, [trackContextMenu]);

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
    const context = getAudioContext();
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
      lastExportSignature: null,
      denoiseStatus: "queued" as const,
      denoiseTaskId: "",
    };
  };

  const loadRecording = async (info: RecordingInfo) => {
    const recording = await decodeRecording(info);
    setRecordings((current) => [...current, recording]);
    return recording;
  };

  const completeDenoise = async (result: DenoiseResult) => {
    if (currentDenoiseTasksRef.current.get(result.recordingId) !== result.taskId) return;
    if (selectedTrackIdRef.current === result.recordingId) {
      sourceNodeRef.current?.stop();
      sourceNodeRef.current = null;
      if (animationFrameRef.current) cancelAnimationFrame(animationFrameRef.current);
      setIsPlaying(false);
    }
    const context = getAudioContext();
    const response = await fetch(convertFileSrc(result.path));
    if (!response.ok) throw new Error("Unable to load ClearVoice output.");
    const buffer = await context.decodeAudioData(await response.arrayBuffer());
    const replace = (recording: Recording): Recording => {
      if (recording.id !== result.recordingId || recording.denoiseTaskId !== result.taskId) return recording;
      return {
        ...recording,
        path: result.path,
        buffer,
        durationSeconds: buffer.duration,
        integratedLufs: result.integratedLufs ?? recording.integratedLufs,
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
  };

  const queueDenoise = async (recording: Recording) => {
    if (activeDenoiseRecordingsRef.current.has(recording.id)) return;
    activeDenoiseRecordingsRef.current.add(recording.id);
    const taskId = crypto.randomUUID();
    currentDenoiseTasksRef.current.set(recording.id, taskId);
    const markQueued = (current: Recording): Recording => current.id === recording.id
      ? { ...current, denoiseStatus: "queued", denoiseTaskId: taskId }
      : current;
    setRecordings((current) => current.map(markQueued));
    setEditorTracks((current) => current.map(markQueued));
    try {
      const availability = await invoke<DenoiseAvailability>("denoise_availability", {
        sampleRate: recording.buffer.sampleRate,
      });
      if (!availability.available) {
        const markUnavailable = (current: Recording) => applyDenoiseUpdate(current, recording.id, taskId, {
          denoiseStatus: "unavailable",
        });
        setRecordings((current) => current.map(markUnavailable));
        setEditorTracks((current) => current.map(markUnavailable));
        return;
      }
      await denoiseListenerReadyRef.current?.promise;
      const result = await invoke<DenoiseResult>("start_denoise", {
        recordingId: recording.id,
        taskId,
        sourcePath: recording.path,
        sampleRate: recording.buffer.sampleRate,
        targetLufs,
      });
      await completeDenoise(result);
    } catch (error) {
      const update = (current: Recording) => applyDenoiseUpdate(current, recording.id, taskId, { denoiseStatus: "failed" });
      setRecordings((current) => current.map(update));
      setEditorTracks((current) => current.map(update));
      if (currentDenoiseTasksRef.current.get(recording.id) === taskId) {
        setMessage(String(error));
      }
    } finally {
      if (currentDenoiseTasksRef.current.get(recording.id) === taskId) {
        activeDenoiseRecordingsRef.current.delete(recording.id);
      }
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
    const selection = await open({
      multiple: true,
      filters: [{ name: "Audio or video", extensions: ["wav", "mp3", "m4a", "aac", "flac", "ogg", "opus", "aiff", "mp4", "mov", "mkv", "webm", "m4v", "avi"] }],
    });
    audioContextNeedsResetRef.current = true;
    const paths = selectedPaths(selection);
    if (!paths.length) return;
    setIsProcessing(true);
    setMessage(paths.length === 1
      ? `Importing and normalizing audio to ${targetLufs} LUFS`
      : `Importing and normalizing ${paths.length} files to ${targetLufs} LUFS`);
    try {
      let imported = 0;
      let failed = 0;
      let firstFailure: { path: string; error: string } | undefined;
      for (const sourcePath of paths) {
        try {
          const recording = await loadRecording(await invoke<RecordingInfo>("import_audio", { sourcePath, targetLufs }));
          imported += 1;
          void queueDenoise(recording);
        } catch (error) {
          failed += 1;
          firstFailure ??= { path: sourcePath, error: String(error) };
        }
      }
      setMessage(importSummary(imported, failed, firstFailure));
    } catch (error) {
      setMessage(String(error));
    } finally {
      setIsProcessing(false);
    }
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    const listenerReady = denoiseListenerReadyRef.current;
    void listen<DenoiseEvent>("denoise-status", async ({ payload }) => {
      const markProcessing = (recording: Recording) => applyDenoiseUpdate(recording, payload.recordingId, payload.taskId, {
        denoiseStatus: "processing",
      });
      setRecordings((current) => current.map(markProcessing));
      setEditorTracks((current) => current.map(markProcessing));
    }).then((dispose) => {
      if (disposed) {
        dispose();
        return;
      }
      unlisten = dispose;
      listenerReady?.resolve();
    }).catch((error) => {
      if (disposed) return;
      listenerReady?.reject(error);
      setMessage(`Unable to monitor denoising: ${String(error)}`);
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

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
    if (!selectedTrack) return;
    try {
      const context = getAudioContext();
      if (context.state === "suspended") await context.resume();
      stopPlayback(false);
      const keptRegions = getKeptRegions(selectedDeletedRegions, selectedTrack.buffer.duration);
      const editedBuffer = createEditedBuffer(selectedTrack.buffer, selectedDeletedRegions, selectedTrack.envelopePoints);
      if (editedBuffer.duration === 0) {
        setMessage("All audio has been removed from this recording.");
        return;
      }
      const safeOffset = offset >= selectedTrack.buffer.duration ? 0 : Math.max(0, offset);
      let editedOffset = editedOffsetAtOriginalTime(safeOffset, keptRegions);
      if (editedOffset >= editedBuffer.duration) editedOffset = 0;
      const source = context.createBufferSource();
      source.buffer = editedBuffer;
      source.connect(context.destination);
      source.onended = () => {
        setIsPlaying(false);
        playbackOffsetRef.current = 0;
        setCurrentTime(0);
      };
      startedAtRef.current = context.currentTime;
      playbackOffsetRef.current = safeOffset;
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
    } catch (error) {
      setIsPlaying(false);
      setMessage(`Unable to play audio: ${String(error)}`);
    }
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

  const removeRecording = (id: string) => {
    setRecordings((current) => current.filter((recording) => recording.id !== id));
    setRecordingContextMenu(null);
    setMessage("Recording deleted from the library");
  };

  const exportTracks = async (tracks: Recording[], singleTrack = false) => {
    if (!tracks.length || exportInProgressRef.current || !canExportTracks(tracks)) return;
    exportInProgressRef.current = true;
    setIsProcessing(true);
    stopPlayback(true);
    try {
      const destinationDir = await open({
        directory: true,
        multiple: false,
        defaultPath: getLastExportDirectory() ?? undefined,
        title: "Choose a folder for exported audio",
      });
      audioContextNeedsResetRef.current = true;
      if (!destinationDir || Array.isArray(destinationDir)) {
        setMessage("Export cancelled");
        return;
      }
      rememberLastExportDirectory(destinationDir);
      const results = await invoke<ExportResult[]>("export_edits", {
        destinationDir,
        edits: tracks.map((track) => ({
          recordingId: track.id,
          name: track.name,
          sourcePath: track.path,
          deletedRegions: combineRegions(track.manualDeletedRegions, track.silenceRegions),
          envelopePoints: track.envelopePoints,
        })),
      });
      const failures = results.filter((result) => result.error);
      const successfulSignatures = new Map(
        results
          .filter((result) => !result.error && result.path)
          .map((result) => {
            const track = tracks.find((candidate) => candidate.id === result.recordingId);
            return [result.recordingId, track ? exportSignature(track) : ""] as const;
          })
          .filter((entry) => entry[1]),
      );
      if (successfulSignatures.size) {
        const markExported = (track: Recording): Recording => {
          const signature = successfulSignatures.get(track.id);
          return signature ? { ...track, lastExportSignature: signature } : track;
        };
        setEditorTracks((current) => current.map(markExported));
        setRecordings((current) => current.map(markExported));
      }
      if (failures.length) {
        const failedTrack = tracks.find((track) => track.id === failures[0].recordingId);
        setMessage(`Exported ${results.length - failures.length}/${results.length} tracks. ${failedTrack?.name ?? "A track"} failed: ${failures[0].error}`);
      } else {
        setMessage(singleTrack
          ? `Exported ${tracks[0].name} to ${destinationDir}`
          : `Exported ${results.length} changed tracks to ${destinationDir}`);
      }
    } catch (error) {
      setMessage(String(error));
    } finally {
      exportInProgressRef.current = false;
      setIsProcessing(false);
    }
  };

  const exportChangedTracks = () => {
    const changedTracks = editorTracks.filter(needsExport);
    if (!changedTracks.length) {
      setMessage("All editor tracks are already exported.");
      return;
    }
    void exportTracks(changedTracks);
  };

  const exportSingleTrack = (trackId: string) => {
    const track = editorTracks.find((candidate) => candidate.id === trackId);
    setTrackContextMenu(null);
    if (track) void exportTracks([track], true);
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
              <article key={recording.id} className="recording-item" draggable onDragStart={(event) => event.dataTransfer.setData("application/simple-audio-cut-recording", recording.id)} onContextMenu={(event) => {
                event.preventDefault();
                setRecordingContextMenu({
                  recordingId: recording.id,
                  x: Math.min(event.clientX, window.innerWidth - 132),
                  y: Math.min(event.clientY, window.innerHeight - 44),
                });
              }}>
                <div className="recording-details"><input aria-label="Recording name" className="recording-name" defaultValue={recording.name} onBlur={(event) => renameRecording(recording.id, event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }} onDragStart={(event) => event.stopPropagation()} /><span>{formatTimeStandard(recording.durationSeconds)}</span></div>
                <div className="recording-meta"><span className="lufs-badge">{recording.integratedLufs?.toFixed(1) ?? "--"} LUFS</span><span className={recording.denoiseStatus === "queued" || recording.denoiseStatus === "processing" ? "denoise-processing" : recording.denoiseStatus === "failed" ? "denoise-failed" : ""}>{denoiseLabel(recording)}</span></div>
              </article>
            ))}
          </div>
        </div>}
        {recordingContextMenu && <div ref={recordingContextMenuRef} className="recording-context-menu" role="menu" style={{ left: recordingContextMenu.x, top: recordingContextMenu.y }}>
          <button type="button" role="menuitem" onClick={() => removeRecording(recordingContextMenu.recordingId)}>Delete</button>
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
            <button type="button" className="export-button" onClick={exportChangedTracks} disabled={isProcessing || pendingExportCount === 0 || !canExportTracks(editorTracks)} title={pendingExportCount === 0 ? "All tracks are up to date" : "Export new and changed tracks"}>Export all ({pendingExportCount})</button>
          </div>}
        </header>
        <div className="editor-content">
          {editorTracks.length > 0 ? <div className="editor-tracks">{editorTracks.map((track) => {
            const deletedRegions = combineRegions(track.manualDeletedRegions, track.silenceRegions);
            const isSelected = track.id === selectedTrackId;
            return <EditorTrack key={track.id} name={track.name} status={denoiseLabel(track)} statusKind={track.denoiseStatus} exportPending={needsExport(track)} buffer={track.buffer} selected={isSelected} collapsed={track.collapsed} currentTime={isSelected ? currentTime : 0} pixelsPerSecond={track.pixelsPerSecond} deletedRegions={deletedRegions} envelopePoints={track.envelopePoints} silenceThresholdDb={track.silenceThresholdDb} onSelect={() => {
              if (track.id === selectedTrackId) return;
              stopPlayback(false);
              setSelectedTrackId(track.id);
              setCurrentTime(0);
              playbackOffsetRef.current = 0;
              editedPlaybackOffsetRef.current = 0;
            }} onScaleContextMenu={(x, y) => {
              setTrackContextMenu({
                trackId: track.id,
                x: Math.max(4, Math.min(x, window.innerWidth - 144)),
                y: Math.max(4, Math.min(y, window.innerHeight - 44)),
              });
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
        {trackContextMenu && <div ref={trackContextMenuRef} className="track-context-menu" role="menu" style={{ left: trackContextMenu.x, top: trackContextMenu.y }}>
          <button type="button" role="menuitem" disabled={isProcessing || !canExportTracks(editorTracks.filter((track) => track.id === trackContextMenu.trackId))} onClick={() => exportSingleTrack(trackContextMenu.trackId)}>Export track</button>
        </div>}
      </section>
    </main>
  );
}

export default App;
