<script lang="ts">
  import { PaneTitle, FieldGroup, Row, RowLabel } from "$lib/controls";
  import { css } from "$lib/util";
  import { store } from "$lib/store.svelte";

  // ── Shortcut state ─────────────────────────────────────────────────────────
  //
  // Each of the three configurable shortcuts can be: "idle" | "active" | "failed".
  // "active"  → the last save call succeeded (optimistic).
  // "failed"  → the last save call returned an error.
  // "idle"    → no save attempt yet in this session.

  type SaveState = "idle" | "active" | "failed";

  // Local editable copies — initialised to placeholder; synced via $effect once config loads.
  let toggleTranslateEdit = $state("Cmd+Shift+T");
  let silenceEdit         = $state("Cmd+Shift+M");
  let captionsEdit        = $state("Cmd+Shift+C");

  // Per-shortcut save state.
  let toggleState: SaveState = $state("idle");
  let silenceState: SaveState = $state("idle");
  let captionsState: SaveState = $state("idle");

  // Keep edits in sync when config loads or changes externally.
  $effect(() => {
    if (store.config) {
      toggleTranslateEdit = store.config.shortcuts.toggle_translate;
      silenceEdit         = store.config.shortcuts.silence;
      captionsEdit        = store.config.shortcuts.captions;
    }
  });

  // ── Helpers ────────────────────────────────────────────────────────────────

  async function applyShortcuts(
    field: "toggle_translate" | "silence" | "captions",
    value: string,
    setStatus: (s: SaveState) => void,
  ) {
    if (!store.config) return;
    const prevError = store.lastError;
    const next = { ...store.config.shortcuts, [field]: value };
    await store.setShortcuts(next);
    // Detect failure: a new error appeared after the call.
    if (store.lastError !== prevError && store.lastError !== null) {
      setStatus("failed");
    } else {
      setStatus("active");
    }
  }

  function stateColor(s: SaveState): string {
    if (s === "active")  return "var(--accent, #5856d6)";
    if (s === "failed")  return "var(--color-error, #ff3b30)";
    return "var(--txt-2)";
  }

  function stateLabel(s: SaveState): string {
    if (s === "active") return "✓ Active";
    if (s === "failed") return "✗ Failed";
    return "";
  }

  // Static-only shortcuts (non-configurable)
  const staticItems = [
    { label: "Push to Talk",   sub: "Hold to send audio; release to silence",  keys: ["Hold", "⌘", "Space"] },
    { label: "Open Settings",  sub: "This window",                              keys: ["⌘", ","] },
  ];
</script>

<PaneTitle
  title="Shortcuts"
  sub="Global hotkeys work even when Intervox isn't focused — so you can control it from your meeting app."
/>

