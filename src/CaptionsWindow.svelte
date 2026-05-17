<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { store } from "$lib/store.svelte";
  import { cmd } from "$lib/tauri";
  import { Chevron, SysIcon } from "$lib/icons";
  import { VUDots } from "$lib/vu";
  import { css } from "$lib/util";
  import { detectedSourceName } from "$lib/constants";

  let expanded = $state(false);
  let captionBody = $state<HTMLDivElement | null>(null);
  let targetTextEl = $state<HTMLDivElement | null>(null);
  let sourceTextEl = $state<HTMLDivElement | null>(null);
  let scrollToken = 0;

  const fontMap: Record<string, number> = { small: 15, medium: 17, large: 20 };
  const base = $derived(fontMap[store.config?.captions.font_size ?? "medium"] ?? 17);
  const targetFontSize = $derived(base + 3);

  const isTranslating = $derived(store.isTranslating);
  const isPass = $derived(store.mode === "pass" && !store.lastError);
  const isSilence = $derived(store.mode === "silence" && !store.lastError);
  const showSource = $derived(store.config?.captions.show_source ?? true);
  const showTarget = $derived(store.config?.captions.show_target ?? true);

  const status = $derived(
    store.lastError
      ? { dot: "#ff6a4f", text: "Reconnecting · Sending silence" }
      : isSilence
        ? { dot: "oklch(0.66 0.012 260)", text: "Silence · No audio is being sent" }
        : isPass
          ? { dot: "oklch(0.62 0.165 245)", text: "Pass-through · Original source only" }
          : store.mixPercent > 0
            ? { dot: "oklch(0.66 0.155 155)", text: `${store.langPairText} · Translation + original ${store.mixPercent}% · ${store.latencyText}` }
            : { dot: "oklch(0.66 0.155 155)", text: `${store.langPairText} · Translating · ${store.latencyText}` }
  );

  const errorMsgs: Record<string, { t: string; s: string }> = {
    network: { t: "Connection lost", s: "Translator Mic is sending silence while Intervox reconnects." },
    mic: { t: "No audio is coming from your source", s: "Check that the right input source is selected in Audio settings." },
    driver: { t: "Translator Mic isn't installed", s: "Install the audio driver so meeting apps can see the virtual mic." },
    permission: { t: "Audio permission missing", s: "Grant the required audio access in System Settings." },
  };

  const inTauri = () => "__TAURI_INTERNALS__" in window;

  onMount(async () => {
    if (!inTauri()) return;
    await store.init();
    await store.setCaptionsWindowExpanded(expanded);
  });

  onDestroy(() => {
    store.dispose();
  });

  function toggleExpanded() {
    expanded = !expanded;
    if (inTauri()) void store.setCaptionsWindowExpanded(expanded);
  }

  function closeCaptions() {
    void store.setCaptions({ enabled: false });
  }

  function startWindowDrag(event: MouseEvent) {
    if (event.button !== 0 || event.defaultPrevented) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest("[data-no-window-drag]")) return;

    event.preventDefault();
    if (!inTauri()) return;
    void cmd.startCaptionsWindowDrag().catch((error) => {
      console.error("Could not start captions window drag", error);
    });
  }

  function scrollToEnd(el: HTMLElement | null) {
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    el.scrollLeft = el.scrollWidth;
  }

  function scheduleAutoScroll() {
    if (typeof window === "undefined") return;
    const token = ++scrollToken;
    void tick().then(() => {
      if (token !== scrollToken) return;
      const requestFrame = window.requestAnimationFrame ?? ((fn: FrameRequestCallback) => window.setTimeout(fn, 0));
      requestFrame(() => {
        scrollToEnd(targetTextEl);
        scrollToEnd(sourceTextEl);
        scrollToEnd(captionBody);
      });
    });
  }

  $effect(() => {
    store.tgtText;
    store.srcText;
    expanded;
    showTarget;
    showSource;
    targetFontSize;
    base;
    scheduleAutoScroll();
  });
</script>

<div
  data-captions-window
  role="presentation"
  class:compact={!expanded}
  class:expanded
  onmousedown={startWindowDrag}
  style={css({
    width: "100vw",
    height: "100vh",
    display: "flex",
    alignItems: "center",
    padding: "10px 12px",
    background: "transparent",
    color: "#fff",
    fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Display", "SF Pro Text", "Helvetica Neue", system-ui, sans-serif',
  })}
