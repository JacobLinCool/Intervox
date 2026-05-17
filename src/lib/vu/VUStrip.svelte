<script lang="ts">
  import { css } from "$lib/util";
  import { rmsToVuLevel } from "./level";

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

  const displayLevel = $derived(rmsToVuLevel(level));
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
      width: displayLevel * 100 + "%",
      background: `linear-gradient(90deg, ${color}, color-mix(in oklch, ${color} 60%, white))`,
      borderRadius: height / 2,
      transition: "width 80ms linear",
    })}
  ></div>
</div>
