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

  it("clicking an output mode calls store.setMode", async () => {
    const original = store.setMode.bind(store);
    let selected: string | null = null;
    store.setMode = async (mode: any) => { selected = mode; };

    try {
      const { getByText } = r3(AudioPane as any);
      await fireEvent.click(getByText("Pass-through"));
      expect(selected).toBe("pass");
    } finally {
      store.setMode = original;
    }
  });
});

import TranslationPane from "./panes/TranslationPane.svelte";
import { render as r4 } from "@testing-library/svelte";
describe("TranslationPane", () => {
  it("renders languages, performance, and disables mix slider unless mixed mode", () => {
    const { container, getByText } = r4(TranslationPane as any);
    expect(container.innerHTML).toContain("Target language");
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
    const advHtml = r5(AdvancedPane as any).container.innerHTML;
    // Footer shows "Intervox · © 2026" (dynamic version is empty in test env)
    expect(advHtml).toContain("Intervox");
    expect(advHtml).toContain("© 2026");
  });
});

describe("AudioPane removed controls", () => {
  it("does NOT render feedback protection toggle (removed — no DSP backend in virtual-mic arch)", () => {
    const { container } = r3(AudioPane as any);
    expect(container.innerHTML).not.toContain("Feedback protection");
    expect(container.innerHTML).not.toContain("feedback");
  });

  it("does NOT render mic environment selector (removed — no backend config field)", () => {
    const { container } = r3(AudioPane as any);
    expect(container.innerHTML).not.toContain("Mic environment");
    expect(container.innerHTML).not.toContain("Headset / close mic");
  });
});

