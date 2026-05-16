import { describe, it, expect } from "vitest";
import { css } from "./util";

describe("css", () => {
  it("adds px to length numbers, leaves unitless alone", () => {
    expect(css({ width: 10, opacity: 0.5, zIndex: 3 }))
      .toBe("width:10px;opacity:0.5;z-index:3");
  });
  it("kebab-cases and drops nullish", () => {
    expect(css({ backgroundColor: "red", color: undefined }))
      .toBe("background-color:red");
  });
  it("drops false and empty-string values", () => {
    expect(css({ display: false, color: "", margin: 0 })).toBe("margin:0px");
  });
  it("keeps zero and negative length numbers", () => {
    expect(css({ top: 0, left: -4 })).toBe("top:0px;left:-4px");
  });
});
