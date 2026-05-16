<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { css } from "$lib/util";
</script>

{#if store.toasts.length}
  <div role="status" aria-live="polite" aria-atomic="true" style={css({ position: "fixed", top: 14, right: 14, zIndex: 9500, display: "flex", flexDirection: "column", gap: 8 })}>
    {#each store.toasts as t (t.id)}
      <div style={css({
        padding: "8px 12px", borderRadius: 8, fontSize: 12.5, color: "#fff",
        background: t.kind === "success" ? "var(--c-translate)" : "var(--c-error)",
        boxShadow: "0 8px 24px rgba(0,0,0,0.25)",
        animation: "pop-in 160ms ease-out both",
      })}>{t.text}</div>
    {/each}
  </div>
{/if}
