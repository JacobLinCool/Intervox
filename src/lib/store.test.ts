import { describe, it, expect } from "vitest";
import {
  applyMeterFrameProjection,
  configWithMode,
  displayLatency,
  indicatorState,
  connectionChip,
  statusWithMode,
  type ChipView,
} from "./store.svelte";
import type { AppStatus, Config } from "./tauri";

describe("displayLatency", () => {
  it("formats ms to 1 decimal seconds, dash when null", () => {
    expect(displayLatency(1180)).toBe("1.2s");
    expect(displayLatency(null)).toBe("—");
  });
});
describe("indicatorState", () => {
  it("error wins, else maps mode", () => {
    expect(indicatorState("translate", "network")).toBe("error");
    expect(indicatorState("silence", null)).toBe("off");
    expect(indicatorState("pass", null)).toBe("pass");
    expect(indicatorState("translate", null)).toBe("translate");
  });
});

describe("connectionChip", () => {
  it("connected shows green interpreting with latency", () => {
    const v: ChipView = connectionChip("translate", "connected", "1.2s", null);
    expect(v.tone).toBe("ok");
    expect(v.text).toBe("Interpreting · connected · 1.2s");
  });
  it("silence is off/neutral", () => {
    expect(connectionChip("silence", "idle", "—", null).tone).toBe("neutral");
  });
  it("pass-through is neutral", () => {
    expect(connectionChip("pass", "idle", "—", null).tone).toBe("neutral");
  });
  it("failed is error with title", () => {
    const v = connectionChip("translate", "failed", "—", "Invalid API key");
    expect(v.tone).toBe("error");
    expect(v.text).toBe("Invalid API key");
  });
  it("failed without title uses default message", () => {
    expect(connectionChip("translate", "failed", "—", null).text).toBe("Translation disconnected");
  });
  it("connecting and reconnecting are warn", () => {
    expect(connectionChip("translate", "connecting", "—", null).tone).toBe("warn");
    expect(connectionChip("translate", "reconnecting", "—", null).tone).toBe("warn");
  });
  it("translate+idle reads as connecting, not off", () => {
    const v = connectionChip("translate", "idle", "—", null);
    expect(v.tone).toBe("warn");
    expect(v.text).toBe("Connecting to OpenAI…");
  });
});

describe("mode state projection", () => {
  it("updates status mode immutably", () => {
    const status = { mode: "translate" } as unknown as AppStatus;
    const next = statusWithMode(status, "silence");
    expect(next?.mode).toBe("silence");
    expect(status.mode).toBe("translate");
  });

  it("updates config audio mode immutably", () => {
    const config = {
      audio: { virtual_mic_mode: "translate" },
    } as unknown as Config;
    const next = configWithMode(config, "pass_through");
    expect(next?.audio.virtual_mic_mode).toBe("pass_through");
    expect(config.audio.virtual_mic_mode).toBe("translate");
    expect(next).not.toBe(config);
    expect(next?.audio).not.toBe(config.audio);
  });
});

describe("applyMeterFrameProjection", () => {
  it("maps backend audio-meter frames into frontend store state", () => {
    const next = applyMeterFrameProjection(
      {
        inputLevel: 0,
        outputLevel: 0,
        inputMeterSequence: 0,
        outputMeterSequence: 0,
        meterFrameSequence: 0,
        meterEventCount: 2,
        meterInputActive: false,
        meterOutputActive: false,
        audioInputDetected: false,
      },
      {
        sequence: 42,
        inputLevel: 0.003,
        outputLevel: 0.002,
        inputActive: true,
        outputActive: true,
        inputSequence: 100,
        outputSequence: 80,
      },
    );

    expect(next.inputLevel).toBe(0.003);
    expect(next.outputLevel).toBe(0.002);
    expect(next.inputMeterSequence).toBe(100);
    expect(next.outputMeterSequence).toBe(80);
    expect(next.meterFrameSequence).toBe(42);
    expect(next.meterEventCount).toBe(3);
    expect(next.meterInputActive).toBe(true);
    expect(next.meterOutputActive).toBe(true);
    expect(next.audioInputDetected).toBe(true);
  });
});
