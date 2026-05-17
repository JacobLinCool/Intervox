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
  function audioPaneConfig(outputPreviewEnabled = false) {
    return {
      version: 1,
      audio: {
        source_id: null,
        output_preview_enabled: outputPreviewEnabled,
        virtual_mic_mode: "silence",
        input_gain_db: 0,
        limiter_enabled: true,
      },
      translation: { target_language: "en" },
      mix: { original_voice_percent: 0, translated_voice_percent: 100, duck_original: true },
      captions: { enabled: true, show_source: true, show_target: true, font_size: "medium", always_on_top: true },
      privacy: { save_transcript_history: true },
      ui: { show_latency_badge: false, launch_at_login: false, hide_dock_icon: false },
      account: { openai_api_key: null, openai_api_key_verified: false, openai_api_key_last_verified: null },
      shortcuts: { toggle_translate: "Cmd+Shift+T", silence: "Cmd+Shift+M", captions: "Cmd+Shift+C" },
      onboarding_completed: true,
    };
  }

  it("renders 3 mode cards and honest 'no input sources' when device list empty", () => {
    const { container } = r3(AudioPane as any);
    expect(container.innerHTML).toContain("Output Mode");
    expect(container.innerHTML).toContain("Translate");
    expect(container.innerHTML).not.toContain("Translate + Original");
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

  it("renders system audio as an input source and calls store.setAudioSource", async () => {
    const previousConfig = store.config;
    const previousDevices = store.devices;
    const original = store.setAudioSource.bind(store);
    let selected: string | null = null;
    store.config = audioPaneConfig(false) as any;
    store.devices = {
      sources: [
        { id: "coreaudio:uid:mic", name: "Built-in Microphone", kind: "microphone" },
        { id: "intervox:source:system-audio", name: "System Audio", kind: "systemAudio" },
      ],
      inputs: [{ id: "coreaudio:uid:mic", name: "Built-in Microphone" }],
      outputs: [{ id: "coreaudio:uid:default-output", name: "Mac Speakers" }],
    };
    store.setAudioSource = async (id: string) => { selected = id; };

    try {
      const { getByText } = r3(AudioPane as any);
      await fireEvent.click(getByText("Built-in Microphone").closest("button")!);
      await fireEvent.click(getByText("System Audio"));
      expect(selected).toBe("intervox:source:system-audio");
    } finally {
      store.setAudioSource = original;
      store.config = previousConfig;
      store.devices = previousDevices;
    }
  });

  it("renders output preview with the default output device name", () => {
    const previousConfig = store.config;
    const previousDevices = store.devices;
    store.config = audioPaneConfig(false) as any;
    store.devices = {
      sources: [],
      inputs: [],
      outputs: [{ id: "coreaudio:uid:default-output", name: "Mac Speakers" }],
    };

    try {
      const { container } = r3(AudioPane as any);
      expect(container.innerHTML).toContain("Output Preview");
      expect(container.innerHTML).toContain("Mirror to speakers");
      expect(container.innerHTML).toContain("Mac Speakers");
    } finally {
      store.config = previousConfig;
      store.devices = previousDevices;
    }
  });

  it("clicking output preview toggle calls store.setOutputPreview", async () => {
    const previousConfig = store.config;
    const previousDevices = store.devices;
    const original = store.setOutputPreview.bind(store);
    let selected: boolean | null = null;
    store.config = audioPaneConfig(false) as any;
    store.devices = {
      sources: [],
      inputs: [],
      outputs: [{ id: "coreaudio:uid:default-output", name: "Mac Speakers" }],
    };
    store.setOutputPreview = async (enabled: boolean) => { selected = enabled; };

    try {
      const { getByLabelText } = r3(AudioPane as any);
      await fireEvent.click(getByLabelText("Mirror audio to default output"));
      expect(selected).toBe(true);
    } finally {
      store.setOutputPreview = original;
      store.config = previousConfig;
      store.devices = previousDevices;
    }
  });
});

import TranslationPane from "./panes/TranslationPane.svelte";
import { render as r4 } from "@testing-library/svelte";
describe("TranslationPane", () => {
  it("renders languages, performance, and original voice mix control", () => {
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
    expect(r5(CaptionsPane as any).container.innerHTML).toContain("Captions window");
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
    store.account = { hasKey: true, verified: true, maskedKey: "sk-...", lastVerified: null, monthMinutes: 0, monthUsd: 0, totalMinutes: 0, totalUsd: 0 };
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
      store.account = { hasKey: false, verified: false, maskedKey: null, lastVerified: null, monthMinutes: 0, monthUsd: 0, totalMinutes: 0, totalUsd: 0 };
      store.micPermission = "notDetermined";
    }
  });

  it("enables the microphone step only when permission is granted", async () => {
    const originalRefresh = store.refreshMicPermission.bind(store);

    store.onboardingOpen = true;
    store.account = { hasKey: true, verified: true, maskedKey: "sk-...", lastVerified: null, monthMinutes: 0, monthUsd: 0, totalMinutes: 0, totalUsd: 0 };
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
      store.account = { hasKey: false, verified: false, maskedKey: null, lastVerified: null, monthMinutes: 0, monthUsd: 0, totalMinutes: 0, totalUsd: 0 };
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

  it("renders compact source placeholder as the second caption line", () => {
    const { container } = rCW(CaptionsWindow as any);
    expect(container.innerHTML).toContain("Waiting for speech");
    expect(container.querySelector(".compact")).not.toBeNull();
  });

  it("toggles expanded layout from the caption window control", async () => {
    const { container, getByLabelText } = rCW(CaptionsWindow as any);
    expect(container.querySelector(".compact")).not.toBeNull();
    await fireEvent.click(getByLabelText("Expand captions"));
    expect(container.querySelector(".expanded")).not.toBeNull();
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
      sourceName: null,
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
      sourceName: "Built-in Microphone",
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
