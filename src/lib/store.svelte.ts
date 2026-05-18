import type { UnlistenFn } from "@tauri-apps/api/event";
import { cmd, on } from "./tauri";
import type {
  AccountStatus,
  AppError,
  AppStatus,
  AudioBackpressureMetrics,
  AudioDevices,
  AudioMeterFrame,
  Config,
  DriverState,
  MicPermission,
  NotificationPermission,
} from "./tauri";
import {
  modeFromBackend, modeToBackend,
  ALL_LANGS, SOURCE_LANGS,
  isSameLang, langPair,
} from "./constants";
import type { BackendMode, UiMode, LangCtx } from "./constants";

const AUDIO_INPUT_DETECTED_RMS = 0.0001;
const MODE_COMMAND_TIMEOUT_MS = 5_000;

function timeoutError(message: string): AppError {
  return {
    code: "MODE_SWITCH_TIMEOUT",
    title: "Mode switch timed out",
    message,
    recovery_action: null,
  };
}

function withTimeout<T>(promise: Promise<T>, ms: number, message: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(timeoutError(message)), ms);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer) clearTimeout(timer);
  });
}

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

export type MeterProjection = {
  inputLevel: number;
  outputLevel: number;
  inputMeterSequence: number;
  outputMeterSequence: number;
  meterFrameSequence: number;
  meterEventCount: number;
  meterInputActive: boolean;
  meterOutputActive: boolean;
  audioInputDetected: boolean;
};

export function applyMeterFrameProjection(
  current: MeterProjection,
  frame: AudioMeterFrame,
): MeterProjection {
  return {
    inputLevel: frame.inputLevel,
    outputLevel: frame.outputLevel,
    inputMeterSequence: frame.inputSequence,
    outputMeterSequence: frame.outputSequence,
    meterFrameSequence: frame.sequence,
    meterEventCount: current.meterEventCount + 1,
    meterInputActive: frame.inputActive,
    meterOutputActive: frame.outputActive,
    audioInputDetected:
      current.audioInputDetected || frame.inputLevel >= AUDIO_INPUT_DETECTED_RMS,
  };
}

export function connectionChip(
  mode: UiMode,
  conn: "idle" | "connecting" | "connected" | "reconnecting" | "failed",
  latencyText: string,
  errorTitle: string | null,
): ChipView {
  if (mode === "silence") return { tone: "neutral", text: "Interpretation off" };
  if (mode === "pass") return { tone: "neutral", text: "Pass-through · no translation" };
  switch (conn) {
    case "connected":
      return { tone: "ok", text: `Interpreting · connected · ${latencyText}` };
    case "idle":
    case "connecting":
      return { tone: "warn", text: "Connecting to OpenAI…" };
    case "reconnecting":
      return { tone: "warn", text: "Reconnecting…" };
    case "failed":
      return { tone: "error", text: errorTitle ?? "Translation disconnected" };
  }
}

export function statusWithMode(status: AppStatus | null, mode: BackendMode): AppStatus | null {
  return status ? { ...status, mode } : null;
}

export function configWithMode(config: Config | null, mode: BackendMode): Config | null {
  return config
    ? { ...config, audio: { ...config.audio, virtual_mic_mode: mode } }
    : null;
}

function isNonTranslatingBackendMode(mode: BackendMode): boolean {
  return mode === "silence" || mode === "pass_through";
}

// ────────────────────────────────────────────────────────────
// Rune-based reactive store
// ────────────────────────────────────────────────────────────

class Store {
  // ── State ──────────────────────────────────────────────────
  status: AppStatus | null = $state(null);
  devices: AudioDevices = $state({ sources: [], inputs: [], outputs: [] });
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
  // Honest default before the backend probe resolves: "prompt" (undetermined),
  // never a fabricated "granted".
  notificationPermission: NotificationPermission = $state("prompt");

