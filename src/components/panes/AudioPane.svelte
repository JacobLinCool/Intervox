<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { MODES } from "$lib/constants";
  import { PaneTitle, FieldGroup, Row, RowLabel, Pulldown } from "$lib/controls";
  import { SysIcon } from "$lib/icons";
  import { VUStrip, formatDbfs } from "$lib/vu";
  import { css } from "$lib/util";
  import ModeCard from "./ModeCard.svelte";

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
  <div style={css({ padding: 14, display: "grid", gridTemplateColumns: "repeat(3,minmax(0,1fr))", gap: 10 })}>
    {#each MODES as m (m.id)}
      <ModeCard
        meta={m}
        selected={store.mode === m.id}
        onSelect={() => store.setMode(m.id)}
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
      <VUStrip level={store.inputLevel} color="var(--c-accent)" />
      <div style={css({ marginTop: 4, textAlign: "right", fontSize: 10.5, color: "var(--txt-3)" })}>
        {formatDbfs(store.inputLevel)}
      </div>
    </div>
  </Row>
</FieldGroup>

{#if store.micPermission !== "granted"}
<FieldGroup title="Microphone Permission">
  <Row last>
    <RowLabel
      title={store.micPermission === "denied" ? "Access denied" : store.micPermission === "restricted" ? "Access restricted" : "Permission not yet granted"}
      sub={store.micPermission === "denied" ? "Intervox needs microphone access to translate." : store.micPermission === "restricted" ? "Microphone access is restricted by a system policy." : "Intervox will request microphone access when you start translating."}
    />
    <span style={css({ display: "flex", alignItems: "center", gap: 6, marginLeft: "auto" })}>
      {#if store.micPermission !== "restricted"}
        <button
          style={css({ fontSize: 12, fontWeight: 500, padding: "4px 10px", borderRadius: 6, border: "1px solid var(--brd-1)", background: "var(--bg-2)", color: "var(--txt-1)", cursor: "pointer" })}
          onclick={() => store.openMicPermission()}
        >Open Privacy Settings</button>
      {/if}
    </span>
  </Row>
</FieldGroup>
{/if}

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
