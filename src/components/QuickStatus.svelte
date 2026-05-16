<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { MODES } from "$lib/constants";
  import { Glyph, Check, Dot, SysIcon, LangChip, Chevron } from "$lib/icons";
  import { VUDots } from "$lib/vu";
  import { css } from "$lib/util";

  // Derived from store
  const meta = $derived(MODES.find((m) => m.id === store.mode) ?? MODES[2]);
  const isTranslating = $derived(store.mode === "translate" || store.mode === "mixed");
  const isError = $derived(!!store.lastError);
  const vuLevel = $derived((!isError && (isTranslating || store.mode === "pass")) ? store.outputLevel : 0);
  const vuColor = $derived(store.mode === "pass" ? "var(--c-pass)" : meta.color);

  // Per-row hover states for ModeRows
  let modeHover = $state<Record<string, boolean>>({});
  // Per-row hover states for DropdownRows
  let rowHover = $state<Record<string, boolean>>({});

  // Robust Escape-to-close: attach a window keydown listener while the panel is open
  $effect(() => {
    if (!store.quickOpen) return;
    function handleKeydown(e: KeyboardEvent) {
      if (e.key === "Escape") store.setQuickOpen(false);
    }
    window.addEventListener("keydown", handleKeydown);
    return () => window.removeEventListener("keydown", handleKeydown);
  });
</script>