  srcText: string = $state("");
  tgtText: string = $state("");
  inputLevel: number = $state(0);
  outputLevel: number = $state(0);
  inputMeterSequence: number = $state(0);
  outputMeterSequence: number = $state(0);
  meterFrameSequence: number = $state(0);
  meterEventCount: number = $state(0);
  meterInputActive: boolean = $state(false);
  meterOutputActive: boolean = $state(false);
  backpressure: AudioBackpressureMetrics = $state(zeroBackpressureMetrics());
  audioInputDetected: boolean = $state(false);
  lastError: AppError | null = $state(null);
  sourceDetected: boolean = $state(false);

  // UI-nav flags
  settingsTab: string = $state("status");
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
  private eventsInstalled = false;
  private backpressureSyncBusy = false;
  private modeChangeGeneration = 0;
  private pendingMode: { generation: number; mode: BackendMode } | null = null;

  // ── Lifecycle ──────────────────────────────────────────────

  async init(): Promise<void> {
    this.installEventListeners();

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

      this.onboardingOpen = !config.onboarding_completed;
      this.settingsTab = "status";
    } catch (e: unknown) {
      if (e && typeof e === "object" && "code" in e && "message" in e) {
        this.lastError = e as AppError;
      }
      // leave honest defaults otherwise
    }

    try { this.appVersion = await cmd.appVersion(); } catch {}

