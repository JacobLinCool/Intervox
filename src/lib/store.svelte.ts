import type { UnlistenFn } from "@tauri-apps/api/event";
import { cmd, on } from "./tauri";
import type { AppStatus, AudioDevices, Config, AccountStatus, AppError, MixSettings } from "./tauri";
import {
  modeFromBackend, modeToBackend,
  qualityToLatency, latencyToQuality,
  ALL_LANGS, SOURCE_LANGS,
  isSameLang, langPair,
} from "./constants";
import type { UiMode, UiLatency, Quality, LangCtx } from "./constants";

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
): "error" | "off" | "pass" | "translate" | "mixed" {
  if (err) return "error";
  return mode === "silence"
    ? "off"
    : mode === "pass"
      ? "pass"
      : mode === "translate"
        ? "translate"
        : "mixed";
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
    usageUsd: 0,
  });

  srcText: string = $state("");
  tgtText: string = $state("");
  inputLevel: number = $state(0);
  outputLevel: number = $state(0);
  lastError: AppError | null = $state(null);
  sourceDetected: boolean = $state(false);

  // UI-nav flags
  settingsTab: string = $state("status");
  captionsOpen: boolean = $state(false);
  quickOpen: boolean = $state(false);
  onboardingOpen: boolean = $state(false);

  // Audio settings
  feedbackProtection: boolean = $state(true);

  // Appearance
  theme: "light" | "dark" = $state("light");
  wallpaper: string = $state("lavender");

  private unlisten: UnlistenFn[] = [];

  // ── Lifecycle ──────────────────────────────────────────────

  async init(): Promise<void> {
    try {
      const [status, devices, config, account] = await Promise.all([
        cmd.getAppStatus(),
        cmd.getAudioDevices(),
        cmd.getConfig(),
        cmd.getAccountStatus(),
      ]);
      this.status = status;
      this.devices = devices;
      this.config = config;
      this.account = account;

      this.captionsOpen = config.captions.enabled;
      this.onboardingOpen = !config.onboarding_completed;
      this.settingsTab = "status";
    } catch (e: unknown) {
      if (e && typeof e === "object" && "code" in e && "message" in e) {
        this.lastError = e as AppError;
      }
      // leave honest defaults otherwise
    }

    // Subscribe to events — push unlisten fns
    this.unlisten.push(await on.status((s) => { this.status = s; }));
    this.unlisten.push(await on.inputLevel((v) => { this.inputLevel = v; }));
    this.unlisten.push(await on.outputLevel((v) => { this.outputLevel = v; }));
    this.unlisten.push(await on.latency((v) => {
      if (this.status) this.status = { ...this.status, latencyMs: v };
    }));
    this.unlisten.push(await on.srcDelta((t) => {
      this.srcText += t;
      this.sourceDetected = true;
    }));
    this.unlisten.push(await on.tgtDelta((t) => { this.tgtText += t; }));
    this.unlisten.push(await on.devices((d) => { this.devices = d; }));
    this.unlisten.push(await on.error((e) => { this.lastError = e; }));
  }

  dispose(): void {
    for (const fn of this.unlisten) fn();
    this.unlisten = [];
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
    const code = this.status?.targetLanguage ?? this.config?.translation.target_language;
    return ALL_LANGS.find((l) => l.code === code) ?? ALL_LANGS[0];
  }

  get sourceLang() {
    return SOURCE_LANGS.find((l) => l.code === this.config?.translation.source_language) ?? SOURCE_LANGS[0];
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
    return (this.mode === "translate" || this.mode === "mixed") && !this.errorKind;
  }

  get mixPercent(): number {
    return this.config?.mix.original_voice_percent ?? 15;
  }

  get latencyPref(): UiLatency {
    return qualityToLatency(this.config?.translation.quality_mode ?? "balanced");
  }

  // ── Actions ────────────────────────────────────────────────

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

  async setMode(m: UiMode): Promise<void> {
    const ok = await this.tryCmd(() => cmd.setMode(modeToBackend(m)));
    if (ok && (m === "silence" || m === "pass")) this.clearTranscripts();
  }

  async setTargetLang(code: string): Promise<void> {
    await this.tryCmd(() => cmd.setTargetLanguage(code));
    if (this.config) this.config.translation.target_language = code;
  }

  async setSourceLang(code: string): Promise<void> {
    await this.tryCmd(() => cmd.setSourceLanguage(code));
    if (this.config) this.config.translation.source_language = code;
  }

  async setSourceMic(id: string): Promise<void> {
    await this.tryCmd(() => cmd.setSourceMic(id));
    if (this.config) this.config.audio.source_mic_id = id;
  }

  async setLatencyPref(v: UiLatency): Promise<void> {
    const quality = latencyToQuality(v);
    await this.tryCmd(() => cmd.setQualityMode(quality));
    if (this.config) this.config.translation.quality_mode = quality;
  }

  async setMixPercent(n: number): Promise<void> {
    const clamped = Math.max(0, Math.min(30, n));
    await this.tryCmd(() => cmd.setMixPercent(clamped));
    if (this.config) this.config.mix.original_voice_percent = clamped;
  }

  async setCaptions(patch: Partial<Config["captions"]>): Promise<void> {
    if (!this.config) return;
    this.config.captions = { ...this.config.captions, ...patch };
    await this.tryCmd(() => cmd.setCaptionsConfig(this.config!.captions));
    this.captionsOpen = this.config.captions.enabled;
  }

  async setPrivacy(patch: Partial<Config["privacy"]>): Promise<void> {
    if (!this.config) return;
    this.config.privacy = { ...this.config.privacy, ...patch };
    await this.tryCmd(() => cmd.setPrivacyConfig(this.config!.privacy));
  }

  async setShortcuts(s: Config["shortcuts"]): Promise<void> {
    await this.tryCmd(() => cmd.setShortcuts(s));
    if (this.config) this.config.shortcuts = s;
  }

  async setApiKey(k: string): Promise<void> {
    try {
      this.account = await cmd.setApiKey(k);
    } catch (e: unknown) {
      if (e && typeof e === "object" && "code" in e && "message" in e) {
        this.lastError = e as AppError;
      }
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
    await this.tryCmd(() => cmd.clearApiKey());
    this.account = {
      hasKey: false,
      verified: false,
      maskedKey: null,
      lastVerified: null,
      usageUsd: 0,
    };
  }

  async installVirtualMic(): Promise<void> {
    // Note: currently rejects with driver_missing — catch → set lastError; do NOT fake installed
    await this.tryCmd(() => cmd.installVirtualMic());
  }

  async openMicPermission(): Promise<void> {
    await this.tryCmd(() => cmd.openMicPermissionSettings());
  }

  async startTest(): Promise<void> {
    await this.tryCmd(() => cmd.startTestPhrase());
  }

  async stopAll(): Promise<void> {
    const ok = await this.tryCmd(() => cmd.stopAllAudio());
    if (ok) this.clearTranscripts();
  }

  async completeOnboarding(): Promise<void> {
    await this.tryCmd(() => cmd.completeOnboarding());
    if (this.config) this.config.onboarding_completed = true;
    this.onboardingOpen = false;
  }

  clearTranscripts(): void {
    this.srcText = "";
    this.tgtText = "";
    this.sourceDetected = false;
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

  quit(): void { void cmd.closeWindow(); }

  async setFeedbackProtection(v: boolean): Promise<void> {
    this.feedbackProtection = v;
    const settings: MixSettings = { original_gain_db: 0, translated_gain_db: 0, duck_original: v, limiter_enabled: v };
    await this.tryCmd(() => cmd.setMixSettings(settings));
  }
}

export const store = new Store();
