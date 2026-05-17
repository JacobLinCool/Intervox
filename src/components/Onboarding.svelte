<script lang="ts">
  import { onMount } from "svelte";
  import { store } from "$lib/store.svelte";
  import { COMMON_LANGS } from "$lib/constants";
  import { Glyph, Check, Dot, SysIcon, LangChip } from "$lib/icons";
  import { VUStrip } from "$lib/vu";
  import { css } from "$lib/util";

  // ── Local state ─────────────────────────────────────────────
  let step = $state(0);

  // Step 1 (API Key)
  let draft = $state("");
  let show = $state(false);
  let verifying = $state(false);
  let keyError = $state<string | null>(null);

  // Step 2 (Mic permission)
  let requestingMic = $state(false);
  let micError = $state<string | null>(null);

  // Step 3 (Driver) — install attempted flag for error display
  let driverAttempted = $state(false);
  let driverLoading = $state(false);

  // Step 7 (Meeting)
  let meetingApp = $state<"zoom" | "meet" | "teams">("zoom");

  // ── Steps definition ────────────────────────────────────────
  const steps = [
    { id: "welcome",  label: "Welcome" },
    { id: "api-key",  label: "API Key" },
    { id: "mic-perm", label: "Microphone" },
    { id: "driver",   label: "Virtual Mic" },
    { id: "source",   label: "Source" },
    { id: "target",   label: "Translate to" },
    { id: "test",     label: "Test" },
    { id: "meeting",  label: "Meeting Setup" },
  ];

  // ── Derived: canNext ────────────────────────────────────────
  const sourceMicSelected = $derived(!!store.config?.audio.source_mic_id);
  const translationTestPassed = $derived(
    sourceMicSelected && store.audioInputDetected && store.tgtText.trim().length > 0
  );
  const canNext = $derived(
    step === 1 ? store.account.verified :
    step === 2 ? store.micPermission === "granted" :
    step === 3 ? (store.status?.virtualMicInstalled === true) :
    step === 4 ? sourceMicSelected :
    step === 6 ? translationTestPassed :
    true
  );

  // ── Navigation ──────────────────────────────────────────────
  async function next() {
    if (step < steps.length - 1) {
      const nextStep = step + 1;
      if (nextStep === 6) store.resetAudioInputDetection();
      step = nextStep;
    } else {
      await store.completeOnboarding();
    }
  }
  function back() {
    step = Math.max(0, step - 1);
  }

  // ── Step 1: API Key helpers ──────────────────────────────────
  const looksValid = $derived(/^sk-[A-Za-z0-9_\-]{20,}$/.test(draft.trim()));

  async function verify() {
    keyError = null;
    verifying = true;
    try {
      await store.setApiKey(draft.trim());
      await store.verifyApiKey();
    } catch (e: unknown) {
      keyError = (e as { message?: string })?.message ?? "Couldn't save the key.";
    }
    verifying = false;
    if (!store.account.verified && !keyError) {
      keyError = "That doesn't look like an OpenAI key. They start with sk- and are about 40+ characters.";
    }
  }

  async function clearKey() {
    await store.clearApiKey();
    draft = "";
    keyError = null;
  }

  // ── Step 2: Mic permission helpers ───────────────────────────
  const micPermissionCopy = $derived(
    requestingMic ? "Waiting for macOS permission…" :
    store.micPermission === "granted" ? "Access granted." :
    store.micPermission === "denied" ? "Access is denied. Open System Settings, enable Intervox, then return here." :
    store.micPermission === "restricted" ? "Microphone access is restricted by a system policy." :
    "Required to translate your voice."
  );

  const micPermissionLabel = $derived(
    store.micPermission === "granted" ? "Allowed" :
    store.micPermission === "denied" ? "Denied" :
    store.micPermission === "restricted" ? "Restricted" :
    "Not allowed"
  );

  async function requestMicrophone() {
    micError = null;
    requestingMic = true;
    try {
      await store.requestMicPermission();
    } catch (e: unknown) {
      micError = (e as { message?: string })?.message ?? "Couldn't check microphone access.";
    } finally {
      requestingMic = false;
      await store.refreshMicPermission();
    }
  }

  $effect(() => {
    if (step === 2) void store.refreshMicPermission();
  });

  onMount(() => {
    const refresh = () => {
      if (store.onboardingOpen && step === 2) void store.refreshMicPermission();
    };
    const refreshWhenVisible = () => {
      if (!document.hidden) refresh();
    };
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  });

  // ── Step 3: Driver install ───────────────────────────────────
  async function installDriver() {
    driverAttempted = true;
    driverLoading = true;
    try {
      await store.installVirtualMic();
    } finally {
      driverLoading = false;
    }
    // store.lastError will be set on failure — component reads it honestly
  }

  // Step 4 uses a dedicated input-level probe because the live engine can
  // still be in Silence mode while selecting the source microphone.
  $effect(() => {
    if (step !== 4) return;
    void store.startMicLevelProbe();
    return () => {
      void store.stopMicLevelProbe();
    };
  });

  // Step 6 is a real end-to-end translation test, so it must run the live
  // translate mode rather than the level-only probe.
  $effect(() => {
    if (step !== 6) return;
    void store.setMode("translate");
  });

  // Derived test stage from real store state (no setTimeout)
  const testStage = $derived(
    store.tgtText ? "done" :
    store.srcText ? "translating" :
    store.audioInputDetected ? "heard" :
    "listen"
  );

  // ── Meeting meta ─────────────────────────────────────────────
  const meetingMeta: Record<string, { name: string; hint: string }> = {
    zoom:  { name: "Zoom",              hint: "Audio → Microphone" },
    meet:  { name: "Google Meet",       hint: "Settings → Audio → Microphone" },
    teams: { name: "Microsoft Teams",   hint: "Settings → Devices → Microphone" },
  };

  // Source mic name for meeting step
  const sourceMicName = $derived(
    store.devices.inputs.find((d) => d.id === store.config?.audio.source_mic_id)?.name ?? "Your microphone"
  );
