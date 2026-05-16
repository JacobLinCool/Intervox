import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@testing-library/svelte";
import Toggle from "./Toggle.svelte";
import Segmented from "./Segmented.svelte";
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
});
