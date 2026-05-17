<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { PaneTitle, FieldGroup, Row, RowLabel, Toggle } from "$lib/controls";
  import { css } from "$lib/util";
  import ConnectionLogModal from "../ConnectionLogModal.svelte";

  let showLog = $state(false);
</script>

<PaneTitle
  title="Advanced"
  sub="Diagnostics, system access, and developer options. Most people don't need to touch these."
/>

<FieldGroup title="Diagnostics">
  <Row>
    <RowLabel title="Show latency badge in menu bar" />
    <span style={css({ marginLeft: "auto" })}>
      <Toggle
        ariaLabel="Toggle latency badge"
        value={store.config?.ui.show_latency_badge ?? false}
        onChange={(v) => store.setUiConfig({ show_latency_badge: v })}
        tint="var(--c-mixed)"
      />
    </span>
  </Row>
  <Row>
    <RowLabel title="Connection log" />
    <span style={css({ marginLeft: "auto" })}>
      <button class="btn" onclick={() => (showLog = true)}>View log</button>
    </span>
  </Row>
  <Row last>
    <RowLabel
      title="Clear transcript history"
      sub="Deletes saved transcript files on this Mac and clears the current session."
    />
    <span style={css({ marginLeft: "auto" })}>
      <button class="btn" onclick={() => store.clearHistory()}>Clear history</button>
    </span>
  </Row>
</FieldGroup>

<FieldGroup title="System">
  <Row>
    <RowLabel title="Launch at login" />
    <span style={css({ marginLeft: "auto" })}>
      <Toggle
        ariaLabel="Toggle launch at login"
        value={store.config?.ui.launch_at_login ?? false}
        onChange={(v) => store.setUiConfig({ launch_at_login: v })}
        tint="var(--c-mixed)"
      />
    </span>
  </Row>
  <Row>
    <RowLabel title="Hide Dock icon" sub="Run as a menu bar app only." />
    <span style={css({ marginLeft: "auto" })}>
      <Toggle
        ariaLabel="Toggle Dock icon visibility"
        value={store.config?.ui.hide_dock_icon ?? false}
        onChange={(v) => store.setUiConfig({ hide_dock_icon: v })}
        tint="var(--c-mixed)"
      />
    </span>
  </Row>
  <Row last>
    <RowLabel title="Re-run first-time setup" />
    <span style={css({ marginLeft: "auto" })}>
      <button class="btn" onclick={() => store.setOnboardingOpen(true)}>Open setup…</button>
    </span>
  </Row>
</FieldGroup>

<div style={css({ fontSize: 11.5, color: "var(--txt-3)", textAlign: "center", marginTop: 24 })}>
  Intervox {store.appVersion ? `v${store.appVersion} ` : ""}·  © 2026
</div>

{#if showLog}<ConnectionLogModal onClose={() => (showLog = false)} />{/if}
