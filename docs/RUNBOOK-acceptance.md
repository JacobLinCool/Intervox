# Intervox Manual Acceptance Runbook

**Purpose.** This runbook is the definitive gate for the 12 Product Acceptance items listed in `docs/STATUS.md`. Executing it ONCE with real hardware is what flips those checkboxes. Completing Task 6.3 (marking them as verified) depends on running this runbook and recording the results in the table at the end.

**Scope note.** These checks are inherently manual. They require:
- A real OpenAI API key (BYOK) that incurs billable API usage.
- A physical microphone and human ears.
- Installed meeting apps (Zoom, Google Meet in a browser, QuickTime Player).
- Administrator access to install a kernel-extension-class audio driver.
- A person speaking into the mic and listening to what a meeting participant hears.

None of these can be emulated by a CI pipeline. Automated test suites (cargo test, pnpm test) cover unit and integration paths; this runbook covers end-to-end product correctness.

---

## Prerequisites

Before beginning, confirm everything on this list is available on the test Mac:

| Prerequisite | Notes |
|---|---|
| macOS 14 (Sonoma) or later | HAL AudioServerPlugIn requires Sonoma+ |
| Rust toolchain (stable) | `rustup show` should print a stable toolchain |
| Xcode Command Line Tools | `xcode-select -p` must succeed |
| `pnpm` | `pnpm --version` must succeed |
| `cmake` and `ninja` | Required by `scripts/build_driver.sh` |
| Apple Developer ID Application certificate | Required by `scripts/sign_driver.sh`. The identity embedded in `scripts/driver_env.sh` is `Developer ID Application: JHEN-KE LIN (5H75SGA7KP)`. If you are testing on a different signing identity, set `SIGN_IDENTITY` in the environment before running the sign script. |
| Apple notarytool keychain profile named `intervox-notary` | Required by `scripts/notarize_driver.sh`. Create once with: `xcrun notarytool store-credentials intervox-notary --apple-id <id> --team-id 5H75SGA7KP --password <app-specific-pw>`. If notarization was already done for the current driver build (and the bundle on disk is stapled), this step may be skipped and you can go directly to install. |
| Administrator password | Required by `scripts/install_driver.sh` (runs `sudo`) and by the in-app "Install Driver" button. |
| OpenAI API key (BYOK) | Must start with `sk-`. Cost is incurred during acceptance testing. |
| Physical microphone | Built-in or USB; must be selectable in System Settings > Sound. |
| Zoom desktop app | Installed and signed-in account ready (does not need to be in an active call for the smoke test; "Test Microphone" in Audio Settings is sufficient for a solo test). |
| Google Chrome or Safari | For Google Meet smoke test. A Google account is required. |
| QuickTime Player | Ships with macOS; no extra install needed. |

---

## Setup (perform once, in order)

### Step S1 — Install pnpm dependencies

```bash
cd /Users/jacoblincool/Documents/GitHub/Intervox
pnpm install
```

### Step S2 — Build the HAL driver

```bash
scripts/build_driver.sh
```

Expected output ends with `Built: <path>/Intervox.driver` and a passing `plutil -lint` line.

### Step S3 — Sign the driver

```bash
scripts/sign_driver.sh
```

Expected output shows `codesign --verify --strict --deep` with no errors and `Authority=Developer ID Application`.

### Step S4 — Notarize the driver (skip if already stapled)

```bash
scripts/notarize_driver.sh
```

This submits the bundle to Apple's notary service and waits. When it completes it staples the ticket. Expected final lines: `Notarized + stapled: .../Intervox.driver` and `validate action worked`.

> **Note:** Notarization is required for Gatekeeper to accept the driver. If you are testing a build where the driver was already notarized and stapled (verify with `xcrun stapler validate driver/build/Intervox.driver`), you may skip this step.

### Step S5 — Capture the app log to a file

Open a fresh terminal window and run:

```bash
cd /Users/jacoblincool/Documents/GitHub/Intervox
pnpm tauri dev 2>&1 | tee /tmp/intervox-acceptance.log
```

Leave this terminal open for the duration of testing. All app output (stdout + stderr) will be written to `/tmp/intervox-acceptance.log`. The Tauri dev window opens automatically.

> **Alternative (built app):** If testing a release binary, launch it from a terminal:
> ```bash
> /path/to/Intervox.app/Contents/MacOS/Intervox 2>&1 | tee /tmp/intervox-acceptance.log
> ```

