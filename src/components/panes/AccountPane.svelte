<script lang="ts">
  import { store } from "$lib/store.svelte";
  import { PaneTitle, FieldGroup, Row, RowLabel } from "$lib/controls";
  import { SysIcon, SidebarIcon } from "$lib/icons";
  import { css } from "$lib/util";

  // ── Local state ─────────────────────────────────────────────
  let draft = $state("");
  let show = $state(false);
  let verifying = $state(false);
  let error: string | null = $state(null);

  // ── Derived ──────────────────────────────────────────────────
  const verified = $derived(store.account.verified);
  const masked = $derived(store.account.maskedKey ?? "");
  const looksValid = $derived(/^sk-[A-Za-z0-9_\-]{20,}$/.test(draft.trim()));

  // ── Actions ──────────────────────────────────────────────────
  async function verify() {
    error = null;
    verifying = true;
    try {
      await store.setApiKey(draft.trim());
      await store.verifyApiKey();
      verifying = false;
      if (store.account.verified) {
        draft = "";
      } else {
        error = "That doesn't look like an OpenAI key. They start with sk- and are about 40+ characters.";
      }
    } catch (e: unknown) {
      error = (e as { message?: string })?.message ?? "Couldn't save the key.";
      verifying = false;
    }
  }

  async function removeKey() {
    await store.clearApiKey();
    draft = "";
  }
</script>

<PaneTitle
  title="Account"
  sub="Intervox uses your own OpenAI API key. Translation is billed directly to your OpenAI account — no Intervox subscription, nothing in the middle."
/>

<FieldGroup
  title="OpenAI API Key"
  hint={verified
    ? "Stored in Intervox's local app config on this Mac. Remove the key to stop translation."
    : "Don't have a key? Create one at platform.openai.com → API keys."}
