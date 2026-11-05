import assert from "node:assert/strict";
import test from "node:test";
import { playbackSourceMatches, savedPlaybackPosition } from "../src/utils/playbackUtils.ts";

const timing = {
  contextTime: 12,
  startedAt: 10,
  editedOffset: 3,
  keptRegions: [{ start: 0, end: 4 }, { start: 7, end: 10 }],
};

test("does not derive a position after playback has already ended", () => {
  assert.equal(savedPlaybackPosition(false, timing), null);
});

test("maps an active edited playback position back to source time", () => {
  assert.equal(savedPlaybackPosition(true, timing), 8);
});

test("does not move backwards when context timing was reset", () => {
  assert.equal(savedPlaybackPosition(true, {
    ...timing,
    contextTime: 1,
    startedAt: 10,
  }), 3);
});

test("uses the playback region snapshot when edits change during playback", () => {
  const playbackSnapshot = [{ start: 0, end: 4 }, { start: 7, end: 10 }];
  const changedRegions = [{ start: 0, end: 10 }];

  assert.equal(savedPlaybackPosition(true, { ...timing, keptRegions: playbackSnapshot }), 8);
  assert.equal(savedPlaybackPosition(true, { ...timing, keptRegions: changedRegions }), 5);
});

function playbackSource(overrides = {}) {
  return {
    buffer: { duration: 10 },
    manualDeletedRegions: [],
    silenceRegions: [],
    mutedRegions: [],
    envelopePoints: [],
    ...overrides,
  };
}

test("keeps playback valid when a selection updates the track object", () => {
  const snapshot = playbackSource();
  const selected = { ...snapshot, selectedRange: { start: 1, end: 2 } };

  assert.equal(playbackSourceMatches(selected, snapshot), true);
});

test("invalidates playback when audio-affecting edits change the source", () => {
  const snapshot = playbackSource();
  const changed = (overrides) => ({ ...snapshot, ...overrides });

  assert.equal(playbackSourceMatches(changed({ buffer: { duration: 10 } }), snapshot), false);
  assert.equal(playbackSourceMatches(changed({ silenceRegions: [{ start: 1, end: 2 }] }), snapshot), false);
  assert.equal(playbackSourceMatches(changed({ manualDeletedRegions: [{ start: 1, end: 2 }] }), snapshot), false);
  assert.equal(playbackSourceMatches(changed({ mutedRegions: [{ start: 1, end: 2 }] }), snapshot), false);
  assert.equal(playbackSourceMatches(changed({ envelopePoints: [{ id: "point", time: 1, gain: 0.5 }] }), snapshot), false);
});

test("invalidates playback when the track disappeared", () => {
  assert.equal(playbackSourceMatches(undefined, playbackSource()), false);
});
