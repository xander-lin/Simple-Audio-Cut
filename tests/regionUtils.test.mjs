import assert from "node:assert/strict";
import test from "node:test";
import { envelopeGainAtTime, regionIsCovered } from "../src/utils/regionUtils.ts";

test("detects whether a selected range is already fully muted", () => {
  const regions = [{ start: 1, end: 2 }, { start: 2, end: 3 }];
  assert.equal(regionIsCovered(regions, { start: 1.5, end: 2.5 }), true);
  assert.equal(regionIsCovered(regions, { start: 0.5, end: 2.5 }), false);
});

test("uses explicit edge keyframes when they exist", () => {
  const points = [
    { id: "start", time: 0, gain: 0 },
    { id: "rise", time: 2, gain: 1.5 },
    { id: "hold", time: 10, gain: 1.5 },
    { id: "end", time: 12, gain: 0 },
  ];

  assert.equal(envelopeGainAtTime(points, 12, 0), 0);
  assert.equal(envelopeGainAtTime(points, 12, 2), 1.5);
  assert.equal(envelopeGainAtTime(points, 12, 12), 0);
});

test("keeps unity gain outside user keyframes when no edge point exists", () => {
  const points = [{ id: "keyframe", time: 5, gain: 0.5 }];

  assert.equal(envelopeGainAtTime(points, 10, 0), 1);
  assert.equal(envelopeGainAtTime(points, 10, 10), 1);
});
