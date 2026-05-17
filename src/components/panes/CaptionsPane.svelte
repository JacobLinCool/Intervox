<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { PaneTitle, FieldGroup, Row, RowLabel, Toggle, Segmented } from "$lib/controls";
  import { css } from "$lib/util";
</script>

<PaneTitle
  title="Captions"
  sub="A compact always-on-top transcript window for meetings."
/>

<FieldGroup title="Visibility">
  <Row>
    <RowLabel title="Captions window" sub="Show the compact always-on-top transcript window." />
    <span style={css({ marginLeft: "auto" })}>
      <Toggle
        ariaLabel="Toggle captions window"
        value={store.config?.captions.enabled ?? false}
        onChange={(v) => store.setCaptions({ enabled: v })}
        tint="var(--c-accent)"
      />
    </span>
  </Row>
  <Row>
    <RowLabel title="Show original captions" sub="Display your original speech as captions when available." />
    <span style={css({ marginLeft: "auto" })}>
      <Toggle
        ariaLabel="Toggle original captions"
        value={store.config?.captions.show_source ?? false}
        onChange={(v) => store.setCaptions({ show_source: v })}
        tint="var(--c-accent)"
      />
    </span>
  </Row>
  <Row last>
    <RowLabel title="Show translated captions" sub="What the meeting hears." />
    <span style={css({ marginLeft: "auto" })}>
      <Toggle
        ariaLabel="Toggle translated captions"
        value={store.config?.captions.show_target ?? false}
        onChange={(v) => store.setCaptions({ show_target: v })}
        tint="var(--c-translate)"
      />
    </span>
  </Row>
</FieldGroup>

<FieldGroup title="Appearance">
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
