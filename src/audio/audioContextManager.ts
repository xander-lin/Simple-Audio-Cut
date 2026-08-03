export interface ManagedAudioContext {
  readonly state: AudioContextState;
  close(): Promise<void>;
}

export interface AudioContextFactory<T extends ManagedAudioContext> {
  create(): T;
}

export class BrowserAudioContextFactory implements AudioContextFactory<AudioContext> {
  create() {
    return new AudioContext();
  }
}

export class MockAudioContext implements ManagedAudioContext {
  state: AudioContextState = "suspended";
  closeCalls = 0;
  private readonly closeContext: () => Promise<void>;

  constructor(closeContext: () => Promise<void> = async () => undefined) {
    this.closeContext = closeContext;
  }

  async close() {
    this.closeCalls += 1;
    await this.closeContext();
    this.state = "closed";
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

export class AudioContextManager<T extends ManagedAudioContext> {
  private context: T | null = null;
  private resetRequested = false;
  private resetPromise: Promise<T> | null = null;
  private activeOperations = 0;
  private idleResolvers: Array<() => void> = [];
  private closed = false;
  private closePromise: Promise<void> | null = null;
  private readonly factory: AudioContextFactory<T>;

  constructor(factory: AudioContextFactory<T>) {
    this.factory = factory;
  }

  requestReset() {
    if (this.closed) return;
    this.resetRequested = true;
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

  private reset() {
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

  private async acquire(trackOperation: boolean): Promise<T> {
    while (true) {
      if (this.closed) throw new Error("Audio context manager is closed.");
      if (this.resetPromise) {
        await this.resetPromise;
        continue;
      }
      if (!this.context || this.context.state === "closed") {
        this.resetRequested = false;
        this.context = this.factory.create();
      } else if (this.resetRequested) {
        await this.reset();
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
}
