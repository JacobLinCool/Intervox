<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { PaneTitle, FieldGroup, Row, RowLabel, Toggle, Segmented } from "$lib/controls";
  import { css } from "$lib/util";

  // UI-only: caption position not persisted yet (honest)
  let pos = $state<"top" | "bottom" | "custom">("bottom");

  // UI-only: auto-hide not persisted yet (honest)
  let autoHide = $state("never");

  const captionsEnabled = $derived(store.config?.captions.enabled ?? false);
</script>

<PaneTitle
  title="Captions"
  sub="A small overlay window that floats above your meeting, showing both transcripts."
/>

<FieldGroup title="Visibility">
  <Row>
    <RowLabel title="Floating captions" sub="Show the always-on-top transcript window." />
    <span style={css({ marginLeft: "auto" })}>
      <Toggle
        value={store.config?.captions.enabled ?? store.captionsOpen}
        onChange={(v) => store.setCaptions({ enabled: v })}
        tint="var(--c-mixed)"
      />
    </span>
  </Row>
  <Row>
    <RowLabel
      title="Pop-out captions window"
      sub={captionsEnabled
        ? "Open a dedicated always-on-top captions window."
        : "Enable floating captions above to use the pop-out window."}
    />
    <span style={css({ marginLeft: "auto", opacity: captionsEnabled ? 1 : 0.35, pointerEvents: captionsEnabled ? "auto" : "none" })}>
      <Toggle
        value={store.captionsWindowOpen}
        onChange={() => store.toggleCaptionsWindow()}
        tint="var(--c-translate)"
      />
    </span>
  </Row>
  <Row>
    <RowLabel title="Show original captions" sub="Display your original speech as captions when available." />
    <span style={css({ marginLeft: "auto" })}>
      <Toggle
        value={store.config?.captions.show_source ?? false}
        onChange={(v) => store.setCaptions({ show_source: v })}
        tint="var(--c-mixed)"
      />
    </span>
  </Row>
  <Row last>
    <RowLabel title="Show translated captions" sub="What the meeting hears." />
    <span style={css({ marginLeft: "auto" })}>
      <Toggle
        value={store.config?.captions.show_target ?? false}
        onChange={(v) => store.setCaptions({ show_target: v })}
        tint="var(--c-translate)"
      />
    </span>
  </Row>
</FieldGroup>

<FieldGroup title="Appearance">
  <Row>
    <RowLabel title="Position" sub="Where the caption window appears on screen." />
    <span style={css({ marginLeft: "auto" })}>
      <!-- UI-only: caption position not persisted yet (honest) -->
      <Segmented
        value={pos}
        options={[
          { value: "top", label: "Top" },
          { value: "bottom", label: "Bottom" },
          { value: "custom", label: "Custom" },
        ]}
        onChange={(v) => { pos = v; }}
      />
    </span>
  </Row>
  <Row last>
    <RowLabel title="Font size" sub="Larger is easier to read across the room." />
    <span style={css({ marginLeft: "auto" })}>
      <Segmented
        value={store.config?.captions.font_size ?? "medium"}
        options={[
          { value: "small", label: "Small" },
          { value: "medium", label: "Medium" },
          { value: "large", label: "Large" },
        ]}
        onChange={(v) => store.setCaptions({ font_size: v })}
      />
    </span>
  </Row>
</FieldGroup>

<FieldGroup title="Behavior">
  <Row last>
    <RowLabel title="Auto-hide after silence" sub="Tuck the captions away when nobody's speaking." />
    <span style={css({ marginLeft: "auto" })}>
      <!-- UI-only: auto-hide not persisted yet (honest) -->
      <Segmented
        value={autoHide}
        options={[
          { value: "never", label: "Never" },
          { value: "5s", label: "5s" },
          { value: "10s", label: "10s" },
          { value: "30s", label: "30s" },
        ]}
        onChange={(v) => { autoHide = v; }}
      />
    </span>
  </Row>
</FieldGroup>
