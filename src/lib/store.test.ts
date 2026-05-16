import { describe, it, expect } from "vitest";
import { displayLatency, indicatorState, connectionChip, type ChipView } from "./store.svelte";

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
    expect(indicatorState("mixed", null)).toBe("mixed");
    expect(indicatorState("translate", null)).toBe("translate");
  });
});

describe("connectionChip", () => {
  it("connected shows green translating with latency", () => {
    const v: ChipView = connectionChip("translate", "connected", "1.2s", null);
    expect(v.tone).toBe("ok");
    expect(v.text).toBe("Translating · connected · 1.2s");
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
  it("failed without title falls back", () => {
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
  it("mixed+idle reads as connecting", () => {
    expect(connectionChip("mixed", "idle", "—", null).tone).toBe("warn");
  });
});
