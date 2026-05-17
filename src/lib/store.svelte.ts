import type { UnlistenFn } from "@tauri-apps/api/event";
import { cmd, on } from "./tauri";
import type {
  AccountStatus,
  AppError,
  AppStatus,
  AudioBackpressureMetrics,
  AudioDevices,
  Config,
  DriverState,
  MicPermission,
} from "./tauri";
import {
  modeFromBackend, modeToBackend,
  ALL_LANGS, SOURCE_LANGS,
  isSameLang, langPair,
} from "./constants";
import type { UiMode, LangCtx } from "./constants";

const AUDIO_INPUT_DETECTED_RMS = 0.0001;

function zeroBackpressureMetrics(): AudioBackpressureMetrics {
  return {
    capturePoolMisses: 0,
    captureCapacityDrops: 0,
    captureSinkDrops: 0,
    uplinkNoSessionDrops: 0,
    uplinkQueueDrops: 0,
    uplinkChunksSent: 0,
  };
}

// ────────────────────────────────────────────────────────────
// Pure helpers (exported so tests can import them directly)
// ────────────────────────────────────────────────────────────

export function displayLatency(ms: number | null): string {
  return ms == null ? "—" : `${(ms / 1000).toFixed(1)}s`;
}

export type ErrorKind = "network" | "mic" | "driver" | "permission" | null;

export function indicatorState(
  mode: UiMode,
  err: ErrorKind,
): "error" | "off" | "pass" | "translate" {
  if (err) return "error";
  return mode === "silence"
    ? "off"
    : mode === "pass"
      ? "pass"
      : "translate";
}

export type ChipView = { tone: "ok" | "warn" | "error" | "neutral"; text: string };

export function connectionChip(
  mode: UiMode,
  conn: "idle" | "connecting" | "connected" | "reconnecting" | "failed",
  latencyText: string,
  errorTitle: string | null,
): ChipView {
  if (mode === "silence") return { tone: "neutral", text: "Translation off" };
  if (mode === "pass") return { tone: "neutral", text: "Pass-through · no translation" };
  switch (conn) {
    case "connected":
      return { tone: "ok", text: `Translating · connected · ${latencyText}` };
    case "idle":
    case "connecting":
      return { tone: "warn", text: "Connecting to OpenAI…" };
    case "reconnecting":
      return { tone: "warn", text: "Reconnecting…" };
    case "failed":
      return { tone: "error", text: errorTitle ?? "Translation disconnected" };
  }
}

// ────────────────────────────────────────────────────────────
// Rune-based reactive store
// ────────────────────────────────────────────────────────────

class Store {
  // ── State ──────────────────────────────────────────────────
  status: AppStatus | null = $state(null);
  devices: AudioDevices = $state({ inputs: [], outputs: [] });
  config: Config | null = $state(null);
  account: AccountStatus = $state({
    hasKey: false,
    verified: false,
    maskedKey: null,
    lastVerified: null,
    monthMinutes: 0,
    monthUsd: 0,
    totalMinutes: 0,
    totalUsd: 0,
  });

  micPermission: MicPermission = $state("notDetermined");
  driverState: DriverState = $state("missing");

  srcText: string = $state("");
  tgtText: string = $state("");
  inputLevel: number = $state(0);
  outputLevel: number = $state(0);
  backpressure: AudioBackpressureMetrics = $state(zeroBackpressureMetrics());
  audioInputDetected: boolean = $state(false);
  lastError: AppError | null = $state(null);
  sourceDetected: boolean = $state(false);

  // UI-nav flags
  settingsTab: string = $state("status");
  captionsOpen: boolean = $state(false);
  captionsWindowOpen: boolean = $state(false);
  quickOpen: boolean = $state(false);
  onboardingOpen: boolean = $state(false);


  // Appearance
  theme: "light" | "dark" = $state("light");
  wallpaper: string = $state("lavender");

  // App version
  appVersion: string = $state("");

  // Toast queue
  toasts: { id: number; kind: "success" | "error"; text: string }[] = $state([]);
  private toastSeq = 0;

  private unlisten: UnlistenFn[] = [];
  private removeMeterResyncListeners: (() => void) | null = null;
  private meterSyncBusy = false;
  private backpressureSyncBusy = false;

