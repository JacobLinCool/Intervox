<script lang="ts">
  import type { Snippet } from "svelte";
  import { css } from "$lib/util";

  let {
    title,
    children,
    hint,
    hintSnippet,
  }: {
    title?: string;
    children: Snippet;
    hint?: string;
    hintSnippet?: Snippet;
  } = $props();
</script>

<div style={css({ marginBottom: 22 })}>
  {#if title}
    <div
      style={css({
        fontSize: 11,
        fontWeight: 600,
        color: "var(--txt-3)",
        letterSpacing: 0.6,
        textTransform: "uppercase",
        marginBottom: 8,
      })}
    >
      {title}
    </div>
  {/if}
  <div class="card" style={css({ overflow: "hidden" })}>
    {@render children()}
  </div>
  {#if hint}
    <div
      style={css({
        fontSize: 11.5,
        color: "var(--txt-3)",
        marginTop: 8,
        lineHeight: 1.5,
      })}
    >
      {hint}
    </div>
  {:else if hintSnippet}
    <div
      style={css({
        fontSize: 11.5,
        color: "var(--txt-3)",
        marginTop: 8,
        lineHeight: 1.5,
      })}
    >
      {@render hintSnippet()}
    </div>
  {/if}
</div>
