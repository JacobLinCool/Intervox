<script lang="ts">
  import { css } from "$lib/util";

  let {
    title,
    sub,
  }: {
    title: string;
    sub?: string;
  } = $props();
</script>

<div style={css({ marginBottom: 18 })}>
  <h2
    style={css({
      fontSize: 17,
      fontWeight: 600,
      margin: 0,
      letterSpacing: -0.1,
    })}
  >
    {title}
  </h2>
  {#if sub}
    <p
      style={css({
        fontSize: 12.5,
        color: "var(--txt-3)",
        margin: "4px 0 0",
        lineHeight: 1.5,
      })}
    >
      {sub}
    </p>
  {/if}
</div>
