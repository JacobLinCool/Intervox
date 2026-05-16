<script lang="ts">
  import { PaneTitle, FieldGroup, Row, RowLabel } from "$lib/controls";
  import { css } from "$lib/util";

  // Display-only: no rebinding backend; keys are static reference defaults
  const items = [
    { label: "Toggle Output Mode",   sub: "Cycle Silence → Pass-through → Translate → Translate + Original", keys: ["⌘", "⇧", "T"] },
    { label: "Mute / Silence",       sub: "Instantly cut audio to your meeting",                             keys: ["⌘", "⇧", "M"] },
    { label: "Push to Talk",         sub: "Hold to send audio; release to silence",                          keys: ["Hold", "⌘", "Space"] },
    { label: "Show / Hide Captions", sub: "Toggle the floating captions window",                             keys: ["⌘", "⇧", "C"] },
    { label: "Open Settings",        sub: "This window",                                                     keys: ["⌘", ","] },
  ];
</script>

<PaneTitle
  title="Shortcuts"
  sub="Global hotkeys work even when Intervox isn't focused — so you can control it from your meeting app."
/>

<FieldGroup title="Global Shortcuts">
  {#each items as it, i (i)}
    <Row last={i === items.length - 1}>
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
              color: "var(--txt-1)",
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
