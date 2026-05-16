<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { css } from "$lib/util";

  // ── Minimal local store: mirrors store.svelte.ts srcText/tgtText accumulation
  // but scoped to this window only — no full app store needed.
  let srcText = $state("");
  let tgtText = $state("");

  const unlisten: UnlistenFn[] = [];

  onMount(async () => {
    // Guard: only subscribe inside Tauri (window.__TAURI_INTERNALS__ present).
    // This allows unit tests (jsdom) to render the component without errors.
    if (!("__TAURI_INTERNALS__" in window)) return;

    unlisten.push(
      await listen<{ text: string }>("source-transcript-delta", (e) => {
        let s = srcText + e.payload.text;
        if (s.length > 4000) s = s.slice(-4000);
        srcText = s;
      }),
    );
    unlisten.push(
      await listen<{ text: string }>("target-transcript-delta", (e) => {
        let s = tgtText + e.payload.text;
        if (s.length > 4000) s = s.slice(-4000);
        tgtText = s;
      }),
    );
    // Issue 3: clear captions when transcript-cleared is emitted (e.g. user clears history).
    unlisten.push(
      await listen("transcript-cleared", () => {
        srcText = "";
        tgtText = "";
      }),
    );
  });

  onDestroy(() => {
    for (const fn of unlisten) fn();
  });
</script>

<!-- Transparent drag-anywhere window; no OS chrome -->
<div
  data-tauri-drag-region
  data-captions-window
  style={css({
    width: "100vw",
    height: "100vh",
    display: "flex",
    flexDirection: "column",
    justifyContent: "flex-end",
    padding: "10px 12px",
    background: "transparent",
  })}
>
  <!-- Glass panel -->
  <div
    style={css({
      background: "rgba(18, 18, 20, 0.80)",
      backdropFilter: "blur(40px) saturate(180%)",
      WebkitBackdropFilter: "blur(40px) saturate(180%)",
      border: "0.5px solid rgba(255,255,255,0.10)",
      borderRadius: 14,
      boxShadow: "0 16px 40px rgba(0,0,0,0.40), 0 0 0 0.5px rgba(0,0,0,0.25)",
      color: "#fff",
      overflow: "hidden",
    })}
  >
    <!-- Captions body -->
    <div
      style={css({
        padding: "14px 18px 16px",
        display: "flex",
        flexDirection: "column",
        gap: 12,
      })}
    >
      <!-- Source line -->
      {#if srcText}
        <div>
          <div
            style={css({
              fontSize: 10,
              fontWeight: 600,
              letterSpacing: 0.6,
              textTransform: "uppercase",
              color: "rgba(255,255,255,0.50)",
              marginBottom: 3,
            })}
          >
            Original
          </div>
          <div
            style={css({
              fontSize: 16,
              lineHeight: 1.35,
              fontWeight: 400,
              color: "rgba(255,255,255,0.65)",
              textWrap: "pretty",
            })}
          >
            {srcText}
          </div>
        </div>
      {/if}

      <!-- Target line -->
      <div>
        <div
          style={css({
            fontSize: 10,
            fontWeight: 600,
            letterSpacing: 0.6,
            textTransform: "uppercase",
            color: "rgba(140,180,255,0.80)",
            marginBottom: 3,
          })}
        >
          Translation
        </div>
        <div
          style={css({
            fontSize: 20,
            lineHeight: 1.35,
            fontWeight: 500,
            color: "#fff",
            textWrap: "pretty",
          })}
        >
          {#if tgtText}
            {tgtText}
          {:else}
            <span style={css({ color: "rgba(255,255,255,0.35)", fontWeight: 400 })}>
              Waiting for translation…
            </span>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>
