/* App root — composes every surface, wires up theme + Tweaks. */

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "theme": "light",
  "wallpaper": "lavender",
  "showHelperDock": true
}/*EDITMODE-END*/;

const WALLPAPERS = {
  lavender: "linear-gradient(155deg, #c9b8d6 0%, #a7b9d8 35%, #b9c6d8 70%, #d5cad3 100%)",
  sonoma:   "linear-gradient(160deg, #e8c2b8 0%, #d6a7c2 35%, #a8b3d4 70%, #b9c6dc 100%)",
  graphite: "linear-gradient(160deg, #d4d4d6 0%, #b8b8be 50%, #a0a0a8 100%)",
  ocean:    "linear-gradient(160deg, #5a9fc9 0%, #4a7ec0 50%, #6a5fb0 100%)",
};

const WALLPAPERS_DARK = {
  lavender: "linear-gradient(155deg, #2a1f3d 0%, #1a2238 35%, #18223a 70%, #2b1f33 100%)",
  sonoma:   "linear-gradient(160deg, #3a2530 0%, #2a1f3a 50%, #1a2238 100%)",
  graphite: "linear-gradient(160deg, #1f1f22 0%, #2a2a2e 50%, #18181b 100%)",
  ocean:    "linear-gradient(160deg, #15233b 0%, #1f2b4a 50%, #2a1f4a 100%)",
};

function App() {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);

  React.useEffect(() => {
    document.documentElement.setAttribute("data-theme", t.theme || "light");
    const pal = t.theme === "dark" ? WALLPAPERS_DARK : WALLPAPERS;
    document.body.style.background = pal[t.wallpaper] || pal.lavender;
  }, [t.theme, t.wallpaper]);

  return (
    <AppProvider initialTheme={t.theme || "light"}>
      <AppInner tweaks={t} setTweak={setTweak}/>
    </AppProvider>
  );
}

function AppInner({ tweaks, setTweak }) {
  const app = useApp();

  React.useEffect(() => {
    app.setTheme(tweaks.theme || "light");
  }, [tweaks.theme]);

  return (
    <div className="stage">
      <MenuBarStrip/>
      <MenuDropdown/>

      {app.settingsOpen && <SettingsWindow/>}
      {app.captionsOpen && !app.onboardingOpen && <FloatingCaptions/>}
      {app.onboardingOpen && <Onboarding/>}

      {tweaks.showHelperDock !== false && <HelperDock app={app}/>}

      <IntervoxTweaks tweaks={tweaks} setTweak={setTweak} app={app}/>
    </div>
  );
}

/* Small floating helper dock — prototype convenience for jumping between
   surfaces. Not part of the real macOS UI. */
function HelperDock({ app }) {
  const items = [
    { id: "menu",     label: "Menu Bar",   active: app.menuOpen,
      onClick: () => app.setMenuOpen(o => !o) },
    { id: "settings", label: "Settings",   active: app.settingsOpen,
      onClick: () => app.setSettingsOpen(o => !o) },
    { id: "captions", label: "Captions",   active: app.captionsOpen,
      onClick: () => app.setCaptionsOpen(o => !o) },
    { id: "onboard",  label: "Onboarding", active: app.onboardingOpen,
      onClick: () => app.setOnboardingOpen(o => !o) },
  ];
  return (
    <div className="helper-dock" role="toolbar" aria-label="Prototype surfaces">
      <span style={{ fontSize: 10.5, fontWeight: 600, color: "var(--txt-3)",
                     letterSpacing: 0.5, textTransform: "uppercase",
                     padding: "0 10px", display: "flex", alignItems: "center" }}>
        Surfaces
      </span>
      {items.map(it => (
        <button key={it.id} className={it.active ? "on" : ""} onClick={it.onClick}>
          {it.label}
        </button>
      ))}
    </div>
  );
}

function IntervoxTweaks({ tweaks, setTweak, app }) {
  return (
    <TweaksPanel title="Tweaks">
      <TweakSection label="Appearance">
        <TweakRadio label="Theme" value={tweaks.theme} options={["light","dark"]}
                    onChange={(v) => setTweak("theme", v)}/>
        <TweakSelect label="Wallpaper"
                     value={tweaks.wallpaper}
                     options={[
                       { value: "lavender", label: "Lavender" },
                       { value: "sonoma",   label: "Sonoma" },
                       { value: "graphite", label: "Graphite" },
                       { value: "ocean",    label: "Ocean" },
                     ]}
                     onChange={(v) => setTweak("wallpaper", v)}/>
        <TweakToggle label="Show surface dock"
                     value={tweaks.showHelperDock !== false}
                     onChange={(v) => setTweak("showHelperDock", v)}/>
      </TweakSection>

      <TweakSection label="Live state">
        <TweakSelect label="Output mode"
                     value={app.mode}
                     options={MODES.map(m => ({ value: m.id, label: m.label }))}
                     onChange={app.setMode}/>
        <TweakSelect label="Target language"
                     value={app.targetLangCode}
                     options={ALL_LANGS.map(l => ({ value: l.code, label: l.name }))}
                     onChange={app.setTargetLang}/>
        <TweakSelect label="Error state"
                     value={app.errorState || "none"}
                     options={[
                       { value: "none", label: "Healthy" },
                       { value: "network", label: "Network lost" },
                       { value: "mic", label: "No mic input" },
                       { value: "driver", label: "Driver missing" },
                       { value: "permission", label: "Permission denied" },
                     ]}
                     onChange={(v) => app.setErrorState(v === "none" ? null : v)}/>
      </TweakSection>
    </TweaksPanel>
  );
}

/* Mount */
const root = ReactDOM.createRoot(document.getElementById("root"));
root.render(<App/>);