  // ── Lifecycle ──────────────────────────────────────────────

  async init(): Promise<void> {
    try {
      const [status, config, account, micPermission, driverState] = await Promise.all([
        cmd.getAppStatus(),
        cmd.getConfig(),
        cmd.getAccountStatus(),
        cmd.getMicPermission(),
        cmd.getDriverState(),
      ]);
      this.status = status;
      this.config = config;
      this.account = account;
      this.micPermission = micPermission;
      this.driverState = driverState;
      this.applyLevels(status.inputLevel, status.outputLevel);

      this.captionsOpen = config.captions.enabled;
      this.onboardingOpen = !config.onboarding_completed;
      this.settingsTab = "status";
    } catch (e: unknown) {
      if (e && typeof e === "object" && "code" in e && "message" in e) {
        this.lastError = e as AppError;
      }
      // leave honest defaults otherwise
    }

    try { this.appVersion = await cmd.appVersion(); } catch {}

    void this.refreshDevices();
    void this.syncMeterLevels();
    void this.syncBackpressureMetrics();
    this.installMeterResyncListeners();

    // Subscribe to events — push unlisten fns
    this.unlisten.push(await on.status((s) => {
      this.status = s;
      // Honest idle: clear transcripts when mode is not translating.
      if (s.mode === "silence" || s.mode === "pass_through") this.clearTranscripts();
    }));
    this.unlisten.push(await on.inputLevel((v) => {
      this.applyLevels(v, this.outputLevel);
    }));
    this.unlisten.push(await on.outputLevel((v) => {
      this.applyLevels(this.inputLevel, v);
    }));
    this.unlisten.push(await on.backpressure((m) => {
      this.backpressure = m;
    }));
    this.unlisten.push(await on.latency((v) => {
      if (this.status) this.status = { ...this.status, latencyMs: v };
    }));
    this.unlisten.push(await on.srcDelta((t) => {
      let s = this.srcText + t;
      if (s.length > 4000) s = s.slice(-4000);
      this.srcText = s;
      this.sourceDetected = true;
    }));
    this.unlisten.push(await on.tgtDelta((t) => {
      let s = this.tgtText + t;
      if (s.length > 4000) s = s.slice(-4000);
      this.tgtText = s;
    }));
    this.unlisten.push(await on.devices((d) => { this.devices = d; }));
    this.unlisten.push(await on.error((e) => { this.lastError = e; }));
    // transcript-cleared: the Rust clear_transcript_history command has already
    // ended the active session log and deleted the on-disk JSONL files; this
    // listener just zeroes the live in-session buffers so the UI reflects it.
    this.unlisten.push(await on.transcriptCleared(() => { this.clearTranscripts(); }));
  }

  dispose(): void {
    for (const fn of this.unlisten) fn();
    this.unlisten = [];
    this.removeMeterResyncListeners?.();
    this.removeMeterResyncListeners = null;
  }

  // ── Derived getters ────────────────────────────────────────

  get mode(): UiMode {
    return modeFromBackend(this.status?.mode ?? "translate");
  }

  get health() {
    return this.status?.health ?? "ready";
  }

  get latencyText(): string {
    return displayLatency(this.status?.latencyMs ?? null);
  }

  get errorKind(): ErrorKind {
    const code = this.lastError?.code;
    if (!code) return null;
    if (code === "MIC_PERMISSION_DENIED") return "permission";
    if (code === "DRIVER_MISSING") return "driver";
    if (code === "NETWORK_ERROR") return "network";
    if (code === "AUDIO_DEVICE_LOST" || code.startsWith("MIC_")) return "mic";
    return null;
  }

  get indicator() {
    return indicatorState(this.mode, this.errorKind);
  }

  get targetLang() {
    const code = this.config?.translation.target_language ?? this.status?.targetLanguage;
    return ALL_LANGS.find((l) => l.code === code) ?? ALL_LANGS[0];
  }

  get sourceLang() {
    // The OpenAI realtime translation endpoint auto-detects the source
    // language; there is no user-facing selector. Always report auto-detect
    // so the language-pair status text stays honest.
    return SOURCE_LANGS[0];
  }

