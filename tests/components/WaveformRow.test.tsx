import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import WaveformRow from "../../src/components/WaveformRow";

afterEach(cleanup);

function audioBuffer() {
  const samples = new Float32Array(480);
  return {
    duration: 10,
    sampleRate: 48,
    numberOfChannels: 1,
    getChannelData: () => samples,
  } as unknown as AudioBuffer;
}

function callbacks() {
  return {
    onSeek: vi.fn(),
    onRegionDragStart: vi.fn(),
    onRegionDragMove: vi.fn(),
    onRegionDragEnd: vi.fn(),
    onDirectEditDragStart: vi.fn(),
    onDirectEditDragMove: vi.fn(),
    onDirectEditDragEnd: vi.fn(),
    onRegionRestore: vi.fn(),
    onRangeMuteToggle: vi.fn(),
    onBlade: vi.fn(),
    onEditPointRemove: vi.fn(),
    onEnvelopePointAdd: vi.fn(),
    onEnvelopePointMove: vi.fn(),
    onEnvelopePointRemove: vi.fn(),
  };
}

function renderRow(overrides: Record<string, unknown> = {}) {
  const handlers = callbacks();
  const result = render(<WaveformRow
    buffer={audioBuffer()}
    startTime={0}
    endTime={10}
    width={500}
    height={96}
    leadingPadding={0}
    trailingPadding={0}
    currentTime={0}
    bladeActive={false}
    showEnvelope
    regions={[]}
    mutedRegions={[]}
    editPoints={[]}
    selectedRange={null}
    envelopePoints={[]}
    rmsFrames={[]}
    silenceThresholdDb={-36}
    showSilenceThreshold={false}
    {...handlers}
    {...overrides}
  />);
  const waveform = screen.getByRole("application");
  Object.defineProperty(waveform, "getBoundingClientRect", {
    value: () => ({ left: 0, top: 0, width: 500, height: 96, right: 500, bottom: 96, x: 0, y: 0, toJSON() {} }),
  });
  return { ...result, waveform, handlers };
}

describe("WaveformRow direct editing", () => {
  it("starts and extends a range drag on the waveform", () => {
    const { waveform, handlers } = renderRow();
    fireEvent.mouseDown(waveform, { button: 0, buttons: 1, clientX: 100, clientY: 20, detail: 1 });
    fireEvent.mouseMove(waveform, { buttons: 1, clientX: 300, clientY: 20 });
    fireEvent.mouseUp(waveform, { button: 0, clientX: 300, clientY: 20 });
    expect(handlers.onRegionDragStart).toHaveBeenCalledWith(2);
    expect(handlers.onRegionDragMove).toHaveBeenCalledWith(6);
    expect(handlers.onRegionDragEnd).toHaveBeenCalledWith(6);
  });

  it("cuts directly when blade mode is active", () => {
    const { waveform, handlers } = renderRow({ bladeActive: true });
    fireEvent.mouseDown(waveform, { button: 0, buttons: 1, clientX: 200, clientY: 20 });
    expect(handlers.onBlade).toHaveBeenCalledWith(4);
    expect(handlers.onSeek).toHaveBeenCalledWith(4);
    expect(handlers.onRegionDragStart).not.toHaveBeenCalled();
  });

  it("captures right-button drags without starting a selection", () => {
    const { waveform, handlers } = renderRow();
    fireEvent.mouseDown(waveform, { button: 2, buttons: 2, clientX: 100, clientY: 20 });
    fireEvent.mouseMove(waveform, { buttons: 2, clientX: 300, clientY: 20 });
    fireEvent.mouseUp(waveform, { button: 2, clientX: 300, clientY: 20 });
    expect(handlers.onDirectEditDragStart).toHaveBeenCalledWith(2);
    expect(handlers.onDirectEditDragMove).toHaveBeenCalledWith(6);
    expect(handlers.onDirectEditDragEnd).toHaveBeenCalledWith(6);
    expect(handlers.onRegionDragStart).not.toHaveBeenCalled();
  });

  it("creates and drags a loudness keyframe from the visible curve", () => {
    const { waveform, handlers } = renderRow();
    fireEvent.mouseDown(waveform, { button: 0, buttons: 1, clientX: 250, clientY: 48 });
    fireEvent.mouseMove(waveform, { buttons: 1, clientX: 250, clientY: 24 });
    expect(handlers.onEnvelopePointAdd).toHaveBeenCalledWith(expect.objectContaining({ time: 5, gain: 1 }));
    const point = handlers.onEnvelopePointAdd.mock.calls[0][0];
    expect(handlers.onEnvelopePointMove).toHaveBeenCalledWith(point.id, 5, 1.5);
    expect(handlers.onRegionDragStart).not.toHaveBeenCalled();
  });

  it("adds a local keyframe when the loudness curve is clicked", () => {
    const { waveform, handlers } = renderRow();
    fireEvent.mouseDown(waveform, { button: 0, buttons: 1, clientX: 250, clientY: 48, detail: 1 });
    fireEvent.mouseUp(waveform, { button: 0, clientX: 250, clientY: 48 });
    expect(handlers.onEnvelopePointAdd).toHaveBeenCalledWith(expect.objectContaining({ time: 5, gain: 1 }));
    expect(handlers.onSeek).not.toHaveBeenCalled();
  });

  it("exposes in-place delete, restore, and cut removal without fade handles", () => {
    const { handlers } = renderRow({
      regions: [{ start: 2, end: 3 }],
      selectedRange: { start: 4, end: 5 },
      editPoints: [6],
    });
    fireEvent.click(screen.getByRole("button", { name: /restore removed audio/i }));
    fireEvent.click(screen.getByRole("button", { name: /mute selected audio/i }));
    fireEvent.click(screen.getByRole("button", { name: /remove edit point/i }));
    expect(handlers.onRegionRestore).toHaveBeenCalledWith({ start: 2, end: 3 });
    expect(handlers.onRangeMuteToggle).toHaveBeenCalledOnce();
    expect(handlers.onEditPointRemove).toHaveBeenCalledWith(6);
    expect(screen.queryByRole("button", { name: /fade/i })).toBeNull();
  });

  it("offers sound restoration when the selected range is already muted", () => {
    renderRow({
      mutedRegions: [{ start: 3, end: 6 }],
      selectedRange: { start: 4, end: 5 },
    });
    expect(screen.getByRole("button", { name: /restore sound to selected audio/i })).toBeTruthy();
  });
});
