<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { MODES } from "$lib/constants";
  import { PaneTitle, FieldGroup, Row } from "$lib/controls";
  import { Glyph, SysIcon } from "$lib/icons";
  import { VUStrip, VUBars } from "$lib/vu";
  import { css } from "$lib/util";

  // ── Derived values ──────────────────────────────────────────
  const meta = $derived(MODES.find((m) => m.id === store.mode) ?? MODES[2]);
  const isTranslating = $derived(store.isTranslating);
  const hasError = $derived(!!store.lastError);

  // ── System check rows ───────────────────────────────────────
  const checks = $derived.by(() => {
    const micPermOk = store.errorKind !== "permission";
    const virtualMicOk = store.status?.virtualMicInstalled === true;
    const keyOk = store.account.verified;
    const audioInputOk = store.errorKind !== "mic" && !!store.status?.sourceMicName;

    const rows: Array<{
      ok: boolean;
      label: string;
      failLabel?: string;
      note?: string;
      cta?: string;
      onCta?: () => void;
      instruction?: boolean;
    }> = [
      {
        ok: micPermOk,
        label: "Microphone permission granted",
        failLabel: "Microphone permission missing",
        cta: "Open System Settings",
        onCta: () => store.openMicPermission(),
      },
      {
        ok: virtualMicOk,
        label: "Translator Mic installed",
        failLabel: "Translator Mic is not installed",
        cta: "Install Virtual Mic",
        onCta: () => store.installVirtualMic(),
      },
      !keyOk
        ? {
            ok: false,
            label: "OpenAI API key required",
            failLabel: "OpenAI API key required",
            cta: "Add key",
            onCta: () => store.setSettingsTab("account"),
          }
        : null,
      keyOk
        ? store.errorKind === "network"
          ? {
              ok: false,
              label: "Couldn't reach translation service",
              failLabel: "Couldn't reach translation service",
              cta: "Retry",
              onCta: () => store.dismissError(),
            }
          : store.status?.openaiConnected
          ? {
              ok: true,
              label: "Translation service connected",
            }
          : store.mode === "pass"
          ? {
              ok: true,
              label: "Translation service idle",
              note: "Pass-through does not use translation.",
            }
          : {
              ok: true,
              label: "Translation service idle",
            }
        : null,
      {
        ok: audioInputOk,
        label: `Audio input detected — ${store.status?.sourceMicName ?? "unknown"}`,
        failLabel: "No audio input detected",
        cta: "Choose another microphone",
        onCta: () => store.setSettingsTab("audio"),
      },
      {
        ok: true,
        instruction: true,
        label: "Select Translator Mic in your meeting app's microphone settings.",
      },
    ].filter(Boolean) as typeof rows;

    return rows;
  });
</script>

<PaneTitle
  title="Status"
  sub="What's happening right now, and what your meeting hears."
/>

<!-- Big mode card -->
<div
  class="card"
  style={css({
    padding: 18,
    background: `linear-gradient(135deg, color-mix(in oklch, ${meta.color} 12%, var(--card-bg)) 0%, var(--card-bg) 80%)`,
    borderColor: `color-mix(in oklch, ${meta.color} 28%, var(--card-border))`,
    marginBottom: 18,
  })}
