mod appcfg;
mod captions_overlay;
mod commands;
mod connection_log;
mod devices;
mod driver_status;
mod engine;
mod permission;
mod platform_integration;
mod shortcuts;
mod single_instance;
mod transcript_log;
mod usage_store;

use commands::AppHandle;
use serde::Serialize;

fn show_main_window(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.show();
    }

    use tauri::Manager as _;
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliProbeReport {
    process_path: String,
    bundled_driver_present: bool,
    installed_driver_present: bool,
    driver_state: driver_status::DriverState,
    intervox_input_visible: bool,
    input_device_count: usize,
    output_device_count: usize,
    mic_before: permission::MicPermission,
    mic_after: permission::MicPermission,
    api_key_file_read: bool,
    api_key_saved: bool,
    api_key_verified: bool,
    api_validation_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    capture_probe: Option<CliCaptureProbe>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ring_probe: Option<CliRingProbe>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliCaptureProbe {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<engine::capture::CaptureProbeReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<intervox_core::AppError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliRingProbe {
    ok: bool,
    mode_before: u32,
    mode_after: u32,
    write_index_before: u64,
    write_index_after: u64,
    read_index_before: u64,
    read_index_after: u64,
    write_delta_frames: u64,
    read_delta_frames: u64,
    available_frames_after: u64,
    recent_max_abs_after: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug)]
struct CliProbeOptions {
    out_path: Option<String>,
    api_key_file: Option<String>,
    request_mic: bool,
    capture_probe: bool,
    ring_probe: bool,
    capture_duration_ms: u64,
}

pub fn run_cli_probe_if_requested() -> bool {
    let mut args = std::env::args().skip(1).peekable();
    let mut options = CliProbeOptions {
        out_path: None,
        api_key_file: None,
        request_mic: false,
        capture_probe: false,
        ring_probe: false,
        capture_duration_ms: 2_000,
    };
    let mut requested = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--intervox-onboarding-probe" => requested = true,
            "--intervox-capture-probe" => {
                requested = true;
                options.capture_probe = true;
            }
            "--intervox-ring-probe" => {
                requested = true;
                options.ring_probe = true;
            }
            "--out" => options.out_path = args.next(),
            "--api-key-file" => options.api_key_file = args.next(),
            "--request-mic" => options.request_mic = true,
            "--capture-duration-ms" => {
                if let Some(value) = args.next() {
                    options.capture_duration_ms = value.parse().unwrap_or(2_000);
                }
            }
            _ => {}
        }
    }

    if !requested {
        return false;
    }

    let report = run_cli_probe(&options);
    let payload = serde_json::to_string_pretty(&report).unwrap_or_else(|e| {
        format!(
            r#"{{"apiKeyFileRead":false,"apiKeySaved":false,"apiKeyVerified":false,"apiValidationStatus":"serialize-error:{e}"}}"#
        )
    });

    if let Some(path) = options.out_path {
        if let Err(e) = std::fs::write(&path, format!("{payload}\n")) {
            eprintln!("failed to write probe report to {path}: {e}");
            println!("{payload}");
        }
    } else {
        println!("{payload}");
    }

    true
}

fn run_cli_probe(options: &CliProbeOptions) -> CliProbeReport {
    let process_path = std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unknown>".into());

    let bundled_driver_present = bundled_driver_present();
    let mic_before = permission::status();
    let mic_after = if options.request_mic {
        permission::request_access()
    } else {
        mic_before
    };

    let (api_key_file_read, api_key_saved, api_key_verified, api_validation_status) =
        probe_api_key(options.api_key_file.as_deref());

    let devices = devices::enumerate();
    let driver_state = driver_status::state_from_devices(&devices);
    let intervox_input_visible = driver_status::visible_in_devices(&devices);
    let capture_probe = if options.capture_probe {
        let cfg = appcfg::load_or_default();
        Some(run_capture_probe(
            cfg.audio.source_id.as_deref(),
            options.capture_duration_ms,
        ))
    } else {
        None
    };
    let ring_probe = if options.ring_probe {
        Some(run_ring_probe(options.capture_duration_ms))
    } else {
        None
    };

    CliProbeReport {
        process_path,
        bundled_driver_present,
        installed_driver_present: driver_status::installed_on_disk(),
        driver_state,
        intervox_input_visible,
        input_device_count: devices.inputs.len(),
        output_device_count: devices.outputs.len(),
        mic_before,
        mic_after,
        api_key_file_read,
        api_key_saved,
        api_key_verified,
        api_validation_status,
        capture_probe,
        ring_probe,
    }
}

