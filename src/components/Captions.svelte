<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { SysIcon } from "$lib/icons";
  import { VUDots } from "$lib/vu";
  import { css } from "$lib/util";
  import { detectedSourceName } from "$lib/constants";

  // ── Derived state ───────────────────────────────────────────
  const isTranslating = $derived(store.isTranslating);
  const isPass        = $derived(store.mode === "pass" && !store.lastError);
  const isSilence     = $derived(store.mode === "silence" && !store.lastError);
  const isError       = $derived(!!store.lastError);

  // caption position UI-only; default bottom
  const posStyle = { bottom: 90 };

  // ── Font size ───────────────────────────────────────────────
  const fontMap: Record<string, number> = { small: 16, medium: 18, large: 22 };
  const base = $derived(fontMap[store.config?.captions.font_size ?? "medium"] ?? 18);

  // ── Status bar ──────────────────────────────────────────────
  const status = $derived(
    isError
      ? { dot: "var(--c-error)",      text: "Reconnecting · Sending silence" }
      : isSilence
      ? { dot: "var(--c-silence)",    text: "Silence · No audio is being sent" }
      : isPass
      ? { dot: "var(--c-pass)",       text: "Pass-through · Original microphone only" }
      : store.mode === "mixed"
      ? { dot: "var(--c-mixed)",      text: `${store.langPairText} · Translation + quiet original · ${store.latencyText}` }
      :   { dot: "var(--c-translate)", text: `${store.langPairText} · Translating · ${store.latencyText}` }
  );

  // ── Error messages map ──────────────────────────────────────
  const errorMsgs: Record<string, { t: string; s: string }> = {
    network:    { t: "Connection lost",
                  s: "Translator Mic is sending silence while Intervox reconnects." },
    mic:        { t: "No audio is coming from your microphone",
                  s: "Check that the right Source Mic is selected in Audio settings." },
    driver:     { t: "Translator Mic isn't installed",
                  s: "Install the audio driver so meeting apps can see the virtual mic." },
    permission: { t: "Microphone permission missing",
                  s: "Grant microphone access in System Settings → Privacy & Security." },
  };
</script>