{#if store.quickOpen}
  <!-- Backdrop catcher -->
  <div
    role="presentation"
    aria-hidden="true"
    onclick={() => store.setQuickOpen(false)}
    style={css({ position: "fixed", inset: 0, zIndex: 60 })}
  ></div>

  <!-- Panel -->
  <div
    role="menu"
    style={css({
      position: "fixed",
      top: 44,
      right: 16,
      width: 320,
      background: "var(--win-bg)",
      backdropFilter: "saturate(180%) blur(40px)",
      WebkitBackdropFilter: "saturate(180%) blur(40px)",
      border: "0.5px solid var(--win-border)",
      borderRadius: 12,
      boxShadow: "0 20px 50px rgba(0,0,0,0.25), 0 0 0 0.5px rgba(0,0,0,0.06)",
      padding: 6,
      zIndex: 70,
      animation: "pop-in 140ms cubic-bezier(.2,.9,.3,1.2) both",
      color: "var(--txt-1)",
      fontSize: 13,
    })}
  >
    <!-- DropdownHeader -->
    <div style={css({ padding: "10px 12px 8px" })}>
      <!-- Title row -->
      <div style={css({ display: "flex", alignItems: "center", gap: 10, marginBottom: 6 })}>
        <div style={css({
          width: 26,
          height: 26,
          borderRadius: 7,
          background: `linear-gradient(135deg, color-mix(in oklch, ${meta.color} 80%, white) 0%, ${meta.color} 100%)`,
          display: "grid",
          placeItems: "center",
          boxShadow: `0 4px 10px -2px color-mix(in oklch, ${meta.color} 60%, transparent)`,
        })}>
          <Glyph size={16} color="#fff" />
        </div>
        <div style={css({ display: "flex", flexDirection: "column", lineHeight: 1.2 })}>
          <span style={css({ fontWeight: 600, fontSize: 13 })}>Translator Mic</span>
          <span style={css({ color: "var(--txt-3)", fontSize: 11.5 })}>
            {#if isError}
              Reconnecting · Sending silence
            {:else if isTranslating}
              {store.langPairText}
            {:else}
              {meta.short}
            {/if}
          </span>
        </div>
        <div style={css({ marginLeft: "auto" })}>
          {#if isError}
            <SysIcon name="warn" size={14} />
          {:else}
            <Dot
              size={8}
              color={meta.color}
              extraStyle={{ boxShadow: `0 0 0 2px color-mix(in oklch, ${meta.color} 30%, transparent)` }}
            />
          {/if}
        </div>
      </div>

      <!-- Status row -->
      <div style={css({ display: "flex", alignItems: "center", gap: 10 })}>
        <div style={css({
          fontSize: 11,
          color: "var(--txt-3)",
          display: "flex",
          alignItems: "center",
          gap: 6,
        })}>
          {#if isError}
            <span style={css({ color: "var(--c-error)" })}>Translator Mic is silent</span>
          {:else if isTranslating}
            <span>Translation latency</span>
            <span style={css({ color: "var(--txt-1)", fontWeight: 500, fontFamily: "ui-monospace, SF Mono, Menlo, monospace" })}>
              {store.latencyText}
            </span>
          {:else if store.mode === "pass"}
            <span>Live · original voice</span>
          {:else}
            <span>No audio is being sent</span>
          {/if}
        </div>
        <div style={css({ marginLeft: "auto" })}>
          <VUDots level={vuLevel} color={vuColor} />
        </div>
      </div>
    </div>

    <!-- DropdownDivider -->
    <div style={css({ height: 1, background: "var(--hairline)", margin: "5px 6px" })}></div>

    <!-- DropdownSectionTitle: Output Mode -->
    <div style={css({
      padding: "4px 10px 2px",
      fontSize: 10.5,
      fontWeight: 600,
      color: "var(--txt-3)",
      textTransform: "uppercase",
      letterSpacing: 0.4,
    })}>Output Mode</div>

    <!-- ModeRows -->
    {#each MODES as m (m.id)}
      {@const over = !!modeHover[m.id]}
      {@const selected = store.mode === m.id}
      <div
        onmouseenter={() => (modeHover[m.id] = true)}
        onmouseleave={() => (modeHover[m.id] = false)}
        onclick={() => store.setMode(m.id)}
        role="menuitemradio"
        aria-checked={selected}
        tabindex="0"
        onkeydown={(e) => e.key === "Enter" && store.setMode(m.id)}
        style={css({
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "5px 10px",
          borderRadius: 6,
          cursor: "default",
          background: over ? "var(--c-mixed)" : "transparent",
          color: over ? "#fff" : "var(--txt-1)",
        })}
      >
        <span style={css({
          width: 14,
          height: 14,
          borderRadius: "50%",
          border: `1.5px solid ${over ? "rgba(255,255,255,0.85)" : "color-mix(in oklch, var(--txt-2) 50%, transparent)"}`,
          display: "grid",
          placeItems: "center",
          background: selected ? (over ? "#fff" : m.color) : "transparent",
        })}>
          {#if selected}
            <Dot size={5} color={over ? m.color : "#fff"} />
          {/if}
        </span>
        <span style={css({ fontWeight: selected ? 500 : 400, flex: 1 })}>{m.label}</span>
        <Dot size={6} color={over ? "rgba(255,255,255,0.85)" : m.color} />
      </div>
    {/each}

    <!-- Divider -->
    <div style={css({ height: 1, background: "var(--hairline)", margin: "5px 6px" })}></div>

    <!-- DropdownComboRow: Target Language -->
    <div style={css({ padding: "6px 10px 7px", display: "flex", flexDirection: "column", gap: 2 })}>
      <span style={css({ fontSize: 11, color: "var(--txt-3)" })}>Target Language</span>
      <div style={css({
        display: "flex",
        alignItems: "center",
        gap: 6,
        padding: "5px 8px",
        background: "var(--control-bg)",
        border: "0.5px solid var(--control-border)",
        borderRadius: 6,
        cursor: "pointer",
      })}>
        <span style={css({
          flex: 1,
          fontSize: 12.5,
          color: "var(--txt-1)",
          display: "flex",
          alignItems: "center",
          gap: 6,
        })}>
          <LangChip code={store.targetLang.code} />
          <span style={css({ marginLeft: 2 })}>{store.targetLang.name}</span>
        </span>
        <Chevron size={9} />
      </div>
    </div>

    <!-- DropdownComboRow: Source Mic -->
    <div style={css({ padding: "6px 10px 7px", display: "flex", flexDirection: "column", gap: 2 })}>
      <span style={css({ fontSize: 11, color: "var(--txt-3)" })}>Source Mic</span>
      <div style={css({
        display: "flex",
        alignItems: "center",
        gap: 6,
        padding: "5px 8px",
        background: "var(--control-bg)",
        border: "0.5px solid var(--control-border)",
        borderRadius: 6,
        cursor: "pointer",
      })}>
        <span style={css({
          flex: 1,
          fontSize: 12.5,
          color: "var(--txt-1)",
          display: "flex",
          alignItems: "center",
          gap: 6,
        })}>
          {store.status?.sourceMicName ?? "No input device"}
        </span>
        <Chevron size={9} />
      </div>
    </div>

    <!-- DropdownComboRow: Virtual Mic (subdued) -->
    <div style={css({ padding: "6px 10px 7px", display: "flex", flexDirection: "column", gap: 2 })}>
      <span style={css({ fontSize: 11, color: "var(--txt-3)" })}>Virtual Mic</span>
      <div style={css({
        display: "flex",
        alignItems: "center",
        gap: 6,
        padding: "5px 8px",
        background: "var(--control-bg)",
        border: "0.5px solid var(--control-border)",
        borderRadius: 6,
        cursor: "default",
      })}>
        <span style={css({
          flex: 1,
          fontSize: 12.5,
          color: "var(--txt-2)",
          display: "flex",
          alignItems: "center",
          gap: 6,
        })}>
          <span>Translator Mic</span>
          {#if store.status?.virtualMicInstalled}
            <Check size={11} color="var(--c-translate)" />
          {:else}
            <SysIcon name="warn" size={11} />
          {/if}
        </span>
      </div>
    </div>

    <!-- Divider -->
    <div style={css({ height: 1, background: "var(--hairline)", margin: "5px 6px" })}></div>

    <!-- DropdownRow: Show Captions -->
    <div
      onmouseenter={() => (rowHover["captions"] = true)}
      onmouseleave={() => (rowHover["captions"] = false)}
      onclick={() => { store.setCaptionsOpen(!store.captionsOpen); store.setQuickOpen(false); }}
      role="menuitem"
      tabindex="0"
      onkeydown={(e) => e.key === "Enter" && (store.setCaptionsOpen(!store.captionsOpen), store.setQuickOpen(false))}
      style={css({
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "6px 10px",
        borderRadius: 6,
        cursor: "default",
        background: rowHover["captions"] ? "var(--c-mixed)" : "transparent",
        color: rowHover["captions"] ? "#fff" : "var(--txt-1)",
        transition: "background 80ms, color 80ms",
      })}
    >
      <span>Show Captions</span>
      <span style={css({ marginLeft: "auto", color: rowHover["captions"] ? "rgba(255,255,255,0.7)" : "var(--txt-3)", fontSize: 12 })}>
        {store.captionsOpen ? "⌘⇧C  ✓" : "⌘⇧C"}
      </span>
    </div>

    <!-- DropdownRow: Open Settings -->
    <div
      onmouseenter={() => (rowHover["settings"] = true)}
      onmouseleave={() => (rowHover["settings"] = false)}
      onclick={() => { store.setSettingsTab("status"); store.setQuickOpen(false); }}
      role="menuitem"
      tabindex="0"
      onkeydown={(e) => e.key === "Enter" && (store.setSettingsTab("status"), store.setQuickOpen(false))}
      style={css({
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "6px 10px",
        borderRadius: 6,
        cursor: "default",
        background: rowHover["settings"] ? "var(--c-mixed)" : "transparent",
        color: rowHover["settings"] ? "#fff" : "var(--txt-1)",
        transition: "background 80ms, color 80ms",
      })}
    >
      <span>Open Settings…</span>
      <span style={css({ marginLeft: "auto", color: rowHover["settings"] ? "rgba(255,255,255,0.7)" : "var(--txt-3)", fontSize: 12 })}>⌘,</span>
    </div>

    <!-- DropdownRow: Run Setup Again -->
    <div
      onmouseenter={() => (rowHover["setup"] = true)}
      onmouseleave={() => (rowHover["setup"] = false)}
      onclick={() => { store.setOnboardingOpen(true); store.setQuickOpen(false); }}
      role="menuitem"
      tabindex="0"
      onkeydown={(e) => e.key === "Enter" && (store.setOnboardingOpen(true), store.setQuickOpen(false))}
      style={css({
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "6px 10px",
        borderRadius: 6,
        cursor: "default",
        background: rowHover["setup"] ? "var(--c-mixed)" : "transparent",
        color: rowHover["setup"] ? "#fff" : "var(--txt-1)",
        transition: "background 80ms, color 80ms",
      })}
    >
      <span>Run Setup Again…</span>
    </div>

    <!-- Divider -->
    <div style={css({ height: 1, background: "var(--hairline)", margin: "5px 6px" })}></div>

    <!-- DropdownRow: Quit Intervox -->
    <div
      onmouseenter={() => (rowHover["quit"] = true)}
      onmouseleave={() => (rowHover["quit"] = false)}
      onclick={() => store.quit()}
      role="menuitem"
      tabindex="0"
      onkeydown={(e) => e.key === "Enter" && store.quit()}
      style={css({
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "6px 10px",
        borderRadius: 6,
        cursor: "default",
        background: rowHover["quit"] ? "var(--c-mixed)" : "transparent",
        color: rowHover["quit"] ? "#fff" : "var(--txt-1)",
        transition: "background 80ms, color 80ms",
      })}
    >
      <span>Quit Intervox</span>
      <span style={css({ marginLeft: "auto", color: rowHover["quit"] ? "rgba(255,255,255,0.7)" : "var(--txt-3)", fontSize: 12 })}>⌘Q</span>
    </div>
  </div>
{/if}
