<script lang="ts">
  import { css } from "$lib/util";
  import { rmsToVuLevel } from "./level";

  let {
    level = 0,
    bars = 12,
    color = "var(--c-translate)",
    height = 14,
    gap = 2,
    barWidth = 3,
    seed = 0,
  }: {
    level?: number;
    bars?: number;
    color?: string;
    height?: number;
    gap?: number;
    barWidth?: number;
    seed?: number;
  } = $props();

  const displayLevel = $derived(rmsToVuLevel(level));
</script>

<span style={css({ display: "inline-flex", alignItems: "flex-end", gap, height })}>
  {#each Array.from({ length: bars }) as _, i}
    {@const phase = (Math.sin(i * 0.45 + seed) + 1) / 2}
    {@const local = Math.max(0.08, Math.min(1, displayLevel * (0.55 + phase * 0.85) - i * 0.025))}
    {@const lit = i / bars < displayLevel}
    <span
      style={css({
        width: barWidth,
        height: Math.max(2, local * height) + "px",
        borderRadius: 1.5,
        background: lit ? color : "rgba(120,120,128,0.22)",
        transition: "height 80ms linear, background 200ms",
      })}
    ></span>
  {/each}
</span>