>
  <section
    class="caption-panel"
    style={css({
      width: "100%",
      minWidth: 0,
      background: "rgba(18, 18, 20, 0.82)",
      backdropFilter: "blur(40px) saturate(180%)",
      WebkitBackdropFilter: "blur(40px) saturate(180%)",
      border: "0.5px solid rgba(255,255,255,0.12)",
      borderRadius: 14,
      boxShadow: "0 16px 40px rgba(0,0,0,0.40), 0 0 0 0.5px rgba(0,0,0,0.25)",
      overflow: "hidden",
    })}
  >
    <header class="caption-header">
      <span
        class="status-dot"
        style={css({
          background: status.dot,
          boxShadow: `0 0 0 2px color-mix(in oklch, ${status.dot} 30%, transparent)`,
          animation: isTranslating ? "pulse-dot 1.4s ease-in-out infinite" : "none",
        })}
      ></span>

      <span class="status-text">{status.text}</span>

      {#if isTranslating}
        <VUDots level={store.outputLevel ?? 0} color="#fff" seed={2} count={5} />
      {/if}

      <span class="caption-actions">
        <button
          data-no-window-drag
          class="icon-button"
          type="button"
          title={expanded ? "Collapse" : "Expand"}
          aria-label={expanded ? "Collapse captions" : "Expand captions"}
          aria-pressed={expanded}
          onmousedown={(event) => event.stopPropagation()}
          onclick={toggleExpanded}
        >
          <Chevron size={11} dir={expanded ? "down" : "up"} />
        </button>
        <button
          data-no-window-drag
          class="icon-button"
          type="button"
          title="Close"
          aria-label="Close captions"
          onmousedown={(event) => event.stopPropagation()}
          onclick={closeCaptions}
        >
          <SysIcon name="close" size={11} />
        </button>
      </span>
    </header>

    <div bind:this={captionBody} class="caption-body">
      {#if isTranslating}
        {#if expanded && store.sameLang}
          <div class="same-language-hint">
            You're already speaking {store.targetLang.name}
          </div>
        {/if}

        {#if showTarget}
          <div class="caption-line target-line">
            <div class="caption-label">{store.targetLang.name}</div>
            <div
              bind:this={targetTextEl}
              data-caption-scroll="target"
              class="caption-text zh"
              style={css({
                fontSize: targetFontSize,
                lineHeight: 1.28,
                maxHeight: targetFontSize * 1.28 * (expanded ? 4 : 1),
              })}
            >
              {#if store.tgtText}
                {store.tgtText}
                <span class="caption-cursor" style={css({ height: targetFontSize * 0.9 })}></span>
              {:else}
                <span class="placeholder">Waiting for translation...</span>
              {/if}
            </div>
          </div>
        {/if}

        {#if showSource}
          <div class="caption-line source-line">
            <div class="caption-label">{detectedSourceName(store.langCtx)}</div>
            <div
              bind:this={sourceTextEl}
              data-caption-scroll="source"
              class="caption-text zh"
              style={css({
                fontSize: base,
                lineHeight: 1.28,
                maxHeight: base * 1.28 * (expanded ? 4 : 1),
              })}
            >
              {#if store.srcText}
                {store.srcText}
                <span class="caption-cursor muted" style={css({ height: base * 0.9 })}></span>
              {:else}
                <span class="placeholder">Waiting for speech...</span>
              {/if}
            </div>
          </div>
        {/if}

        {#if !showTarget && !showSource}
          <div class="caption-line">
            <div class="caption-label">Captions</div>
            <div class="caption-text" style={css({ fontSize: base, lineHeight: 1.28 })}>
              <span class="placeholder">No caption lines selected</span>
            </div>
          </div>
        {/if}
      {:else if store.lastError}
        {@const m = errorMsgs[store.errorKind ?? "network"] ?? errorMsgs.network}
        <div class="caption-line state-line">
          <div class="caption-label">Status</div>
          <div class="caption-text" style={css({ fontSize: base, lineHeight: 1.32 })}>
            <strong>{m.t}</strong>
            <span class="state-detail">{m.s}</span>
          </div>
        </div>
      {:else if isPass}
        <div class="caption-line state-line">
          <div class="caption-label">Status</div>
          <div class="caption-text" style={css({ fontSize: base, lineHeight: 1.32 })}>
            <strong>Pass-through</strong>
            <span class="state-detail">Original source audio is sent unchanged.</span>
          </div>
        </div>
      {:else}
        <div class="caption-line state-line">
          <div class="caption-label">Status</div>
          <div class="caption-text" style={css({ fontSize: base, lineHeight: 1.32 })}>
            <strong>Translation off</strong>
            <span class="state-detail">No audio is being sent.</span>
          </div>
        </div>
      {/if}
    </div>
  </section>
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    padding: 0;
    background: transparent !important;
    overflow: hidden;
  }

  :global(*) {
    box-sizing: border-box;
  }

  .caption-header {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 30px;
    padding: 8px 11px 7px 13px;
    border-bottom: 0.5px solid rgba(255, 255, 255, 0.08);
    color: rgba(255, 255, 255, 0.74);
    font-size: 11.5px;
    min-width: 0;
    cursor: grab;
    user-select: none;
  }

  .caption-panel,
  .caption-body,
  .caption-line {
    cursor: grab;
    user-select: none;
  }

  .status-dot {
    width: 7px;
    height: 7px;
    border-radius: 999px;
    flex: 0 0 auto;
  }

  .status-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }

  .caption-actions {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: 0 0 auto;
  }

  .icon-button {
    width: 22px;
    height: 20px;
    display: inline-grid;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: rgba(255, 255, 255, 0.68);
    cursor: pointer;
    user-select: none;
  }

  .icon-button:hover,
  .icon-button:focus-visible {
    background: rgba(255, 255, 255, 0.10);
    color: #fff;
    outline: none;
  }

  .caption-body {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 9px 14px 11px;
    min-height: 0;
  }

  .expanded .caption-body {
    gap: 10px;
    max-height: 238px;
    overflow: auto;
    padding: 12px 16px 16px;
  }

  .caption-line {
    display: grid;
    grid-template-columns: 82px minmax(0, 1fr);
    gap: 10px;
    align-items: baseline;
    min-width: 0;
  }

  .caption-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: rgba(255, 255, 255, 0.50);
    font-size: 10px;
    font-weight: 650;
    letter-spacing: 0.4px;
    text-transform: uppercase;
  }

  .target-line .caption-label {
    color: rgba(140, 255, 205, 0.78);
  }

  .caption-text {
    min-width: 0;
    display: block;
    overflow: hidden;
    overflow-wrap: anywhere;
    text-wrap: pretty;
  }

  .expanded .caption-text {
    overflow-y: auto;
    scrollbar-width: none;
  }

  .caption-text::-webkit-scrollbar {
    display: none;
  }

  .target-line .caption-text {
    color: #fff;
    font-weight: 600;
  }

  .source-line .caption-text {
    color: rgba(255, 255, 255, 0.68);
    font-weight: 400;
  }

  .placeholder,
  .state-detail {
    color: rgba(255, 255, 255, 0.38);
    font-weight: 400;
  }

  .state-line .caption-text {
    color: #fff;
  }

  .state-detail {
    display: block;
    margin-top: 2px;
  }

  .caption-cursor {
    display: inline-block;
    width: 2px;
    margin-left: 3px;
    vertical-align: -2px;
    background: currentColor;
    animation: pulse-dot 0.9s ease-in-out infinite;
  }

  .caption-cursor.muted {
    opacity: 0.65;
  }

  .same-language-hint {
    border-radius: 8px;
    padding: 7px 9px;
    border: 0.5px solid rgba(255, 255, 255, 0.10);
    background: rgba(255, 255, 255, 0.06);
    color: rgba(255, 255, 255, 0.76);
    font-size: 12px;
    line-height: 1.35;
  }

  @keyframes pulse-dot {
    0%,
    100% {
      opacity: 1;
      transform: scale(1);
    }
    50% {
      opacity: 0.48;
      transform: scale(0.75);
    }
  }
</style>
