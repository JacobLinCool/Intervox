<script lang="ts">
  import type { UiMode } from "$lib/constants";
  import { Glyph, Check } from "$lib/icons";
  import { css } from "$lib/util";

  let {
    meta,
    selected,
    onclick,
  }: {
    meta: { id: UiMode; label: string; color: string; short: string; body: string };
    selected: boolean;
    onclick: () => void;
  } = $props();
</script>

<div
  role="button"
  tabindex="0"
  {onclick}
  onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") onclick(); }}
  style={css({
    cursor: "pointer",
    padding: 12,
    borderRadius: 10,
    background: selected
      ? `linear-gradient(135deg, color-mix(in oklch, ${meta.color} 18%, var(--card-bg)) 0%, var(--card-bg) 80%)`
      : "var(--card-bg)",
    border: selected
      ? `1px solid color-mix(in oklch, ${meta.color} 50%, transparent)`
      : "0.5px solid var(--card-border)",
    transition: "all 120ms",
    position: "relative",
  })}
>
  <div style={css({ display: "flex", alignItems: "center", gap: 10, marginBottom: 6 })}>
    <span
      style={css({
        width: 24,
        height: 24,
        borderRadius: 6,
        background: `linear-gradient(135deg, color-mix(in oklch, ${meta.color} 80%, white) 0%, ${meta.color} 100%)`,
        display: "grid",
        placeItems: "center",
      })}
    >
      <Glyph size={14} color="#fff" />
    </span>
    <span style={css({ fontSize: 13, fontWeight: 600 })}>{meta.label}</span>
    {#if selected}
      <span style={css({ marginLeft: "auto", color: meta.color })}>
        <Check size={13} />
      </span>
    {/if}
  </div>
  <div style={css({ fontSize: 11.5, color: "var(--txt-3)", lineHeight: 1.45 })}>
    {meta.body}
  </div>
</div>
