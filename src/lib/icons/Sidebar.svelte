<script lang="ts">
  let { name }: { name: "status" | "account" | "audio" | "translation" | "captions" | "shortcuts" | "privacy" | "advanced" } = $props();
</script>

{#if name === "status"}
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
    <circle cx="8" cy="8" r="5.5" stroke="currentColor" stroke-width="1.4"/>
    <circle cx="8" cy="8" r="2" fill="currentColor"/>
  </svg>
{:else if name === "audio"}
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round">
    <rect x="6" y="2.5" width="4" height="7" rx="2"/>
    <path d="M3.5 7.5v.5a4.5 4.5 0 0 0 9 0v-.5M8 12.5v1.5"/>
  </svg>
{:else if name === "translation"}
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
    <path d="M2 4h6M5 2.5v1.5M2.5 6.5c1.4 2.4 3 3.7 4.6 4.6M7.4 4c-.4 2-1.4 4.4-3.9 7"/>
    <path d="M9 13.5l2.5-6 2.5 6M9.8 11.5h3.4"/>
  </svg>
{:else if name === "captions"}
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
    <rect x="1.5" y="3.5" width="13" height="9" rx="1.8"/>
    <path d="M5.5 7.5c-.4-.5-1.2-.5-1.6 0s-.4 1.5 0 2 1.2.5 1.6 0M11.5 7.5c-.4-.5-1.2-.5-1.6 0s-.4 1.5 0 2 1.2.5 1.6 0"/>
  </svg>
{:else if name === "shortcuts"}
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4">
    <rect x="2" y="3.5" width="12" height="9" rx="1.5"/>
    <path d="M4.5 6.5h.01M7 6.5h.01M9.5 6.5h.01M12 6.5h.01M4.5 9h.01M7 9h.01M9.5 9h.01M12 9h.01M5 11.5h6" stroke-linecap="round"/>
  </svg>
{:else if name === "privacy" || name === "account"}
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round">
    <path d="M8 1.5 13 3.5v4c0 3.5-2.2 6-5 7-2.8-1-5-3.5-5-7v-4L8 1.5Z"/>
    <path d="M6 8l1.5 1.5L10.5 6.5" stroke-linecap="round" stroke-width="1.6"/>
  </svg>
{:else if name === "advanced"}
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4">
    <circle cx="8" cy="8" r="2"/>
    <path d="M8 1.5v2M8 12.5v2M14.5 8h-2M3.5 8h-2M12.6 3.4l-1.4 1.4M4.8 11.2l-1.4 1.4M12.6 12.6l-1.4-1.4M4.8 4.8 3.4 3.4" stroke-linecap="round"/>
  </svg>
{/if}
