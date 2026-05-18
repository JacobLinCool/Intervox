<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { PaneTitle, FieldGroup, Row, RowLabel, Toggle, Pulldown } from "$lib/controls";
  import { css } from "$lib/util";
  import ConnectionLogModal from "../ConnectionLogModal.svelte";

  let showLog = $state(false);

  const INACTIVITY_OPTIONS = [
    { value: 0, label: "Off" },
    { value: 5, label: "5 minutes" },
    { value: 10, label: "10 minutes" },
    { value: 15, label: "15 minutes" },
    { value: 30, label: "30 minutes" },
    { value: 60, label: "60 minutes" },
  ];

  const notifyStatusText = $derived(
    store.notificationPermission === "denied"
      ? "Denied — enable Intervox in System Settings ▸ Notifications to receive these reminders."
      : store.notificationPermission === "unsupported"
        ? "Unavailable on this system. Interpret still works normally."
        : store.notificationPermission === "granted"
          ? "Allowed — reminders show silently, with no alert sound."
          : "Waiting for permission…",
  );

  const captureDrops = $derived(
    store.backpressure.capturePoolMisses
      + store.backpressure.captureCapacityDrops
      + store.backpressure.captureSinkDrops,
  );
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
        tint="var(--c-accent)"
      />
    </span>
  </Row>
  <Row>
    <RowLabel title="Connection log" />
    <span style={css({ marginLeft: "auto" })}>
      <button class="btn" onclick={() => (showLog = true)}>View log</button>
    </span>
  </Row>
  <Row>
    <RowLabel
      title="Audio backpressure"
      sub={`Capture ${captureDrops} · queue ${store.backpressure.uplinkQueueDrops} · no session ${store.backpressure.uplinkNoSessionDrops}`}
    />
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

<FieldGroup title="Notifications">
  <Row>
    <RowLabel
      title="Long-session reminders"
      sub="Silent desktop notifications at 1, 2, and 3 hours of continuous Interpret."
    />
  </Row>
  <Row>
    <RowLabel
      title="Inactivity reminder"
      sub="Notify when Interpret is on but no speech has been interpreted for this long."
    />
    <span style={css({ marginLeft: "auto" })}>
      <Pulldown
        width={150}
        value={store.config?.ui.inactivity_reminder_minutes ?? 10}
        options={INACTIVITY_OPTIONS}
        onChange={(v) => store.setUiConfig({ inactivity_reminder_minutes: v })}
      />
    </span>
  </Row>
  <Row last>
    <RowLabel title="Permission" sub={notifyStatusText} />
    <span style={css({ marginLeft: "auto" })}>
      <span
        style={css({
          fontSize: 12,
          fontWeight: 600,
          color: store.notificationsBlocked ? "var(--c-error)" : "var(--txt-2)",
        })}
      >
        {store.notificationsBlocked ? "Blocked" : "OK"}
      </span>
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
        tint="var(--c-accent)"
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
        tint="var(--c-accent)"
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
