export interface ManagedAudioContext {
  readonly state: AudioContextState;
  close(): Promise<void>;
}

export interface ResumableAudioContext extends ManagedAudioContext {
  resume(): Promise<void>;
}

export interface AudioContextFactory<T extends ManagedAudioContext> {
  create(): T;
}

export class BrowserAudioContextFactory implements AudioContextFactory<AudioContext> {
  create() {
    return new AudioContext();
  }
}

export class MockAudioContext implements ResumableAudioContext {
  state: AudioContextState = "suspended";
  closeCalls = 0;
  resumeCalls = 0;
  private readonly closeContext: () => Promise<void>;
  private readonly resumeContext: () => Promise<void>;

  constructor(
    closeContext: () => Promise<void> = async () => undefined,
    resumeContext: () => Promise<void> = async () => undefined,
  ) {
    this.closeContext = closeContext;
    this.resumeContext = resumeContext;
  }

  async close() {
    this.closeCalls += 1;
    await this.closeContext();
    this.state = "closed";
  }

  async resume() {
    this.resumeCalls += 1;
    await this.resumeContext();
    const state = this.state as AudioContextState;
    if (state !== "closed") this.state = "running";
  }
}

export class MockAudioContextFactory implements AudioContextFactory<MockAudioContext> {
  readonly contexts: MockAudioContext[] = [];
  private readonly createContext: () => MockAudioContext;

  constructor(createContext: () => MockAudioContext = () => new MockAudioContext()) {
    this.createContext = createContext;
  }

  create() {
    const context = this.createContext();
    this.contexts.push(context);
    return context;
  }
}

export const AUDIO_CONTEXT_PLAYBACK_TIMEOUT_MS = 2_000;

export class AudioContextOperationTimeoutError extends Error {
  readonly operation: string;
  readonly timeoutMs: number;

  constructor(operation: string, timeoutMs: number) {
    super(`${operation} did not complete within ${timeoutMs} ms.`);
    this.name = "AudioContextOperationTimeoutError";
    this.operation = operation;
    this.timeoutMs = timeoutMs;
  }
}

export function waitForAudioContext<T>(operation: PromiseLike<T>, operationName: string, timeoutMs: number) {
  let timeoutId: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timeoutId = setTimeout(() => reject(new AudioContextOperationTimeoutError(operationName, timeoutMs)), timeoutMs);
  });
  return Promise.race([Promise.resolve(operation), timeout]).finally(() => {
    if (timeoutId !== undefined) clearTimeout(timeoutId);
  });
}

export async function resumeAudioContext(
  context: ResumableAudioContext,
  timeoutMs = AUDIO_CONTEXT_PLAYBACK_TIMEOUT_MS,
) {
  const state = context.state as AudioContextState;
  if (state === "closed") throw new Error("Cannot resume a closed audio context.");
  if (state === "running") return;
  await waitForAudioContext(
    Promise.resolve().then(() => context.resume()),
    "AudioContext.resume()",
    timeoutMs,
  );
  const finalState = context.state as AudioContextState;
  if (finalState !== "running") {
    throw new Error(`Audio context did not reach the running state (state: ${finalState}).`);
  }
}

export class AudioContextManager<T extends ManagedAudioContext> {
  private context: T | null = null;
  private resetRequested = false;
  private resetPromise: Promise<T> | null = null;
  private activeOperations = 0;
  private idleResolvers: Array<() => void> = [];
  private closed = false;
  private closePromise: Promise<void> | null = null;
  private lifecycleVersion = 0;
  private readonly factory: AudioContextFactory<T>;

  constructor(factory: AudioContextFactory<T>) {
    this.factory = factory;
  }

  requestReset() {
    if (this.closed) return;
    this.lifecycleVersion += 1;
    this.resetRequested = true;
  }

  reset() {
    if (this.closed) return Promise.reject(new Error("Audio context manager is closed."));
    this.requestReset();
    return this.acquire(false);
  }

  get version() {
    return this.lifecycleVersion;
  }

  isCurrent(context: T, version = this.lifecycleVersion) {
    return !this.closed
      && this.context === context
      && this.lifecycleVersion === version
      && !this.resetRequested
      && !this.resetPromise;
  }

  async get() {
    return this.acquire(false);
  }

  async run<R>(operation: (context: T) => Promise<R>) {
    const context = await this.acquire(true);
    try {
      return await operation(context);
    } finally {
      this.activeOperations -= 1;
      if (this.activeOperations === 0) {
        for (const resolve of this.idleResolvers.splice(0)) resolve();
      }
    }
  }

  async close() {
    if (this.closePromise) return this.closePromise;
    this.closed = true;
    this.resetRequested = false;
    this.closePromise = (async () => {
      const pendingReset = this.resetPromise;
      if (pendingReset) {
        try {
          await pendingReset;
        } catch {
          // The active context is still closed below when creation failed.
        }
      }
      await this.waitForIdle();
      const context = this.context;
      this.context = null;
      if (context && context.state !== "closed") await context.close();
    })();
    return this.closePromise;
  }

  private async acquire(trackOperation: boolean): Promise<T> {
    while (true) {
      if (this.closed) throw new Error("Audio context manager is closed.");
      if (this.resetPromise) {
        await this.resetPromise;
        continue;
      }
      if (!this.context) {
        this.resetRequested = false;
        this.context = this.factory.create();
      } else if (this.context.state === "closed") {
        this.lifecycleVersion += 1;
        this.resetRequested = false;
        this.context = this.factory.create();
      } else if (this.resetRequested) {
        await this.resetContext();
        continue;
      }
      if (trackOperation) this.activeOperations += 1;
      return this.context;
    }
  }

  private waitForIdle() {
    if (this.activeOperations === 0) return Promise.resolve();
    return new Promise<void>((resolve) => this.idleResolvers.push(resolve));
  }

  private resetContext() {
    if (this.resetPromise) return this.resetPromise;
    this.resetRequested = false;
    const previous = this.context;
    this.resetPromise = (async () => {
      await this.waitForIdle();
      if (previous && previous.state !== "closed") await previous.close();
      if (this.closed) throw new Error("Audio context manager is closed.");
      this.context = this.factory.create();
      return this.context;
    })().catch((error) => {
      if (!this.closed) this.resetRequested = true;
      throw error;
    }).finally(() => {
      this.resetPromise = null;
    });
    return this.resetPromise;
  }
}
