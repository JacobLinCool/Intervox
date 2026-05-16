<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { SOURCE_LANGS, ALL_LANGS, COMMON_LANGS } from "$lib/constants";
  import { PaneTitle, FieldGroup, Row, RowLabel, Pulldown, Segmented, Slider } from "$lib/controls";
  import { LangChip, Check, SysIcon } from "$lib/icons";
  import { css } from "$lib/util";

  // ── Local UI state only ────────────────────────────────────────
  let showAll = $state(false);
  let search = $state("");
</script>

{#snippet showAllHint()}
  {#if !showAll}
    <span
      style={css({
        cursor: "pointer",
        color: "var(--c-mixed)",
        textDecoration: "underline",
        textUnderlineOffset: 2,
      })}
      role="button"
      tabindex="0"
      onclick={() => (showAll = true)}
      onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") showAll = true; }}
    >Show all languages…</span>
  {/if}
{/snippet}

<PaneTitle
  title="Translation"
  sub="Language and latency settings for translated speech."
/>

{#snippet langLeft(o: { value: string; label: string })}
  <LangChip code={o.value} size={16} />
{/snippet}

<FieldGroup title="Languages">
  <Row>
    <RowLabel title="Source language" sub="What you speak." />
    <Pulldown
      value={store.sourceLang.code}
      options={SOURCE_LANGS.map((l) => ({ value: l.code, label: l.name }))}
      onChange={(v) => store.setSourceLang(v)}
    />
  </Row>
  <Row last>
    <RowLabel title="Target language" sub="What the meeting hears." />
    <Pulldown
      value={store.targetLang.code}
      options={ALL_LANGS.map((l) => ({ value: l.code, label: l.name }))}
      onChange={(v) => store.setTargetLang(v)}
      optionLeft={langLeft}
    />
  </Row>
</FieldGroup>

<FieldGroup title="Common Languages" hintSnippet={showAllHint}>
  <div
    style={css({
      padding: 6,
      display: "grid",
      gridTemplateColumns: "repeat(3, 1fr)",
      gap: 4,
    })}
  >
    {#each COMMON_LANGS as l (l.code)}
      {@const selected = store.targetLang.code === l.code}
      <div
        role="button"
        tabindex="0"
        onclick={() => store.setTargetLang(l.code)}
        onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") store.setTargetLang(l.code); }}
        style={css({
          display: "flex",
          alignItems: "center",
          gap: 9,
          padding: "7px 9px",
          borderRadius: 6,
          cursor: "pointer",
          background: selected
            ? "color-mix(in oklch, var(--c-mixed) 14%, transparent)"
            : "transparent",
          border: selected
            ? "0.5px solid color-mix(in oklch, var(--c-mixed) 35%, transparent)"
            : "0.5px solid transparent",
        })}
      >
        <LangChip code={l.code} size={16} />
        <span style={css({ fontSize: 12.5, fontWeight: selected ? 500 : 400 })}>{l.name}</span>
        {#if selected}
          <span style={css({ marginLeft: "auto", color: "var(--c-mixed)" })}>
            <Check size={10} />
          </span>
        {/if}
      </div>
    {/each}
  </div>

  {#if showAll}
    <div style={css({ borderTop: "0.5px solid var(--hairline)", padding: "8px 10px" })}>
      <div
        style={css({
          display: "flex",
          alignItems: "center",
          gap: 6,
          padding: "5px 9px",
          background: "var(--control-bg)",
          border: "0.5px solid var(--control-border)",
          borderRadius: 6,
        })}
      >
        <span style={css({ color: "var(--txt-3)" })}>
          <SysIcon name="search" size={12} />
        </span>
        <input
          value={search}
          oninput={(e) => (search = (e.currentTarget as HTMLInputElement).value)}
          placeholder="Search language…"
          style={css({
            flex: 1,
            border: "none",
            outline: "none",
            background: "transparent",
            fontSize: 12.5,
            fontFamily: "inherit",
            color: "var(--txt-1)",
          })}
        />
      </div>
    </div>
    <div
      style={css({
        padding: 6,
        display: "grid",
        gridTemplateColumns: "repeat(3, 1fr)",
        gap: 4,
        maxHeight: 180,
        overflowY: "auto",
      })}
    >
      {#each ALL_LANGS.filter((l) => l.name.toLowerCase().includes(search.toLowerCase())) as l (l.code)}
        {@const selected = store.targetLang.code === l.code}
        <div
          role="button"
          tabindex="0"
          onclick={() => store.setTargetLang(l.code)}
          onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") store.setTargetLang(l.code); }}
          style={css({
            display: "flex",
            alignItems: "center",
            gap: 9,
            padding: "6px 9px",
            borderRadius: 6,
            cursor: "pointer",
            background: selected
              ? "color-mix(in oklch, var(--c-mixed) 14%, transparent)"
              : "transparent",
            fontSize: 12.5,
          })}
        >
          <LangChip code={l.code} size={16} />
          <span>{l.name}</span>
        </div>
      {/each}
    </div>
  {/if}
</FieldGroup>

<FieldGroup title="Performance">
  <Row>
    <RowLabel
      title="Latency"
      sub="How aggressively to deliver translated audio. Doesn't change accuracy."
    />
    <Segmented
      value={store.latencyPref}
      options={[
        { value: "fastest", label: "Fastest" },
        { value: "balanced", label: "Balanced" },
        { value: "smooth", label: "Smoother audio" },
      ]}
      onChange={(v) => store.setLatencyPref(v)}
    />
  </Row>
  <Row last>
    <RowLabel
      title="Original voice volume"
      sub={store.mode === "mixed"
        ? "Keeps a quiet trace of your voice under the translation."
        : "Only available in Translate + Original."}
    />
    <div
      style={css({
        flex: 1,
        display: "flex",
        alignItems: "center",
        gap: 10,
        opacity: store.mode === "mixed" ? 1 : 0.45,
      })}
    >
      <span style={css({ fontSize: 11, color: "var(--txt-3)" })}>0%</span>
      <Slider
        value={store.mixPercent}
        min={0}
        max={30}
        onChange={(v) => store.setMixPercent(v)}
        disabled={store.mode !== "mixed"}
      />
      <span style={css({ fontSize: 11, color: "var(--txt-3)" })}>30%</span>
      <span
        class="mono"
        style={css({ fontSize: 12, color: "var(--txt-1)", width: 36, textAlign: "right" })}
      >{store.mixPercent}%</span>
    </div>
  </Row>
</FieldGroup>

<div
  style={css({
    fontSize: 11.5,
    color: "var(--txt-3)",
    marginTop: -10,
    marginBottom: 22,
    lineHeight: 1.5,
  })}
>
  Fastest: lower delay, more likely to sound choppy. Balanced is recommended. Smoother audio is more stable with slightly more delay.
</div>
