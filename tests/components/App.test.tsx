import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  confirm: vi.fn(),
  invoke: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => unknown>(),
  open: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  invoke: mocks.invoke,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: mocks.confirm, open: mocks.open }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, callback: (event: { payload: unknown }) => unknown) => {
    mocks.listeners.set(name, callback);
    return Promise.resolve(() => mocks.listeners.delete(name));
  }),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    destroy: vi.fn(),
    onCloseRequested: vi.fn(() => Promise.resolve(() => undefined)),
  }),
}));

import App from "../../src/App";

const providerStatus = {
  providerId: "clearvoice",
  providerName: "ClearVoice",
  configured: {
    enabled: true,
    executablePath: "/env/bin/python",
    modelRoot: "/models",
    device: "auto",
    highSampleRateModel: "MossFormer2_SE_48K",
    lowSampleRateModel: "FRCRN_SE_16K",
  },
  effective: {
    enabled: true,
    executablePath: "/env/bin/python",
    modelRoot: "/models",
    device: "auto",
    highSampleRateModel: "MossFormer2_SE_48K",
    lowSampleRateModel: "FRCRN_SE_16K",
  },
  environmentOverrides: [],
  models: [],
  devices: [],
  checks: [],
  ready: true,
  summary: "ClearVoice is ready.",
};

const fakeBuffer = {
  duration: 1,
  sampleRate: 48_000,
  numberOfChannels: 1,
  getChannelData: () => new Float32Array(48_000),
} as unknown as AudioBuffer;

class FakeAudioContext {
  state: AudioContextState = "running";
  currentTime = 0;
  decodeAudioData = vi.fn(async () => fakeBuffer);
  close = vi.fn(async () => { this.state = "closed"; });
}

beforeEach(() => {
  mocks.listeners.clear();
  mocks.confirm.mockResolvedValue(true);
  mocks.open.mockResolvedValue("/tmp/voice.wav");
  mocks.invoke.mockImplementation((command: string) => {
    if (command === "denoise_provider_status") return Promise.resolve(providerStatus);
    if (command === "denoise_availability") return Promise.resolve({ available: false, providerName: "ClearVoice", modelName: null, reason: "Unavailable in test" });
    return Promise.resolve(undefined);
  });
  vi.stubGlobal("AudioContext", FakeAudioContext);
  vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, arrayBuffer: async () => new ArrayBuffer(16) })));
  vi.stubGlobal("ResizeObserver", class {
    observe() {}
    disconnect() {}
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

async function importReadySource() {
  fireEvent.click(screen.getByRole("button", { name: "Import" }));
  await screen.findByDisplayValue("voice");
  const invokeCall = await waitFor(() => {
    const call = mocks.invoke.mock.calls.find(([command]) => command === "start_import_audio");
    expect(call).toBeTruthy();
    return call;
  });
  const recordingId = invokeCall?.[1].recordingId as string;
  await act(async () => {
    await mocks.listeners.get("import-status")?.({
      payload: {
        status: "complete",
        info: { id: recordingId, name: "voice", path: "/tmp/normalized.wav", durationSeconds: 1, integratedLufs: -14 },
      },
    });
  });
  await screen.findByRole("button", { name: "Add voice to timeline" });
}

describe("App deletion controls", () => {
  it("deletes a source from its own media card", async () => {
    render(<App />);
    await importReadySource();
    fireEvent.mouseDown(screen.getByDisplayValue("voice"));
    expect((screen.getByRole("button", { name: "Delete selected timeline track" }) as HTMLButtonElement).disabled).toBe(true);
    const deleteButton = screen.getByRole("button", { name: "Delete voice from source media" });
    fireEvent.click(deleteButton);
    expect(mocks.confirm).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByDisplayValue("voice")).toBeNull());
  });

  it("uses the header delete button only for the selected timeline track", async () => {
    render(<App />);
    await importReadySource();
    fireEvent.click(screen.getByRole("button", { name: "Add voice to timeline" }));
    const deleteButton = await screen.findByRole("button", { name: "Delete selected timeline track" });
    fireEvent.click(deleteButton);
    expect(mocks.confirm).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getByText("Build your timeline")).toBeTruthy());
  });
});
