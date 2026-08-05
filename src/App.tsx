import { useEffect, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { confirm as confirmDialog, open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import EditorTrack from "./components/EditorTrack";
import DenoiseSettings, { type DenoiseProviderStatus } from "./components/DenoiseSettings";
import Icon from "./components/Icon";
import { combineRegions, mergeRegions, regionIsCovered, subtractRegion, type EnvelopePoint, type Region } from "./utils/regionUtils";
import { formatTimeStandard } from "./utils/timeUtils";
import { dbfsToAmplitude, detectSilence } from "./utils/audioAnalysis";
import { createEditedBuffer, editedOffsetAtOriginalTime, getKeptRegions, originalTimeAtEditedOffset } from "./utils/exportUtils";
import { appendUniqueTrack } from "./utils/trackUtils";
import { applyDenoiseUpdate, canExportTracks, type DenoiseState } from "./utils/denoiseUtils";
import { exportSignature, getLastExportDirectory, needsExport, rememberLastExportDirectory } from "./utils/exportState";
import { fileBasename, fileStem, importQueuedSummary, selectedPaths } from "./utils/importUtils";
import { AudioContextManager, BrowserAudioContextFactory } from "./audio/audioContextManager";
import { savedPlaybackPosition } from "./utils/playbackUtils";
import { addEditPoint } from "./utils/directEdit";
import "./App.css";

const APP_ICON_URL = new URL("../src-tauri/icons/icon.png", import.meta.url).href;

interface RecordingInfo {
  id: string;
  name: string;
  path: string;
  durationSeconds: number;
  integratedLufs: number | null;
}

type ImportStatus = "normalizing" | "ready" | "failed";

interface Recording extends RecordingInfo, DenoiseState {
  buffer: AudioBuffer | null;
  importStatus: ImportStatus;
  importError: string | null;
  sourcePath: string | null;
  pixelsPerSecond: number;
  manualDeletedRegions: Region[];
  silenceRegions: Region[];
  mutedRegions: Region[];
  silenceDetectionEnabled: boolean;
  silenceThresholdDb: number;
  minimumSilenceDurationMs: number;
  envelopePoints: EnvelopePoint[];
  editPoints: number[];
  selectedRange: Region | null;
  collapsed: boolean;
  lastExportSignature: string | null;
}

type ReadyRecording = Recording & { buffer: AudioBuffer; importStatus: "ready"; importError: null };

interface DenoiseResult {
  recordingId: string;
  taskId: string;
  path: string;
  integratedLufs: number | null;
}

interface DenoiseAvailability {
  available: boolean;
  providerName: string;
  modelName: string | null;
  reason: string | null;
}

type ImportEvent =
  | { status: "normalizing"; recordingId: string }
  | { status: "complete"; info: RecordingInfo }
  | { status: "failed"; recordingId: string; sourcePath: string; error: string };

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

interface TrackContextMenu {
  trackId: string;
  x: number;
  y: number;
  kind: "export" | "actions";
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

function denoiseLabel(recording: Recording) {
  if (recording.importStatus === "normalizing") return "Normalizing";
  if (recording.importStatus === "failed") return "Import failed";
  if (recording.denoiseStatus === "queued") return "Denoise queued";
  if (recording.denoiseStatus === "processing") return "Denoising";
  if (recording.denoiseStatus === "unavailable") return "No denoise";
  return recording.denoiseStatus === "complete" ? "ClearVoice" : "Denoise failed";
}

function recordingStatusClass(recording: Recording) {
  if (recording.importStatus === "normalizing") return "denoise-processing";
  if (recording.importStatus === "failed" || recording.denoiseStatus === "failed") return "denoise-failed";
  return recording.denoiseStatus === "queued" || recording.denoiseStatus === "processing" ? "denoise-processing" : "";
}

function isRecordingReady(recording: Recording): recording is ReadyRecording {
  return recording.importStatus === "ready" && recording.buffer !== null;
}

function createImportPlaceholder(sourcePath: string, recordingId: string): Recording {
  return {
    id: recordingId,
    name: fileStem(sourcePath) || fileBasename(sourcePath),
    path: sourcePath,
    durationSeconds: 0,
    integratedLufs: null,
    buffer: null,
    importStatus: "normalizing",
    importError: null,
    sourcePath,
    pixelsPerSecond: 48,
    manualDeletedRegions: [],
    silenceRegions: [],
    mutedRegions: [],
    silenceDetectionEnabled: false,
    silenceThresholdDb: -36,
    minimumSilenceDurationMs: 200,
    envelopePoints: [],
    editPoints: [],
    selectedRange: null,
    collapsed: false,
    lastExportSignature: null,
    denoiseStatus: "unavailable",
    denoiseTaskId: "",
  };
}

function App() {
  const [recordings, setRecordings] = useState<Recording[]>([]);
  const [editorTracks, setEditorTracks] = useState<ReadyRecording[]>([]);
  const [selectedTrackId, setSelectedTrackId] = useState<string | null>(null);
  const [selectedSourceId, setSelectedSourceId] = useState<string | null>(null);
  const [isRecording, setIsRecording] = useState(false);
  const [isRecordingTransitioning, setIsRecordingTransitioning] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [recordingSeconds, setRecordingSeconds] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [message, setMessage] = useState("Ready to record");
  const [targetLufs, setTargetLufs] = useState(-14);
  const [bladeActive, setBladeActive] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [denoiseProviderStatus, setDenoiseProviderStatus] = useState<DenoiseProviderStatus | null>(null);
  const [exportingTrackIds, setExportingTrackIds] = useState<string[]>([]);
  const [failedExportTrackIds, setFailedExportTrackIds] = useState<string[]>([]);
  const [trackContextMenu, setTrackContextMenu] = useState<TrackContextMenu | null>(null);

  const audioContextManagerRef = useRef<AudioContextManager<AudioContext> | null>(null);
  const playbackContextRef = useRef<AudioContext | null>(null);
  const sourceNodeRef = useRef<AudioBufferSourceNode | null>(null);
  const recordingTransitionRef = useRef(false);
  const startedAtRef = useRef(0);
  const playbackOffsetRef = useRef(0);
  const editedPlaybackOffsetRef = useRef(0);
  const playbackKeptRegionsRef = useRef<Region[]>([]);
  const playbackRequestRef = useRef(0);
  const animationFrameRef = useRef<number | null>(null);
  const recordingStartedAtRef = useRef(0);
  const trackContextMenuRef = useRef<HTMLDivElement>(null);
  const editorTracksRef = useRef<ReadyRecording[]>([]);
  const completeImportRef = useRef<((info: RecordingInfo) => Promise<void>) | null>(null);
  const selectedTrackIdRef = useRef<string | null>(null);
  const importListenerReadyRef = useRef<ReturnType<typeof createDeferred> | null>(null);
  const denoiseListenerReadyRef = useRef<ReturnType<typeof createDeferred> | null>(null);
  const currentDenoiseTasksRef = useRef(new Map<string, string>());
  const activeDenoiseRecordingsRef = useRef(new Set<string>());
  const removedRecordingIdsRef = useRef(new Set<string>());
  const exportInProgressRef = useRef(false);
  const closePromptOpenRef = useRef(false);
  const hasUnsavedWorkRef = useRef(false);
  if (!importListenerReadyRef.current) importListenerReadyRef.current = createDeferred();
  if (!denoiseListenerReadyRef.current) denoiseListenerReadyRef.current = createDeferred();
  editorTracksRef.current = editorTracks;
  selectedTrackIdRef.current = selectedTrackId;
  const selectedTrack = editorTracks.find((track) => track.id === selectedTrackId) ?? null;
  const selectedSource = recordings.find((recording) => recording.id === selectedSourceId) ?? null;
  const selectedDeletedRegions = selectedTrack
    ? combineRegions(selectedTrack.manualDeletedRegions, selectedTrack.silenceRegions)
    : [];
  const selectedSilenceThresholdDb = selectedTrack?.silenceThresholdDb ?? -36;
  const selectedMinimumSilenceDurationMs = selectedTrack?.minimumSilenceDurationMs ?? 200;
  const pendingImportCount = recordings.filter((recording) => recording.importStatus === "normalizing").length;
  const pendingDenoiseCount = [...recordings, ...editorTracks].filter((recording) => recording.denoiseStatus === "queued" || recording.denoiseStatus === "processing").length;
  const pendingExportCount = editorTracks.filter(needsExport).length;
  const unexportedSessionCount = [...recordings, ...editorTracks].filter(isRecordingReady).filter(needsExport).length;
  const activeTaskCount = pendingImportCount + pendingDenoiseCount + (isRecording || isRecordingTransitioning ? 1 : 0) + (isProcessing ? 1 : 0);
  const hasUnsavedWork = activeTaskCount > 0 || isRecordingTransitioning || unexportedSessionCount > 0;
  hasUnsavedWorkRef.current = hasUnsavedWork;
  if (!audioContextManagerRef.current) {
    audioContextManagerRef.current = new AudioContextManager(new BrowserAudioContextFactory());
  }

  useEffect(() => {
    const manager = audioContextManagerRef.current
      ?? new AudioContextManager(new BrowserAudioContextFactory());
    audioContextManagerRef.current = manager;
    const suppressWebViewContextMenu = (event: MouseEvent) => event.preventDefault();
    document.addEventListener("contextmenu", suppressWebViewContextMenu, true);
    return () => {
      document.removeEventListener("contextmenu", suppressWebViewContextMenu, true);
      playbackRequestRef.current += 1;
      if (animationFrameRef.current !== null) cancelAnimationFrame(animationFrameRef.current);
      animationFrameRef.current = null;
      if (sourceNodeRef.current) {
        sourceNodeRef.current.onended = null;
        try { sourceNodeRef.current.stop(); } catch { /* Already stopped. */ }
      }
      sourceNodeRef.current = null;
      playbackContextRef.current = null;
      playbackKeptRegionsRef.current = [];
      if (audioContextManagerRef.current === manager) audioContextManagerRef.current = null;
      void manager.close().catch(() => undefined);
    };
  }, []);

  const getAudioContext = async () => {
    const manager = audioContextManagerRef.current;
    if (!manager) throw new Error("Audio context is unavailable.");
    return manager.get();
  };

  const decodeAudioData = (audioData: ArrayBuffer) => {
    const manager = audioContextManagerRef.current;
    if (!manager) throw new Error("Audio context is unavailable.");
    return manager.run((context) => context.decodeAudioData(audioData));
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
        const confirmed = await confirmDialog("Recording, processing, or unexported changes are still active. Quit anyway?", {
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
    let disposed = false;
    void invoke<DenoiseProviderStatus>("denoise_provider_status")
      .then((status) => {
        if (!disposed) setDenoiseProviderStatus(status);
      })
      .catch(() => undefined);
    return () => { disposed = true; };
  }, []);

  const stopPlayback = (savePosition: boolean) => {
    playbackRequestRef.current += 1;
    const source = sourceNodeRef.current;
    if (source) {
      source.onended = null;
      try { source.stop(); } catch { /* Already stopped. */ }
      sourceNodeRef.current = null;
    }
    if (animationFrameRef.current !== null) {
      cancelAnimationFrame(animationFrameRef.current);
      animationFrameRef.current = null;
    }
    const context = playbackContextRef.current;
    const position = context
      ? savedPlaybackPosition(Boolean(source) && savePosition, {
        contextTime: context.currentTime,
        startedAt: startedAtRef.current,
        editedOffset: editedPlaybackOffsetRef.current,
        keptRegions: playbackKeptRegionsRef.current,
      })
      : null;
    if (position !== null) {
      playbackOffsetRef.current = position;
      setCurrentTime(position);
    }
    playbackContextRef.current = null;
    playbackKeptRegionsRef.current = [];
    startedAtRef.current = 0;
    editedPlaybackOffsetRef.current = 0;
    setIsPlaying(false);
  };

  const decodeRecording = async (info: RecordingInfo): Promise<ReadyRecording> => {
    const response = await fetch(convertFileSrc(info.path));
    if (!response.ok) throw new Error("Unable to load the completed recording.");
    const audioData = await response.arrayBuffer();
    const buffer = await decodeAudioData(audioData);
    return {
      ...info,
      buffer,
      importStatus: "ready" as const,
      importError: null,
      sourcePath: null,
      pixelsPerSecond: 48,
      durationSeconds: info.durationSeconds || buffer.duration,
      manualDeletedRegions: [],
      silenceRegions: [],
      mutedRegions: [],
      silenceDetectionEnabled: false,
      silenceThresholdDb: -36,
      minimumSilenceDurationMs: 200,
      envelopePoints: [],
      editPoints: [],
      selectedRange: null,
      collapsed: false,
      lastExportSignature: null,
      denoiseStatus: "queued" as const,
      denoiseTaskId: "",
    };
  };

  const completeImport = async (info: RecordingInfo) => {
    if (removedRecordingIdsRef.current.has(info.id)) return;
    const recording = await decodeRecording(info);
    if (removedRecordingIdsRef.current.has(info.id)) return;
    const mergeReady = (current: Recording): Recording => current.id === recording.id
      ? {
        ...current,
        ...recording,
        name: current.name.trim() || recording.name,
        sourcePath: current.sourcePath,
      }
      : current;
    setRecordings((current) => current.map(mergeReady));
    if (editorTracksRef.current.some((track) => track.id === recording.id)) {
      const nextTracks = editorTracksRef.current.map((track) => track.id === recording.id
        ? { ...track, ...recording, name: track.name.trim() || recording.name }
        : track);
      editorTracksRef.current = nextTracks;
      setEditorTracks(nextTracks);
    }
    void queueDenoise(recording);
    setMessage(`${recording.name} is ready in the media pool.`);
  };
  completeImportRef.current = completeImport;

  const completeDenoise = async (result: DenoiseResult) => {
    if (removedRecordingIdsRef.current.has(result.recordingId)) return;
    if (currentDenoiseTasksRef.current.get(result.recordingId) !== result.taskId) return;
    if (selectedTrackIdRef.current === result.recordingId) {
      stopPlayback(true);
    }
    const response = await fetch(convertFileSrc(result.path));
    if (!response.ok) throw new Error("Unable to load ClearVoice output.");
    const audioData = await response.arrayBuffer();
    const buffer = await decodeAudioData(audioData);
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
    const nextTracks = editorTracksRef.current.map((recording) => replace(recording) as ReadyRecording);
    editorTracksRef.current = nextTracks;
    setEditorTracks(nextTracks);
  };

  const queueDenoise = async (recording: ReadyRecording) => {
    if (removedRecordingIdsRef.current.has(recording.id)) return;
    if (activeDenoiseRecordingsRef.current.has(recording.id)) return;
    activeDenoiseRecordingsRef.current.add(recording.id);
    const taskId = crypto.randomUUID();
    currentDenoiseTasksRef.current.set(recording.id, taskId);
    const markQueued = (current: Recording): Recording => current.id === recording.id
      ? { ...current, denoiseStatus: "queued", denoiseTaskId: taskId }
      : current;
    setRecordings((current) => current.map(markQueued));
    const queuedTracks = editorTracksRef.current.map((track) => markQueued(track) as ReadyRecording);
    editorTracksRef.current = queuedTracks;
    setEditorTracks(queuedTracks);
    try {
      const availability = await invoke<DenoiseAvailability>("denoise_availability", {
        sampleRate: recording.buffer.sampleRate,
      });
      if (!availability.available) {
        const markUnavailable = (current: Recording) => applyDenoiseUpdate(current, recording.id, taskId, {
          denoiseStatus: "unavailable",
        });
        setRecordings((current) => current.map(markUnavailable));
        const unavailableTracks = editorTracksRef.current.map((track) => markUnavailable(track) as ReadyRecording);
        editorTracksRef.current = unavailableTracks;
        setEditorTracks(unavailableTracks);
        if (currentDenoiseTasksRef.current.get(recording.id) === taskId) {
          setMessage(availability.reason
            ? `${recording.name}: ${availability.reason}`
            : `${recording.name} will use normalized source audio.`);
        }
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
      if (currentDenoiseTasksRef.current.get(recording.id) === taskId) {
        setMessage(`${recording.name}: denoising complete.`);
      }
    } catch (error) {
      const update = (current: Recording) => applyDenoiseUpdate(current, recording.id, taskId, { denoiseStatus: "failed" });
      setRecordings((current) => current.map(update));
      const failedTracks = editorTracksRef.current.map((track) => update(track) as ReadyRecording);
      editorTracksRef.current = failedTracks;
      setEditorTracks(failedTracks);
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
    if (isRecording || recordingTransitionRef.current) return;
    recordingTransitionRef.current = true;
    setIsRecordingTransitioning(true);
    try {
      await invoke("start_recording");
      recordingStartedAtRef.current = Date.now();
      setRecordingSeconds(0);
      setIsRecording(true);
      setMessage("Recording from the default microphone");
    } catch (error) {
      setMessage(String(error));
    } finally {
      recordingTransitionRef.current = false;
      setIsRecordingTransitioning(false);
    }
  };

  const stopRecording = async () => {
    if (!isRecording || recordingTransitionRef.current) return;
    recordingTransitionRef.current = true;
    setIsRecordingTransitioning(true);
    setIsRecording(false);
    const placeholderId = crypto.randomUUID();
    const placeholder: Recording = {
      ...createImportPlaceholder("recording.wav", placeholderId),
      name: `Recording ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`,
      sourcePath: null,
    };
    setRecordings((current) => [...current, placeholder]);
    setMessage(`Measuring loudness and normalizing to ${targetLufs} LUFS`);
    try {
      const recording = await decodeRecording(await invoke<RecordingInfo>("stop_recording", { targetLufs }));
      setRecordings((current) => current.map((item) => item.id === placeholderId ? recording : item));
      void queueDenoise(recording);
      setMessage("Recording is ready in the media pool.");
    } catch (error) {
      const failure = String(error);
      setRecordings((current) => current.map((item) => item.id === placeholderId
        ? { ...item, importStatus: "failed", importError: failure, denoiseStatus: "failed" }
        : item));
      setMessage(failure);
    } finally {
      recordingTransitionRef.current = false;
      setIsRecordingTransitioning(false);
    }
  };

  const startImportJob = async (recordingId: string, sourcePath: string) => {
    try {
      await importListenerReadyRef.current?.promise;
      await invoke("start_import_audio", { recordingId, sourcePath, targetLufs });
    } catch (error) {
      const failure = String(error);
      setRecordings((current) => current.map((recording) => recording.id === recordingId
        ? { ...recording, importStatus: "failed", importError: failure, denoiseStatus: "failed" }
        : recording));
      setMessage(`Import failed for ${fileBasename(sourcePath)}: ${failure}`);
    }
  };

  const importAudio = async () => {
    stopPlayback(true);
    let selection: string | string[] | null;
    try {
      selection = await open({
        multiple: true,
        filters: [{ name: "Audio or video", extensions: ["wav", "mp3", "m4a", "aac", "flac", "ogg", "opus", "aiff", "mp4", "mov", "mkv", "webm", "m4v", "avi"] }],
      });
    } catch (error) {
      setMessage(String(error));
      return;
    } finally {
      audioContextManagerRef.current?.requestReset();
    }
    const paths = selectedPaths(selection);
    if (!paths.length) return;
    const imports = paths.map((sourcePath) => ({ sourcePath, recordingId: crypto.randomUUID() }));
    setRecordings((current) => [...current, ...imports.map(({ sourcePath, recordingId }) => createImportPlaceholder(sourcePath, recordingId))]);
    setMessage(importQueuedSummary(imports.length));
    for (const { sourcePath, recordingId } of imports) {
      void startImportJob(recordingId, sourcePath);
    }
  };

  useEffect(() => {
    const importShortcut = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "i" || settingsOpen || isRecording) return;
      event.preventDefault();
      void importAudio();
    };
    window.addEventListener("keydown", importShortcut);
    return () => window.removeEventListener("keydown", importShortcut);
  });

  const retryImport = (recording: Recording) => {
    if (!recording.sourcePath) return;
    setRecordings((current) => current.map((item) => item.id === recording.id
      ? { ...item, importStatus: "normalizing", importError: null, denoiseStatus: "unavailable" }
      : item));
    setMessage(`Retrying ${recording.name}`);
    void startImportJob(recording.id, recording.sourcePath);
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    const listenerReady = importListenerReadyRef.current;
    void listen<ImportEvent>("import-status", async ({ payload }) => {
      if (payload.status === "normalizing") {
        setRecordings((current) => current.map((recording) => recording.id === payload.recordingId
          ? { ...recording, importStatus: "normalizing", importError: null }
          : recording));
        return;
      }
      if (payload.status === "failed") {
        setRecordings((current) => current.map((recording) => recording.id === payload.recordingId
          ? { ...recording, importStatus: "failed", importError: payload.error, denoiseStatus: "failed" }
          : recording));
        setMessage(`Import failed for ${fileBasename(payload.sourcePath)}: ${payload.error}`);
        return;
      }
      try {
        await completeImportRef.current?.(payload.info);
      } catch (error) {
        const failure = String(error);
        setRecordings((current) => current.map((recording) => recording.id === payload.info.id
          ? { ...recording, importStatus: "failed", importError: failure, denoiseStatus: "failed" }
          : recording));
        setMessage(`Imported file could not be decoded: ${failure}`);
      }
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
      setMessage(`Unable to monitor imports: ${String(error)}`);
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    const listenerReady = denoiseListenerReadyRef.current;
    void listen<DenoiseEvent>("denoise-status", async ({ payload }) => {
      const markProcessing = (recording: Recording) => applyDenoiseUpdate(recording, payload.recordingId, payload.taskId, {
        denoiseStatus: "processing",
      });
      setRecordings((current) => current.map(markProcessing));
      const processingTracks = editorTracksRef.current.map((recording) => markProcessing(recording) as ReadyRecording);
      editorTracksRef.current = processingTracks;
      setEditorTracks(processingTracks);
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

  const startPlayback = async (offset: number) => {
    if (!selectedTrack) return;
    stopPlayback(false);
    const playbackRequest = playbackRequestRef.current;
    try {
      const context = await getAudioContext();
      if (playbackRequest !== playbackRequestRef.current) return;
      if (context.state === "suspended") await context.resume();
      if (playbackRequest !== playbackRequestRef.current) return;
      const keptRegions = getKeptRegions(selectedDeletedRegions, selectedTrack.buffer.duration);
      const editedBuffer = createEditedBuffer(selectedTrack.buffer, selectedDeletedRegions, selectedTrack.envelopePoints, selectedTrack.mutedRegions);
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
        if (sourceNodeRef.current !== source || playbackRequest !== playbackRequestRef.current) return;
        sourceNodeRef.current = null;
        if (animationFrameRef.current !== null) {
          cancelAnimationFrame(animationFrameRef.current);
          animationFrameRef.current = null;
        }
        startedAtRef.current = 0;
        editedPlaybackOffsetRef.current = 0;
        playbackContextRef.current = null;
        playbackKeptRegionsRef.current = [];
        setIsPlaying(false);
        playbackOffsetRef.current = 0;
        setCurrentTime(0);
      };
      startedAtRef.current = context.currentTime;
      playbackOffsetRef.current = safeOffset;
      editedPlaybackOffsetRef.current = editedOffset;
      source.start(0, editedOffset);
      sourceNodeRef.current = source;
      playbackContextRef.current = context;
      playbackKeptRegionsRef.current = keptRegions;
      setIsPlaying(true);
      const tick = () => {
        if (sourceNodeRef.current !== source || playbackRequest !== playbackRequestRef.current) return;
        const editedTime = editedPlaybackOffsetRef.current + context.currentTime - startedAtRef.current;
        if (editedTime < editedBuffer.duration) {
          setCurrentTime(originalTimeAtEditedOffset(editedTime, keptRegions));
          animationFrameRef.current = requestAnimationFrame(tick);
        }
      };
      animationFrameRef.current = requestAnimationFrame(tick);
    } catch (error) {
      if (playbackRequest !== playbackRequestRef.current) return;
      stopPlayback(false);
      setMessage(`Unable to play audio: ${String(error)}`);
    }
  };

  const updateSelectedTrack = (update: (track: ReadyRecording) => ReadyRecording) => {
    if (!selectedTrackId) return;
    const next = editorTracksRef.current.map((track) => track.id === selectedTrackId ? update(track) : track);
    editorTracksRef.current = next;
    setEditorTracks(next);
  };

  const updateTrack = (id: string, update: (track: ReadyRecording) => ReadyRecording) => {
    const next = editorTracksRef.current.map((track) => track.id === id ? update(track) : track);
    editorTracksRef.current = next;
    setEditorTracks(next);
  };

  const commitTrackEdit = (
    id: string,
    _action: string,
    update: (track: ReadyRecording) => ReadyRecording,
    _coalesce = false,
  ) => {
    const track = editorTracksRef.current.find((candidate) => candidate.id === id);
    if (!track) return;
    const next = update(track);
    if (next === track) return;
    const nextTracks = editorTracksRef.current.map((candidate) => candidate.id === id ? next : candidate);
    editorTracksRef.current = nextTracks;
    setEditorTracks(nextTracks);
  };

  const toggleSelectedRangeMute = () => {
    if (!selectedTrack?.selectedRange || selectedTrack.collapsed) return;
    const range = selectedTrack.selectedRange;
    stopPlayback(false);
    const unmute = regionIsCovered(selectedTrack.mutedRegions, range);
    commitTrackEdit(selectedTrack.id, unmute ? "unmute range" : "mute range", (track) => ({
      ...track,
      mutedRegions: unmute
        ? subtractRegion(track.mutedRegions, range)
        : mergeRegions(track.mutedRegions, range),
      selectedRange: null,
    }));
    setMessage(`${unmute ? "Restored sound to" : "Muted"} ${formatTimeStandard(range.end - range.start)}`);
  };

  const toggleTrackRangeMute = (trackId: string) => {
    const track = editorTracksRef.current.find((candidate) => candidate.id === trackId);
    if (!track?.selectedRange || track.collapsed) return;
    const range = track.selectedRange;
    stopPlayback(false);
    const unmute = regionIsCovered(track.mutedRegions, range);
    commitTrackEdit(trackId, unmute ? "unmute range" : "mute range", (current) => ({
      ...current,
      mutedRegions: unmute
        ? subtractRegion(current.mutedRegions, range)
        : mergeRegions(current.mutedRegions, range),
      selectedRange: null,
    }));
    setMessage(`${unmute ? "Restored sound to" : "Muted"} ${formatTimeStandard(range.end - range.start)}`);
  };

  const directEditTrackRange = (trackId: string, start: number, end: number, operation: "delete" | "restore") => {
    const track = editorTracksRef.current.find((candidate) => candidate.id === trackId);
    if (!track || track.collapsed || end - start <= 0.01) return;
    const range = { start, end };
    stopPlayback(false);
    commitTrackEdit(trackId, operation === "delete" ? "delete audio" : "restore audio", (current) => operation === "delete"
      ? {
        ...current,
        manualDeletedRegions: mergeRegions(current.manualDeletedRegions, range),
        selectedRange: null,
      }
      : {
        ...current,
        manualDeletedRegions: subtractRegion(current.manualDeletedRegions, range),
        silenceRegions: subtractRegion(current.silenceRegions, range),
        selectedRange: null,
      });
    setMessage(`${operation === "delete" ? "Deleted" : "Restored"} ${formatTimeStandard(end - start)} with right-drag`);
  };

  const moveToEditor = (id: string) => {
    const recording = recordings.find((item) => item.id === id);
    if (!recording) return;
    if (!isRecordingReady(recording)) {
      setMessage(recording.importStatus === "failed"
        ? `${recording.name} failed to import.`
        : `${recording.name} is still normalizing.`);
      return;
    }
    stopPlayback(false);
    const nextTracks = appendUniqueTrack(editorTracksRef.current, recording);
    editorTracksRef.current = nextTracks;
    setEditorTracks(nextTracks);
    setSelectedTrackId(recording.id);
    selectedTrackIdRef.current = recording.id;
    setSelectedSourceId(null);
    setRecordings((current) => current.filter((item) => item.id !== id));
    setCurrentTime(0);
    playbackOffsetRef.current = 0;
    editedPlaybackOffsetRef.current = 0;
    setMessage("Editing selected recording");
  };

  const selectAfterTrackRemoval = (removedId: string, selectNext = true) => {
    const nextTracks = editorTracksRef.current.filter((track) => track.id !== removedId);
    const nextTrack = selectNext ? nextTracks[0] ?? null : null;
    editorTracksRef.current = nextTracks;
    setEditorTracks(nextTracks);
    setSelectedTrackId(nextTrack?.id ?? null);
    selectedTrackIdRef.current = nextTrack?.id ?? null;
    setCurrentTime(0);
    playbackOffsetRef.current = 0;
    editedPlaybackOffsetRef.current = 0;
  };

  const returnToLibrary = () => {
    if (!selectedTrack) return;
    stopPlayback(false);
    setRecordings((current) => appendUniqueTrack(current, selectedTrack));
    setSelectedSourceId(selectedTrack.id);
    selectAfterTrackRemoval(selectedTrack.id, false);
    setMessage("Recording returned to the library");
  };

  const invalidateRemovedRecording = (id: string) => {
    removedRecordingIdsRef.current.add(id);
    currentDenoiseTasksRef.current.delete(id);
    activeDenoiseRecordingsRef.current.delete(id);
  };

  const removeTrack = (trackId: string) => {
    const track = editorTracksRef.current.find((candidate) => candidate.id === trackId);
    if (!track) return;
    stopPlayback(false);
    invalidateRemovedRecording(track.id);
    selectAfterTrackRemoval(track.id);
    setMessage(`${track.name} deleted from the timeline`);
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
    const recording = recordings.find((item) => item.id === id);
    if (!recording) return;
    invalidateRemovedRecording(recording.id);
    setRecordings((current) => current.filter((recording) => recording.id !== id));
    setSelectedSourceId(null);
    setMessage(`${recording.name} deleted from the source shelf`);
  };

  const deleteSelectedItem = () => {
    if (selectedSource) {
      removeRecording(selectedSource.id);
      return;
    }
    if (selectedTrack) removeTrack(selectedTrack.id);
  };

  const exportTracks = async (tracks: Recording[], singleTrack = false) => {
    if (!tracks.length || exportInProgressRef.current || !canExportTracks(tracks)) return;
    exportInProgressRef.current = true;
    setIsProcessing(true);
    setExportingTrackIds(tracks.map((track) => track.id));
    setFailedExportTrackIds((current) => current.filter((id) => !tracks.some((track) => track.id === id)));
    stopPlayback(true);
    try {
      let destinationDir: string | string[] | null;
      try {
        destinationDir = await open({
          directory: true,
          multiple: false,
          defaultPath: getLastExportDirectory() ?? undefined,
          title: "Choose a folder for exported audio",
        });
      } finally {
        audioContextManagerRef.current?.requestReset();
      }
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
          mutedRegions: track.mutedRegions,
          envelopePoints: track.envelopePoints,
        })),
      });
      const failures = results.filter((result) => result.error);
      setFailedExportTrackIds(failures.map((result) => result.recordingId));
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
        const exportedTracks = editorTracksRef.current.map((track) => markExported(track) as ReadyRecording);
        editorTracksRef.current = exportedTracks;
        setEditorTracks(exportedTracks);
        setRecordings((current) => current.map(markExported));
      }
      if (failures.length) {
        const failedTrack = tracks.find((track) => track.id === failures[0].recordingId);
        setMessage(`Exported ${results.length - failures.length}/${results.length} tracks. ${failedTrack?.name ?? "A track"} failed: ${failures[0].error}`);
      } else {
        setMessage(singleTrack
          ? `Exported ${tracks[0].name} to ${destinationDir}`
          : `Exported ${results.length} changed timeline tracks to ${destinationDir}`);
      }
    } catch (error) {
      setFailedExportTrackIds(tracks.map((track) => track.id));
      setMessage(String(error));
    } finally {
      exportInProgressRef.current = false;
      setIsProcessing(false);
      setExportingTrackIds([]);
    }
  };

  const exportChangedTracks = () => {
    const changedTracks = editorTracks.filter(needsExport);
    if (!changedTracks.length) {
      setMessage("All editor tracks are already exported.");
      return;
    }
    if (!canExportTracks(changedTracks)) {
      setMessage("Changed tracks are not ready to export yet.");
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
    commitTrackEdit(selectedTrack.id, "detect silence", (track) => ({
      ...track,
      silenceRegions,
      silenceDetectionEnabled: true,
      selectedRange: null,
    }));
  };

  const updateSilenceSettings = (partial: Partial<Pick<ReadyRecording, "silenceThresholdDb" | "minimumSilenceDurationMs">>) => {
    if (!selectedTrack) return;
    const nextThreshold = partial.silenceThresholdDb ?? selectedTrack.silenceThresholdDb;
    const nextDuration = partial.minimumSilenceDurationMs ?? selectedTrack.minimumSilenceDurationMs;
    const silenceRegions = selectedTrack.silenceDetectionEnabled
      ? detectSilence(selectedTrack.buffer, {
        threshold: dbfsToAmplitude(nextThreshold),
        minDuration: nextDuration / 1_000,
      })
      : selectedTrack.silenceRegions;
    commitTrackEdit(selectedTrack.id, "silence settings", (track) => ({
      ...track,
      ...partial,
      silenceRegions,
    }), true);
  };

  const clearSilenceCuts = () => {
    if (!selectedTrack?.silenceDetectionEnabled) return;
    commitTrackEdit(selectedTrack.id, "clear silence cuts", (track) => ({
      ...track,
      silenceRegions: [],
      silenceDetectionEnabled: false,
    }));
  };

  useEffect(() => {
    const handleShortcut = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const key = event.key.toLowerCase();
      if ((event.ctrlKey || event.metaKey) && key === ",") {
        event.preventDefault();
        setSettingsOpen(true);
        return;
      }
      if (target?.closest("input, textarea, select, [contenteditable='true']") || settingsOpen) return;
      if (event.ctrlKey || event.metaKey || event.altKey) return;
      if (key === " " || key === "k") {
        event.preventDefault();
        if (!selectedTrack) return;
        if (isPlaying) stopPlayback(true);
        else void startPlayback(playbackOffsetRef.current);
        return;
      }
      if (key === "j") {
        event.preventDefault();
        const next = Math.max(0, playbackOffsetRef.current - 5);
        playbackOffsetRef.current = next;
        setCurrentTime(next);
        if (isPlaying) void startPlayback(next);
        return;
      }
      if (key === "l") {
        event.preventDefault();
        const next = Math.min(selectedTrack?.buffer.duration ?? 0, playbackOffsetRef.current + 5);
        playbackOffsetRef.current = next;
        setCurrentTime(next);
        if (selectedTrack) void startPlayback(next);
        return;
      }
      if (key === "m") {
        event.preventDefault();
        toggleSelectedRangeMute();
        return;
      }
      if (key === "b" || key === "c") {
        event.preventDefault();
        setBladeActive(true);
        setMessage("Blade active. Click the waveform to add an edit point; press A to return.");
        return;
      }
      if (key === "a" || key === "v") {
        event.preventDefault();
        setBladeActive(false);
        setMessage("Direct editing active. Drag the waveform to select a range.");
      }
    };
    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  });

  return <main className="app-shell">
    <header className="titlebar">
      <div className="titlebar-brand" data-tauri-drag-region>
        <img className="app-mark" src={APP_ICON_URL} alt="" />
        <div><h1>Simple Audio Cut</h1><span>Local spoken-word editor</span></div>
      </div>
      <div className="titlebar-tools">
        <button type="button" className={isRecording ? "record-button is-active" : "record-button"} onClick={isRecording ? stopRecording : startRecording} disabled={isProcessing || isRecordingTransitioning}>
          <Icon name={isRecording ? "pause" : "record"} />
          <span>{isRecording ? "Stop" : "Record"}</span>
          {isRecording && <strong>{formatTimeStandard(recordingSeconds)}</strong>}
        </button>
        <button type="button" className="import-button" onClick={() => void importAudio()} disabled={isRecording}>
          <Icon name="import" /><span>Import</span>
        </button>
        <label className="loudness-setting">
          <span><strong>Normalization</strong></span>
          <span className="number-field"><input aria-label="Normalization target" type="number" min="-70" max="-5" step="1" value={targetLufs} disabled={isRecording || pendingImportCount > 0} onChange={(event) => setTargetLufs(Math.max(-70, Math.min(-5, Number(event.target.value) || -14)))} /><b>LUFS</b></span>
        </label>
        <button type="button" className="delete-selection-button" onClick={deleteSelectedItem} disabled={isProcessing || (!selectedSource && !selectedTrack)} aria-label={selectedSource ? "Delete selected source" : selectedTrack ? "Delete selected track" : "Delete selected item"} title={selectedSource ? `Delete source: ${selectedSource.name}` : selectedTrack ? `Delete track: ${selectedTrack.name}` : "Select a source or track to delete"}><Icon name="remove" /></button>
      </div>
      <div className="titlebar-actions">
        <div className="titlebar-session" data-tauri-drag-region>
          <span className={hasUnsavedWork ? "session-dot has-changes" : "session-dot"} />
          {hasUnsavedWork ? [unexportedSessionCount > 0 ? `${unexportedSessionCount} unexported` : "", activeTaskCount > 0 ? `${activeTaskCount} processing` : ""].filter(Boolean).join(" · ") : "Up to date"}
        </div>
        <button type="button" className={`provider-button ${denoiseProviderStatus?.ready ? "is-ready" : "needs-setup"}`} onClick={() => setSettingsOpen(true)} title={denoiseProviderStatus?.summary ?? "Open denoising settings"}>
          <span className="provider-status-dot" />{denoiseProviderStatus?.providerName ?? "Denoising"}<Icon name="settings" />
        </button>
      </div>
    </header>

    <div className="workspace">
      <aside className="media-panel" aria-label="Media pool">
        <div className="media-list-scroll">
          {recordings.length === 0 ? <div className="media-empty">
            <Icon name="volume" size={20} />
            <span><strong>No source media</strong><small>Record or import audio to begin</small></span>
          </div> : <div className="media-list">{recordings.map((recording) => {
            const ready = isRecordingReady(recording);
            const processing = recording.importStatus === "normalizing" || recording.denoiseStatus === "queued" || recording.denoiseStatus === "processing";
            const selectSource = () => {
              if (selectedTrackId) stopPlayback(false);
              setSelectedSourceId(recording.id);
              setSelectedTrackId(null);
              selectedTrackIdRef.current = null;
            };
            return <article key={recording.id} className={`media-item ${ready ? "is-ready" : "is-unavailable"} ${recording.id === selectedSourceId ? "is-selected" : ""} ${recording.importStatus === "failed" ? "has-error" : ""}`} draggable={ready} aria-current={recording.id === selectedSourceId ? "true" : undefined} aria-disabled={!ready} onMouseDownCapture={selectSource} onFocusCapture={selectSource} onDragStart={(event) => {
              if (!ready) return event.preventDefault();
              event.dataTransfer.setData("application/simple-audio-cut-recording", recording.id);
            }}>
              <div className="media-item-icon"><Icon name={recording.importStatus === "failed" ? "warning" : "volume"} /></div>
              <div className="media-item-body">
                <input aria-label="Media name" className="media-name" value={recording.name} disabled={!ready} onChange={(event) => setRecordings((current) => current.map((item) => item.id === recording.id ? { ...item, name: event.target.value } : item))} onBlur={(event) => renameRecording(recording.id, event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }} onDragStart={(event) => event.stopPropagation()} />
                <div className="media-item-meta"><span>{ready ? formatTimeStandard(recording.durationSeconds) : "--:--"}</span><span>{recording.integratedLufs?.toFixed(1) ?? "--"} LUFS</span></div>
                <div className={`media-state ${recordingStatusClass(recording)}`}>{processing && <span className="activity-spinner" />}{denoiseLabel(recording)}</div>
                {recording.importError && <p className="media-error" title={recording.importError}>{recording.importError}</p>}
              </div>
              <div className="media-item-actions">
                {recording.importStatus === "failed" && recording.sourcePath && <button type="button" onClick={() => retryImport(recording)}>Retry</button>}
                <button type="button" className="add-timeline-button" onClick={() => moveToEditor(recording.id)} disabled={!ready} title="Add to timeline" aria-label={`Add ${recording.name} to timeline`}><Icon name="add" /></button>
              </div>
              {processing && <span className="media-progress" />}
            </article>;
          })}</div>}
        </div>
      </aside>

      <section className="timeline-panel" aria-label="Timeline editor" onDragOver={(event) => event.preventDefault()} onDrop={handleDrop}>
        <header className="timeline-heading">
          <div className="timeline-title"><p className="section-kicker">Timeline</p><h2>{selectedTrack?.name ?? (editorTracks.length ? "Select a track" : "Untitled session")}</h2></div>
          <div className="timeline-playback">
            <button type="button" className="play-control" onClick={() => isPlaying ? stopPlayback(true) : void startPlayback(playbackOffsetRef.current)} disabled={!selectedTrack} title="Play / pause (Space)"><Icon name={isPlaying ? "pause" : "play"} /></button>
            <span className="timecode"><strong>{formatTimeStandard(currentTime)}</strong><i>/</i><span>{formatTimeStandard(selectedTrack?.buffer.duration ?? 0)}</span></span>
          </div>
          <div className="timeline-heading-meta"><span>{editorTracks.length} {editorTracks.length === 1 ? "track" : "tracks"}</span><span>{pendingExportCount} changed</span></div>
          <button type="button" className="export-button" onClick={exportChangedTracks} disabled={isProcessing || pendingExportCount === 0}>
            {isProcessing ? <span className="activity-spinner" /> : <Icon name="export" />}<span>{isProcessing ? "Exporting" : "Export changed"}</span>{pendingExportCount > 0 && <b>{pendingExportCount}</b>}
          </button>
        </header>

        {selectedTrack && <div className="inspector-bar">
          <div className="inspector-group silence-inspector">
            <span className="inspector-label">Silence</span>
            <label><span>Threshold</span><input type="number" min="-70" max="-12" step="1" value={selectedSilenceThresholdDb} onChange={(event) => updateSilenceSettings({ silenceThresholdDb: Math.max(-70, Math.min(-12, Number(event.target.value))) })} /><b>dBFS</b></label>
            <label><span>Minimum</span><input type="number" min="0" max="5000" step="10" value={selectedMinimumSilenceDurationMs} onChange={(event) => updateSilenceSettings({ minimumSilenceDurationMs: Math.max(0, Math.min(5_000, Number(event.target.value))) })} /><b>ms</b></label>
            <button type="button" className={selectedTrack.silenceDetectionEnabled ? "compact-button is-active" : "compact-button"} onClick={selectedTrack.silenceDetectionEnabled ? clearSilenceCuts : markSilence}>{selectedTrack.silenceDetectionEnabled ? "Silence cuts on" : "Detect silence"}</button>
          </div>
          <div className="direct-edit-hint"><span>Right-drag right: delete · left: restore</span><span>Click yellow curve to add a point</span><span>Drag points to shape loudness</span></div>
          <div className="inspector-group track-actions">
            <button type="button" className="track-menu-button" onClick={(event) => setTrackContextMenu({ trackId: selectedTrack.id, x: Math.max(4, event.currentTarget.getBoundingClientRect().right - 174), y: event.currentTarget.getBoundingClientRect().bottom + 4, kind: "actions" })} aria-label="Track actions" title="Track actions">•••</button>
          </div>
        </div>}

        <div className={bladeActive ? "timeline-content is-blade-active" : "timeline-content"}>
          {editorTracks.length > 0 ? <div className="editor-tracks">{editorTracks.map((track) => {
            const deletedRegions = combineRegions(track.manualDeletedRegions, track.silenceRegions);
            const isSelected = track.id === selectedTrackId;
            const exportState = exportingTrackIds.includes(track.id)
              ? "exporting"
              : failedExportTrackIds.includes(track.id)
                ? "failed"
                : needsExport(track)
                  ? "pending"
                  : "exported";
            return <EditorTrack key={track.id} name={track.name} status={denoiseLabel(track)} statusKind={track.denoiseStatus} exportState={exportState} buffer={track.buffer} selected={isSelected} collapsed={track.collapsed} currentTime={isSelected ? currentTime : 0} bladeActive={bladeActive} pixelsPerSecond={track.pixelsPerSecond} deletedRegions={deletedRegions} mutedRegions={track.mutedRegions} editPoints={track.editPoints} selectedRange={isSelected ? track.selectedRange : null} envelopePoints={track.envelopePoints} silenceThresholdDb={track.silenceThresholdDb} onSelect={() => {
              setSelectedSourceId(null);
              if (track.id === selectedTrackId) return;
              stopPlayback(false); setSelectedTrackId(track.id); selectedTrackIdRef.current = track.id; setCurrentTime(0); playbackOffsetRef.current = 0; editedPlaybackOffsetRef.current = 0;
            }} onScaleContextMenu={(x, y) => setTrackContextMenu({ trackId: track.id, x: Math.max(4, Math.min(x, window.innerWidth - 156)), y: Math.max(4, Math.min(y, window.innerHeight - 52)), kind: "export" })} onScale={(deltaY) => updateTrack(track.id, (current) => ({ ...current, pixelsPerSecond: Math.max(0.25, current.pixelsPerSecond * (deltaY < 0 ? 1.15 : 1 / 1.15)) }))} onSeek={(time) => {
              setCurrentTime(time); playbackOffsetRef.current = time; updateTrack(track.id, (current) => ({ ...current, selectedRange: null })); if (isPlaying && isSelected) void startPlayback(time);
            }} onRangeSelect={(start, end) => {
              setSelectedTrackId(track.id); selectedTrackIdRef.current = track.id; updateTrack(track.id, (current) => ({ ...current, selectedRange: { start, end } })); setMessage(`Range selected: ${formatTimeStandard(end - start)}. Press M to mute or restore sound.`);
            }} onDirectRegionEdit={(start, end, operation) => {
              setSelectedTrackId(track.id); selectedTrackIdRef.current = track.id; directEditTrackRange(track.id, start, end, operation);
            }} onRegionRestore={(region) => {
              stopPlayback(false); commitTrackEdit(track.id, "restore audio", (current) => ({ ...current, manualDeletedRegions: subtractRegion(current.manualDeletedRegions, region), silenceRegions: subtractRegion(current.silenceRegions, region), selectedRange: null }));
            }} onRangeMuteToggle={() => toggleTrackRangeMute(track.id)} onEditPointRemove={(time) => {
              commitTrackEdit(track.id, "remove edit point", (current) => ({ ...current, editPoints: current.editPoints.filter((point) => Math.abs(point - time) >= 0.01) }));
            }} onBlade={(time) => {
              stopPlayback(false); commitTrackEdit(track.id, "add edit point", (current) => ({ ...current, editPoints: addEditPoint(current.editPoints, time, current.buffer.duration), selectedRange: null })); setCurrentTime(time); playbackOffsetRef.current = time; setMessage(`Edit point added at ${formatTimeStandard(time)}`);
            }} onEnvelopePointAdd={(point) => {
              stopPlayback(false); commitTrackEdit(track.id, "add volume keyframe", (current) => ({ ...current, envelopePoints: [...current.envelopePoints, point].sort((left, right) => left.time - right.time) }));
            }} onEnvelopePointMove={(id, time, gain) => {
              stopPlayback(false); commitTrackEdit(track.id, "move volume keyframe", (current) => ({ ...current, envelopePoints: current.envelopePoints.map((point) => point.id === id ? { ...point, time: Math.max(0, Math.min(current.buffer.duration, time)), gain: Math.max(0, Math.min(2, gain)) } : point).sort((left, right) => left.time - right.time) }), true);
            }} onEnvelopePointRemove={(id) => {
              stopPlayback(false); commitTrackEdit(track.id, "remove volume keyframe", (current) => ({ ...current, envelopePoints: current.envelopePoints.filter((point) => point.id !== id) }));
            }} />;
          })}</div> : <div className="timeline-empty">
            <span className="timeline-empty-mark"><Icon name="add" size={24} /></span>
            <h3>Build your timeline</h3>
            <p>Add a ready item from the media pool or drag it here. Each take stays independent and exports as its own WAV file.</p>
            <div><span><kbd>Drag</kbd> Select range</span><span><kbd>Right-drag →</kbd> Delete</span><span><kbd>Right-drag ←</kbd> Restore</span><span><kbd>Space</kbd> Play</span></div>
          </div>}
        </div>

        <footer className="status-bar" aria-live="polite">
          <div className="status-message"><span className={isProcessing || pendingImportCount + pendingDenoiseCount > 0 || bladeActive ? "status-pulse is-active" : "status-pulse"} />{bladeActive ? "Blade active: click the waveform to cut, A returns to direct editing" : message}</div>
          <div className="status-counts">{pendingImportCount > 0 && <span>{pendingImportCount} normalizing</span>}{pendingDenoiseCount > 0 && <span>{pendingDenoiseCount} denoising</span>}{isProcessing && <span>{exportingTrackIds.length} exporting</span>}</div>
          <div className="status-hint"><kbd>A</kbd> Direct <kbd>B</kbd> Blade <kbd>Space</kbd> Play <kbd>M</kbd> Mute range</div>
        </footer>
      </section>
    </div>

    {trackContextMenu && <div ref={trackContextMenuRef} className="context-menu" role="menu" style={{ left: trackContextMenu.x, top: trackContextMenu.y }}>
      {trackContextMenu.kind === "export" && <button type="button" role="menuitem" disabled={isProcessing || !canExportTracks(editorTracks.filter((track) => track.id === trackContextMenu.trackId))} onClick={() => exportSingleTrack(trackContextMenu.trackId)}><Icon name="export" />Force export track</button>}
      {trackContextMenu.kind === "actions" && trackContextMenu.trackId === selectedTrackId && <>
        <button type="button" role="menuitem" onClick={() => { setTrackContextMenu(null); updateSelectedTrack((track) => ({ ...track, collapsed: !track.collapsed, selectedRange: null })); }} disabled={selectedDeletedRegions.length === 0}><Icon name="collapse" />{selectedTrack?.collapsed ? "Show all edits" : "Collapse deleted audio"}</button>
        <button type="button" role="menuitem" disabled={isProcessing} onClick={() => { setTrackContextMenu(null); returnToLibrary(); }}><Icon name="return" />Move to media pool</button>
      </>}
    </div>}
    <DenoiseSettings visible={settingsOpen} onClose={() => setSettingsOpen(false)} onDialogClosed={() => audioContextManagerRef.current?.requestReset()} onStatusChange={setDenoiseProviderStatus} onSaved={(status) => {
      setDenoiseProviderStatus(status);
      setMessage(status.ready ? "Denoising setup saved. Queuing eligible media." : status.summary);
      if (status.ready) {
        [...recordings, ...editorTracks]
          .filter(isRecordingReady)
          .filter((recording) => recording.denoiseStatus === "unavailable" || recording.denoiseStatus === "failed")
          .forEach((recording) => void queueDenoise(recording));
      }
    }} />
  </main>;
}

export default App;
