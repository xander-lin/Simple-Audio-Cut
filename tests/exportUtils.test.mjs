import assert from "node:assert/strict";
import test from "node:test";
import { createEditedBuffer, getKeptRegions } from "../src/utils/exportUtils.ts";

class TestAudioBuffer {
  constructor({ length, numberOfChannels, sampleRate }) {
    this.length = length;
    this.numberOfChannels = numberOfChannels;
    this.sampleRate = sampleRate;
    this.duration = length / sampleRate;
    this.channels = Array.from({ length: numberOfChannels }, () => new Float32Array(length));
  }

  getChannelData(channel) {
    return this.channels[channel];
  }
}

globalThis.AudioBuffer = TestAudioBuffer;

test("normalizes overlapping and out-of-bounds deleted regions", () => {
  assert.deepEqual(getKeptRegions([
    { start: 8, end: 20 },
    { start: -2, end: 2 },
    { start: 1, end: 4 },
    { start: 6, end: 5 },
    { start: Number.NaN, end: 7 },
  ], 10), [
    { start: 4, end: 8 },
  ]);
});

test("returns the complete source when no valid deletion exists", () => {
  assert.deepEqual(getKeptRegions([{ start: 4, end: 4 }], 10), [{ start: 0, end: 10 }]);
  assert.deepEqual(getKeptRegions([], 0), []);
});

test("replaces muted ranges with silence without changing duration", () => {
  const source = new TestAudioBuffer({ length: 10, numberOfChannels: 1, sampleRate: 10 });
  source.getChannelData(0).fill(1);

  const result = createEditedBuffer(source, [], [], [{ start: 0.2, end: 0.5 }]);

  assert.equal(result.length, source.length);
  assert.deepEqual([...result.getChannelData(0)], [1, 1, 0, 0, 0, 1, 1, 1, 1, 1]);
});