    void this.refreshNotificationStatus();
    void this.refreshDevices();
    void this.syncBackpressureMetrics();
  }

  private installEventListeners(): void {
    if (this.eventsInstalled) return;
    this.eventsInstalled = true;

    // Subscribe to events — push unlisten fns
    const install = (name: string, setup: () => Promise<UnlistenFn>) => {
      void setup().then((unlisten) => {
        this.traceFrontendLifecycle(`listener-installed:${name}`);
        if (this.eventsInstalled) {
          this.unlisten.push(unlisten);
        } else {
          unlisten();
        }
      }).catch(() => {
        this.traceFrontendLifecycle(`listener-install-error:${name}`);
        // A failed listener must not prevent initial backend state hydration.
      });
    };

    install("meter", () => on.meter((frame) => {
      this.applyMeterFrame(frame);
    }));
    void cmd.recordFrontendMeterDiagnostics({
      eventCount: this.meterEventCount,
      frameSequence: this.meterFrameSequence,
      inputSequence: this.inputMeterSequence,
      outputSequence: this.outputMeterSequence,
      inputLevel: this.inputLevel,
      outputLevel: this.outputLevel,
      inputActive: this.meterInputActive,
      outputActive: this.meterOutputActive,
    }).catch(() => {});

    install("status", () => on.status((s) => {
      this.traceFrontendLifecycle("status-event", s.mode);
      this.applyStatusSnapshot(s);
    }));
    install("backpressure", () => on.backpressure((m) => {
      this.backpressure = m;
    }));
    install("latency", () => on.latency((v) => {
      if (this.status) this.status = { ...this.status, latencyMs: v };
    }));
    install("src-delta", () => on.srcDelta((t) => {
      let s = this.srcText + t;
      if (s.length > 4000) s = s.slice(-4000);
      this.srcText = s;
      this.sourceDetected = true;
    }));
    install("tgt-delta", () => on.tgtDelta((t) => {
      let s = this.tgtText + t;
      if (s.length > 4000) s = s.slice(-4000);
      this.tgtText = s;
    }));
    install("devices", () => on.devices((d) => { this.devices = d; }));
    install("captions-config", () => on.captionsConfig((captions) => {
      if (this.config) this.config = { ...this.config, captions };
    }));
    install("notification-permission", () => on.notificationPermission((p) => {
      this.notificationPermission = p;
    }));
    install("error", () => on.error((e) => { this.lastError = e; }));
    // transcript-cleared: the Rust clear_transcript_history command has already
    // ended the active session log and deleted the on-disk JSONL files; this
    // listener just zeroes the live in-session buffers so the UI reflects it.
    install("transcript-cleared", () => on.transcriptCleared(() => { this.clearTranscripts(); }));
  }

  dispose(): void {
    for (const fn of this.unlisten) fn();
    this.unlisten = [];
    this.eventsInstalled = false;
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
    if (code === "MIC_PERMISSION_DENIED" || code === "SYSTEM_AUDIO_PERMISSION_DENIED") return "permission";
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
      this.captureAppError(e);
      return false;
    }
  }

  private captureAppError(e: unknown): void {
    if (e && typeof e === "object" && "code" in e && "message" in e) {
      this.lastError = e as AppError;
    }
  }

  private traceFrontendLifecycle(
    event: string,
    mode: BackendMode | null = null,
    elapsedMs: number | null = null,
  ): void {
    void cmd.recordFrontendLifecycleDiagnostics({
      event,
      mode,
      statusMode: this.status?.mode ?? null,
      configMode: this.config?.audio.virtual_mic_mode ?? null,
      modeGeneration: this.pendingMode?.generation ?? null,
      elapsedMs,
    }).catch(() => {});
  }

  private applyStatusSnapshot(status: AppStatus): boolean {
    const pending = this.pendingMode;
    if (pending && status.mode !== pending.mode) {
      this.traceFrontendLifecycle("status-event-ignored-stale", status.mode);
      return false;
    }

    this.status = status;
    this.config = configWithMode(this.config, status.mode);
    this.applyLevels(status.inputLevel, status.outputLevel);
    if (isNonTranslatingBackendMode(status.mode)) this.clearTranscripts();
    return true;
  }

  private applyLevels(inputLevel: number, outputLevel: number): void {
    this.inputLevel = inputLevel;
    this.outputLevel = outputLevel;
    if (inputLevel >= AUDIO_INPUT_DETECTED_RMS) this.audioInputDetected = true;
  }

  private applyMeterFrame(frame: AudioMeterFrame): void {
    const next = applyMeterFrameProjection(
      {
        inputLevel: this.inputLevel,
        outputLevel: this.outputLevel,
        inputMeterSequence: this.inputMeterSequence,
        outputMeterSequence: this.outputMeterSequence,
        meterFrameSequence: this.meterFrameSequence,
        meterEventCount: this.meterEventCount,
        meterInputActive: this.meterInputActive,
        meterOutputActive: this.meterOutputActive,
        audioInputDetected: this.audioInputDetected,
      },
      frame,
    );
    this.inputLevel = next.inputLevel;
    this.outputLevel = next.outputLevel;
    this.inputMeterSequence = next.inputMeterSequence;
    this.outputMeterSequence = next.outputMeterSequence;
    this.meterFrameSequence = next.meterFrameSequence;
    this.meterEventCount = next.meterEventCount;
    this.meterInputActive = next.meterInputActive;
    this.meterOutputActive = next.meterOutputActive;
    this.audioInputDetected = next.audioInputDetected;

    if (this.meterEventCount > 0 && this.meterEventCount % 20 === 0) {
      void cmd.recordFrontendMeterDiagnostics({
        eventCount: this.meterEventCount,
        frameSequence: this.meterFrameSequence,
        inputSequence: this.inputMeterSequence,
        outputSequence: this.outputMeterSequence,
        inputLevel: this.inputLevel,
        outputLevel: this.outputLevel,
        inputActive: this.meterInputActive,
        outputActive: this.meterOutputActive,
      }).catch(() => {});
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

  async setMode(m: UiMode): Promise<void> {
    const mode = modeToBackend(m);
    const generation = ++this.modeChangeGeneration;
    const started = performance.now();

    this.pendingMode = { generation, mode };
    this.traceFrontendLifecycle("mode-set-start", mode);

    try {
      await withTimeout(
        cmd.setMode(mode),
        MODE_COMMAND_TIMEOUT_MS,
        `Backend did not confirm ${m} mode within ${MODE_COMMAND_TIMEOUT_MS / 1000}s.`,
      );
    } catch (e: unknown) {
      if (this.pendingMode?.generation === generation) {
        this.pendingMode = null;
      }
      this.captureAppError(e);
      this.traceFrontendLifecycle("mode-set-error", mode, Math.round(performance.now() - started));
      return;
    }

    this.traceFrontendLifecycle("mode-set-command-ok", mode, Math.round(performance.now() - started));
    if (this.pendingMode?.generation !== generation) return;

    try {
      const status = await cmd.getAppStatus();
      if (this.pendingMode?.generation === generation) {
        this.applyStatusSnapshot(status);
      }
    } catch {
      // The optimistic projection is already the intended local state; the next
      // status event will reconcile native fields such as levels or connection.
    } finally {
      if (this.pendingMode?.generation === generation) {
        this.pendingMode = null;
      }
      this.traceFrontendLifecycle("mode-set-done", mode, Math.round(performance.now() - started));
    }
  }

  async setTargetLang(code: string): Promise<void> {
    const ok = await this.tryCmd(() => cmd.setTargetLanguage(code));
    if (!ok) return;
    if (this.config) this.config.translation.target_language = code;
    if (this.status) this.status = { ...this.status, targetLanguage: code };
  }

  async setAudioSource(id: string): Promise<void> {
    const previousId = this.config?.audio.source_id ?? null;
    const previousName = this.status?.sourceName ?? null;
    const nextName =
      this.devices.sources.find((d) => d.id === id)?.name
        ?? id.replace(/^coreaudio:uid:/, "CoreAudio device ");
    if (this.config) this.config.audio.source_id = id;
    if (this.status) this.status = { ...this.status, sourceName: nextName };

    const ok = await this.tryCmd(() => cmd.setAudioSource(id));
    if (!ok) {
      if (this.config) this.config.audio.source_id = previousId;
      if (this.status) this.status = { ...this.status, sourceName: previousName };
      return;
    }
    await this.refreshStatus();
  }

  async setOutputPreview(enabled: boolean): Promise<void> {
    if (!this.config) return;
    const previous = this.config.audio.output_preview_enabled;
    this.config.audio.output_preview_enabled = enabled;

    const ok = await this.tryCmd(() => cmd.setOutputPreviewEnabled(enabled));
    if (!ok) {
      if (this.config) this.config.audio.output_preview_enabled = previous;
      this.pushToast(
        "error",
        enabled ? "Couldn't start output preview" : "Couldn't stop output preview",
      );
    }
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
  }

  async setCaptionsWindowExpanded(expanded: boolean): Promise<void> {
    await this.tryCmd(() => cmd.setCaptionsWindowExpanded(expanded));
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
      this.applyStatusSnapshot(await cmd.getAppStatus());
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
      this.config = await cmd.getConfig();
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

  async openSystemAudioPermission(): Promise<void> {
    await this.tryCmd(() => cmd.openSystemAudioPermissionSettings());
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

  async refreshNotificationStatus(): Promise<void> {
    try {
      const s = await cmd.getNotificationStatus();
      this.notificationPermission = s.permission;
      // The backend is the source of truth for the persisted period; keep the
      // config copy in sync if it drifted (e.g. clamped on load).
      if (this.config) this.config.ui.inactivity_reminder_minutes = s.inactivityMinutes;
    } catch {
      // Non-critical: leave honest defaults; Interpret is unaffected.
    }
  }

  // True when the OS will not show our (silent) reminders. Interpret keeps
  // working regardless — this only drives an informational warning in the UI.
  get notificationsBlocked(): boolean {
    return this.notificationPermission === "denied"
      || this.notificationPermission === "unsupported";
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
  setOnboardingOpen(b: boolean): void { this.onboardingOpen = b; }
  setTheme(t: "light" | "dark"): void { this.theme = t; }
  setWallpaper(w: string): void { this.wallpaper = w; }

  quit(): void { void cmd.quitApp(); }

}

export const store = new Store();
