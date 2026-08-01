import assert from "node:assert/strict";
import test from "node:test";
import { applyDenoiseUpdate, canExportTracks } from "../src/utils/denoiseUtils.ts";

const recording = {
  id: "recording-1",
  denoiseStatus: "processing",
  denoiseTaskId: "current-task",
};

test("applies updates from the current denoise task", () => {
  const updated = applyDenoiseUpdate(recording, "recording-1", "current-task", {
    denoiseStatus: "complete",
  });

  assert.equal(updated.denoiseStatus, "complete");
});

test("ignores a late result from an obsolete denoise task", () => {
  const updated = applyDenoiseUpdate(recording, "recording-1", "old-task", {
    denoiseStatus: "complete",
  });

  assert.strictEqual(updated, recording);
});

test("allows export once a normalized source exists regardless of denoise state", () => {
  assert.equal(canExportTracks([{ ...recording, denoiseStatus: "failed" }]), true);
  assert.equal(canExportTracks([{ ...recording, denoiseStatus: "unavailable" }]), true);
  assert.equal(canExportTracks([{ ...recording, denoiseStatus: "complete" }]), true);
  assert.equal(canExportTracks([{ ...recording, denoiseStatus: "processing" }]), true);
  assert.equal(canExportTracks([{ ...recording, denoiseStatus: "queued" }]), true);
  assert.equal(canExportTracks([]), false);
});
