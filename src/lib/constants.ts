/* Shared constants: modes, language lists, and UI<->backend mappings. */

export type UiMode = "silence" | "pass" | "translate";
export type BackendMode =
  | "silence" | "pass_through" | "translate";

export const MODES: { id: UiMode; label: string; color: string; short: string; body: string }[] = [
  {
    id: "silence",
    label: "Silence",
    color: "var(--c-silence)",
    short: "No audio is sent",
    body: "No audio is sent through Translator Mic.",
  },
  {
    id: "pass",
    label: "Pass-through",
    color: "var(--c-pass)",
    short: "Original audio only",
    body: "Your original microphone audio is sent unchanged.",
  },
  {
    id: "translate",
    label: "Translate",
    color: "var(--c-translate)",
    short: "Translating",
    body: "Translated speech is sent. Original voice volume is controlled in Translation.",
  },
];

/* OpenAI Realtime Translation currently supports 13 target output languages.
   Source (input) accepts a broader set plus auto-detect. */
export const TARGET_LANGS: { code: string; name: string }[] = [
  { code: "en", name: "English"    },
  { code: "zh", name: "Chinese"    },
  { code: "ja", name: "Japanese"   },
  { code: "ko", name: "Korean"     },
  { code: "es", name: "Spanish"    },
  { code: "fr", name: "French"     },
  { code: "de", name: "German"     },
  { code: "it", name: "Italian"    },
  { code: "pt", name: "Portuguese" },
  { code: "ru", name: "Russian"    },
  { code: "hi", name: "Hindi"      },
  { code: "id", name: "Indonesian" },
  { code: "vi", name: "Vietnamese" },
];

/* Common picks surfaced in the Translation pane's quick grid. */
export const COMMON_LANGS: { code: string; name: string }[] =
  TARGET_LANGS.filter((l) => ["en", "zh", "ja", "ko", "es", "fr"].includes(l.code));

export const ALL_LANGS: { code: string; name: string }[] = TARGET_LANGS;

export const SOURCE_LANGS: { code: string; name: string }[] = [
  { code: "auto", name: "Auto-detect" },
  { code: "zh",   name: "Chinese" },
  { code: "en",   name: "English" },
  { code: "ja",   name: "Japanese" },
  { code: "ko",   name: "Korean" },
  { code: "es",   name: "Spanish" },
  { code: "fr",   name: "French" },
  { code: "de",   name: "German" },
  { code: "it",   name: "Italian" },
  { code: "pt",   name: "Portuguese" },
  { code: "ru",   name: "Russian" },
  { code: "hi",   name: "Hindi" },
  { code: "id",   name: "Indonesian" },
  { code: "vi",   name: "Vietnamese" },
];

/* Mode mappings: UI id <-> Rust VirtualMicMode (snake_case) */
const M2B: Record<UiMode, BackendMode> = {
  silence: "silence", pass: "pass_through",
  translate: "translate",
};
export const modeToBackend = (m: UiMode): BackendMode => M2B[m];
export const modeFromBackend = (b: BackendMode): UiMode =>
  (Object.keys(M2B) as UiMode[]).find((k) => M2B[k] === b) ?? "translate";

/* Language display helpers (pure functions; no React dependency). */
export interface LangCtx {
  sourceLangCode: string; sourceDetected: boolean;
  sourceLangName: string; targetLangName: string; targetLangCode: string;
}
export function detectedSourceName(c: LangCtx): string {
  // The realtime translation endpoint auto-detects the source language and
  // does not report which language it picked. Never claim a specific source;
  // show a neutral label instead of a hardcoded guess.
  if (c.sourceLangCode === "auto") return c.sourceDetected ? "Auto" : "Listening";
  return c.sourceLangName;
}
export function langPair(c: LangCtx): string {
  return `${detectedSourceName(c)} → ${c.targetLangName}`;
}
export function isSameLang(c: LangCtx): boolean {
  // Source is auto-detected and not reported back, so we cannot know whether
  // it matches the target. Only flag a same-language clash for an explicit
  // (non-auto) source.
  if (c.sourceLangCode === "auto") return false;
  return c.sourceLangCode === c.targetLangCode;
}
