<script lang="ts">
  import { css } from "$lib/util";

  let {
    title,
    sub,
    width,
  }: {
    title: string;
    sub?: string;
    width?: number;
  } = $props();
</script>

<div style={css({ width: width ?? 180, flexShrink: 0 })}>
  <div style={css({ fontSize: 13, fontWeight: 500 })}>{title}</div>
  {#if sub}
    <div
      style={css({
        fontSize: 11.5,
        color: "var(--txt-3)",
        marginTop: 1,
      })}
    >
      {sub}
    </div>
  {/if}
</div>
