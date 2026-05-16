<script lang="ts">
  import { css } from "$lib/util";

  let {
    level = 0,
    color = "var(--c-translate)",
    seed = 0,
    count = 6,
  }: {
    level?: number;
    color?: string;
    seed?: number;
    count?: number;
  } = $props();
</script>

<span style={css({ display: "inline-flex", gap: 2, alignItems: "flex-end", height: 12 })}>
  {#each Array.from({ length: count }) as _, i}
    {@const phase = (Math.sin(i * 0.5 + seed) + 1) / 2}
    {@const lvl = Math.max(0.15, Math.min(1, level * (0.6 + phase * 0.7)))}
    <span
      style={css({
        width: 2.5,
        height: 2 + lvl * 10 + "px",
        background: color,
        borderRadius: 1,
        opacity: 0.95,
        transition: "height 80ms linear",
      })}
    ></span>
  {/each}
</span>
