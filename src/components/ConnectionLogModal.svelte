<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { css } from "$lib/util";
  let { onClose }: { onClose: () => void } = $props();
  let entries = $state<{ ts: string; kind: string; detail: string }[]>([]);
  $effect(() => {
    let alive = true;
    store.loadConnectionLog().then((e) => {
      if (alive) entries = e.slice().reverse();
    });
    return () => {
      alive = false;
    };
  });
  $effect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events (scrim; Esc handled on window, Close btn is focusable) -->
<div
  role="presentation"
  onclick={onClose}
  style={css({ position: "fixed", inset: 0, background: "rgba(0,0,0,0.35)", zIndex: 9000, display: "grid", placeItems: "center" })}
>
  <div
    role="dialog"
    tabindex="-1"
    aria-labelledby="conn-log-title"
    onclick={(e) => e.stopPropagation()}
    style={css({ width: 560, maxHeight: "70vh", overflow: "auto", background: "var(--win-bg-solid)", border: "0.5px solid var(--hairline)", borderRadius: 12, padding: 16 })}
  >
    <div style={css({ display: "flex", alignItems: "center", marginBottom: 10 })}>
      <strong id="conn-log-title" style={css({ fontSize: 13 })}>Connection log</strong>
      <button class="btn ghost" style={css({ marginLeft: "auto" })} onclick={onClose}>Close</button>
    </div>
    {#if entries.length === 0}
      <div style={css({ fontSize: 12.5, color: "var(--txt-3)" })}>No connection events yet.</div>
    {:else}
      {#each entries as e, i (i)}
        <div class="mono" style={css({ fontSize: 11.5, color: "var(--txt-2)", padding: "3px 0", borderBottom: "0.5px solid var(--hairline)" })}>
          {e.ts} [{e.kind}] {e.detail}
        </div>
      {/each}
    {/if}
  </div>
</div>