fn run_capture_probe(device_id: Option<&str>, duration_ms: u64) -> CliCaptureProbe {
    let duration_ms = duration_ms.clamp(250, 10_000);
    match engine::capture::probe_level(device_id, std::time::Duration::from_millis(duration_ms)) {
        Ok(report) => CliCaptureProbe {
            ok: report.callback_count > 0 && report.stream_error.is_none(),
            report: Some(report),
            error: None,
        },
        Err(error) => CliCaptureProbe {
            ok: false,
            report: None,
            error: Some(error),
        },
    }
}

fn run_ring_probe(duration_ms: u64) -> CliRingProbe {
    use intervox_core::virtual_mic::ring_buffer::{SharedRingMap, DEFAULT_SHM_NAME};
    use std::sync::atomic::Ordering;

    let duration_ms = duration_ms.clamp(250, 10_000);
    let map = match SharedRingMap::open(DEFAULT_SHM_NAME) {
        Ok(map) => map,
        Err(e) => {
            return CliRingProbe {
                ok: false,
                mode_before: 0,
                mode_after: 0,
                write_index_before: 0,
                write_index_after: 0,
                read_index_before: 0,
                read_index_after: 0,
                write_delta_frames: 0,
                read_delta_frames: 0,
                available_frames_after: 0,
                recent_max_abs_after: 0.0,
                error: Some(e.to_string()),
            };
        }
    };

    let ring = map.get();
    let mode_before = ring.mode.load(Ordering::Acquire);
    let write_before = ring.write_index.load(Ordering::Acquire);
    let read_before = ring.read_index.load(Ordering::Acquire);
    std::thread::sleep(std::time::Duration::from_millis(duration_ms));
    let mode_after = ring.mode.load(Ordering::Acquire);
    let write_after = ring.write_index.load(Ordering::Acquire);
    let read_after = ring.read_index.load(Ordering::Acquire);
    let write_delta = write_after.saturating_sub(write_before);
    let read_delta = read_after.saturating_sub(read_before);
    let recent_max_abs_after = ring.recent_max_abs(48_000);

    CliRingProbe {
        ok: write_delta > 0,
        mode_before,
        mode_after,
        write_index_before: write_before,
        write_index_after: write_after,
        read_index_before: read_before,
        read_index_after: read_after,
        write_delta_frames: write_delta,
        read_delta_frames: read_delta,
        available_frames_after: write_after.saturating_sub(read_after),
        recent_max_abs_after,
        error: None,
    }
}

fn bundled_driver_present() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(contents_dir) = exe.parent().and_then(|p| p.parent()) else {
        return false;
    };
    contents_dir
        .join("Resources")
        .join("driver/build/Intervox.driver")
        .is_dir()
}

