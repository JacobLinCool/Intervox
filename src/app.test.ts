import { describe, it, expect } from "vitest";
import { render } from "@testing-library/svelte";
import App from "./App.svelte";
describe("App root", () => {
  it("renders the stage + Settings shell, no fake desktop menubar", () => {
    const { container } = render(App as any);
    expect(container.querySelector(".stage")).toBeTruthy();
    expect(container.querySelector(".menubar")).toBeNull();   // no fake macOS menu bar
    expect(container.querySelector(".traffic")).toBeNull();    // no traffic lights
    expect(container.innerHTML).toContain("Intervox");         // settings sidebar brand
  });
});
