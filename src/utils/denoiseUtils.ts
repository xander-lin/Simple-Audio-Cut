export interface DenoiseState {
  denoiseStatus: "queued" | "processing" | "complete" | "failed" | "unavailable";
  denoiseTaskId: string;
}

export function applyDenoiseUpdate<T extends { id: string } & DenoiseState>(
  recording: T,
  recordingId: string,
  taskId: string,
  update: Partial<DenoiseState>,
): T {
  return recording.id === recordingId && recording.denoiseTaskId === taskId
    ? { ...recording, ...update }
    : recording;
}

export function canExportTracks(tracks: DenoiseState[]) {
  return tracks.length > 0 && tracks.every((track) =>
    track.denoiseStatus === "complete" || track.denoiseStatus === "failed" || track.denoiseStatus === "unavailable");
}
