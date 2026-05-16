<script lang="ts">
  let {
    name,
    size,
    color,
  }: {
    name: "mic" | "micSlash" | "waveform" | "speaker" | "globe" | "cloud" | "warn" | "ok" | "battery" | "wifi" | "search" | "close";
    size?: number;
    color?: string;
  } = $props();
</script>

{#if name === "mic"}
  {@const s = size ?? 16}
  <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke={color ?? "currentColor"} stroke-width="1.4" stroke-linecap="round">
    <rect x="6" y="2" width="4" height="7" rx="2"/>
    <path d="M3.5 7v1a4.5 4.5 0 0 0 9 0V7M8 12.5v1.5"/>
  </svg>
{:else if name === "micSlash"}
  {@const s = size ?? 16}
  <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke={color ?? "currentColor"} stroke-width="1.4" stroke-linecap="round">
    <rect x="6" y="2" width="4" height="7" rx="2"/>
    <path d="M3.5 7v1a4.5 4.5 0 0 0 9 0V7M8 12.5v1.5M2 2l12 12"/>
  </svg>
{:else if name === "waveform"}
  {@const s = size ?? 16}
  <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke={color ?? "currentColor"} stroke-width="1.6" stroke-linecap="round">
    <path d="M2 8h1.5M4.5 6v4M7 4v8M9.5 5.5v5M12 7v2M14 8h0.5"/>
  </svg>
{:else if name === "speaker"}
  {@const s = size ?? 16}
  <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke={color ?? "currentColor"} stroke-width="1.4" stroke-linejoin="round">
    <path d="M3 6h2.5L9 3v10L5.5 10H3z" fill="currentColor" stroke="none"/>
    <path d="M11 6c1 1 1 3 0 4M13 4c2 2 2 6 0 8" stroke-linecap="round"/>
  </svg>
{:else if name === "globe"}
  {@const s = size ?? 16}
  <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke={color ?? "currentColor"} stroke-width="1.4">
    <circle cx="8" cy="8" r="6"/>
    <ellipse cx="8" cy="8" rx="2.5" ry="6"/>
    <path d="M2 8h12"/>
  </svg>
{:else if name === "cloud"}
  {@const s = size ?? 16}
  <svg width={s} height={s} viewBox="0 0 16 16" fill="none" stroke={color ?? "currentColor"} stroke-width="1.4" stroke-linejoin="round">
    <path d="M5 12h7a2.5 2.5 0 0 0 .3-5A4 4 0 0 0 5 7 3 3 0 0 0 5 12Z"/>
  </svg>
{:else if name === "warn"}
  {@const s = size ?? 14}
  {@const c = color ?? "var(--c-error)"}
  <svg width={s} height={s} viewBox="0 0 16 16">
    <circle cx="8" cy="8" r="7" fill={c}/>
    <path d="M8 4.5v4.2M8 11v0.6" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/>
  </svg>
{:else if name === "ok"}
  {@const s = size ?? 14}
  {@const c = color ?? "var(--c-translate)"}
  <svg width={s} height={s} viewBox="0 0 16 16">
    <circle cx="8" cy="8" r="7" fill={c}/>
    <path d="M4.8 8.3 7 10.4 11.4 5.8" stroke="#fff" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
  </svg>
{:else if name === "battery"}
  <svg width="22" height="11" viewBox="0 0 22 11" fill="none">
    <rect x="0.5" y="0.5" width="18" height="10" rx="2.5" stroke="currentColor" opacity="0.6"/>
    <rect x="2" y="2" width="13" height="7" rx="1" fill="currentColor"/>
    <rect x="19.5" y="3" width="1.5" height="5" rx="0.8" fill="currentColor" opacity="0.6"/>
  </svg>
{:else if name === "wifi"}
  <svg width="16" height="12" viewBox="0 0 16 12" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round">
    <path d="M1.5 4.5C5.5 1 10.5 1 14.5 4.5"/>
    <path d="M3.8 6.8C6.4 4.4 9.6 4.4 12.2 6.8" opacity="0.85"/>
    <path d="M6 9C7.2 7.9 8.8 7.9 10 9" opacity="0.85"/>
    <circle cx="8" cy="10.5" r="0.9" fill="currentColor" stroke="none"/>
  </svg>
{:else if name === "search"}
  {@const s = size ?? 14}
  <svg width={s} height={s} viewBox="0 0 14 14" fill="none" stroke={color ?? "currentColor"} stroke-width="1.4" stroke-linecap="round">
    <circle cx="6" cy="6" r="4"/>
    <path d="m9 9 3.5 3.5"/>
  </svg>
{:else if name === "close"}
  {@const s = size ?? 12}
  <svg width={s} height={s} viewBox="0 0 12 12" fill="none" stroke={color ?? "currentColor"} stroke-width="1.5" stroke-linecap="round">
    <path d="M3 3l6 6M9 3l-6 6"/>
  </svg>
{/if}
