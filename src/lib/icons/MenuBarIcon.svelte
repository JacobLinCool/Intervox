<script lang="ts">
  import { css } from "$lib/util";
  import Glyph from "./Glyph.svelte";

  let { state = "off", size = 16 } = $props();

  const colors: Record<string, string> = {
    off:       "#8a8a8e",
    pass:      "var(--c-pass)",
    translate: "var(--c-translate)",
    mixed:     "var(--c-mixed)",
    error:     "var(--c-error)",
  };

  let c = $derived(colors[state] ?? colors.off);
</script>

<span style={css({ position: 'relative', display: 'inline-flex', alignItems: 'center', justifyContent: 'center' })}>
  <Glyph {size} color={c} filled={state !== "off"} />
  {#if state === "translate"}
    <span style={css({
      position: 'absolute',
      right: -1,
      bottom: -1,
      width: 6,
      height: 6,
      borderRadius: '50%',
      background: 'var(--c-translate)',
      boxShadow: '0 0 0 1.2px var(--menubar-bg)',
      animation: 'pulse-dot 1.6s ease-in-out infinite',
    })}></span>
  {/if}
  {#if state === "error"}
    <span style={css({
      position: 'absolute',
      right: -2,
      top: -2,
      width: 7,
      height: 7,
      borderRadius: '50%',
      background: 'var(--c-error)',
      boxShadow: '0 0 0 1.2px var(--menubar-bg)',
    })}></span>
  {/if}
</span>