<FieldGroup title="Global Shortcuts">
  <!-- toggle_translate -->
  <Row>
    <RowLabel
      title="Toggle Translate"
      sub="If translating → Silence; otherwise → Translate"
      width={220}
    />
    <span style={css({ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" })}>
      {#if toggleState !== "idle"}
        <span style={css({ fontSize: 11, color: stateColor(toggleState) })}>
          {stateLabel(toggleState)}
        </span>
      {/if}
      <input
        type="text"
        value={toggleTranslateEdit}
        oninput={(e) => { toggleTranslateEdit = (e.target as HTMLInputElement).value; }}
        onblur={() => applyShortcuts("toggle_translate", toggleTranslateEdit, (s) => { toggleState = s; })}
        onkeydown={(e) => { if (e.key === "Enter") applyShortcuts("toggle_translate", toggleTranslateEdit, (s) => { toggleState = s; }); }}
        style={css({
          background: "var(--control-bg)",
          border: `0.5px solid ${toggleState === "failed" ? "var(--color-error, #ff3b30)" : "var(--control-border)"}`,
          borderRadius: 5,
          padding: "2px 7px",
          fontSize: 12,
          color: "var(--txt-1)",
          width: 130,
          fontFamily: "ui-monospace, SF Mono, Menlo, monospace",
          outline: "none",
        })}
        placeholder="Cmd+Shift+T"
        aria-label="Toggle translate shortcut"
      />
    </span>
  </Row>

  <!-- silence -->
  <Row>
    <RowLabel
      title="Silence"
      sub="Instantly cut audio to your meeting"
      width={220}
    />
    <span style={css({ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" })}>
      {#if silenceState !== "idle"}
        <span style={css({ fontSize: 11, color: stateColor(silenceState) })}>
          {stateLabel(silenceState)}
        </span>
      {/if}
      <input
        type="text"
        value={silenceEdit}
        oninput={(e) => { silenceEdit = (e.target as HTMLInputElement).value; }}
        onblur={() => applyShortcuts("silence", silenceEdit, (s) => { silenceState = s; })}
        onkeydown={(e) => { if (e.key === "Enter") applyShortcuts("silence", silenceEdit, (s) => { silenceState = s; }); }}
        style={css({
          background: "var(--control-bg)",
          border: `0.5px solid ${silenceState === "failed" ? "var(--color-error, #ff3b30)" : "var(--control-border)"}`,
          borderRadius: 5,
          padding: "2px 7px",
          fontSize: 12,
          color: "var(--txt-1)",
          width: 130,
          fontFamily: "ui-monospace, SF Mono, Menlo, monospace",
          outline: "none",
        })}
        placeholder="Cmd+Shift+M"
        aria-label="Silence shortcut"
      />
    </span>
  </Row>

  <!-- captions -->
  <Row last>
    <RowLabel
      title="Show / Hide Captions"
      sub="Toggle the captions window"
      width={220}
    />
    <span style={css({ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" })}>
      {#if captionsState !== "idle"}
        <span style={css({ fontSize: 11, color: stateColor(captionsState) })}>
          {stateLabel(captionsState)}
        </span>
      {/if}
      <input
        type="text"
        value={captionsEdit}
        oninput={(e) => { captionsEdit = (e.target as HTMLInputElement).value; }}
        onblur={() => applyShortcuts("captions", captionsEdit, (s) => { captionsState = s; })}
        onkeydown={(e) => { if (e.key === "Enter") applyShortcuts("captions", captionsEdit, (s) => { captionsState = s; }); }}
        style={css({
          background: "var(--control-bg)",
          border: `0.5px solid ${captionsState === "failed" ? "var(--color-error, #ff3b30)" : "var(--control-border)"}`,
          borderRadius: 5,
          padding: "2px 7px",
          fontSize: 12,
          color: "var(--txt-1)",
          width: 130,
          fontFamily: "ui-monospace, SF Mono, Menlo, monospace",
          outline: "none",
        })}
        placeholder="Cmd+Shift+C"
        aria-label="Captions shortcut"
      />
    </span>
  </Row>
</FieldGroup>

<FieldGroup title="Fixed Shortcuts">
  {#each staticItems as it, i (i)}
    <Row last={i === staticItems.length - 1}>
      <RowLabel title={it.label} sub={it.sub} width={260} />
      <span style={css({ marginLeft: "auto", display: "flex", gap: 4 })}>
        {#each it.keys as k, ki (ki)}
          <kbd
            style={css({
              background: "var(--control-bg)",
              border: "0.5px solid var(--control-border)",
              borderBottom: "1.5px solid var(--control-border)",
              borderRadius: 5,
              padding: k.length > 1 ? "2px 7px" : "2px 6px",
              fontSize: 12,
              fontFamily: k.length > 1 ? "inherit" : "ui-monospace, SF Mono, Menlo, monospace",
              color: "var(--txt-2)",
              minWidth: 22,
              textAlign: "center",
            })}
          >{k}</kbd>
        {/each}
      </span>
    </Row>
  {/each}
</FieldGroup>

<FieldGroup
  title="When Push to Talk is held"
  hint="Quick way to stay safe — your meeting only hears you when you're holding the keys."
>
  <Row last>
    <RowLabel title="What's sent" sub="Audio uses whatever output mode you've already chosen." />
    <span style={css({ marginLeft: "auto", fontSize: 12, color: "var(--txt-2)" })}>
      Current mode
    </span>
  </Row>
</FieldGroup>
