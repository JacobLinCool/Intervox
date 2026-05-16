<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { MODES } from "$lib/constants";
  import { PaneTitle, FieldGroup, Row, RowLabel, Pulldown, Toggle, Segmented } from "$lib/controls";
  import { SysIcon } from "$lib/icons";
  import { VUStrip } from "$lib/vu";
  import { css } from "$lib/util";
  import ModeCard from "./ModeCard.svelte";

  // UI-only: no backend field yet; not persisted (honest — does not fake persistence)
  let micEnv = $state<"near" | "far">("near");

  let sourceMicValue = $derived(
    store.config?.audio.source_mic_id ?? store.devices.inputs[0]?.id ?? "",
  );

  let noDevices = $derived(store.devices.inputs.length === 0);
</script>

<PaneTitle
  title="Audio"
  sub="What Intervox listens to, and what it sends to your meeting."
/>

<FieldGroup title="Output Mode">
  <div style={css({ padding: 14, display: "grid", gridTemplateColumns: "repeat(2,1fr)", gap: 10 })}>
    {#each MODES as m (m.id)}
      <ModeCard
        meta={m}
        selected={store.mode === m.id}
        onclick={() => store.setMode(m.id)}
      />
    {/each}
  </div>
</FieldGroup>

{#snippet micLeft(_o: unknown)}
  <span style={css({ color: "var(--txt-2)" })}>
    <SysIcon name="mic" size={13} />
  </span>
{/snippet}

<FieldGroup title="Source Microphone">
  <Row last>
    <RowLabel title="Listen to" sub="Intervox will translate audio from this mic." />
    {#if noDevices}
      <Pulldown
        value=""
        onChange={() => {}}
        options={[{ value: "", label: "No input devices" }]}
        optionLeft={micLeft}
        width={300}
      />
    {:else}
      <Pulldown
        value={sourceMicValue}
        onChange={(v) => store.setSourceMic(v)}
        options={store.devices.inputs.map((d) => ({ value: d.id, label: d.name }))}
        optionLeft={micLeft}
        width={300}
      />
    {/if}
    <div style={css({ marginLeft: "auto", width: 120 })}>
      <VUStrip level={store.inputLevel} color="var(--c-mixed)" />
    </div>
  </Row>
</FieldGroup>

<FieldGroup
  title="Virtual Microphone"
  hint="Select Translator Mic as your microphone in Zoom, Google Meet, Teams, or Discord."
>
  <Row last>
    <div
      style={css({
        width: 36,
        height: 36,
        borderRadius: 9,
        background: "color-mix(in oklch, var(--c-translate) 18%, transparent)",
        color: "var(--c-translate)",
        display: "grid",
        placeItems: "center",
        flexShrink: 0,
      })}
    >
      <SysIcon name="mic" size={20} />
    </div>
    <div style={css({ flex: 1 })}>
      <div style={css({ fontSize: 13, fontWeight: 500 })}>Translator Mic</div>
      <div style={css({ fontSize: 11.5, color: "var(--txt-3)", marginTop: 1 })}>
        {#if store.status?.virtualMicInstalled}
          Virtual audio device · installed
        {:else}
          Audio driver · not installed
        {/if}
      </div>
    </div>
    <span style={css({ display: "flex", alignItems: "center", gap: 6, fontSize: 12, color: "var(--txt-2)", fontWeight: 500 })}>
      {#if store.status?.virtualMicInstalled}
        <SysIcon name="ok" size={13} /> Installed
      {:else}
        <SysIcon name="warn" size={13} /> Not installed
      {/if}
    </span>
  </Row>
</FieldGroup>

<FieldGroup title="Input">
  <Row>
    <RowLabel title="Mic environment" sub="Helps Intervox choose the right input cleanup for your mic." />
    <span style={css({ marginLeft: "auto" })}>
      <Segmented
        value={micEnv}
        options={[
          { value: "near", label: "Headset / close mic" },
          { value: "far", label: "Laptop or room mic" },
        ]}
        onChange={(v) => { micEnv = v; }}
      />
    </span>
  </Row>
  <Row last>
    <RowLabel title="Feedback protection" sub="Helps reduce repeated audio when using speakers." />
    <span style={css({ marginLeft: "auto" })}>
      <Toggle
        value={store.feedbackProtection}
        onChange={(v) => { store.setFeedbackProtection(v); }}
        tint="var(--c-mixed)"
      />
    </span>
  </Row>
</FieldGroup>
