import { describe, it, expect } from "vitest";
import { MODES, modeToBackend, modeFromBackend, latencyToQuality,
         qualityToLatency, TARGET_LANGS, SOURCE_LANGS } from "./constants";

describe("mode mapping", () => {
  it("maps ui id <-> backend VirtualMicMode", () => {
    expect(modeToBackend("pass")).toBe("pass_through");
    expect(modeToBackend("mixed")).toBe("translate_with_original");
    expect(modeFromBackend("translate_with_original")).toBe("mixed");
    expect(modeFromBackend("silence")).toBe("silence");
  });
  it("MODES has the four ids", () => {
    expect(MODES.map((m) => m.id)).toEqual(["silence","pass","translate","mixed"]);
  });
});
describe("latency/quality mapping", () => {
  it("maps both directions", () => {
    expect(latencyToQuality("fastest")).toBe("low_latency");
    expect(latencyToQuality("smooth")).toBe("accuracy");
    expect(qualityToLatency("accuracy")).toBe("smooth");
    expect(qualityToLatency("balanced")).toBe("balanced");
  });
});
describe("language lists", () => {
  it("has 13 target langs and auto source", () => {
    expect(TARGET_LANGS.length).toBe(13);
    expect(SOURCE_LANGS[0].code).toBe("auto");
  });
});
