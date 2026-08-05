import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke, open } = vi.hoisted(() => ({ invoke: vi.fn(), open: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open }));

import DenoiseSettings, { type DenoiseProviderStatus } from "../../src/components/DenoiseSettings";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const status: DenoiseProviderStatus = {
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
  models: [
    { id: "MossFormer2_SE_48K", sampleRate: 48_000, ready: true, detail: "Ready" },
    { id: "FRCRN_SE_16K", sampleRate: 16_000, ready: true, detail: "Ready" },
  ],
  devices: [{ id: "auto", label: "Automatic" }, { id: "cpu", label: "CPU" }],
  checks: [{ id: "python", label: "Python environment", state: "ready", detail: "Ready" }],
  ready: true,
  summary: "ClearVoice is ready.",
};

describe("DenoiseSettings", () => {
  it("loads, inspects, and saves one provider configuration", async () => {
    const changedStatus = {
      ...status,
      configured: { ...status.configured, executablePath: "/new/bin/python", device: "cpu" },
      effective: { ...status.effective, executablePath: "/new/bin/python", device: "cpu" },
    };
    invoke.mockImplementation((command: string) => Promise.resolve(
      command === "denoise_provider_status" ? status : changedStatus,
    ));
    const onSaved = vi.fn();
    render(<DenoiseSettings visible onClose={vi.fn()} onDialogClosed={vi.fn()} onStatusChange={vi.fn()} onSaved={onSaved} />);
    expect(await screen.findByDisplayValue("/env/bin/python")).toBeTruthy();

    fireEvent.change(screen.getByDisplayValue("/env/bin/python"), { target: { value: "/new/bin/python" } });
    fireEvent.click(screen.getByRole("combobox", { name: /Processing device/ }));
    fireEvent.click(screen.getByRole("option", { name: "CPU" }));
    fireEvent.click(screen.getByRole("button", { name: "Check setup" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("inspect_denoise_config", {
      config: expect.objectContaining({ executablePath: "/new/bin/python", device: "cpu" }),
    }));

    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("save_denoise_config", {
      config: expect.objectContaining({ executablePath: "/new/bin/python", device: "cpu" }),
    }));
    expect(onSaved).toHaveBeenCalledWith(changedStatus);
  });

  it("uses native pickers without installing or discovering paths itself", async () => {
    invoke.mockResolvedValue(status);
    open.mockResolvedValueOnce("/custom/python").mockResolvedValueOnce("/custom/models");
    const onDialogClosed = vi.fn();
    render(<DenoiseSettings visible onClose={vi.fn()} onDialogClosed={onDialogClosed} onStatusChange={vi.fn()} onSaved={vi.fn()} />);
    await screen.findByDisplayValue("/env/bin/python");
    fireEvent.click(screen.getByRole("button", { name: "Browse for Python executable" }));
    await screen.findByDisplayValue("/custom/python");
    fireEvent.click(screen.getByRole("button", { name: "Browse for model library" }));
    await screen.findByDisplayValue("/custom/models");
    expect(open).toHaveBeenNthCalledWith(1, expect.objectContaining({ directory: false }));
    expect(open).toHaveBeenNthCalledWith(2, expect.objectContaining({ directory: true }));
    expect(onDialogClosed).toHaveBeenCalledTimes(2);
  });

  it("keeps the settings dialog open when Escape closes a choice list", async () => {
    invoke.mockResolvedValue(status);
    const onClose = vi.fn();
    render(<DenoiseSettings visible onClose={onClose} onDialogClosed={vi.fn()} onStatusChange={vi.fn()} onSaved={vi.fn()} />);
    const deviceChoice = await screen.findByRole("combobox", { name: /Processing device/ });

    fireEvent.click(deviceChoice);
    const cpuOption = screen.getByRole("option", { name: "CPU" });
    fireEvent.keyDown(cpuOption, { key: "Escape" });

    expect(screen.queryByRole("option", { name: "CPU" })).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("shows unavailable builds without exposing unusable provider fields", async () => {
    invoke.mockResolvedValue({
      ...status,
      providerId: "disabled",
      providerName: "No denoising provider",
      ready: false,
      summary: "Denoising is not included in this build.",
    });
    render(<DenoiseSettings visible onClose={vi.fn()} onDialogClosed={vi.fn()} onStatusChange={vi.fn()} onSaved={vi.fn()} />);

    expect(await screen.findByText("No denoising provider")).toBeTruthy();
    expect(screen.queryByText("Python executable")).toBeNull();
    expect((screen.getByRole("button", { name: "Check setup" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "Save settings" }) as HTMLButtonElement).disabled).toBe(true);
  });
});