>
  <div
    style={css({
      display: "flex",
      alignItems: "center",
      gap: 14,
      marginBottom: 14,
    })}
  >
    <!-- Mode icon -->
    <div
      style={css({
        width: 48,
        height: 48,
        borderRadius: 12,
        background: `linear-gradient(135deg, color-mix(in oklch, ${meta.color} 80%, white) 0%, ${meta.color} 100%)`,
        display: "grid",
        placeItems: "center",
        color: "#fff",
        boxShadow: `0 8px 20px -4px color-mix(in oklch, ${meta.color} 50%, transparent)`,
      })}
    >
      <Glyph size={26} color="#fff" />
    </div>

    <!-- Title + sub -->
    <div style={css({ flex: 1 })}>
      <div
        style={css({
          fontSize: 11,
          fontWeight: 600,
          letterSpacing: 0.5,
          textTransform: "uppercase",
          color: meta.color,
          marginBottom: 3,
        })}
      >
        {hasError ? "Error" : "Current Mode"}
      </div>
      <div
        style={css({
          fontSize: 20,
          fontWeight: 600,
          letterSpacing: -0.1,
        })}
      >
        {hasError
          ? "Translation paused"
          : isTranslating
            ? `Translating ${store.langPairText}`
            : meta.label}
      </div>
      <div
        style={css({
          fontSize: 12.5,
          color: "var(--txt-3)",
          marginTop: 2,
        })}
      >
        {hasError ? "Resolve the issue below to resume." : meta.body}
      </div>
    </div>

    <!-- Right badge -->
    <div
      style={css({
        padding: "6px 10px",
        borderRadius: 8,
        background: hasError
          ? "color-mix(in oklch, var(--c-error) 14%, transparent)"
          : `color-mix(in oklch, ${meta.color} 14%, transparent)`,
        display: "flex",
        alignItems: "center",
        gap: 8,
        fontSize: 11.5,
        fontWeight: 500,
        color: hasError ? "var(--c-error)" : meta.color,
      })}
    >
      {#if hasError}
        <span style={css({ color: "var(--c-error)" })}>Paused</span>
      {:else if isTranslating}
        <span class="mono">{store.latencyText}</span>
        <span style={css({ color: "var(--txt-3)", fontWeight: 400 })}>translation latency</span>
      {:else if store.mode === "pass"}
        <span>Live</span>
      {:else}
        <span>No audio</span>
      {/if}
    </div>
  </div>

  <!-- Level meters grid -->
  <div
    style={css({
      display: "grid",
      gridTemplateColumns: "1fr 1fr",
      gap: 14,
    })}
  >
    <!-- Input meter -->
    <div
      style={css({
        padding: 12,
        borderRadius: 9,
        background: "color-mix(in oklch, var(--txt-1) 4%, transparent)",
      })}
    >
      <div
        style={css({
          display: "flex",
          alignItems: "baseline",
          marginBottom: 8,
        })}
      >
        <span
          style={css({
            fontSize: 11,
            fontWeight: 600,
            color: "var(--txt-3)",
            letterSpacing: 0.5,
            textTransform: "uppercase",
          })}
        >Input</span>
        <span style={css({ marginLeft: "auto", fontSize: 11, color: "var(--txt-3)" })}>
          {store.status?.sourceMicName ?? "No input device"}
        </span>
      </div>
      <VUStrip
        level={!hasError && store.mode !== "silence" ? store.inputLevel : 0}
        color="var(--c-mixed)"
        seed={1}
        height={6}
      />
      <div style={css({ marginTop: 8 })}>
        <VUBars
          level={!hasError && store.mode !== "silence" ? store.inputLevel : 0}
          bars={28}
          color="var(--c-mixed)"
          height={18}
          barWidth={3}
          seed={6}
        />
      </div>
    </div>

    <!-- Output meter -->
    <div
      style={css({
        padding: 12,
        borderRadius: 9,
        background: "color-mix(in oklch, var(--txt-1) 4%, transparent)",
      })}
    >
      <div
        style={css({
          display: "flex",
          alignItems: "baseline",
          marginBottom: 8,
        })}
      >
        <span
          style={css({
            fontSize: 11,
            fontWeight: 600,
            color: "var(--txt-3)",
            letterSpacing: 0.5,
            textTransform: "uppercase",
          })}
        >Output</span>
        <span style={css({ marginLeft: "auto", fontSize: 11, color: "var(--txt-3)" })}>
          {store.mode === "pass"
            ? "Original voice"
            : store.mode === "silence"
              ? "Silenced"
              : "Translated voice"}
        </span>
      </div>
      <VUStrip
        level={!hasError && store.mode !== "silence" ? store.outputLevel : 0}
        color={meta.color}
        seed={3}
        height={6}
      />
      <div style={css({ marginTop: 8 })}>
        <VUBars
          level={!hasError && store.mode !== "silence" ? store.outputLevel : 0}
          bars={28}
          color={meta.color}
          height={18}
          barWidth={3}
          seed={8}
        />
      </div>
    </div>
  </div>
</div>

<!-- System Check -->
<FieldGroup title="System Check">
  {#each checks as c, i (i)}
    <Row last={i === checks.length - 1}>
      <!-- Icon -->
      <span style={css({ display: "flex", flexShrink: 0 })}>
        {#if c.instruction}
          <svg
            width="15"
            height="15"
            viewBox="0 0 16 16"
            fill="none"
            stroke="var(--txt-3)"
            stroke-width="1.4"
          >
            <circle cx="8" cy="8" r="6.5" />
            <path d="M8 5v3.5M8 11v.5" stroke-linecap="round" />
          </svg>
        {:else if c.ok}
          <SysIcon name="ok" size={15} />
        {:else}
          <SysIcon name="warn" size={15} />
        {/if}
      </span>

      <!-- Label -->
      <div
        style={css({
          flex: 1,
          display: "flex",
          flexDirection: "column",
          gap: 1,
        })}
      >
        <span
          style={css({
            fontSize: 13,
            color: c.instruction ? "var(--txt-2)" : "var(--txt-1)",
          })}
        >
          {c.ok ? c.label : (c.failLabel ?? c.label)}
        </span>
        {#if c.note}
          <span style={css({ fontSize: 11.5, color: "var(--txt-3)" })}>{c.note}</span>
        {/if}
      </div>

      <!-- CTA button (only for failed rows) -->
      {#if !c.ok && c.cta}
        <button class="btn" onclick={c.onCta} style={css({ fontSize: 12 })}>
          {c.cta}
        </button>
      {/if}

      <!-- Instruction chip -->
      {#if c.instruction}
        <span
          class="mono"
          style={css({
            fontSize: 11.5,
            color: "var(--txt-3)",
            padding: "2px 7px",
            borderRadius: 4,
            background: "rgba(120,120,128,0.10)",
          })}
        >
          Translator Mic
        </span>
      {/if}
    </Row>
  {/each}
</FieldGroup>
