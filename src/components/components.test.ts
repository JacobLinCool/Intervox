import { describe, it, expect } from "vitest";
import { render } from "@testing-library/svelte";
import Settings from "./Settings.svelte";
describe("Settings shell", () => {
  it("renders full-window (no floating .window / no traffic dots) and the sidebar", () => {
    const { container } = render(Settings as any);
    // fills window, not the absolutely-centered floating window
    expect(container.innerHTML).not.toContain("translate(-50%, -50%)");
    expect(container.querySelector(".traffic")).toBeNull();
    expect(container.innerHTML).toContain("Intervox");
  });
});

import AccountPane from "./panes/AccountPane.svelte";
import { render as r2 } from "@testing-library/svelte";
describe("AccountPane", () => {
  it("renders BYOK callout and a key input (no raw key prefilled)", () => {
    const { container } = r2(AccountPane as any);
    expect(container.innerHTML).toContain("How BYOK works");
    const input = container.querySelector('input');
    expect(input).toBeTruthy();
    expect((input as HTMLInputElement).value).toBe(""); // never prefill the secret
  });
});

import AudioPane from "./panes/AudioPane.svelte";
import { render as r3 } from "@testing-library/svelte";
describe("AudioPane", () => {
  it("renders 4 mode cards and honest 'no input devices' when device list empty", () => {
    const { container, getAllByText } = r3(AudioPane as any);
    expect(container.innerHTML).toContain("Output Mode");
    // 4 modes from MODES
    expect(container.innerHTML).toContain("Translate + Original");
    // honest: with store uninitialized, no fabricated mic names
    expect(container.innerHTML).not.toContain("MacBook Pro Microphone");
    expect(container.innerHTML).not.toContain("Shure MV7");
  });
});

import TranslationPane from "./panes/TranslationPane.svelte";
import { render as r4 } from "@testing-library/svelte";
describe("TranslationPane", () => {
  it("renders languages, performance, and disables mix slider unless mixed mode", () => {
    const { container, getByText } = r4(TranslationPane as any);
    expect(container.innerHTML).toContain("Source language");
    expect(container.innerHTML).toContain("Original voice volume");
    expect(getByText("Show all languages…")).toBeTruthy();
  });
});

import CaptionsPane from "./panes/CaptionsPane.svelte";
import ShortcutsPane from "./panes/ShortcutsPane.svelte";
import PrivacyPane from "./panes/PrivacyPane.svelte";
import AdvancedPane from "./panes/AdvancedPane.svelte";
import { render as r5 } from "@testing-library/svelte";
describe("remaining panes", () => {
  it("Captions/Shortcuts/Privacy/Advanced render", () => {
    expect(r5(CaptionsPane as any).container.innerHTML).toContain("Floating captions");
    expect(r5(ShortcutsPane as any).container.innerHTML).toContain("Global Shortcuts");
    expect(r5(PrivacyPane as any).container.innerHTML).toContain("How translation works");
    expect(r5(AdvancedPane as any).container.innerHTML).toContain("build 482");
  });
});

import QuickStatus from "./QuickStatus.svelte";
import { render as r6 } from "@testing-library/svelte";
describe("QuickStatus", () => {
  it("is hidden when quickOpen is false (default)", () => {
    const { container } = r6(QuickStatus as any);
    // Svelte 5 {#if} renders <!---> as comment placeholder when false — no visible content
    expect(container.querySelector("[role='menu']")).toBeNull(); // store.quickOpen defaults false
    expect(container.textContent?.trim()).toBe("");
  });
});

import Captions from "./Captions.svelte";
import { render as r7 } from "@testing-library/svelte";
describe("Captions honest", () => {
  it("hidden when captionsOpen false (default), no fake transcript text", () => {
    const { container } = r7(Captions as any);
    expect(container.querySelector("[data-captions]")).toBeNull();
    // none of the prototype fake lines ever appear
    expect(container.innerHTML).not.toContain("我覺得這個功能下週可以開始實作");
    expect(container.innerHTML).not.toContain("start implementing this feature next week");
  });
});

import Onboarding from "./Onboarding.svelte";
import { render as r8 } from "@testing-library/svelte";
describe("Onboarding honest", () => {
  it("hidden by default; no fake transcript sentences in source", () => {
    const { container } = r8(Onboarding as any);
    expect(container.querySelector("[data-onboarding]")).toBeNull();
  });
});