{#if store.captionsOpen}
  <!-- outer positioning wrapper -->
  <div
    data-captions
    style={css({
      position: "absolute",
      left: "50%",
      transform: "translateX(-50%)",
      ...posStyle,
      width: 620,
      zIndex: 40,
      animation: "captions-in 220ms ease-out both",
    })}
  >
    <!-- glass panel -->
    <div style={css({
      background: store.theme === "dark" ? "rgba(20, 20, 22, 0.78)" : "rgba(28, 28, 32, 0.72)",
      backdropFilter: "blur(40px) saturate(180%)",
      WebkitBackdropFilter: "blur(40px) saturate(180%)",
      border: "0.5px solid rgba(255,255,255,0.10)",
      borderRadius: 14,
      boxShadow: "0 16px 40px rgba(0,0,0,0.35), 0 0 0 0.5px rgba(0,0,0,0.20)",
      color: "#fff",
      overflow: "hidden",
    })}>

      <!-- Drag handle row -->
      <div style={css({
        display: "flex", alignItems: "center", gap: 10,
        padding: "8px 14px 7px",
        borderBottom: "0.5px solid rgba(255,255,255,0.08)",
        fontSize: 11.5,
        color: "rgba(255,255,255,0.75)",
      })}>
        <!-- status dot -->
        <span style={css({
          width: 7, height: 7, borderRadius: "50%",
          background: status.dot,
          boxShadow: `0 0 0 2px color-mix(in oklch, ${status.dot} 30%, transparent)`,
          animation: isTranslating ? "pulse-dot 1.4s ease-in-out infinite" : "none",
        })}></span>

        <span style={css({ fontWeight: 500, letterSpacing: 0.2 })}>{status.text}</span>

        {#if isTranslating}
          <VUDots level={store.outputLevel ?? 0} color="#fff" seed={2} />
        {/if}

        <span style={css({ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" })}>
          <!-- Pin icon (verbatim from reference) -->
          <span title="Pin" style={css({ opacity: 0.6, cursor: "default" })}>
            <svg width="11" height="11" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4">
              <path d="M10.5 1.5l4 4-2 2-1 5-3-3-4 4-1-1 4-4-3-3 5-1z" stroke-linejoin="round"/>
            </svg>
          </span>
          <!-- Close button -->
          <span
            role="button"
            tabindex="0"
            onclick={() => store.setCaptionsOpen(false)}
            onkeydown={(e) => e.key === "Enter" && store.setCaptionsOpen(false)}
            style={css({ opacity: 0.65, cursor: "pointer", padding: 2 })}
          >
            <SysIcon name="close" size={10} />
          </span>
        </span>
      </div>

      <!-- Captions body -->
      <div style={css({ padding: "18px 22px 20px", display: "flex", flexDirection: "column", gap: 16 })}>

        <!-- SameLanguageHint -->
        {#if isTranslating && store.sameLang}
          <div style={css({
            display: "flex", gap: 10, alignItems: "flex-start",
            padding: "8px 10px",
            borderRadius: 8,
            background: "rgba(255,255,255,0.06)",
            border: "0.5px solid rgba(255,255,255,0.10)",
            color: "rgba(255,255,255,0.85)",
            fontSize: 12,
            lineHeight: 1.45,
          })}>
            <span style={css({ color: "var(--c-mixed)", marginTop: 1 })}>
              <svg width="14" height="14" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7" fill="currentColor"/><path d="M8 4.5v4M8 10.6v0.6" stroke="#fff" stroke-width="1.5" stroke-linecap="round"/></svg>
            </span>
            <div>
              <div style={css({ color: "#fff", fontWeight: 500, marginBottom: 1 })}>
                You're already speaking {store.targetLang.name}
              </div>
              Intervox may stay quiet on these segments. Switch to Translate + Original if you often code-switch.
            </div>
          </div>
        {/if}

        <!-- Source CaptionLine -->
        {#if isTranslating && (store.config?.captions.show_source ?? true)}
          {@const srcText = store.srcText}
          <div>
            <div style={css({
              fontSize: 10.5, fontWeight: 600, letterSpacing: 0.6,
              textTransform: "uppercase",
              color: "rgba(255,255,255,0.55)",
              marginBottom: 4,
            })}>{detectedSourceName(store.langCtx)}</div>
            <div
              class="zh"
              style={css({
                fontSize: base,
                lineHeight: 1.35,
                fontWeight: 400,
                color: "rgba(255,255,255,0.65)",
                textWrap: "pretty",
              })}
            >
              {#if srcText}
                {srcText}
                {#if isTranslating}
                  <span style={css({
                    display: "inline-block", width: 2, height: base * 0.95,
                    background: "currentColor", marginLeft: 2, verticalAlign: "-2px",
                    animation: "pulse-dot 0.9s ease-in-out infinite",
                  })}></span>
                {/if}
              {:else}
                <span style={css({ color: "rgba(255,255,255,0.4)" })}>Waiting for speech…</span>
              {/if}
            </div>
          </div>
        {/if}

        <!-- Target CaptionLine -->
        {#if isTranslating && (store.config?.captions.show_target ?? true)}
          {@const tgtText = store.tgtText}
          {@const tgtFontSize = base + 4}
          <div>
            <div style={css({
              fontSize: 10.5, fontWeight: 600, letterSpacing: 0.6,
              textTransform: "uppercase",
              color: "var(--c-translate)",
              marginBottom: 4,
            })}>{store.targetLang.name}</div>
            <div style={css({
              fontSize: tgtFontSize,
              lineHeight: 1.35,
              fontWeight: 500,
              color: "#fff",
              textWrap: "pretty",
            })}>
              {#if tgtText}
                {tgtText}
                {#if isTranslating}
                  <span style={css({
                    display: "inline-block", width: 2, height: tgtFontSize * 0.95,
                    background: "currentColor", marginLeft: 2, verticalAlign: "-2px",
                    animation: "pulse-dot 0.9s ease-in-out infinite",
                  })}></span>
                {/if}
              {:else}
                <span style={css({ color: "rgba(255,255,255,0.4)" })}>Waiting for translation…</span>
              {/if}
            </div>
          </div>
        {/if}

        <!-- NonTranslatingState -->
        {#if !isTranslating}
          {#if store.lastError}
            {@const m = errorMsgs[store.errorKind ?? "network"] ?? errorMsgs.network}
            <div style={css({ fontSize: base, color: "#fff", lineHeight: 1.4 })}>
              <div style={css({ fontWeight: 500, marginBottom: 4 })}>{m.t}</div>
              <div style={css({ color: "rgba(255,255,255,0.65)", fontSize: base - 4 })}>{m.s}</div>
            </div>

          {:else if store.mode === "silence"}
            <div style={css({ display: "flex", alignItems: "center", gap: 12, color: "rgba(255,255,255,0.85)" })}>
              <div style={css({
                width: 38, height: 38, borderRadius: 10,
                background: "rgba(255,255,255,0.08)",
                display: "grid", placeItems: "center",
                color: "rgba(255,255,255,0.75)",
              })}>
                <SysIcon name="micSlash" size={20} />
              </div>
              <div>
                <div style={css({ fontSize: base, fontWeight: 500 })}>Silence</div>
                <div style={css({ fontSize: base - 4, color: "rgba(255,255,255,0.55)" })}>
                  Your meeting hears nothing from Translator Mic.
                </div>
              </div>
            </div>

          {:else}
            <!-- pass-through -->
            <div style={css({ display: "flex", alignItems: "center", gap: 12 })}>
              <div style={css({
                width: 38, height: 38, borderRadius: 10,
                background: "color-mix(in oklch, var(--c-pass) 30%, transparent)",
                display: "grid", placeItems: "center",
                color: "#fff",
              })}>
                <SysIcon name="waveform" size={20} />
              </div>
              <div>
                <div style={css({ fontSize: base, fontWeight: 500 })}>Pass-through</div>
                <div style={css({ fontSize: base - 4, color: "rgba(255,255,255,0.65)" })}>
                  Your original microphone audio is sent unchanged.
                </div>
              </div>
              <div style={css({ marginLeft: "auto" })}>
                <VUDots level={store.outputLevel ?? 0} color="var(--c-pass)" count={6} />
              </div>
            </div>
          {/if}
        {/if}

      </div>
    </div>
  </div>
{/if}
