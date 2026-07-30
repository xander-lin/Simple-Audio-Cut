import assert from "node:assert/strict";
import test from "node:test";
import { exportSignature, getLastExportDirectory, needsExport, rememberLastExportDirectory } from "../src/utils/exportState.ts";

const track = {
  name: "voice",
  path: "/tmp/voice.wav",
  manualDeletedRegions: [{ start: 1, end: 2 }],
  silenceRegions: [],
  envelopePoints: [{ id: "point-1", time: 3, gain: 0.5 }],
  lastExportSignature: null,
};

test("only tracks changed since their successful export need batch export", () => {
  const signature = exportSignature(track);

  assert.equal(needsExport(track), true);
  assert.equal(needsExport({ ...track, lastExportSignature: signature }), false);
  assert.equal(needsExport({ ...track, name: "renamed", lastExportSignature: signature }), true);
  assert.equal(needsExport({
    ...track,
    manualDeletedRegions: [...track.manualDeletedRegions, { start: 4, end: 5 }],
    lastExportSignature: signature,
  }), true);
});

test("envelope point identity and ordering do not create false changes", () => {
  const withTwoPoints = {
    ...track,
    envelopePoints: [
      { id: "later", time: 4, gain: 0.8 },
      { id: "earlier", time: 2, gain: 0.3 },
    ],
  };
  const signature = exportSignature(withTwoPoints);

  assert.equal(needsExport({
    ...withTwoPoints,
    envelopePoints: [
      { id: "new-earlier-id", time: 2, gain: 0.3 },
      { id: "new-later-id", time: 4, gain: 0.8 },
    ],
    lastExportSignature: signature,
  }), false);
});

test("equivalent deleted regions do not create false changes", () => {
  const signature = exportSignature(track);

  assert.equal(needsExport({
    ...track,
    manualDeletedRegions: [],
    silenceRegions: [
      { start: 1.5, end: 2 },
      { start: 1, end: 1.5 },
    ],
    lastExportSignature: signature,
  }), false);
});

test("remembers the last chosen export directory", () => {
  const values = new Map();
  const storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };

  assert.equal(getLastExportDirectory(storage), null);
  rememberLastExportDirectory("  /tmp/exports  ", storage);
  assert.equal(getLastExportDirectory(storage), "/tmp/exports");
  rememberLastExportDirectory("", storage);
  assert.equal(getLastExportDirectory(storage), "/tmp/exports");
});

test("export directory memory is best-effort", () => {
  const storage = {
    getItem: () => { throw new Error("blocked"); },
    setItem: () => { throw new Error("blocked"); },
  };

  assert.equal(getLastExportDirectory(storage), null);
  assert.doesNotThrow(() => rememberLastExportDirectory("/tmp/exports", storage));
});