### Step S6 — Install the Intervox HAL driver

**Preferred path — in-app install:**

1. In the Intervox app, click **Status** in the left sidebar.
2. If the "Translator Mic installed" row shows a warning icon, a **Driver Recovery** card appears at the bottom of the Status pane with an **Install Driver** button.
3. Click **Install Driver**. A macOS administrator password prompt appears.
4. Enter the admin password. The app calls `scripts/install_driver.sh` via `osascript` with `INTERVOX_ASSUME_YES=1`, which runs `sudo cp`, `sudo chown`, and `sudo killall coreaudiod`. All audio on the machine is briefly interrupted.
5. After ~2 seconds, the Status pane's "Translator Mic installed" row should show a green checkmark.

**Manual fallback (if the in-app button is not available):**

```bash
INTERVOX_ASSUME_YES=1 sudo bash scripts/install_driver.sh
```

Enter the admin password when prompted. The script prints `OK: 'Intervox' input device is registered.` on success.

**Verify the driver is visible:**

```bash
system_profiler SPAudioDataType | grep -A3 Intervox
```

Expected output includes `Intervox`, `1 ch`, `48000 Hz`, and `Virtual`.

### Step S7 — Enter and verify the BYOK key

1. In the Intervox app, click **Account** in the left sidebar.
2. The "OpenAI API Key" field group shows **Not connected**.
3. Paste your OpenAI API key (starting with `sk-`) into the text field.
4. Click **Verify & save**.
5. The app makes a real OpenAI network request to validate the key. On success the field group changes to show **Connected** with a green checkmark and the masked key.
6. If verification fails, re-check the key on `platform.openai.com` and retry.

### Step S8 — Select "Translator Mic" in each meeting app

Before running each app-specific smoke test (steps A10–A12), select the Intervox virtual microphone as the audio input in that app:

- **Zoom:** Settings > Audio > Microphone > select **Translator Mic**.
- **Google Meet:** gear icon > Audio > Microphone > select **Translator Mic**.
- **QuickTime Player:** File > New Audio Recording > microphone selector (the dropdown arrow next to the record button) > select **Translator Mic**.

---

## Acceptance Steps

Each step maps 1:1 to one item in the `docs/STATUS.md` "## Product Acceptance" section.

---

### A1 — Silence mode: meeting app hears silence and OpenAI is disconnected

**Action.**
1. In the Intervox app, go to the **Audio** pane (or the Status pane mode card).
2. Select the **Silence** mode card. The mode card reads "Silence — No audio is sent through Translator Mic." and the tray title updates to "Silence".
3. Alternatively, press **Cmd+Shift+M** to jump to Silence mode.
4. Wait 5 seconds.
5. In Zoom Audio Settings (or Meet audio settings), confirm the input meter for **Translator Mic** shows no movement.
6. In the Intervox **Status** pane, confirm the "Translation service idle" row (not "Translation service connected").

**Expected observation.**
- Intervox Status pane: Output meter shows `Silenced` with no activity.
- Meeting app input meter: flat / no signal.
- Status row: "Translation service idle" (OpenAI websocket is not open in Silence mode).

**PASS criterion.** The meeting app microphone level is continuously zero for a sustained speaking period AND the Status pane does NOT show "Translation service connected".

- [ ] **A1 PASS**

---

### A2 — Pass-through mode: meeting app hears original mic with low latency

**Action.**
1. Select the **Pass-through** mode card in the Audio pane (label: "Pass-through — Your original microphone audio is sent unchanged.") or click the tray menu item **Pass-Through**.
2. Speak into your microphone.
3. Listen to what the meeting app receives: ask a second participant, or use the meeting app's own "Test Microphone" playback feature.

**Expected observation.**
- The meeting app hears your voice as captured by your physical microphone, with no translation delay.
- The Intervox Status pane Output meter shows activity labeled "Original voice".
- Latency is subjectively low (comparable to using the microphone directly).
- No OpenAI connection is established (Status row: "Translation service idle — Pass-through does not use translation.").

**PASS criterion.** Your original voice is clearly audible in the meeting app with subjectively minimal latency, and no translation artifact is present.

- [ ] **A2 PASS**

---

### A3 — Translate mode: meeting app hears translated speech only

