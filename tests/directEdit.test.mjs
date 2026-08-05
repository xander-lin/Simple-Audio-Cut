import assert from "node:assert/strict";
import test from "node:test";
import { addEditPoint, directRegionEdit, segmentAtTime } from "../src/utils/directEdit.ts";

test("maps right-button drag direction to the original delete and restore gestures", () => {
  assert.deepEqual(directRegionEdit(2, 6), { start: 2, end: 6, operation: "delete" });
  assert.deepEqual(directRegionEdit(6, 2), { start: 2, end: 6, operation: "restore" });
  assert.equal(directRegionEdit(2, 2.005), null);
  assert.equal(directRegionEdit(2, 5, 12), null);
});

test("adds ordered edit points without duplicating nearby cuts", () => {
  assert.deepEqual(addEditPoint([7, 2], 4, 10), [2, 4, 7]);
  assert.deepEqual(addEditPoint([4], 4.005, 10), [4]);
  assert.deepEqual(addEditPoint([], 0, 10), []);
  assert.deepEqual(addEditPoint([], 10, 10), []);
});

test("maps a direct click to the clip bounded by edit points", () => {
  assert.deepEqual(segmentAtTime([3, 7], 5, 10), { start: 3, end: 7 });
  assert.deepEqual(segmentAtTime([3, 7], 7, 10), { start: 7, end: 10 });
  assert.equal(segmentAtTime([], 5, 10), null);
});
