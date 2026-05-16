import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@testing-library/svelte";
import Toggle from "./Toggle.svelte";
import Segmented from "./Segmented.svelte";
import Pulldown from "./Pulldown.svelte";
describe("controls", () => {
  it("Toggle calls onChange with negated value", async () => {
    const onChange = vi.fn();
    const { container } = render(Toggle as any, { props: { value: false, onChange } });
    await fireEvent.click(container.querySelector("button")!);
    expect(onChange).toHaveBeenCalledWith(true);
  });
  it("Segmented marks the active option and switches", async () => {
    const onChange = vi.fn();
    const { getByText } = render(Segmented as any, {
      props: { value: "a", options: [{value:"a",label:"A"},{value:"b",label:"B"}], onChange },
    });
    await fireEvent.click(getByText("B"));
    expect(onChange).toHaveBeenCalledWith("b");
  });
  it("Pulldown renders its menu fixed to the viewport so cards cannot clip it", async () => {
    const onChange = vi.fn();
    const { container, getByRole, getByText } = render(Pulldown as any, {
      props: {
        value: "a",
        onChange,
        options: [{ value: "a", label: "Alpha" }, { value: "b", label: "Beta" }],
      },
    });

    await fireEvent.click(container.querySelector("button")!);
    expect(getByRole("listbox").getAttribute("style")).toContain("position: fixed");
    await fireEvent.click(getByText("Beta"));
    expect(onChange).toHaveBeenCalledWith("b");
  });
});