fn probe_api_key(api_key_file: Option<&str>) -> (bool, bool, bool, String) {
    let Some(path) = api_key_file else {
        return (false, false, false, "not-requested".into());
    };

    let key = match std::fs::read_to_string(path) {
        Ok(key) => key.trim().to_string(),
        Err(e) => return (false, false, false, format!("read-error:{e}")),
    };
    if key.is_empty() {
        return (true, false, false, "empty-key-file".into());
    }

    let mut cfg = appcfg::load_or_default();
    cfg.account.openai_api_key = Some(key.clone());
    cfg.account.openai_api_key_verified = false;
    cfg.account.openai_api_key_last_verified = None;
    if let Err(e) = appcfg::persist(&cfg) {
        return (true, false, false, format!("save-error:{e}"));
    }

    let verified = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(async {
            use intervox_core::realtime::openai_auth::{
                classify_validation, KeyValidation, VALIDATION_URL,
            };

            let client = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
            {
                Ok(client) => client,
                Err(e) => return (false, format!("client-error:{e}")),
            };

            let http_status = match client.get(VALIDATION_URL).bearer_auth(&key).send().await {
                Ok(resp) => Some(resp.status().as_u16()),
                Err(e) => return (false, format!("network-error:{e}")),
            };

            match classify_validation(http_status) {
                KeyValidation::Verified => {
                    let mut cfg = appcfg::load_or_default();
                    cfg.account.openai_api_key_verified = true;
                    if let Err(e) = appcfg::persist(&cfg) {
                        return (false, format!("save-error:{e}"));
                    }
                    (true, "verified".into())
                }
                KeyValidation::InvalidKey => (false, "invalid-key".into()),
                KeyValidation::Offline => (false, "offline".into()),
                KeyValidation::Unknown => {
                    let status = http_status
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "none".into());
                    (false, format!("unknown-status:{status}"))
                }
            }
        }),
        Err(e) => (false, format!("runtime-error:{e}")),
    };

    (true, true, verified.0, verified.1)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let single_instance_guard = match crate::single_instance::acquire() {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("[intervox:lifecycle] another Intervox instance is already running: {error}");
            if let crate::single_instance::SingleInstanceError::AlreadyRunning { pid, .. } = error {
                crate::single_instance::activate_existing(pid);
            }
            return;
        }
    };

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(single_instance_guard)
        .manage(AppHandle::hydrated())
        .setup(|app| {
            use tauri::Manager as _;
            // ── Engine + ring producer ────────────────────────────────────────
            let cfg = crate::appcfg::load_or_default();
            let engine =
                std::sync::Arc::new(crate::engine::Engine::new(app.handle().clone(), &cfg));
            app.manage(engine.clone());
            let initial_mode = {
                use tauri::Manager;
                let h = app.state::<AppHandle>();
                let mode = h.state.lock().unwrap().status.mode;
                mode
            };
            engine.set_mode(initial_mode);

            // ── Platform integration (dock policy, login item) ────────────────
            // FIX-4: reuse the `cfg` already loaded above — no second disk read.
            // FIX-3d: initialise the engine's show_latency_badge atomic from the
            // same config so the pull_task starts with the correct value.
            crate::platform_integration::apply_ui_config(app.handle(), &cfg.ui);
            engine.set_show_latency_badge(cfg.ui.show_latency_badge);

            // ── Native macOS tray menu ────────────────────────────────────────
            // Read the current mode from persisted config to set initial
            // checked-state on the CheckMenuItems.
            let initial_mode = {
                use tauri::Manager;
                let h = app.state::<AppHandle>();
                let mode = h.state.lock().unwrap().status.mode;
                mode
            };
            let checks = commands::tray_menu_checks(initial_mode);

            use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
            use tauri::tray::TrayIconBuilder;

            // Build the 3 mode CheckMenuItems (radio-style via manual check management).
            let mode_silence = CheckMenuItem::with_id(
                app,
                "mode_silence",
                "Silence",
                true,
                checks[0],
                None::<&str>,
            )?;
            let mode_passthrough = CheckMenuItem::with_id(
                app,
                "mode_passthrough",
                "Pass-Through",
                true,
                checks[1],
                None::<&str>,
            )?;
            let mode_translate = CheckMenuItem::with_id(
                app,
                "mode_translate",
                "Interpret",
                true,
                checks[2],
                None::<&str>,
            )?;

            // Non-mode items.
            let sep1 = PredefinedMenuItem::separator(app)?;
            let show_window =
                MenuItem::with_id(app, "show_window", "Show Window", true, None::<&str>)?;
            let captions = MenuItem::with_id(app, "captions", "Captions", true, None::<&str>)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Intervox", true, None::<&str>)?;

            let menu = Menu::with_items(
                app,
                &[
                    &mode_silence,
                    &mode_passthrough,
                    &mode_translate,
                    &sep1,
                    &show_window,
                    &captions,
                    &sep2,
                    &quit,
                ],
            )?;

            let initial_title = commands::tray_mode_label(initial_mode);

            // Build the tray icon.  We keep the TrayIcon handle in managed
            // state (via TrayState) so apply_mode can update title +
            // checked-state whenever mode changes, regardless of whether the
            // change was command-driven or tray-driven.
            let tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip(format!("Intervox — {initial_title}"))
                .title(initial_title)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| {
                    use intervox_core::state::VirtualMicMode;
                    use tauri::Manager;

                    match event.id().as_ref() {
                        // ── Mode items ──────────────────────────────────────
                        id @ ("mode_silence" | "mode_passthrough" | "mode_translate") => {
                            let mode = match id {
                                "mode_silence" => VirtualMicMode::Silence,
                                "mode_passthrough" => VirtualMicMode::PassThrough,
                                _ => VirtualMicMode::Translate,
                            };
                            let h = app.state::<AppHandle>();
                            let engine = app.state::<std::sync::Arc<crate::engine::Engine>>();
                            if let Err(e) = commands::apply_mode(app, &h, &engine, mode) {
                                use tauri::Emitter as _;
                                let _ = app.emit("error", e);
                            }
                        }

                        // ── Show Window ─────────────────────────────────────
                        "show_window" => {
                            show_main_window(app);
                        }

                        // ── Captions ─────────────────────────────────────────
                        "captions" => {
                            let h = app.state::<commands::AppHandle>();
                            if let Err(e) = commands::toggle_captions_window(app, &h) {
                                use tauri::Emitter as _;
                                let _ = app.emit("error", e);
                            }
                        }

                        // ── Quit ─────────────────────────────────────────────
                        "quit" => {
                            // app.exit(0) triggers the existing RunEvent::Exit →
                            // engine.shutdown() path which flushes ring silence.
                            app.exit(0);
                        }

                        _ => {}
                    }
                })
                .build(app)?;

            // Persist TrayIcon + CheckMenuItem handles in managed state so
            // apply_mode() can update them on every mode change.
            app.manage(commands::TrayState {
                tray,
                mode_silence,
                mode_passthrough,
                mode_translate,
            });

            // ── Low-frequency device poll (device list + driver status) ───────
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
                // Avoid competing with frontend startup, which performs its own
                // first device enumeration after the shell is responsive.
                tick.tick().await;
                loop {
                    tick.tick().await;

                    // ── device list ──────────────────────────────────────────
                    // CoreAudio enumeration can block when coreaudiod is wedged.
                    // Use one bounded in-flight query and derive all later
                    // status from the same snapshot.
                    let enum_flag = {
                        use tauri::Manager;
                        let app_handle = handle.state::<commands::AppHandle>();
                        std::sync::Arc::clone(&app_handle.audio_enumeration_running)
                    };
                    let devices = match commands::enumerate_audio_devices_bounded(enum_flag).await {
                        Ok(devices) => devices,
                        Err(_) => continue,
                    };

                    // ── driver presence ──────────────────────────────────────
                    // Derive this from the device snapshot above; do not trigger
                    // a second CoreAudio enumeration.
                    let driver_state = crate::driver_status::state_from_devices(&devices);
                    let installed = driver_state == crate::driver_status::DriverState::Healthy;
                    let default_output_id = devices.outputs.first().map(|device| device.id.clone());

                    if let Some(engine) =
                        handle.try_state::<std::sync::Arc<crate::engine::Engine>>()
                    {
                        let engine = std::sync::Arc::clone(&engine);
                        let _ = tauri::async_runtime::spawn_blocking(move || {
                            engine.sync_output_preview_default_device(default_output_id.as_deref());
                        })
                        .await;
                    }

                    // Update managed state and capture a clone for the event.
                    // The MutexGuard is dropped before the next await point.
                    use tauri::Manager;
                    let status_clone = {
                        let app_handle = handle.state::<commands::AppHandle>();
                        *app_handle.driver_state.lock().unwrap() = driver_state;
                        let mut st = app_handle.state.lock().unwrap();
                        st.status.virtual_mic_installed = installed;
                        st.status.clone()
                    }; // MutexGuard dropped here

                    use tauri::Emitter;
                    let _ = handle.emit("device-list-changed", devices);
                    let _ = handle.emit("status-changed", status_clone);
                }
            });

            // ── Global shortcuts ──────────────────────────────────────────────
            crate::shortcuts::register_shortcuts(app.handle());

            if cfg.onboarding_completed {
                commands::open_initial_captions_window(app.handle(), &cfg.captions);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::set_virtual_mic_mode,
            commands::get_audio_devices,
            commands::get_audio_backpressure_metrics,
            commands::get_audio_meter_diagnostics,
            commands::record_frontend_meter_diagnostics,
            commands::record_frontend_lifecycle_diagnostics,
            commands::set_audio_source,
            commands::set_output_preview_enabled,
            commands::set_target_language,
            commands::set_mix_settings,
            commands::install_virtual_mic,
            commands::update_virtual_mic,
            commands::uninstall_virtual_mic,
            commands::get_driver_state,
            commands::open_audio_midi_setup,
            commands::open_system_mic_permission_settings,
            commands::open_system_audio_permission_settings,
            commands::get_mic_permission,
            commands::request_mic_permission,
            commands::start_test_phrase,
            commands::start_mic_level_probe,
            commands::stop_mic_level_probe,
            commands::clear_transcript_history,
            commands::stop_all_audio,
            commands::get_config,
            commands::get_account_status,
            commands::set_api_key,
            commands::verify_api_key,
            commands::clear_api_key,
            commands::set_mix_percent,
            commands::set_captions_config,
            commands::set_captions_window_expanded,
            commands::start_captions_window_drag,
            commands::set_privacy_config,
            commands::set_shortcuts,
            commands::complete_onboarding,
            commands::quit_app,
            commands::open_accessibility_settings,
            commands::get_connection_log,
            commands::set_ui_config,
            commands::open_external_url,
        ]);

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while building Intervox");
    app.run(|app_handle, event| {
        match event {
            // ── Window close → hide (keep engine + ring + tray alive) ────────
            // Choosing run-event closure over .on_window_event() on the builder
            // because the run closure has access to a fully-built AppHandle and
            // avoids an extra builder chain step.
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } if label == "main" => {
                // Prevent the OS close and hide the window instead.
                // The engine, ring, and tray remain alive.
                // "Show Window" in the tray re-shows it; "Quit" truly exits.
                api.prevent_close();
                use tauri::Manager as _;
                if let Some(win) = app_handle.get_webview_window("main") {
                    let _ = win.hide();
                }
            }

            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { .. },
                ..
            } if label == "captions" => {
                use tauri::Manager as _;
                // Capture the final placement before the window is gone so it
                // is remembered even when captions are being disabled.
                commands::persist_captions_geometry_now(app_handle);
                let h = app_handle.state::<commands::AppHandle>();
                let _ = commands::record_captions_window_closed(app_handle, &h);
            }

            // ── Captions moved/resized → debounced placement persistence ─────
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_),
                ..
            } if label == "captions" => {
                commands::schedule_persist_captions_geometry(app_handle);
            }

            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                show_main_window(app_handle);
            }

            // ── Exit / ExitRequested → flush ring silence via shutdown ────────
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => {
                use tauri::Manager;
                if let Some(engine) =
                    app_handle.try_state::<std::sync::Arc<crate::engine::Engine>>()
                {
                    engine.shutdown();
                }
            }

            _ => {}
        }
    });
}