>
  <Row extraStyle={{ padding: "14px", flexDirection: "column", alignItems: "stretch" }}>
    <div style={css({ display: "flex", alignItems: "center", gap: 12 })}>
      <!-- Lock icon -->
      <div
        style={css({
          width: 36,
          height: 36,
          borderRadius: 9,
          flexShrink: 0,
          background: verified
            ? "color-mix(in oklch, var(--c-translate) 18%, transparent)"
            : "color-mix(in oklch, var(--c-error) 14%, transparent)",
          color: verified ? "var(--c-translate)" : "var(--c-error)",
          display: "grid",
          placeItems: "center",
        })}
      >
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round">
          <rect x="2.5" y="7.5" width="11" height="6" rx="1.5"/>
          <path d="M5 7.5V5a3 3 0 1 1 6 0v2.5" stroke-linecap="round"/>
        </svg>
      </div>

      <!-- Status label -->
      <div style={css({ flex: 1 })}>
        {#if verified}
          <div style={css({ fontSize: 13, fontWeight: 500, display: "flex", alignItems: "center", gap: 8 })}>
            Connected
            <span style={css({ color: "var(--c-translate)" })}>
              <SysIcon name="ok" size={12} />
            </span>
          </div>
          <div class="mono" style={css({ fontSize: 11.5, color: "var(--txt-3)", marginTop: 2 })}>
            {masked}
          </div>
        {:else}
          <div style={css({ fontSize: 13, fontWeight: 500 })}>Not connected</div>
          <div style={css({ fontSize: 11.5, color: "var(--txt-3)", marginTop: 2 })}>
            Translation is disabled until a key is added.
          </div>
        {/if}
      </div>

      <!-- Remove button (only when verified) -->
      {#if verified}
        <button class="btn" onclick={removeKey} style={css({ fontSize: 12 })}>Remove key</button>
      {/if}
    </div>

    <!-- Key input (only when not verified) -->
    {#if !verified}
      <div style={css({ marginTop: 12 })}>
        <div
          style={css({
            display: "flex",
            alignItems: "center",
            gap: 6,
            padding: "6px 6px 6px 12px",
            background: "var(--control-bg)",
            border: `1px solid ${error ? "var(--c-error)" : "var(--control-border)"}`,
            borderRadius: 8,
          })}
        >
          <input
            type={show ? "text" : "password"}
            value={draft}
            oninput={(e) => { draft = (e.currentTarget as HTMLInputElement).value; error = null; }}
            placeholder="sk-…"
            spellcheck={false}
            autocomplete="off"
            disabled={verifying}
            style={css({
              flex: 1,
              border: "none",
              outline: "none",
              background: "transparent",
              fontFamily: "ui-monospace, SF Mono, Menlo, monospace",
              fontSize: 13,
              padding: "5px 0",
              color: "var(--txt-1)",
            })}
          />
          <button
            class="btn ghost"
            onclick={() => { show = !show; }}
            style={css({ padding: "3px 8px", fontSize: 11.5, color: "var(--txt-3)" })}
          >
            {show ? "Hide" : "Show"}
          </button>
          <button
            class="btn primary"
            onclick={verify}
            disabled={!draft || verifying}
            style={css({ padding: "5px 12px", fontSize: 12.5, opacity: draft ? 1 : 0.5 })}
          >
            {verifying ? "Verifying…" : "Verify & save"}
          </button>
        </div>

        {#if error}
          <div
            style={css({
              marginTop: 8,
              display: "flex",
              alignItems: "flex-start",
              gap: 8,
              fontSize: 12,
              color: "var(--c-error)",
              lineHeight: 1.45,
            })}
          >
            <SysIcon name="warn" size={13} /><span>{error}</span>
          </div>
        {/if}
      </div>
    {/if}
  </Row>
</FieldGroup>

{#if verified}
  <FieldGroup title="Usage" hint="Estimated locally from translation audio sent from this Mac (~$0.034/min). This is an estimate, not your OpenAI bill.">
    <Row>
      <RowLabel title="This month" sub="Translation minutes sent this calendar month." />
      <span class="mono" style={css({ marginLeft: "auto", fontSize: 14, fontWeight: 500 })}>
        {store.account.monthMinutes.toFixed(1)} min · ${store.account.monthUsd.toFixed(2)}
      </span>
    </Row>
    <Row>
      <RowLabel title="All time" sub="Total since you started using Intervox." />
      <span class="mono" style={css({ marginLeft: "auto", fontSize: 14, fontWeight: 500 })}>
        {store.account.totalMinutes.toFixed(1)} min · ${store.account.totalUsd.toFixed(2)}
      </span>
    </Row>
    <Row>
      <RowLabel title="Last verified" sub="When you last verified the key here." />
      <span style={css({ marginLeft: "auto", fontSize: 12.5, color: "var(--txt-2)" })}>
        {store.account.lastVerified ?? "—"}
      </span>
    </Row>
    <Row last>
      <RowLabel title="Manage on OpenAI" sub="Open the OpenAI dashboard to view billing and rotate keys." />
      <span style={css({ marginLeft: "auto" })}>
        <a
          href="https://platform.openai.com"
          onclick={(e) => { e.preventDefault(); store.openExternalUrl("https://platform.openai.com"); }}
          style={css({ fontSize: 12.5, color: "var(--c-mixed)", textDecoration: "none", cursor: "pointer" })}
        >
          platform.openai.com ↗
        </a>
      </span>
    </Row>
  </FieldGroup>
{/if}

<!-- How BYOK works callout -->
<div
  class="card"
  style={css({
    padding: 14,
    display: "flex",
    gap: 12,
    background: "color-mix(in oklch, var(--c-mixed) 6%, var(--card-bg))",
    borderColor: "color-mix(in oklch, var(--c-mixed) 22%, var(--card-border))",
  })}
>
  <div style={css({ color: "var(--c-mixed)", marginTop: 1 })}>
    <SidebarIcon name="privacy" />
  </div>
  <div style={css({ fontSize: 12.5, lineHeight: 1.55, color: "var(--txt-2)" })}>
    <div style={css({ fontSize: 13, fontWeight: 500, color: "var(--txt-1)", marginBottom: 4 })}>
      How BYOK works
    </div>
    Your key is stored in Intervox's local app config on this Mac and used only
    on your Mac. Intervox connects directly from your machine to OpenAI's
    Realtime Translation endpoint — your audio, transcripts, and key never pass
    through Intervox servers.
  </div>
</div>
