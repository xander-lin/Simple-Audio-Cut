export function appendUniqueTrack<T extends { id: string }>(tracks: T[], track: T): T[] {
  return tracks.some((current) => current.id === track.id) ? tracks : [...tracks, track];
}