  get langCtx(): LangCtx {
    return {
      sourceLangCode: this.sourceLang.code,
      sourceDetected: this.sourceDetected,
      sourceLangName: this.sourceLang.name,
      targetLangName: this.targetLang.name,
      targetLangCode: this.targetLang.code,
    };
  }

  get langPairText(): string {
    return langPair(this.langCtx);
  }

  get sameLang(): boolean {
    return isSameLang(this.langCtx);
  }

  get isTranslating(): boolean {
    return this.mode === "translate" && !this.errorKind;
  }

  get mixPercent(): number {
    return this.config?.mix.original_voice_percent ?? 0;
  }

  // ── Actions ────────────────────────────────────────────────

  pushToast(kind: "success" | "error", text: string): void {
    const id = ++this.toastSeq;
    this.toasts = [...this.toasts, { id, kind, text }];
    setTimeout(() => {
      this.toasts = this.toasts.filter((t) => t.id !== id);
    }, 3000);
  }

  private async tryCmd(fn: () => Promise<unknown>): Promise<boolean> {
    try {
      await fn();
      return true;
    } catch (e: unknown) {
      if (e && typeof e === "object" && "code" in e && "message" in e) {
        this.lastError = e as AppError;
      }
      return false;
    }
  }

  private applyLevels(inputLevel: number, outputLevel: number): void {
    this.inputLevel = inputLevel;
    this.outputLevel = outputLevel;
    if (inputLevel >= AUDIO_INPUT_DETECTED_RMS) this.audioInputDetected = true;
  }

  private async syncMeterLevels(): Promise<void> {
    if (this.meterSyncBusy) return;
    this.meterSyncBusy = true;
    try {
      const levels = await cmd.getAudioLevels();
      this.applyLevels(levels.inputLevel, levels.outputLevel);
    } catch {
      // Event listeners carry realtime updates; this is only a foreground snapshot.
    } finally {
      this.meterSyncBusy = false;
    }
  }

  private async syncBackpressureMetrics(): Promise<void> {
    if (this.backpressureSyncBusy) return;
    this.backpressureSyncBusy = true;
    try {
      this.backpressure = await cmd.getAudioBackpressureMetrics();
    } catch {
      // Diagnostics snapshot only; live capture continues without it.
    } finally {
      this.backpressureSyncBusy = false;
    }
  }

  private installMeterResyncListeners(): void {
    if (this.removeMeterResyncListeners) return;
    if (typeof window === "undefined" || typeof document === "undefined") return;

    const sync = () => {
      void this.syncMeterLevels();
      void this.syncBackpressureMetrics();
    };
    const syncWhenVisible = () => {
      if (document.visibilityState === "visible") sync();
    };

    window.addEventListener("focus", sync);
    document.addEventListener("visibilitychange", syncWhenVisible);
    this.removeMeterResyncListeners = () => {
      window.removeEventListener("focus", sync);
      document.removeEventListener("visibilitychange", syncWhenVisible);
    };
  }

  async setMode(m: UiMode): Promise<void> {
    const ok = await this.tryCmd(() => cmd.setMode(modeToBackend(m)));
    if (ok) {
      await this.refreshStatus();
      if (this.config) this.config.audio.virtual_mic_mode = modeToBackend(m);
      if (m === "silence" || m === "pass") this.clearTranscripts();
    }
  }

  async setTargetLang(code: string): Promise<void> {
    const ok = await this.tryCmd(() => cmd.setTargetLanguage(code));
    if (!ok) return;
    if (this.config) this.config.translation.target_language = code;
    if (this.status) this.status = { ...this.status, targetLanguage: code };
  }

  async setSourceMic(id: string): Promise<void> {
    const previousId = this.config?.audio.source_mic_id ?? null;
    const previousName = this.status?.sourceMicName ?? null;
    const nextName =
      this.devices.inputs.find((d) => d.id === id)?.name ?? id.replace(/^coreaudio:/, "");
    if (this.config) this.config.audio.source_mic_id = id;
    if (this.status) this.status = { ...this.status, sourceMicName: nextName };

    const ok = await this.tryCmd(() => cmd.setSourceMic(id));
    if (!ok) {
      if (this.config) this.config.audio.source_mic_id = previousId;
      if (this.status) this.status = { ...this.status, sourceMicName: previousName };
      return;
    }
    await this.refreshStatus();
  }

