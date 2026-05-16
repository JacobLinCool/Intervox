<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { store } from "$lib/store.svelte";
  import Settings from "./components/Settings.svelte";
  import Captions from "./components/Captions.svelte";
  import Onboarding from "./components/Onboarding.svelte";
  import QuickStatus from "./components/QuickStatus.svelte";
  import { css } from "$lib/util";

  const WALLPAPERS: Record<string, string> = {
    lavender: "linear-gradient(155deg, #c9b8d6 0%, #a7b9d8 35%, #b9c6d8 70%, #d5cad3 100%)",
    sonoma:   "linear-gradient(160deg, #e8c2b8 0%, #d6a7c2 35%, #a8b3d4 70%, #b9c6dc 100%)",
    graphite: "linear-gradient(160deg, #d4d4d6 0%, #b8b8be 50%, #a0a0a8 100%)",
    ocean:    "linear-gradient(160deg, #5a9fc9 0%, #4a7ec0 50%, #6a5fb0 100%)",
  };

  const WALLPAPERS_DARK: Record<string, string> = {
    lavender: "linear-gradient(155deg, #2a1f3d 0%, #1a2238 35%, #18223a 70%, #2b1f33 100%)",
    sonoma:   "linear-gradient(160deg, #3a2530 0%, #2a1f3a 50%, #1a2238 100%)",
    graphite: "linear-gradient(160deg, #1f1f22 0%, #2a2a2e 50%, #18181b 100%)",
    ocean:    "linear-gradient(160deg, #15233b 0%, #1f2b4a 50%, #2a1f4a 100%)",
  };

  onMount(async () => {
    store.setTheme(
      window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light"
    );
    try {
      await store.init();
    } catch {
      // store.lastError may be set; shell still renders
    }
  });

  onDestroy(() => store.dispose());

  $effect(() => {
    document.documentElement.setAttribute("data-theme", store.theme);
    const pal = store.theme === "dark" ? WALLPAPERS_DARK : WALLPAPERS;
    document.body.style.background = pal[store.wallpaper] ?? pal.lavender;
  });

  const runRecovery = async () => {
    const a = store.lastError?.recovery_action;
    if (!a) return;
    try {
      await invoke(a.command);
    } catch {}
    store.dismissError();
  };
</script>

<div class="stage">
  <Settings />

  {#if store.captionsOpen && !store.onboardingOpen}
    <Captions />
  {/if}

  {#if store.onboardingOpen}
    <Onboarding />
  {/if}

  {#if store.quickOpen}
    <QuickStatus />
  {/if}

  {#if store.lastError}
    <div
      style={css({
        position: "fixed",
        top: 0,
        left: 0,
        right: 0,
        zIndex: 9000,
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "10px 16px",
        background: "color-mix(in oklch, var(--c-error) 14%, var(--win-bg-solid))",
        borderBottom: "0.5px solid color-mix(in oklch, var(--c-error) 40%, var(--hairline))",
        color: "var(--txt-1)",
        fontSize: 13,
        backdropFilter: "saturate(180%) blur(20px)",
      })}
    >
      <span
        style={css({
          width: 8,
          height: 8,
          borderRadius: "50%",
          background: "var(--c-error)",
          flexShrink: 0,
        })}
      ></span>
      <span style={css({ display: "flex", flexDirection: "column", gap: 1 })}>
        <strong>{store.lastError.title}</strong>
        <span style="color:var(--txt-3);font-size:12px">{store.lastError.message}</span>
      </span>
      <span style={css({ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 })}></span>
      {#if store.lastError.recovery_action}
        <button class="btn" onclick={runRecovery}>{store.lastError.recovery_action.label}</button>
      {/if}
      <button class="btn ghost" onclick={() => store.dismissError()} aria-label="Dismiss">✕</button>
    </div>
  {/if}
</div>
