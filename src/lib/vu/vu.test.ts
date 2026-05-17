import { describe, it, expect } from "vitest";
import { render } from "@testing-library/svelte";
import { VUBars, VUStrip, VUDots } from "./index";
import { rmsToVuLevel } from "./level";
describe("vu meters honest idle", () => {
  it("VUBars at level 0 renders bars, none lit (muted bg)", () => {
    const { container } = render(VUBars as any, { props: { level: 0, bars: 5 } });
    const spans = container.querySelectorAll("span > span");
    expect(spans.length).toBe(5);
    // every bar uses the muted idle background (no bar is lit at level 0)
    // browser normalises rgba() with spaces, so we match the substring without spaces
    [...spans].forEach((s) => {
      const style = (s as HTMLElement).getAttribute("style") || "";
      // rgba with or without spaces — honest idle: muted background, not the accent color
      expect(style).toMatch(/rgba\(120,\s*120,\s*128,\s*0\.22\)/);
    });
  });
  it("VUStrip at level 0 has 0% fill", () => {
    const { container } = render(VUStrip as any, { props: { level: 0 } });
    // browser may render "width: 0%" (with space) — match both forms
    expect(container.innerHTML).toMatch(/width:\s*0%/);
  });
  it("VUStrip renders only the current-level fill layer", () => {
    const { container } = render(VUStrip as any, { props: { level: 0.2 } });
    const strip = container.firstElementChild as HTMLElement;
    expect(strip.children).toHaveLength(1);
  });
  it("VUDots renders count dots", () => {
    const { container } = render(VUDots as any, { props: { level: 0, count: 4 } });
    expect(container.querySelectorAll("span > span").length).toBe(4);
  });
  it("maps low microphone RMS into a visible display level", () => {
    expect(rmsToVuLevel(0)).toBe(0);
    expect(rmsToVuLevel(0.0017)).toBeGreaterThan(0.08);
    expect(rmsToVuLevel(1)).toBe(1);
  });
});
