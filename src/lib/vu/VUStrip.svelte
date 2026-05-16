<script lang="ts">
  import { css } from "$lib/util";

  let {
    level = 0,
    color = "var(--c-translate)",
    height = 6,
    seed = 0,
  }: {
    level?: number;
    color?: string;
    height?: number;
    seed?: number;
  } = $props();

  let peak = $state(0);

  $effect(() => {
    peak = Math.max(level, peak * 0.95);
  });
</script>

<div
  style={css({
    position: "relative",
    width: "100%",
    height,
    borderRadius: height / 2,
    background: "rgba(120,120,128,0.18)",
    overflow: "hidden",
  })}
>
  <div
    style={css({
      position: "absolute",
      top: 0,
      bottom: 0,
      left: 0,
      width: level * 100 + "%",
      background: `linear-gradient(90deg, ${color}, color-mix(in oklch, ${color} 60%, white))`,
      borderRadius: height / 2,
      transition: "width 80ms linear",
    })}
  ></div>
  <div
    style={css({
      position: "absolute",
      top: -1,
      bottom: -1,
      left: "calc(" + peak * 100 + "% - 1px)",
      width: 2,
      background: "rgba(0,0,0,0.35)",
      borderRadius: 1,
      transition: "left 120ms ease-out",
    })}
  ></div>
</div>
