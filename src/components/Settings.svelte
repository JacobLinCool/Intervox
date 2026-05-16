<script lang="ts">
  import { store, connectionChip } from "$lib/store.svelte";
  import { Glyph, SidebarIcon, Dot } from "$lib/icons";
  import { MODES } from "$lib/constants";
  import { css } from "$lib/util";
  import StatusPane from "./panes/StatusPane.svelte";
  import AccountPane from "./panes/AccountPane.svelte";
  import AudioPane from "./panes/AudioPane.svelte";
  import TranslationPane from "./panes/TranslationPane.svelte";
  import CaptionsPane from "./panes/CaptionsPane.svelte";
  import ShortcutsPane from "./panes/ShortcutsPane.svelte";
  import PrivacyPane from "./panes/PrivacyPane.svelte";
  import AdvancedPane from "./panes/AdvancedPane.svelte";

  const SIDEBAR = [
    { id: "status",      label: "Status",      tint: "var(--c-mixed)" },
    { id: "account",     label: "Account",     tint: "var(--c-mixed)" },
    { id: "audio",       label: "Audio",       tint: "var(--c-pass)" },
    { id: "translation", label: "Translation", tint: "var(--c-translate)" },
    { id: "captions",    label: "Captions",    tint: "var(--c-mixed)" },
    { id: "shortcuts",   label: "Shortcuts",   tint: "var(--c-silence)" },
    { id: "privacy",     label: "Privacy",     tint: "var(--c-translate)" },
    { id: "advanced",    label: "Advanced",    tint: "var(--c-silence)" },
  ] as const;

  // Connection chip
  const chip = $derived(
    connectionChip(
      store.mode,
      store.status?.translation ?? "idle",
      store.latencyText,
      store.lastError?.title ?? null,
    )
  );
  const chipColor = $derived(
    chip.tone === "error"
      ? "var(--c-error)"
      : chip.tone === "warn"
        ? "var(--c-pass)"
        : chip.tone === "ok"
          ? "var(--c-translate)"
          : "var(--txt-3)"
  );
  const chipText = $derived(chip.text);

  // Current sidebar item's label for title bar
  const currentLabel = $derived(
    SIDEBAR.find((s) => s.id === store.settingsTab)?.label ?? ""
  );

  // Quick Status trigger: mode meta for dot color
  const quickMeta = $derived(MODES.find((m) => m.id === store.mode) ?? MODES[2]);

  // Hover state for sidebar items
  let hoveredId = $state<string | null>(null);
</script>

