import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import type { BackendMode, Quality } from "./constants";

export type Health = "ready" | "warning" | "error";
export type TranslationConn =
  | "idle" | "connecting" | "connected" | "reconnecting" | "failed";
export interface AppStatus {
  mode: BackendMode; health: Health; translation: TranslationConn;
  sourceMicName: string | null; virtualMicInstalled: boolean;
  openaiConnected: boolean; latencyMs: number | null;
  targetLanguage: string; inputLevel: number; outputLevel: number;
}
export interface DeviceInfo { id: string; name: string }
export interface AudioDevices { inputs: DeviceInfo[]; outputs: DeviceInfo[] }
export interface AudioLevels { inputLevel: number; outputLevel: number }
export interface RecoveryAction { label: string; command: string }
export interface AppError {
  code: string; title: string; message: string;
  recovery_action: RecoveryAction | null;
}
export interface AccountStatus {
  hasKey: boolean; verified: boolean;
  maskedKey: string | null; lastVerified: string | null;
  monthMinutes: number; monthUsd: number; totalMinutes: number; totalUsd: number;
}
export type MicPermission = "granted" | "denied" | "notDetermined" | "restricted";
export type DriverState = "missing" | "installedNotRunning" | "healthy" | "stale";
export interface ConnLogEntry { ts: string; kind: string; detail: string }
export interface MixSettings {
  original_gain_db: number;
  translated_gain_db: number;
  duck_original: boolean;
  limiter_enabled: boolean;
}
export interface Config {
  version: number;
  audio: { source_mic_id: string | null; monitor_output_id: string | null;
           virtual_mic_mode: string; input_gain_db: number; limiter_enabled: boolean };
  translation: { target_language: string; quality_mode: Quality };
  mix: { original_voice_percent: number; translated_voice_percent: number; duck_original: boolean };
  captions: { enabled: boolean; show_source: boolean; show_target: boolean;
              font_size: string; always_on_top: boolean };
  privacy: { save_transcript_history: boolean };
  ui: { show_latency_badge: boolean; launch_at_login: boolean; hide_dock_icon: boolean };
  account: {
    openai_api_key: string | null;
    openai_api_key_verified: boolean;
    openai_api_key_last_verified: string | null;
  };
  shortcuts: { toggle_translate: string; silence: string; captions: string };
  onboarding_completed: boolean;
}

export const cmd = {
  getAppStatus: () => invoke<AppStatus>("get_app_status"),
  getAudioDevices: () => invoke<AudioDevices>("get_audio_devices"),
  getAudioLevels: () => invoke<AudioLevels>("get_audio_levels"),
  getConfig: () => invoke<Config>("get_config"),
  getAccountStatus: () => invoke<AccountStatus>("get_account_status"),
  setMode: (mode: BackendMode) => invoke("set_virtual_mic_mode", { mode }),
  setSourceMic: (deviceId: string) => invoke("set_source_mic", { deviceId }),
  setMonitorOutput: (deviceId: string | null) => invoke("set_monitor_output", { deviceId }),
  setTargetLanguage: (language: string) => invoke("set_target_language", { language }),
  setQualityMode: (quality: string) => invoke("set_quality_mode", { quality }),
  setMixPercent: (percent: number) => invoke("set_mix_percent", { percent }),
  setCaptionsConfig: (c: Config["captions"]) => invoke("set_captions_config", { c }),
  setPrivacyConfig: (p: Config["privacy"]) => invoke("set_privacy_config", { p }),
  setShortcuts: (s: Config["shortcuts"]) => invoke("set_shortcuts", { s }),
  setApiKey: (key: string) => invoke<AccountStatus>("set_api_key", { key }),
  verifyApiKey: () => invoke<AccountStatus>("verify_api_key"),
  clearApiKey: () => invoke("clear_api_key"),
  installVirtualMic: () => invoke("install_virtual_mic"),
  updateVirtualMic: () => invoke("update_virtual_mic"),
  uninstallVirtualMic: () => invoke("uninstall_virtual_mic"),
  getDriverState: () => invoke<DriverState>("get_driver_state"),
  openAudioMidiSetup: () => invoke("open_audio_midi_setup"),
  openMicPermissionSettings: () => invoke<MicPermission>("open_system_mic_permission_settings"),
  getMicPermission: () => invoke<MicPermission>("get_mic_permission"),
  requestMicPermission: () => invoke<MicPermission>("request_mic_permission"),
  startTestPhrase: () => invoke("start_test_phrase"),
  startMicLevelProbe: () => invoke("start_mic_level_probe"),
  stopMicLevelProbe: () => invoke("stop_mic_level_probe"),
  clearTranscriptHistory: () => invoke<number>("clear_transcript_history"),
  getConnectionLog: () => invoke<ConnLogEntry[]>("get_connection_log"),
  setUiConfig: (ui: Config["ui"]) => invoke("set_ui_config", { ui }),
  openExternalUrl: (url: string) => invoke("open_external_url", { url }),
  appVersion: () => getVersion(),
  stopAllAudio: () => invoke("stop_all_audio"),
  completeOnboarding: () => invoke("complete_onboarding"),
  closeWindow: () => getCurrentWindow().close(),
  setMixSettings: (settings: MixSettings) => invoke("set_mix_settings", { settings }),
  openCaptionsWindow: () => invoke("open_captions_window"),
  closeCaptionsWindow: () => invoke("close_captions_window"),
  openAccessibilitySettings: () => invoke("open_accessibility_settings"),
};
export const on = {
  status: (f: (s: AppStatus) => void) => listen<AppStatus>("status-changed", (e) => f(e.payload)),
  inputLevel: (f: (v: number) => void) => listen<number>("input-level", (e) => f(e.payload)),
  outputLevel: (f: (v: number) => void) => listen<number>("output-level", (e) => f(e.payload)),
  latency: (f: (v: number) => void) => listen<number>("latency-changed", (e) => f(e.payload)),
  srcDelta: (f: (t: string) => void) =>
    listen<{ text: string }>("source-transcript-delta", (e) => f(e.payload.text)),
  tgtDelta: (f: (t: string) => void) =>
    listen<{ text: string }>("target-transcript-delta", (e) => f(e.payload.text)),
  devices: (f: (d: AudioDevices) => void) =>
    listen<AudioDevices>("device-list-changed", (e) => f(e.payload)),
  error: (f: (err: AppError) => void) => listen<AppError>("error", (e) => f(e.payload)),
  transcriptCleared: (f: () => void) => listen("transcript-cleared", () => f()),
};
export type { UnlistenFn };
