import { describe, it, expect } from "vitest";
import { MODES, modeToBackend, modeFromBackend, latencyToQuality,
         qualityToLatency, TARGET_LANGS, SOURCE_LANGS,
         detectedSourceName, langPair, isSameLang } from "./constants";
import type { LangCtx } from "./constants";

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
describe("auto-detect source is never misrepresented", () => {
  const auto = (sourceDetected: boolean): LangCtx => ({
    sourceLangCode: "auto", sourceDetected,
    sourceLangName: "Auto-detect", targetLangName: "English", targetLangCode: "en",
  });
  it("shows a neutral label, never a hardcoded language", () => {
    expect(detectedSourceName(auto(false))).toBe("Listening");
    expect(detectedSourceName(auto(true))).toBe("Auto");
    expect(detectedSourceName(auto(true))).not.toBe("Chinese");
    expect(langPair(auto(true))).toBe("Auto → English");
  });
  it("never claims same-language when the source is auto-detected", () => {
    expect(isSameLang(auto(true))).toBe(false);
    expect(isSameLang(auto(false))).toBe(false);
  });
  it("still flags same-language for an explicit source", () => {
    const ctx: LangCtx = {
      sourceLangCode: "en", sourceDetected: true,
      sourceLangName: "English", targetLangName: "English", targetLangCode: "en",
    };
    expect(isSameLang(ctx)).toBe(true);
  });
});
