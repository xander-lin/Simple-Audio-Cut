import assert from "node:assert/strict";
import test from "node:test";
import { fileBasename, importSummary, selectedPaths } from "../src/utils/importUtils.ts";

test("normalizes file picker selections", () => {
  assert.deepEqual(selectedPaths(null), []);
  assert.deepEqual(selectedPaths("/tmp/one.wav"), ["/tmp/one.wav"]);
  assert.deepEqual(selectedPaths(["/tmp/one.wav", "/tmp/two.wav"]), ["/tmp/one.wav", "/tmp/two.wav"]);
});

test("summarizes multi-file import results", () => {
  assert.equal(importSummary(1, 0), "Imported audio is ready. Drag it into the editor.");
  assert.equal(importSummary(3, 0), "Imported 3 audio files. Drag them into the editor.");
  assert.equal(importSummary(2, 1), "Imported 2 audio files; 1 failed.");
  assert.equal(importSummary(0, 2), "Import failed for 2 files.");
  assert.equal(importSummary(1, 1, { path: "/tmp/broken.mp4", error: "No audio stream" }), "Imported 1 audio file; 1 failed. First failure: broken.mp4 - No audio stream");
});

test("extracts display names from selected paths", () => {
  assert.equal(fileBasename("/tmp/video.mov"), "video.mov");
  assert.equal(fileBasename("C:\\Users\\me\\audio.wav"), "audio.wav");
});
