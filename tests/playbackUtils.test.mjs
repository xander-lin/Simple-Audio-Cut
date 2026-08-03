import assert from "node:assert/strict";
import test from "node:test";
import { savedPlaybackPosition } from "../src/utils/playbackUtils.ts";

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
