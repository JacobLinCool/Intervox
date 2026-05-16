<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { PaneTitle, FieldGroup, Row, RowLabel, Toggle } from "$lib/controls";
  import { css } from "$lib/util";

  // UI-only: not persisted yet
  let showLatencyBadge = $state(true);
  // UI-only: not persisted yet
  let launchAtLogin = $state(true);
  // UI-only: not persisted yet
  let hideDockIcon = $state(true);
</script>

<PaneTitle
  title="Advanced"
  sub="Diagnostics, system access, and developer options. Most people don't need to touch these."
/>

<FieldGroup title="Diagnostics">
  <Row>
    <RowLabel title="Show latency badge in menu bar" />
    <span style={css({ marginLeft: "auto" })}>
      <!-- UI-only: not persisted yet -->
      <Toggle
        value={showLatencyBadge}
        onChange={(v) => { showLatencyBadge = v; }}
        tint="var(--c-mixed)"
      />
    </span>
  </Row>
  <Row>
    <RowLabel title="Connection log" />
    <span style={css({ marginLeft: "auto" })}>
      <!-- UI-only: not persisted yet -->
      <button class="btn" onclick={() => {}}>View log</button>
    </span>
  </Row>
  <Row last>
    <RowLabel
      title="Clear transcript history"
      sub="Clears the current session transcript from the display. No transcript is saved to disk."
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
      <!-- UI-only: not persisted yet -->
      <Toggle
        value={launchAtLogin}
        onChange={(v) => { launchAtLogin = v; }}
        tint="var(--c-mixed)"
      />
    </span>
  </Row>
  <Row>
    <RowLabel title="Hide Dock icon" sub="Run as a menu bar app only." />
    <span style={css({ marginLeft: "auto" })}>
      <!-- UI-only: not persisted yet -->
      <Toggle
        value={hideDockIcon}
        onChange={(v) => { hideDockIcon = v; }}
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
  Intervox v1.0.0 (build 482)  ·  © 2026
</div>