<!-- Full-window container: fills the Tauri window -->
<div style={css({ width: "100%", height: "100vh", display: "flex" })}>

  <!-- ── Sidebar ── -->
  <aside
    style={css({
      width: 210,
      background: "var(--sidebar-bg)",
      borderRight: "0.5px solid var(--hairline)",
      display: "flex",
      flexDirection: "column",
    })}
  >
    <!-- App glyph + version (no traffic dots) -->
    <div
      style={css({
        padding: "8px 12px 14px",
        display: "flex",
        alignItems: "center",
        gap: 10,
      })}
    >
      <div
        style={css({
          width: 26,
          height: 26,
          borderRadius: 7,
          background:
            "linear-gradient(135deg, color-mix(in oklch, var(--c-mixed) 75%, white) 0%, var(--c-mixed) 100%)",
          display: "grid",
          placeItems: "center",
          boxShadow:
            "0 3px 8px -2px color-mix(in oklch, var(--c-mixed) 60%, transparent)",
        })}
      >
        <Glyph size={15} color="#fff" />
      </div>
      <div
        style={css({
          display: "flex",
          flexDirection: "column",
          lineHeight: 1.2,
        })}
      >
        <span style={css({ fontWeight: 600, fontSize: 13 })}>Intervox</span>
        <span style={css({ fontSize: 10.5, color: "var(--txt-3)" })}>{store.appVersion ? `v${store.appVersion}` : ""}</span>
      </div>
    </div>

    <!-- Sidebar nav items -->
    <div
      style={css({
        padding: "0 8px",
        display: "flex",
        flexDirection: "column",
        gap: 1,
      })}
    >
      {#each SIDEBAR as s (s.id)}
        {@const active = store.settingsTab === s.id}
        {@const over = hoveredId === s.id}
        <div
          onmouseenter={() => (hoveredId = s.id)}
          onmouseleave={() => (hoveredId = null)}
          onclick={() => store.setSettingsTab(s.id)}
          role="button"
          tabindex="0"
          onkeydown={(e) => e.key === "Enter" && store.setSettingsTab(s.id)}
          style={css({
            display: "flex",
            alignItems: "center",
            gap: 9,
            padding: "5px 8px",
            borderRadius: 6,
            background: active
              ? "var(--c-mixed)"
              : over
                ? "var(--row-hover)"
                : "transparent",
            color: active ? "#fff" : "var(--txt-1)",
            cursor: "pointer",
            fontSize: 12.5,
            transition: "background 80ms",
          })}
        >
          <span style={css({ color: active ? "#fff" : s.tint, display: "flex" })}>
            <SidebarIcon name={s.id} />
          </span>
          <span>{s.label}</span>
        </div>
      {/each}
    </div>

    <!-- Bottom status chip -->
    <div style={css({ marginTop: "auto", padding: 12 })}>
      <div
        style={css({
          background: `color-mix(in oklch, ${chipColor} 10%, transparent)`,
          border: `0.5px solid color-mix(in oklch, ${chipColor} 30%, var(--hairline))`,
          borderRadius: 8,
          padding: "8px 10px",
          display: "flex",
          alignItems: "center",
          gap: 8,
          fontSize: 11.5,
          color: "var(--txt-2)",
        })}
      >
        <Dot
          size={7}
          color={chipColor}
          extraStyle={{
            boxShadow: `0 0 0 2px color-mix(in oklch, ${chipColor} 25%, transparent)`,
            animation: "pulse-dot 1.6s ease-in-out infinite",
          }}
        />
        <span>{chipText}</span>
      </div>
    </div>
  </aside>

  <!-- ── Main pane ── -->
  <main
    style={css({
      flex: 1,
      display: "flex",
      flexDirection: "column",
      minWidth: 0,
    })}
  >
    <!-- Title bar (no fake nav chevrons) -->
    <div
      style={css({
        height: 38,
        padding: "0 18px",
        display: "flex",
        alignItems: "center",
        gap: 10,
        borderBottom: "0.5px solid var(--hairline)",
        background: "color-mix(in oklch, var(--win-bg-solid) 50%, transparent)",
      })}
    >
      <span style={css({ fontSize: 13, fontWeight: 600 })}>{currentLabel}</span>
      <button
        class="btn ghost"
        title="Quick status"
        onclick={() => store.setQuickOpen(!store.quickOpen)}
        style={css({
          marginLeft: "auto",
          display: "flex",
          alignItems: "center",
          gap: 5,
          padding: "3px 8px",
          borderRadius: 6,
          border: "0.5px solid var(--hairline)",
          background: store.quickOpen ? "var(--row-hover)" : "transparent",
          cursor: "default",
          color: "var(--txt-2)",
          fontSize: 12,
        })}
      >
        <Glyph size={15} color="var(--txt-2)" />
        <Dot size={6} color={quickMeta.color} />
      </button>
    </div>

    <!-- Scroll container -->
    <div
      style={css({
        flex: 1,
        overflowY: "auto",
        padding: "20px 26px 32px",
      })}
    >
      {#if store.settingsTab === "status"}
        <StatusPane />
      {:else if store.settingsTab === "account"}
        <AccountPane />
      {:else if store.settingsTab === "audio"}
        <AudioPane />
      {:else if store.settingsTab === "translation"}
        <TranslationPane />
      {:else if store.settingsTab === "captions"}
        <CaptionsPane />
      {:else if store.settingsTab === "shortcuts"}
        <ShortcutsPane />
      {:else if store.settingsTab === "privacy"}
        <PrivacyPane />
      {:else if store.settingsTab === "advanced"}
        <AdvancedPane />
      {/if}
    </div>
  </main>
</div>
