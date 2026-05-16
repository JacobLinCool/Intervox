<script lang="ts">
  import type { Snippet } from "svelte";
  import { css } from "$lib/util";

  let {
    last = false,
    children,
    extraStyle = {},
  }: {
    last?: boolean;
    children: Snippet;
    extraStyle?: Record<string, string | number>;
  } = $props();
</script>

<div
  style={css({
    padding: "11px 14px",
    borderBottom: last ? "none" : "0.5px solid var(--hairline)",
    display: "flex",
    alignItems: "center",
    gap: 12,
    ...extraStyle,
  })}
>
  {@render children()}
</div>