**Action.**
1. Select the **Translate** mode card (label: "Translate — Only translated speech is sent.") or the tray item **Translate**.
2. In the **Translation** pane, confirm the source language matches the language you will speak and the target language is the intended output.
3. Speak a sentence in the source language.
4. Listen to what the meeting app receives. It should be in the target language only.

**Expected observation.**
- The meeting app hears translated speech in the target language.
- Your original-language voice is NOT audible to the meeting (no leakage of the source mic signal).
- The Intervox Status pane shows "Translation service connected" and the latency badge displays a numeric value (e.g. `340 ms`).
- The Input meter shows activity; the Output meter shows activity labeled "Translated voice".

**PASS criterion.** The meeting app receives ONLY translated speech; no original-language audio leaks through, and the content is semantically correct relative to what was spoken.

- [ ] **A3 PASS**

---

### A4 — Translate + Original mode: meeting app hears translated speech with faint delayed original

**Action.**
1. Select the **Translate + Original** mode card (label: "Translate + Original — Translated speech is sent with your original voice quietly underneath.") or the tray item **Translate + Original**.
2. Speak a sentence in the source language.
3. Listen to what the meeting app receives.

**Expected observation.**
- The meeting app hears the translated speech prominently.
- A faint, delayed version of your original voice is audible underneath (default original volume is 15% per config default; default translated volume is 100%).
- The Intervox Status pane shows "Translation service connected".
- Both Input and Output meters are active.

**PASS criterion.** The translated voice is clearly dominant; the original-language voice is faintly audible underneath and time-delayed relative to the translated speech. Both signals are present but the original does not overwhelm the translation.

- [ ] **A4 PASS**

---

### A5 — Captions show live source and target text

**Action.**
1. With the app in **Translate** or **Translate + Original** mode and an active OpenAI session.
2. Open the dedicated captions window via one of:
   - **Captions** pane > "Pop-out captions window" toggle (enable the "Floating captions" toggle first, then toggle "Pop-out captions window").
   - Tray menu > **Captions**.
   - Global shortcut **Cmd+Shift+C**.
3. Speak a sentence in the source language.
4. Watch the captions window.

**Expected observation.**
- The always-on-top captions window is visible above all other windows.
- As you speak, the source-language transcript appears live (if "Show original captions" is enabled in the Captions pane).
- As translation completes, the target-language transcript appears live (if "Show translated captions" is enabled).
- Both transcripts update in realtime as the OpenAI stream delivers deltas.

**PASS criterion.** The captions window displays both source and target text live as speech is recognised and translated, without fabricating text during silence.

- [ ] **A5 PASS**

---

### A6 — App quit keeps the virtual mic device available and silent

**Action.**
1. With the app in any mode, confirm the meeting app shows "Translator Mic" as the selected input.
2. Quit Intervox via **Cmd+Q** or the tray menu **Quit Intervox**.
3. Do NOT close/reopen the meeting app.
4. Observe the meeting app's microphone input level and the microphone selector list.

**Expected observation.**
- The Intervox app process exits (the tray icon disappears).
- The "Translator Mic" entry remains selectable in the meeting app's microphone list (the HAL driver is still registered with CoreAudio; the shared-memory ring is still present on disk as `/intervox.ring`).
- The meeting app's microphone input level for "Translator Mic" is zero (the driver reads silence on underrun, which is the state when no ring producer is running).

**PASS criterion.** "Translator Mic" remains listed in the meeting app's microphone selector after Intervox quits, and the meeting app input level for it is zero.

- [ ] **A6 PASS**

---

### A7 — Driver restart and app restart recover without manual cleanup

**Action.**

**Driver restart test:**
1. With the app running and in any mode, run in a terminal:
   ```bash
   sudo killall coreaudiod
   ```
   Enter the admin password when prompted. All audio is briefly interrupted.
2. Wait 5 seconds.
3. In the Intervox Status pane, observe the "Translator Mic installed" row.
4. Switch modes and speak to confirm audio flows correctly again.

**App restart test:**
1. Quit Intervox (Cmd+Q or tray > Quit Intervox).
2. Relaunch: run `pnpm tauri dev` again in the terminal (or reopen the built app).
3. Without running any install or setup commands, confirm the Status pane shows the virtual mic as installed and that translation works.

**Expected observation.**
- After `killall coreaudiod`: CoreAudio restarts, the driver is reloaded, and within ~5 seconds (one polling cycle) the Intervox Status pane reflects the correct state with no user action.
- After app restart: the Status pane shows "Translator Mic installed" (green) because the driver was already installed on disk; translation resumes without reinstalling.

