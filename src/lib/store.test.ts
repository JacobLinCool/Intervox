import { describe, it, expect } from "vitest";
import { displayLatency, indicatorState } from "./store.svelte";

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