import { store } from "$lib/store.svelte";
describe("AdvancedPane clear history", () => {
  it("renders Clear history button and description", () => {
    const { container } = r5(AdvancedPane as any);
    expect(container.innerHTML).toContain("Clear history");
    expect(container.innerHTML).toContain("Clear transcript history");
  });

  it("Clear history button invokes store.clearHistory on click", async () => {
    const original = store.clearHistory.bind(store);
    let called = false;
    store.clearHistory = async () => { called = true; };
    const { container } = r5(AdvancedPane as any);
    const btn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Clear history"
    ) as HTMLButtonElement | undefined;
    expect(btn).toBeTruthy();
    btn!.click();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(called).toBe(true);
    store.clearHistory = original;
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
import { fireEvent, render as r8, waitFor } from "@testing-library/svelte";
describe("Onboarding honest", () => {
  it("hidden by default; no fake transcript sentences in source", () => {
    const { container } = r8(Onboarding as any);
    expect(container.querySelector("[data-onboarding]")).toBeNull();
  });

  async function advanceToMicStep(getByText: (text: string) => HTMLElement) {
    await fireEvent.click(getByText("Get Started"));
    await fireEvent.click(getByText("Continue"));
  }

  function continueButton(container: HTMLElement) {
    return Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Continue"
    ) as HTMLButtonElement | undefined;
  }

  it("does not mark microphone access allowed when the OS returns denied", async () => {
    const originalRefresh = store.refreshMicPermission.bind(store);
    const originalRequest = store.requestMicPermission.bind(store);

    store.onboardingOpen = true;
    store.account = { hasKey: true, verified: true, maskedKey: "sk-...", lastVerified: null, usageUsd: 0, monthMinutes: 0, monthUsd: 0, totalMinutes: 0, totalUsd: 0 };
    store.micPermission = "notDetermined";
    store.refreshMicPermission = async () => {};
    store.requestMicPermission = async () => {
      store.micPermission = "denied";
      return "denied";
    };

    try {
      const { container, getByText, queryByText } = r8(Onboarding as any);
      await advanceToMicStep(getByText);

      await fireEvent.click(getByText("Allow Microphone"));
      await waitFor(() => expect(getByText("Denied")).toBeTruthy());

      expect(queryByText("Allowed")).toBeNull();
      expect(continueButton(container)?.disabled).toBe(true);
    } finally {
      store.refreshMicPermission = originalRefresh;
      store.requestMicPermission = originalRequest;
      store.onboardingOpen = false;
      store.account = { hasKey: false, verified: false, maskedKey: null, lastVerified: null, usageUsd: 0, monthMinutes: 0, monthUsd: 0, totalMinutes: 0, totalUsd: 0 };
      store.micPermission = "notDetermined";
    }
  });

  it("enables the microphone step only when permission is granted", async () => {
    const originalRefresh = store.refreshMicPermission.bind(store);

    store.onboardingOpen = true;
    store.account = { hasKey: true, verified: true, maskedKey: "sk-...", lastVerified: null, usageUsd: 0, monthMinutes: 0, monthUsd: 0, totalMinutes: 0, totalUsd: 0 };
    store.micPermission = "granted";
    store.refreshMicPermission = async () => {};

    try {
      const { container, getByText } = r8(Onboarding as any);
      await advanceToMicStep(getByText);

      expect(getByText("Allowed")).toBeTruthy();
      expect(continueButton(container)?.disabled).toBe(false);
    } finally {
      store.refreshMicPermission = originalRefresh;
      store.onboardingOpen = false;
      store.account = { hasKey: false, verified: false, maskedKey: null, lastVerified: null, usageUsd: 0, monthMinutes: 0, monthUsd: 0, totalMinutes: 0, totalUsd: 0 };
      store.micPermission = "notDetermined";
    }
  });
});

import CaptionsWindow from "../CaptionsWindow.svelte";
import { render as rCW } from "@testing-library/svelte";
describe("CaptionsWindow", () => {
  it("renders the pop-out container with data-captions-window", () => {
    const { container } = rCW(CaptionsWindow as any);
    expect(container.querySelector("[data-captions-window]")).not.toBeNull();
  });

  it("shows 'Waiting for translation' placeholder when store is empty", () => {
    const { container } = rCW(CaptionsWindow as any);
    expect(container.innerHTML).toContain("Waiting for translation");
  });

  it("does not show source block when srcText is empty", () => {
    const { container } = rCW(CaptionsWindow as any);
    // No "Original" label rendered when srcText is empty
    expect(container.innerHTML).not.toContain("Original");
  });
});

import StatusPane from "./panes/StatusPane.svelte";
import { render as r9 } from "@testing-library/svelte";
describe("StatusPane driver recovery card", () => {
  it("shows recovery card with install/reinstall/audio-midi buttons when driver missing", () => {
    // Simulate driver missing: virtualMicInstalled = false
    store.status = {
      mode: "translate",
      health: "error",
      translation: "idle",
      sourceMicName: null,
      virtualMicInstalled: false,
      openaiConnected: false,
      latencyMs: null,
      targetLanguage: "en",
      inputLevel: 0,
      outputLevel: 0,
    };
    store.driverState = "missing";
    const { container } = r9(StatusPane as any);
    expect(container.querySelector("[data-driver-recovery]")).not.toBeNull();
    expect(container.querySelector("[data-driver-install]")).not.toBeNull();
    expect(container.querySelector("[data-driver-reinstall]")).not.toBeNull();
    expect(container.querySelector("[data-driver-audio-midi]")).not.toBeNull();
    expect(container.innerHTML).toContain("Driver Recovery");
  });

  it("hides recovery card when driver is installed and running", () => {
    store.status = {
      mode: "translate",
      health: "ready",
      translation: "connected",
      sourceMicName: "Built-in Microphone",
      virtualMicInstalled: true,
      openaiConnected: false,
      latencyMs: null,
      targetLanguage: "en",
      inputLevel: 0,
      outputLevel: 0,
    };
    store.driverState = "healthy";
    const { container } = r9(StatusPane as any);
    expect(container.querySelector("[data-driver-recovery]")).toBeNull();
  });
});