**PASS criterion.** Both driver restart and app restart recover to a working translation state without any manual driver reinstall or cleanup step.

- [ ] **A7 PASS**

---

### A8 — No raw audio or transcripts are logged by default

**Action.**
1. Use the log file captured in Step S5: `/tmp/intervox-acceptance.log`.
2. Run the following greps against the captured log while (or after) running a Translate session:

```bash
# Check for any transcript text fragment — look for common words in the log
grep -i "hello\|你好\|this is a test" /tmp/intervox-acceptance.log

# Check for audio-related data patterns (base64 blobs typically contain these)
grep -E "data:audio|audio/pcm|[A-Za-z0-9+/]{50,}={0,2}" /tmp/intervox-acceptance.log

# Check for the word "transcript" in log lines (should only appear in code-path labels, not content)
grep -i "transcript.*:" /tmp/intervox-acceptance.log
```

3. Review any hits. Log lines like `[realtime] connect error` or `[engine] failed to start capture: ...` are acceptable. Lines that contain the literal text of spoken phrases, base64-encoded audio payloads, or raw transcript content are failures.

**Expected observation.**
- The `grep` commands return no lines containing spoken transcript content.
- The `grep` commands return no lines containing audio payloads or base64 blobs.
- Code-path labels (`[realtime]`, `[capture]`, `[engine]`) appear only with structural error descriptions, not with audio content.

**PASS criterion.** None of the grep commands above produce output that contains actual spoken words, audio bytes, or transcript text. Structural error messages (URLs, error codes, retry counts) are acceptable.

- [ ] **A8 PASS**

---

### A9 — BYOK key is never written to logs

**Action.**
1. Using the same log file (`/tmp/intervox-acceptance.log`), run:

```bash
grep -i "sk-" /tmp/intervox-acceptance.log
```

2. Also check for partial key fragments (the first 12 characters of your key):

```bash
# Replace sk-xxxxxxxxxxxx with the first 12 chars of your actual key:
grep "sk-xxxxxxxxxxxx" /tmp/intervox-acceptance.log
```

**Expected observation.**
- Both greps return no output (empty result).
- The key is stored only in the macOS Keychain and is never written to stdout, stderr, or any file in the process's working directory.

**PASS criterion.** `grep -i 'sk-' /tmp/intervox-acceptance.log` returns nothing. The log file contains no fragment of the OpenAI API key.

- [ ] **A9 PASS**

---

### A10 — Zoom smoke test passes

**Prerequisite:** Zoom is installed, signed in, and "Translator Mic" is selected in Zoom Audio Settings.

**Action.** Perform the following checks inside Zoom (use Zoom's built-in "Test Microphone" feature in Settings > Audio, or join a test call / Zoom's echo test):

1. Set mode to **Silence**. Confirm Zoom input meter shows zero.
2. Set mode to **Pass-through**. Speak and confirm Zoom input meter shows activity and audio is clear.
3. Set mode to **Translate**. Speak in the source language. Confirm Zoom input meter shows activity and the audio received is in the target language only.
4. Set mode to **Translate + Original**. Speak in the source language. Confirm Zoom receives translated speech with faint original underneath.

**Expected observation.** All four mode behaviors from A1–A4 are reproduced with Zoom as the consumer of the Translator Mic input.

**PASS criterion.** Silence = zero level in Zoom; Pass-through = original voice in Zoom; Translate = translated-only voice in Zoom; Translate+Original = translated + quiet original in Zoom.

- [ ] **A10 PASS**

---

### A11 — Google Meet smoke test passes

**Prerequisite:** Google Chrome or Safari with a Google account. "Translator Mic" is selected in Meet's audio settings (gear icon > Audio > Microphone).

**Action.** Open a Google Meet call (a personal meeting room or an echo test). Perform the same four checks as A10:

1. Silence: Meet input meter is zero.
2. Pass-through: Meet hears original voice.
3. Translate: Meet hears translated-only voice.
4. Translate + Original: Meet hears translated with faint original.

**Expected observation.** Same as A10 but via Meet's WebRTC audio stack.

**PASS criterion.** All four modes behave correctly in Google Meet's microphone input.

- [ ] **A11 PASS**

---

### A12 — QuickTime/CoreAudio capture smoke test passes

**Prerequisite:** QuickTime Player is open. In the recording setup (File > New Audio Recording), the microphone dropdown (the arrow next to the record button) shows **Translator Mic** as the selected device.

