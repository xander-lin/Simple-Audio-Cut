import assert from "node:assert/strict";
import test from "node:test";
import {
  amplitudeToDbfs,
  analyzeRmsFromChannels,
  dbfsToAmplitude,
  detectSilenceFromChannels,
} from "../src/utils/audioAnalysis.ts";

const sampleRate = 1_000;

function detect(channels, options = {}) {
  return detectSilenceFromChannels(channels, sampleRate, {
    threshold: dbfsToAmplitude(-30),
    ...options,
  });
}

function assertRegion(region, start, end) {
  assert.ok(Math.abs(region.start - start) <= 0.011, `${region.start} != ${start}`);
  assert.ok(Math.abs(region.end - end) <= 0.011, `${region.end} != ${end}`);
}

test("uses the loudest channel when classifying stereo audio", () => {
  const left = new Float32Array(3_000);
  const right = new Float32Array(3_000);
  right.fill(0.2, 500, 1_500);

  const regions = detect([left, right]);

  assert.equal(regions.length, 2);
  assertRegion(regions[0], 0.145, 0.345);
  assertRegion(regions[1], 2.15, 2.35);
});

test("protects a quiet gap shorter than the default 200 ms", () => {
  const audio = new Float32Array(1_000).fill(0.2);
  audio.fill(0, 400, 500);

  assert.deepEqual(detect([audio]), []);
});

test("uses the actual sample count in the final analysis window", () => {
  const audio = new Float32Array(1_005).fill(0.2);

  assert.deepEqual(detect([audio]), []);
});

test("reclassifies regions when the threshold moves in either direction", () => {
  const audio = new Float32Array(1_000).fill(0.02);

  const regions = detect([audio], { threshold: dbfsToAmplitude(-30) });
  assert.equal(regions.length, 1);
  assertRegion(regions[0], 0.4, 0.6);
  assert.equal(detect([audio], { threshold: dbfsToAmplitude(-40) }).length, 0);
});

test("exposes the same multichannel RMS frames used by silence detection", () => {
  const left = new Float32Array(100).fill(0.1);
  const right = new Float32Array(100).fill(0.25);

  const frames = analyzeRmsFromChannels([left, right], 1_000, {
    windowDuration: 0.02,
    hopDuration: 0.01,
  });

  assert.equal(frames.length, 10);
  assert.ok(Math.abs(frames[0].amplitude - 0.25) < 1e-6);
  assert.ok(Math.abs(amplitudeToDbfs(frames[0].amplitude) + 12.0412) < 0.001);
});

test("detects a quiet gap without duration gating", () => {
  const audio = new Float32Array(1_000).fill(0.2);
  audio.fill(0, 400, 615);

  const regions = detect([audio]);

  assert.equal(regions.length, 1);
  assertRegion(regions[0], 0.4075, 0.6075);
});

test("detects a quiet gap shorter than the former minimum duration", () => {
  const audio = new Float32Array(1_000).fill(0.2);
  audio.fill(0, 400, 590);

  const regions = detect([audio], { minDuration: 0.15 });

  assert.equal(regions.length, 1);
  assertRegion(regions[0], 0.42, 0.57);
});

test("does not create zero-duration silence cuts", () => {
  const audio = new Float32Array(1_000).fill(0.2);
  audio.fill(0, 400, 410);

  const regions = detect([audio], { minDuration: 0 });

  assert.deepEqual(regions, []);
});

test("does not let a short loud section hide neighboring long quiet sections", () => {
  const audio = new Float32Array(1_000);
  audio.fill(0.2, 495, 505);

  const regions = detect([audio]);

  assert.equal(regions.length, 2);
  assertRegion(regions[0], 0.1475, 0.3475);
  assertRegion(regions[1], 0.6525, 0.8525);
});