  async setMixPercent(n: number): Promise<void> {
    const clamped = Math.max(0, Math.min(30, n));
    const ok = await this.tryCmd(() => cmd.setMixPercent(clamped));
    if (!ok) return;
    if (this.config) this.config.mix.original_voice_percent = clamped;
  }

  async setCaptions(patch: Partial<Config["captions"]>): Promise<void> {
    if (!this.config) return;
    const next = { ...this.config.captions, ...patch };
    const ok = await this.tryCmd(() => cmd.setCaptionsConfig(next));
    if (!ok) return;
    this.config.captions = next;
    this.captionsOpen = next.enabled;
  }

  async setPrivacy(patch: Partial<Config["privacy"]>): Promise<void> {
    if (!this.config) return;
    const next = { ...this.config.privacy, ...patch };
    const ok = await this.tryCmd(() => cmd.setPrivacyConfig(next));
    if (ok) this.config.privacy = next;
    this.pushToast(ok ? "success" : "error", ok ? "Privacy setting saved" : "Couldn't save setting");
  }

  async setShortcuts(s: Config["shortcuts"]): Promise<void> {
    const ok = await this.tryCmd(() => cmd.setShortcuts(s));
    if (!ok) return;
    if (this.config) this.config.shortcuts = s;
  }

  async setApiKey(k: string): Promise<void> {
    try {
      this.account = await cmd.setApiKey(k);
      this.pushToast("success", "API key saved");
    } catch (e: unknown) {
      if (e && typeof e === "object" && "code" in e && "message" in e) {
        this.lastError = e as AppError;
      }
      this.pushToast("error", "Couldn't save the key");
    }
  }

  async verifyApiKey(): Promise<void> {
    try {
      this.account = await cmd.verifyApiKey();
    } catch (e: unknown) {
      if (e && typeof e === "object" && "code" in e && "message" in e) {
        this.lastError = e as AppError;
      }
    }
  }

  async clearApiKey(): Promise<void> {
    const ok = await this.tryCmd(() => cmd.clearApiKey());
    if (!ok) {
      this.pushToast("error", "Couldn't remove the key");
      return;
    }
    this.account = {
      hasKey: false,
      verified: false,
      maskedKey: null,
      lastVerified: null,
      monthMinutes: 0,
      monthUsd: 0,
      totalMinutes: 0,
      totalUsd: 0,
    };
    this.pushToast("success", "API key removed");
  }

  async installVirtualMic(): Promise<void> {
    const ok = await this.tryCmd(() => cmd.installVirtualMic());
    await this.refreshStatus();
    await this.refreshDriverState();
    this.pushToast(ok ? "success" : "error", ok ? "Driver installed" : "Couldn't install driver");
  }

  async updateVirtualMic(): Promise<void> {
    const ok = await this.tryCmd(() => cmd.updateVirtualMic());
    await this.refreshStatus();
    await this.refreshDriverState();
    this.pushToast(ok ? "success" : "error", ok ? "Driver updated" : "Couldn't update driver");
  }

  async uninstallVirtualMic(): Promise<void> {
    const ok = await this.tryCmd(() => cmd.uninstallVirtualMic());
    await this.refreshStatus();
    await this.refreshDriverState();
    this.pushToast(ok ? "success" : "error", ok ? "Driver uninstalled" : "Couldn't uninstall driver");
  }

  async refreshStatus(): Promise<void> {
    try {
      this.status = await cmd.getAppStatus();
    } catch {
      // leave existing value
    }
  }

  async refreshDriverState(): Promise<void> {
    try {
      this.driverState = await cmd.getDriverState();
    } catch {
      // leave existing value
    }
  }

  async refreshDevices(): Promise<void> {
    try {
      this.devices = await cmd.getAudioDevices();
      await this.refreshStatus();
      await this.refreshDriverState();
    } catch {
      // leave existing value
    }
  }

  async openAudioMidiSetup(): Promise<void> {
    await this.tryCmd(() => cmd.openAudioMidiSetup());
  }

  async openMicPermission(): Promise<void> {
    try {
      this.micPermission = await cmd.openMicPermissionSettings();
    } catch (e: unknown) {
      if (e && typeof e === "object" && "code" in e && "message" in e) {
        this.lastError = e as AppError;
      }
    }
  }