</script>

{#if store.onboardingOpen}
  <!-- Backdrop -->
  <div
    data-onboarding
    style={css({
      position: "absolute",
      inset: 0,
      zIndex: 200,
      display: "grid",
      placeItems: "center",
      background: "rgba(0,0,0,0.18)",
      backdropFilter: "blur(2px)",
      animation: "pop-in 200ms ease-out both",
    })}
  >
    <!-- Sheet -->
    <div
      class="window"
      style={css({
        position: "relative",
        width: 780,
        height: 520,
        display: "flex",
        borderRadius: 14,
        animation: "pop-in 220ms cubic-bezier(.2,.9,.3,1.1) both",
      })}
    >
      <!-- Sidebar: step rail -->
      <div style={css({
        width: 220,
        background: "var(--sidebar-bg)",
        borderRight: "0.5px solid var(--hairline)",
        display: "flex",
        flexDirection: "column",
        padding: "0 0 14px",
      })}>
        <div class="traffic">
          <button
            class="dot close"
            aria-label="Close setup"
            onclick={() => store.setOnboardingOpen(false)}
          ></button>
          <span class="dot min"></span>
          <span class="dot max"></span>
        </div>

        <!-- Sidebar header glyph block -->
        <div style={css({ padding: "10px 16px 16px" })}>
          <div style={css({ display: "flex", alignItems: "center", gap: 10 })}>
            <div style={css({
              width: 32,
              height: 32,
              borderRadius: 8,
              background: "linear-gradient(135deg, color-mix(in oklch, var(--c-mixed) 75%, white) 0%, var(--c-mixed) 100%)",
              display: "grid",
              placeItems: "center",
              boxShadow: "0 4px 12px -2px color-mix(in oklch, var(--c-mixed) 60%, transparent)",
            })}>
              <Glyph size={18} color="#fff" />
            </div>
            <div>
              <div style={css({ fontSize: 13, fontWeight: 600 })}>Intervox</div>
              <div style={css({ fontSize: 11, color: "var(--txt-3)" })}>First-run setup</div>
            </div>
          </div>
        </div>

        <!-- Step rail rows -->
        <div style={css({ padding: "4px 8px", display: "flex", flexDirection: "column", gap: 2 })}>
          {#each steps as s, i (s.id)}
            <div style={css({
              display: "flex",
              alignItems: "center",
              gap: 10,
              padding: "7px 10px",
              borderRadius: 6,
              background: step === i ? "var(--c-mixed)" : "transparent",
              color: step === i ? "#fff" : "var(--txt-1)",
              fontSize: 12.5,
            })}>
              <span style={css({
                width: 18,
                height: 18,
                borderRadius: "50%",
                background: i < step ? "var(--c-translate)" : (step === i ? "rgba(255,255,255,0.25)" : "rgba(120,120,128,0.18)"),
                color: step === i ? "#fff" : "var(--txt-2)",
                display: "grid",
                placeItems: "center",
                fontSize: 11,
                fontWeight: 600,
                flexShrink: 0,
              })}>
                {#if i < step}
                  <Check size={10} color="#fff" />
                {:else}
                  {i + 1}
                {/if}
              </span>
              <span>{s.label}</span>
            </div>
          {/each}
        </div>

        <div style={css({ marginTop: "auto", padding: "0 16px", fontSize: 11, color: "var(--txt-3)" })}>
          You can re-run setup anytime from the menu bar.
        </div>
      </div>

      <!-- Content area -->
      <div style={css({ flex: 1, display: "flex", flexDirection: "column" })}>
        <div class="traffic" style={css({ visibility: "hidden" })}></div>

        <div style={css({ flex: 1, padding: "10px 48px 0", overflow: "auto" })}>

          <!-- ── Step 0: Welcome ──────────────────────────── -->
          {#if step === 0}
            <!-- StepTitle -->
            <div style={css({ marginTop: 28, marginBottom: 24 })}>
              <div style={css({ fontSize: 11, fontWeight: 600, letterSpacing: 0.6,
                                textTransform: "uppercase", color: "var(--c-mixed)",
                                marginBottom: 8 })}>Welcome</div>
              <h1 style={css({ fontSize: 24, fontWeight: 600, margin: 0, lineHeight: 1.2, letterSpacing: -0.2 })}>
                Speak in your language.
              </h1>
              <p style={css({ fontSize: 14, color: "var(--txt-2)", marginTop: 8, lineHeight: 1.5, maxWidth: 460 })}>
                Meetings hear the translation.
              </p>
            </div>
            <!-- Example cards -->
            <div style={css({ marginTop: 32, display: "flex", gap: 16, alignItems: "stretch" })}>
              <div class="card" style={css({ flex: 1, padding: 16, display: "flex", flexDirection: "column", gap: 8 })}>
                <span class="zh" style={css({ fontSize: 11, color: "var(--txt-3)", letterSpacing: 0.4, textTransform: "uppercase", fontWeight: 600 })}>You speak</span>
                <span class="zh" style={css({ fontSize: 18, fontWeight: 500 })}>我覺得這個功能下週可以開始實作。</span>
                <span style={css({ fontSize: 11.5, color: "var(--txt-3)" })}>Source: Auto-detected</span>
              </div>
              <div style={css({ display: "grid", placeItems: "center", padding: "0 4px", color: "var(--c-mixed)" })}>
                <svg width="22" height="14" viewBox="0 0 22 14" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M2 7h17M14 2l5 5-5 5"/>
                </svg>
              </div>
              <div class="card" style={css({
                flex: 1, padding: 16, display: "flex", flexDirection: "column", gap: 8,
                background: "color-mix(in oklch, var(--c-mixed) 12%, var(--card-bg))",
                borderColor: "color-mix(in oklch, var(--c-mixed) 30%, var(--card-border))",
              })}>
                <span style={css({ fontSize: 11, color: "var(--c-mixed)", letterSpacing: 0.4, textTransform: "uppercase", fontWeight: 600 })}>Meeting hears</span>
                <span style={css({ fontSize: 18, fontWeight: 500 })}>I think we can start implementing this feature next week.</span>
                <span style={css({ fontSize: 11.5, color: "var(--txt-3)" })}>Target: English · live translation</span>
              </div>
            </div>
            <div style={css({ marginTop: 24, fontSize: 12.5, color: "var(--txt-3)", lineHeight: 1.5 })}>
              Setup takes about a minute. We'll ask for microphone access, install a virtual mic,
              and confirm what to translate.
            </div>

          <!-- ── Step 1: API Key ───────────────────────────── -->
          {:else if step === 1}
            <!-- StepTitle -->
            <div style={css({ marginTop: 28, marginBottom: 24 })}>
              <div style={css({ fontSize: 11, fontWeight: 600, letterSpacing: 0.6,
                                textTransform: "uppercase", color: "var(--c-mixed)",
                                marginBottom: 8 })}>Bring Your Own Key</div>
              <h1 style={css({ fontSize: 24, fontWeight: 600, margin: 0, lineHeight: 1.2, letterSpacing: -0.2 })}>
                Connect your OpenAI API key
              </h1>
              <p style={css({ fontSize: 14, color: "var(--txt-2)", marginTop: 8, lineHeight: 1.5, maxWidth: 460 })}>
                Intervox runs translation through OpenAI's Realtime Translation. You use your own key — usage is billed directly to your OpenAI account, and nothing routes through our servers.
              </p>
            </div>

            <div class="card" style={css({ padding: 18 })}>
              <div style={css({ fontSize: 11, fontWeight: 600, color: "var(--txt-3)",
                                letterSpacing: 0.5, textTransform: "uppercase", marginBottom: 8 })}>
                OpenAI API Key
              </div>
              <!-- Input row -->
              <div style={css({
                display: "flex", alignItems: "center", gap: 8,
                padding: "6px 6px 6px 12px",
                background: "var(--control-bg)",
                border: `1px solid ${keyError ? "var(--c-error)" : store.account.verified ? "color-mix(in oklch, var(--c-translate) 45%, transparent)" : "var(--control-border)"}`,
                borderRadius: 8,
              })}>
                <span style={css({ color: "var(--txt-3)" })}>
                  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
                    <rect x="2.5" y="7.5" width="11" height="6" rx="1.5"/>
                    <path d="M5 7.5V5a3 3 0 1 1 6 0v2.5"/>
                  </svg>
                </span>
                <input
                  type={show ? "text" : "password"}
                  value={draft}
                  oninput={(e) => { draft = (e.target as HTMLInputElement).value; keyError = null; }}
                  placeholder="sk-…"
                  spellcheck={false}
                  autocomplete="off"
                  disabled={verifying}
                  style={css({
                    flex: 1, border: "none", outline: "none",
                    background: "transparent",
                    fontFamily: "ui-monospace, SF Mono, Menlo, monospace",
                    fontSize: 13, padding: "5px 0",
                    color: "var(--txt-1)",
                  })}
                />
                <button class="btn ghost"
                        onclick={() => (show = !show)}
                        style={css({ padding: "3px 8px", fontSize: 11.5, color: "var(--txt-3)" })}>
                  {show ? "Hide" : "Show"}
                </button>
                {#if store.account.verified}
                  <button class="btn" onclick={clearKey}
                          style={css({ padding: "4px 10px", fontSize: 12 })}>Replace</button>
                {:else}
                  <button class="btn primary"
                          onclick={verify}
                          disabled={!draft || verifying}
                          style={css({ padding: "5px 12px", fontSize: 12.5, opacity: draft ? 1 : 0.5 })}>
                    {verifying ? "Verifying…" : "Verify"}
                  </button>
                {/if}
              </div>

              <!-- Verification result -->
              {#if keyError}
                <div style={css({ marginTop: 10, display: "flex", alignItems: "flex-start", gap: 8,
                                  fontSize: 12, color: "var(--c-error)", lineHeight: 1.45 })}>
                  <SysIcon name="warn" size={13} /><span>{keyError}</span>
                </div>
              {/if}
              {#if store.account.verified}
                <div style={css({ marginTop: 10, display: "flex", alignItems: "center", gap: 8,
                                  fontSize: 12, color: "var(--c-translate)", fontWeight: 500 })}>
                  <SysIcon name="ok" size={13} />
                  <span>Key verified — billed to your OpenAI account.</span>
                </div>
              {/if}

              <!-- KeyPoint list -->
              <div style={css({ marginTop: 14, paddingTop: 14, borderTop: "0.5px solid var(--hairline)",
                                display: "flex", flexDirection: "column", gap: 8 })}>
                <!-- KeyPoint: lock -->
                <div style={css({ display: "flex", gap: 9, alignItems: "flex-start", fontSize: 12, color: "var(--txt-2)", lineHeight: 1.45 })}>
                  <span style={css({ color: "var(--txt-3)", marginTop: 2 })}>
                    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round">
                      <rect x="3" y="7.5" width="10" height="6" rx="1.4"/>
                      <path d="M5.5 7.5V5a2.5 2.5 0 0 1 5 0v2.5" stroke-linecap="round"/>
                    </svg>
                  </span>
                  <span>Stored in the local Intervox config file. Never sent to Intervox servers.</span>
                </div>
                <!-- KeyPoint: card -->
                <div style={css({ display: "flex", gap: 9, alignItems: "flex-start", fontSize: 12, color: "var(--txt-2)", lineHeight: 1.45 })}>
                  <span style={css({ color: "var(--txt-3)", marginTop: 2 })}>
                    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round">
                      <rect x="2" y="4" width="12" height="8" rx="1.4"/>
                      <path d="M2 7h12M4.5 10h2" stroke-linecap="round"/>
                    </svg>
                  </span>
                  <span>Realtime Translation is roughly ~$0.034 per active minute on your OpenAI bill.</span>
                </div>
                <!-- KeyPoint: link -->
                <div style={css({ display: "flex", gap: 9, alignItems: "flex-start", fontSize: 12, color: "var(--txt-2)", lineHeight: 1.45 })}>
                  <span style={css({ color: "var(--txt-3)", marginTop: 2 })}>
                    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M6 10a2.5 2.5 0 0 1 0-4l2-2a2.5 2.5 0 1 1 3.5 3.5l-1 1"/>
                      <path d="M10 6a2.5 2.5 0 0 1 0 4l-2 2a2.5 2.5 0 1 1-3.5-3.5l1-1"/>
                    </svg>
                  </span>
                  <span>Don't have a key? <a href="https://platform.openai.com/api-keys"
                        style={css({ color: "var(--c-mixed)", textDecoration: "underline", textUnderlineOffset: 2, cursor: "pointer" })}
                        onclick={(e) => { e.preventDefault(); store.openExternalUrl("https://platform.openai.com/api-keys"); }}>
                        Create one at platform.openai.com</a>.</span>
                </div>
              </div>
            </div>

          <!-- ── Step 2: Microphone permission ─────────────── -->
          {:else if step === 2}
            <!-- StepTitle -->
            <div style={css({ marginTop: 28, marginBottom: 24 })}>
              <div style={css({ fontSize: 11, fontWeight: 600, letterSpacing: 0.6,
                                textTransform: "uppercase", color: "var(--c-mixed)",
                                marginBottom: 8 })}>Permission</div>
              <h1 style={css({ fontSize: 24, fontWeight: 600, margin: 0, lineHeight: 1.2, letterSpacing: -0.2 })}>
                Allow microphone access
              </h1>
              <p style={css({ fontSize: 14, color: "var(--txt-2)", marginTop: 8, lineHeight: 1.5, maxWidth: 460 })}>
                Intervox needs to hear you so it can translate your voice. Your audio is only used to generate the translated speech and captions.
              </p>
            </div>
            <div class="card" style={css({ padding: 20, display: "flex", gap: 16, alignItems: "center" })}>
              <div style={css({
                width: 56, height: 56, borderRadius: 14,
                background: store.micPermission === "granted" ? "color-mix(in oklch, var(--c-translate) 18%, transparent)" : "rgba(120,120,128,0.14)",
                display: "grid", placeItems: "center",
                color: store.micPermission === "granted" ? "var(--c-translate)" : "var(--txt-2)",
              })}>
                <SysIcon name="mic" size={28} />
              </div>
              <div style={css({ flex: 1 })}>
                <div style={css({ fontSize: 14, fontWeight: 500 })}>Microphone access</div>
                <div style={css({ fontSize: 12.5, color: "var(--txt-3)", marginTop: 2 })}>
                  {micPermissionCopy}
                </div>
                {#if micError}
                  <div style={css({ fontSize: 12, color: "var(--c-error)", marginTop: 6 })}>{micError}</div>
                {/if}
              </div>
              {#if store.micPermission === "granted"}
                <span style={css({ display: "flex", alignItems: "center", gap: 6, color: "var(--c-translate)", fontSize: 13, fontWeight: 500 })}>
                  <SysIcon name="ok" size={14} /> {micPermissionLabel}
                </span>
              {:else}
                <div style={css({ display: "flex", gap: 8, alignItems: "center", justifyContent: "flex-end", flexWrap: "wrap" })}>
                  <span style={css({ display: "flex", alignItems: "center", gap: 6, color: "var(--txt-2)", fontSize: 13, fontWeight: 500 })}>
                    <SysIcon name="warn" size={14} /> {micPermissionLabel}
                  </span>
                  {#if store.micPermission === "notDetermined"}
                    <button class="btn primary" disabled={requestingMic} onclick={requestMicrophone}>
                      {requestingMic ? "Requesting…" : "Allow Microphone"}
                    </button>
                  {:else if store.micPermission === "denied"}
                    <button class="btn primary" onclick={async () => { await store.openMicPermission(); }}>
                      Open Settings
                    </button>
                  {/if}
                  {#if store.micPermission !== "restricted"}
                    <button class="btn" onclick={() => store.refreshMicPermission()}>
                      Check Again
                    </button>
                  {/if}
                </div>
              {/if}
            </div>

          <!-- ── Step 3: Virtual Mic / Driver ──────────────── -->
          {:else if step === 3}
            <!-- StepTitle -->
            <div style={css({ marginTop: 28, marginBottom: 24 })}>
              <div style={css({ fontSize: 11, fontWeight: 600, letterSpacing: 0.6,
                                textTransform: "uppercase", color: "var(--c-mixed)",
                                marginBottom: 8 })}>Virtual Mic</div>
              <h1 style={css({ fontSize: 24, fontWeight: 600, margin: 0, lineHeight: 1.2, letterSpacing: -0.2 })}>
                Install Translator Mic
              </h1>
              <p style={css({ fontSize: 14, color: "var(--txt-2)", marginTop: 8, lineHeight: 1.5, maxWidth: 460 })}>
                This adds a virtual microphone that meeting apps can pick up. It's how Zoom, Google Meet, or Teams hear the translated voice instead of you directly.
              </p>
            </div>
            {@const installed = store.status?.virtualMicInstalled === true}
            <div class="card" style={css({ padding: 20 })}>
              <div style={css({ display: "flex", gap: 16, alignItems: "center", marginBottom: 14 })}>
                <div style={css({
                  width: 56, height: 56, borderRadius: 14,
                  background: installed ? "color-mix(in oklch, var(--c-translate) 18%, transparent)" : "color-mix(in oklch, var(--c-mixed) 14%, transparent)",
                  display: "grid", placeItems: "center",
                  color: installed ? "var(--c-translate)" : "var(--c-mixed)",
                })}>
                  <SysIcon name="speaker" size={28} />
                </div>
                <div style={css({ flex: 1 })}>
                  <div style={css({ fontSize: 14, fontWeight: 500 })}>Translator Mic</div>
                  <div style={css({ fontSize: 12.5, color: "var(--txt-3)", marginTop: 2 })}>
                    {installed ? "Installed and ready" : "Audio driver · virtual microphone"}
                  </div>
                </div>
                {#if installed}
                  <span style={css({ display: "flex", alignItems: "center", gap: 6, color: "var(--c-translate)", fontSize: 13, fontWeight: 500 })}>
                    <SysIcon name="ok" size={14} /> Installed
                  </span>
                {:else}
                  <button class="btn primary" onclick={installDriver} disabled={driverLoading}>
                    {driverLoading ? "Installing…" : "Install Translator Mic"}
                  </button>
                {/if}
              </div>
              {#if !installed}
                <div style={css({
                  fontSize: 12, color: "var(--txt-3)", lineHeight: 1.5,
                  background: "rgba(120,120,128,0.06)",
                  border: "0.5px solid var(--hairline)",
                  borderRadius: 8, padding: "9px 12px",
                })}>
                  {#if driverAttempted && store.lastError}
                    {store.lastError.message} — The driver isn't available yet; you can skip this step and install it later.
                  {:else}
                    macOS may ask for administrator permission to install the virtual microphone.
                    This is required for meeting apps to see "Translator Mic" as an audio device.
                  {/if}
                </div>
              {/if}
            </div>

          <!-- ── Step 4: Source Mic ─────────────────────────── -->
          {:else if step === 4}
            <!-- StepTitle -->
            <div style={css({ marginTop: 28, marginBottom: 24 })}>
              <div style={css({ fontSize: 11, fontWeight: 600, letterSpacing: 0.6,
                                textTransform: "uppercase", color: "var(--c-mixed)",
                                marginBottom: 8 })}>Source</div>
              <h1 style={css({ fontSize: 24, fontWeight: 600, margin: 0, lineHeight: 1.2, letterSpacing: -0.2 })}>
                Which microphone do you want to translate?
              </h1>
              <p style={css({ fontSize: 14, color: "var(--txt-2)", marginTop: 8, lineHeight: 1.5, maxWidth: 460 })}>
                Pick the mic Intervox should listen to. We'll show the input level so you know it's hearing you.
              </p>
            </div>
            <div class="card" style={css({ padding: 6 })}>
              {#if store.devices.inputs.length === 0}
                <div style={css({
                  display: "flex", alignItems: "center", gap: 12,
                  padding: "10px 12px",
                  borderRadius: 7,
                  color: "var(--txt-3)",
                  fontSize: 13,
                })}>
                  No input devices found.
                </div>
              {:else}
                {#each store.devices.inputs as m (m.id)}
                  {@const selected = store.config?.audio.source_mic_id === m.id}
                  <div onclick={() => store.setSourceMic(m.id)}
                       role="radio"
                       aria-checked={selected}
                       tabindex="0"
                       onkeydown={(e) => (e.key === "Enter" || e.key === " ") && store.setSourceMic(m.id)}
                       style={css({
                         display: "flex", alignItems: "center", gap: 12,
                         padding: "10px 12px",
                         borderRadius: 7,
                         cursor: "pointer",
                         background: selected ? "color-mix(in oklch, var(--c-mixed) 14%, transparent)" : "transparent",
                         border: selected ? "1px solid color-mix(in oklch, var(--c-mixed) 35%, transparent)" : "1px solid transparent",
                       })}>
                    <span style={css({
                      width: 18, height: 18, borderRadius: "50%",
                      border: `1.5px solid ${selected ? "var(--c-mixed)" : "var(--txt-3)"}`,
                      display: "grid", placeItems: "center",
                    })}>
                      {#if selected}<Dot size={8} color="var(--c-mixed)" />{/if}
                    </span>
                    <span style={css({ color: "var(--txt-2)" })}><SysIcon name="mic" size={15} /></span>
                    <span style={css({ fontWeight: 500, fontSize: 13.5 })}>{m.name}</span>
                    <div style={css({ marginLeft: "auto", width: 110 })}>
                      <VUStrip level={selected ? store.inputLevel : 0} color="var(--c-translate)" />
                    </div>
                  </div>
                {/each}
              {/if}
            </div>

          <!-- ── Step 5: Target Language ────────────────────── -->
          {:else if step === 5}
            <!-- StepTitle -->
            <div style={css({ marginTop: 28, marginBottom: 24 })}>
              <div style={css({ fontSize: 11, fontWeight: 600, letterSpacing: 0.6,
                                textTransform: "uppercase", color: "var(--c-mixed)",
                                marginBottom: 8 })}>Target</div>
              <h1 style={css({ fontSize: 24, fontWeight: 600, margin: 0, lineHeight: 1.2, letterSpacing: -0.2 })}>
                Translate my speech to:
              </h1>
              <p style={css({ fontSize: 14, color: "var(--txt-2)", marginTop: 8, lineHeight: 1.5, maxWidth: 460 })}>
                You can change this anytime from the menu bar.
              </p>
            </div>
            <div class="card" style={css({ padding: 6 })}>
              {#each COMMON_LANGS as l (l.code)}
                {@const selected = store.targetLang.code === l.code}
                <div onclick={() => store.setTargetLang(l.code)}
                     role="radio"
                     aria-checked={selected}
                     tabindex="0"
                     onkeydown={(e) => (e.key === "Enter" || e.key === " ") && store.setTargetLang(l.code)}
                     style={css({
                       display: "flex", alignItems: "center", gap: 12,
                       padding: "10px 12px",
                       borderRadius: 7,
                       cursor: "pointer",
                       background: selected ? "color-mix(in oklch, var(--c-mixed) 14%, transparent)" : "transparent",
                       border: selected ? "1px solid color-mix(in oklch, var(--c-mixed) 35%, transparent)" : "1px solid transparent",
                     })}>
                  <span style={css({
                    width: 18, height: 18, borderRadius: "50%",
                    border: `1.5px solid ${selected ? "var(--c-mixed)" : "var(--txt-3)"}`,
                    display: "grid", placeItems: "center",
                  })}>
                    {#if selected}<Dot size={8} color="var(--c-mixed)" />{/if}
                  </span>
                  <LangChip code={l.code} size={20} />
                  <span style={css({ fontWeight: 500, fontSize: 13.5 })}>{l.name}</span>
                </div>
              {/each}
            </div>

          <!-- ── Step 6: Test ──────────────────────────────── -->
          {:else if step === 6}
            <!-- StepTitle -->
            <div style={css({ marginTop: 28, marginBottom: 24 })}>
              <div style={css({ fontSize: 11, fontWeight: 600, letterSpacing: 0.6,
                                textTransform: "uppercase", color: "var(--c-mixed)",
                                marginBottom: 8 })}>Test</div>
              <h1 style={css({ fontSize: 24, fontWeight: 600, margin: 0, lineHeight: 1.2, letterSpacing: -0.2 })}>
                Say something in your source language.
              </h1>
              <p style={css({ fontSize: 14, color: "var(--txt-2)", marginTop: 8, lineHeight: 1.5, maxWidth: 460 })}>
                The source language is detected automatically. The translation will appear below.
              </p>
            </div>
            <div class="card" style={css({ padding: 18, display: "flex", flexDirection: "column", gap: 14 })}>
              <div style={css({ display: "flex", alignItems: "center", gap: 12 })}>
                <div style={css({
                  width: 44, height: 44, borderRadius: 12,
                  background: testStage === "done" || testStage === "heard"
                    ? "color-mix(in oklch, var(--c-translate) 18%, transparent)"
                    : "color-mix(in oklch, var(--c-mixed) 14%, transparent)",
                  color: testStage === "done" || testStage === "heard" ? "var(--c-translate)" : "var(--c-mixed)",
                  display: "grid", placeItems: "center",
                })}>
                  <SysIcon name="mic" size={22} />
                </div>
                <div style={css({ flex: 1 })}>
                  <div style={css({ fontSize: 13, fontWeight: 500 })}>
                    {testStage === "listen" ? "Listening…" :
                     testStage === "heard" ? "Audio input detected." :
                     testStage === "translating" ? `Translating to ${store.targetLang.name}…` :
                     "Heard you loud and clear."}
                  </div>
                  <div style={css({ fontSize: 11.5, color: "var(--txt-3)" })}>
                    Source: {store.devices.inputs.find((d) => d.id === store.config?.audio.source_mic_id)?.name ?? "No input device"}
                  </div>
                </div>
                <div style={css({ width: 140 })}>
                  <VUStrip
                    level={store.inputLevel}
                    color={testStage === "done" ? "var(--c-translate)" : "var(--c-mixed)"}
                  />
                </div>
              </div>
              <!-- Source text -->
              <div class="zh" style={css({
                fontSize: 16, padding: "10px 12px",
                background: "rgba(120,120,128,0.08)",
                borderRadius: 8,
                color: testStage === "listen" ? "var(--txt-3)" : "var(--txt-1)",
              })}>
                {store.srcText || "等你說話…"}
              </div>
              <!-- Target text -->
              <div style={css({
                fontSize: 16,
                padding: "10px 12px",
                background: "color-mix(in oklch, var(--c-mixed) 8%, transparent)",
                borderRadius: 8,
                color: testStage === "done" ? "var(--txt-1)" : "var(--txt-3)",
                fontWeight: 500,
              })}>
                {store.tgtText || "Waiting for translation…"}
              </div>
              {#if store.tgtText}
                <div style={css({ display: "flex", alignItems: "center", gap: 6, color: "var(--c-translate)", fontSize: 12.5, fontWeight: 500 })}>
                  <SysIcon name="ok" size={14} /> Working — translation received.
                </div>
              {:else if store.audioInputDetected}
                <div style={css({ color: "var(--txt-3)", fontSize: 12.5 })}>
                  Keep speaking until translated text appears.
                </div>
              {/if}
            </div>

          <!-- ── Step 7: Meeting Setup ──────────────────────── -->
          {:else if step === 7}
            <!-- StepTitle -->
            <div style={css({ marginTop: 28, marginBottom: 24 })}>
              <div style={css({ fontSize: 11, fontWeight: 600, letterSpacing: 0.6,
                                textTransform: "uppercase", color: "var(--c-mixed)",
                                marginBottom: 8 })}>Final Step</div>
              <h1 style={css({ fontSize: 24, fontWeight: 600, margin: 0, lineHeight: 1.2, letterSpacing: -0.2 })}>
                In your meeting app, pick Translator Mic
              </h1>
              <p style={css({ fontSize: 14, color: "var(--txt-2)", marginTop: 8, lineHeight: 1.5, maxWidth: 460 })}>
                That's the virtual mic Intervox installed. It carries the translated voice.
              </p>
            </div>

            <!-- Meeting app tabs -->
            <div style={css({ display: "flex", gap: 8, marginBottom: 14 })}>
              {#each Object.entries(meetingMeta) as [key, m] (key)}
                <button
                  class={"btn " + (meetingApp === key ? "primary" : "")}
                  onclick={() => (meetingApp = key as typeof meetingApp)}
                >
                  {m.name}
                </button>
              {/each}
            </div>

            <div class="card" style={css({ padding: 16 })}>
              <div style={css({ fontSize: 11.5, color: "var(--txt-3)", marginBottom: 8 })}>
                {meetingMeta[meetingApp].name}  ·  {meetingMeta[meetingApp].hint}
              </div>
              <!-- Fake picker (illustrative instructional UI) -->
              <div style={css({
                padding: 10, borderRadius: 8,
                background: "color-mix(in oklch, var(--c-mixed) 8%, var(--card-bg))",
                border: "0.5px solid var(--card-border)",
              })}>
                <div style={css({ fontSize: 11, color: "var(--txt-3)", marginBottom: 6 })}>Microphone</div>
                <div style={css({ display: "flex", flexDirection: "column", gap: 4 })}>
                  <!-- FakeMicRow: source mic (dimmed) -->
                  <div style={css({
                    display: "flex", alignItems: "center", gap: 10,
                    padding: "8px 10px",
                    borderRadius: 6,
                    background: "transparent",
                    color: "var(--txt-3)",
                    border: "0.5px solid transparent",
                    fontSize: 12.5,
                  })}>
                    <span style={css({
                      width: 14, height: 14, borderRadius: 3,
                      border: "1.5px solid var(--txt-3)",
                      background: "transparent",
                      display: "grid", placeItems: "center", color: "#fff",
                    })}></span>
                    <span>{sourceMicName}</span>
                  </div>
                  <!-- FakeMicRow: Translator Mic (selected) -->
                  <div style={css({
                    display: "flex", alignItems: "center", gap: 10,
                    padding: "8px 10px",
                    borderRadius: 6,
                    background: "color-mix(in oklch, var(--c-mixed) 18%, transparent)",
                    color: "var(--txt-1)",
                    border: "0.5px solid color-mix(in oklch, var(--c-mixed) 35%, transparent)",
                    fontSize: 12.5,
                  })}>
                    <span style={css({
                      width: 14, height: 14, borderRadius: 3,
                      border: "1.5px solid var(--c-mixed)",
                      background: "var(--c-mixed)",
                      display: "grid", placeItems: "center", color: "#fff",
                    })}>
                      <Check size={9} color="#fff" />
                    </span>
                    <span>Translator Mic</span>
                    <span style={css({ marginLeft: "auto" })}>
                      <span style={css({ fontSize: 10.5, color: "var(--c-mixed)", fontWeight: 600 })}>Intervox</span>
                    </span>
                  </div>
                  <!-- FakeMicRow: AirPods (dimmed) -->
                  <div style={css({
                    display: "flex", alignItems: "center", gap: 10,
                    padding: "8px 10px",
                    borderRadius: 6,
                    background: "transparent",
                    color: "var(--txt-3)",
                    border: "0.5px solid transparent",
                    fontSize: 12.5,
                  })}>
                    <span style={css({
                      width: 14, height: 14, borderRadius: 3,
                      border: "1.5px solid var(--txt-3)",
                      background: "transparent",
                      display: "grid", placeItems: "center", color: "#fff",
                    })}></span>
                    <span>AirPods Pro Microphone</span>
                  </div>
                </div>
              </div>
              <div style={css({ fontSize: 12, color: "var(--txt-3)", marginTop: 12, lineHeight: 1.5 })}>
                Tip: keep your original mic selected as the *input device* in Intervox, and let
                the meeting app use Translator Mic. The two don't compete.
              </div>
            </div>
          {/if}
        </div>

        <!-- Footer -->
        <div style={css({
          display: "flex", alignItems: "center",
          padding: "12px 24px",
          borderTop: "0.5px solid var(--hairline)",
          background: "color-mix(in oklch, var(--win-bg-solid) 60%, transparent)",
        })}>
          <button
            class="btn ghost"
            style={css({ visibility: step === 0 ? "hidden" : "visible" })}
            onclick={back}
          >Back</button>
          <div style={css({ marginLeft: "auto", display: "flex", gap: 10, alignItems: "center" })}>
            {#if step < steps.length - 1}
              <button class="btn ghost"
                      onclick={() => { store.setOnboardingOpen(false); step = 0; }}>
                Skip setup
              </button>
            {/if}
            <button
              class={"btn " + (canNext ? "primary" : "")}
              disabled={!canNext}
              style={css({ opacity: canNext ? 1 : 0.5, cursor: canNext ? "pointer" : "not-allowed" })}
              onclick={next}
            >
              {step === 0 ? "Get Started" : step === steps.length - 1 ? "Done" : "Continue"}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}
