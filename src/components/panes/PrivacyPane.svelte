<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { PaneTitle, FieldGroup, Row, RowLabel, Toggle } from "$lib/controls";
  import { SidebarIcon } from "$lib/icons";
  import { css } from "$lib/util";
</script>

<PaneTitle
  title="Privacy"
  sub="What leaves your Mac, and what stays on it."
/>

<div
  class="card"
  style={css({
    padding: 16,
    marginBottom: 18,
    background: "color-mix(in oklch, var(--c-translate) 8%, var(--card-bg))",
    borderColor: "color-mix(in oklch, var(--c-translate) 25%, var(--card-border))",
  })}
>
  <div style={css({ display: "flex", gap: 12, alignItems: "flex-start" })}>
    <div style={css({ color: "var(--c-translate)", marginTop: 1 })}>
      <SidebarIcon name="privacy" />
    </div>
    <div style={css({ fontSize: 12.5, lineHeight: 1.55, color: "var(--txt-2)" })}>
      <div style={css({ fontSize: 13, fontWeight: 500, color: "var(--txt-1)", marginBottom: 4 })}>
        How translation works
      </div>
      When translation is active, your microphone audio is streamed to OpenAI to
      generate translated speech and captions. <strong style={css({ color: "var(--txt-1)" })}>
      Intervox does not record or save your audio</strong> by default. Transcripts are
      not saved unless you turn on transcript history below.
    </div>
  </div>
</div>

<FieldGroup title="Data">
  <Row>
    <RowLabel
      title="Save transcript history"
      sub="Keep a searchable transcript history on this Mac only."
    />
    <span style={css({ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 })}>
      {#if store.config?.privacy.save_transcript_history}
        <!-- UI-only: no clear-history backend yet; button must not pretend to clear (honest) -->
        <button class="btn" onclick={() => {}}>Clear history</button>
      {/if}
      <Toggle
        value={store.config?.privacy.save_transcript_history ?? false}
        onChange={(v) => store.setPrivacy({ save_transcript_history: v })}
        tint="var(--c-mixed)"
      />
    </span>
  </Row>
  <Row last>
    <RowLabel
      title="Send diagnostics"
      sub="Anonymous crash reports and performance data. No audio or transcripts are sent."
    />
    <span style={css({ marginLeft: "auto" })}>
      <Toggle
        value={store.config?.privacy.send_diagnostics ?? false}
        onChange={(v) => store.setPrivacy({ send_diagnostics: v })}
        tint="var(--c-mixed)"
      />
    </span>
  </Row>
</FieldGroup>
