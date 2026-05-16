import { describe, it, expect } from "vitest";
import { render } from "@testing-library/svelte";
import Glyph from "./Glyph.svelte";
import { SysIcon } from "./index";
describe("icons", () => {
  it("Glyph renders an svg", () => {
    const { container } = render(Glyph, { props: { size: 20 } });
    expect(container.querySelector("svg")).toBeTruthy();
  });
  it("Sys renders the named icon", () => {
    const { container } = render(SysIcon as any, { props: { name: "mic" } });
    expect(container.querySelector("svg")).toBeTruthy();
  });
});
