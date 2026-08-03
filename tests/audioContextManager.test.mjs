import assert from "node:assert/strict";
import test from "node:test";
import {
  AudioContextManager,
  MockAudioContext,
  MockAudioContextFactory,
} from "../src/audio/audioContextManager.ts";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

test("creates the audio context lazily and reuses it", async () => {
  const factory = new MockAudioContextFactory();
  const manager = new AudioContextManager(factory);

  assert.equal(factory.contexts.length, 0);
  const first = await manager.get();
  const second = await manager.get();

  assert.strictEqual(second, first);
  assert.equal(factory.contexts.length, 1);
});

test("waits for close before creating one shared replacement", async () => {
  const close = deferred();
  let created = 0;
  const factory = new MockAudioContextFactory(() => {
    created += 1;
    return new MockAudioContext(created === 1 ? () => close.promise : undefined);
  });
  const manager = new AudioContextManager(factory);
  const first = await manager.get();

  manager.requestReset();
  const firstRequest = manager.get();
  const secondRequest = manager.get();
  await Promise.resolve();

  assert.equal(first.closeCalls, 1);
  assert.equal(factory.contexts.length, 1);
  close.resolve();
  const [firstReplacement, secondReplacement] = await Promise.all([firstRequest, secondRequest]);

  assert.strictEqual(firstReplacement, secondReplacement);
  assert.equal(factory.contexts.length, 2);
});

test("does not reset a context while an operation is using it", async () => {
  const operationStarted = deferred();
  const finishOperation = deferred();
  const factory = new MockAudioContextFactory();
  const manager = new AudioContextManager(factory);
  const first = await manager.get();
  const operation = manager.run(async (context) => {
    operationStarted.resolve();
    await finishOperation.promise;
    return context;
  });
  await operationStarted.promise;

  manager.requestReset();
  const replacement = manager.get();
  await Promise.resolve();
  assert.equal(first.closeCalls, 0);

  finishOperation.resolve();
  assert.strictEqual(await operation, first);
  assert.notStrictEqual(await replacement, first);
  assert.equal(first.closeCalls, 1);
});

test("does not admit a new operation while reset is waiting for an old one", async () => {
  const finishFirst = deferred();
  const factory = new MockAudioContextFactory();
  const manager = new AudioContextManager(factory);
  const first = await manager.get();
  const firstOperation = manager.run(async () => finishFirst.promise);

  manager.requestReset();
  const replacement = manager.get();
  const secondOperationStarted = deferred();
  const secondOperation = manager.run(async (context) => {
    secondOperationStarted.resolve();
    return context;
  });
  await Promise.resolve();

  assert.equal(first.closeCalls, 0);
  assert.equal(factory.contexts.length, 1);
  finishFirst.resolve();
  await firstOperation;

  const replacementContext = await replacement;
  assert.strictEqual(await secondOperation, replacementContext);
  await secondOperationStarted.promise;
  assert.equal(first.closeCalls, 1);
});

test("recovers on the next request when replacement creation fails", async () => {
  let created = 0;
  const factory = new MockAudioContextFactory(() => {
    created += 1;
    if (created === 2) throw new Error("creation failed");
    return new MockAudioContext();
  });
  const manager = new AudioContextManager(factory);
  const first = await manager.get();

  manager.requestReset();
  await assert.rejects(manager.get(), /creation failed/);
  const recovered = await manager.get();

  assert.equal(first.closeCalls, 1);
  assert.notStrictEqual(recovered, first);
  assert.equal(created, 3);
});

test("keeps a reset request made while the previous reset is closing", async () => {
  const close = deferred();
  let created = 0;
  const factory = new MockAudioContextFactory(() => {
    created += 1;
    return new MockAudioContext(created === 1 ? () => close.promise : undefined);
  });
  const manager = new AudioContextManager(factory);
  const first = await manager.get();

  manager.requestReset();
  const firstReset = manager.get();
  await Promise.resolve();
  manager.requestReset();
  close.resolve();
  const latest = await firstReset;
  const intermediate = factory.contexts[1];

  assert.equal(first.closeCalls, 1);
  assert.equal(intermediate.closeCalls, 1);
  assert.notStrictEqual(latest, intermediate);
  assert.strictEqual(await manager.get(), latest);
  assert.equal(factory.contexts.length, 3);
});

test("waits for active operations before closing and rejects later use", async () => {
  const finishOperation = deferred();
  const factory = new MockAudioContextFactory();
  const manager = new AudioContextManager(factory);
  const first = await manager.get();
  const operation = manager.run(async () => finishOperation.promise);
  const closing = manager.close();
  const duplicateClose = manager.close();
  await Promise.resolve();

  assert.equal(first.closeCalls, 0);
  finishOperation.resolve();
  await operation;
  await Promise.all([closing, duplicateClose]);

  assert.equal(first.closeCalls, 1);
  await assert.rejects(manager.get(), /manager is closed/);
  await assert.rejects(manager.run(async () => undefined), /manager is closed/);
});