  async openAccessibilitySettings(): Promise<void> {
    await this.tryCmd(() => cmd.openAccessibilitySettings());
  }

  async requestMicPermission(): Promise<MicPermission> {
    try {
      this.micPermission = await cmd.requestMicPermission();
      return this.micPermission;
    } catch (e: unknown) {
      if (e && typeof e === "object" && "code" in e && "message" in e) {
        this.lastError = e as AppError;
      }
      throw e;
    }
  }

  async refreshMicPermission(): Promise<void> {
    try {
      this.micPermission = await cmd.getMicPermission();
    } catch {
      // leave existing value
    }
  }

  async startTest(): Promise<void> {
    await this.tryCmd(() => cmd.startTestPhrase());
  }

  resetAudioInputDetection(): void {
    this.audioInputDetected = false;
  }

  async startMicLevelProbe(): Promise<void> {
    await this.tryCmd(() => cmd.startMicLevelProbe());
  }

  async stopMicLevelProbe(): Promise<void> {
    const ok = await this.tryCmd(() => cmd.stopMicLevelProbe());
    if (ok) this.inputLevel = 0;
  }

  async stopAll(): Promise<void> {
    const ok = await this.tryCmd(() => cmd.stopAllAudio());
    if (ok) this.clearTranscripts();
  }

  async completeOnboarding(): Promise<void> {
    const ok = await this.tryCmd(() => cmd.completeOnboarding());
    if (!ok) return;
    if (this.config) this.config.onboarding_completed = true;
    this.onboardingOpen = false;
  }

  clearTranscripts(): void {
    this.srcText = "";
    this.tgtText = "";
    this.sourceDetected = false;
  }

  async clearHistory(): Promise<void> {
    try {
      const n = await cmd.clearTranscriptHistory();
      this.pushToast(
        "success",
        n > 0 ? `Cleared ${n} saved transcript${n === 1 ? "" : "s"}` : "Cleared session transcript",
      );
    } catch (e: unknown) {
      this.pushToast("error", "Couldn't clear transcripts");
      if (e && typeof e === "object" && "code" in e && "message" in e) {
        this.lastError = e as AppError;
      }
    }
  }

  async setUiConfig(patch: Partial<Config["ui"]>): Promise<void> {
    if (!this.config) return;
    const next = { ...this.config.ui, ...patch };
    const ok = await this.tryCmd(() => cmd.setUiConfig(next));
    if (ok) this.config.ui = next;
    this.pushToast(ok ? "success" : "error", ok ? "Setting saved" : "Couldn't save setting");
  }

  async openExternalUrl(url: string): Promise<void> {
    const ok = await this.tryCmd(() => cmd.openExternalUrl(url));
    if (!ok) this.pushToast("error", "Couldn't open the link");
  }

  async loadConnectionLog(): Promise<{ ts: string; kind: string; detail: string }[]> {
    try {
      return await cmd.getConnectionLog();
    } catch {
      return [];
    }
  }

  dismissError(): void {
    this.lastError = null;
  }

  // UI setters
  setSettingsTab(t: string): void { this.settingsTab = t; }
  setCaptionsOpen(b: boolean): void { this.captionsOpen = b; }
  setQuickOpen(b: boolean): void { this.quickOpen = b; }
  setOnboardingOpen(b: boolean): void { this.onboardingOpen = b; }
  setTheme(t: "light" | "dark"): void { this.theme = t; }
  setWallpaper(w: string): void { this.wallpaper = w; }

  async openCaptionsWindow(): Promise<void> {
    const ok = await this.tryCmd(() => cmd.openCaptionsWindow());
    if (ok) this.captionsWindowOpen = true;
  }

  async closeCaptionsWindow(): Promise<void> {
    const ok = await this.tryCmd(() => cmd.closeCaptionsWindow());
    if (ok) this.captionsWindowOpen = false;
  }

  async toggleCaptionsWindow(): Promise<void> {
    if (this.captionsWindowOpen) {
      await this.closeCaptionsWindow();
    } else {
      await this.openCaptionsWindow();
    }
  }

  quit(): void { void cmd.closeWindow(); }

}

export const store = new Store();