**Action.**
1. Set mode to **Silence**. Press record. Observe the QuickTime level meter. Stop recording. Confirm the recorded audio is silent.
2. Set mode to **Pass-through**. Press record, speak for 5 seconds, stop. Play back the recording. Confirm your original voice is heard.
3. Set mode to **Translate**. Press record, speak for 5 seconds, stop. Play back. Confirm translated speech only is heard.
4. Set mode to **Translate + Original**. Press record, speak for 5 seconds, stop. Play back. Confirm translated speech with faint original.

**Expected observation.** The CoreAudio capture path (used by QuickTime) correctly reflects all four modes just as meeting apps do. This validates the HAL driver layer independently of meeting-app WebRTC.

**PASS criterion.** Silence recording is silent; Pass-through recording contains original voice; Translate recording contains translated-only voice; Translate+Original recording contains translated speech with faint original underneath.

- [ ] **A12 PASS**

---

## Result Recording

Fill in this table after completing all steps. Date, tester name, and SHA of the code under test should be recorded for auditability.

| Field | Value |
|---|---|
| Date | |
| Tester | |
| Git SHA (`git rev-parse HEAD`) | |
| OpenAI key prefix (first 8 chars, for audit) | sk-...... |
| Mac model / macOS version | |

| Step | Result (PASS / FAIL / SKIP) | Notes |
|---|---|---|
| A1 Silence | | |
| A2 Pass-through | | |
| A3 Translate | | |
| A4 Translate + Original | | |
| A5 Captions live | | |
| A6 App quit keeps vmic | | |
| A7 Driver + app restart | | |
| A8 No raw audio/transcripts logged | | |
| A9 BYOK key not in logs | | |
| A10 Zoom smoke test | | |
| A11 Google Meet smoke test | | |
| A12 QuickTime smoke test | | |

Once all 12 items show PASS, update `docs/STATUS.md` by checking all 12 Product Acceptance boxes (Task 6.3).

---

## Troubleshooting

### "Translator Mic" is not visible in Audio MIDI Setup or meeting apps

1. In the Intervox Status pane, look for the **Driver Recovery** card. Click **Open Audio MIDI Setup** to confirm whether the device is registered.
2. If the device is missing, use the **Install Driver** or **Reinstall** button in the Driver Recovery card to reinstall. You will be prompted for your admin password.
3. If the in-app buttons are not available, run manually:
   ```bash
   INTERVOX_ASSUME_YES=1 sudo bash scripts/install_driver.sh
   ```
4. After install, verify with:
   ```bash
   system_profiler SPAudioDataType | grep -A3 Intervox
   ```

### Microphone permission denied

1. In the Intervox **Audio** pane, if the "Microphone Permission" section shows "Access denied", click **Open Privacy Settings**.
2. In macOS System Settings > Privacy & Security > Microphone, enable the toggle for Intervox.
3. Relaunch Intervox.
4. The Audio pane's permission section should disappear (granted state). The Status pane's "Microphone permission granted" row should show a green checkmark.

### BYOK key verification fails

1. In the Intervox **Account** pane, the error reads "That doesn't look like an OpenAI key…" for format errors, or a network error for connectivity issues.
2. Confirm the key is active on `platform.openai.com` → API Keys.
3. Confirm the Mac has internet access and can reach `api.openai.com`.
4. Re-paste the key and click **Verify & save** again.

### Global shortcut not working (Cmd+Shift+T / Cmd+Shift+M / Cmd+Shift+C)

1. If another app has already claimed one of the default shortcuts, the Intervox **Status** pane or tray will show an error notification on startup.
2. Go to **Shortcuts** pane in Intervox Settings and assign a different key combination.
3. If the shortcut still fails after reassignment, check macOS System Settings > Privacy & Security > Accessibility; Intervox should not require Accessibility permission for global shortcuts (they use the standard `tauri-plugin-global-shortcut` path), but if macOS is blocking them, adding Intervox there may help.
4. As a fallback, use the tray menu or the in-app mode cards to switch modes manually.

### Driver signed but not notarized (Gatekeeper rejects it)

If `spctl -a -t install -vv driver/build/Intervox.driver` shows `rejected`, the driver is not notarized or the staple ticket is missing. Re-run:

```bash
scripts/notarize_driver.sh
```

Then reinstall via `scripts/install_driver.sh`.
