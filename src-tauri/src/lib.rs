mod appcfg;
mod commands;
mod devices;
mod driver_status;
mod engine;
mod permission;
mod secrets;
mod shortcuts;

use commands::AppHandle;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    secrets::migrate_legacy();
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppHandle::hydrated())
        .setup(|app| {
            use tauri::Manager as _;
            // ── Engine + ring producer ────────────────────────────────────────
            let cfg = crate::appcfg::load_or_default();
            let engine =
                std::sync::Arc::new(crate::engine::Engine::new(app.handle().clone(), &cfg));
            app.manage(engine.clone());

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

            // Build the 4 mode CheckMenuItems (radio-style via manual check management).
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
                "Translate",
                true,
                checks[2],
                None::<&str>,
            )?;
            let mode_translate_orig = CheckMenuItem::with_id(
                app,
                "mode_translate_orig",
                "Translate + Original",
                true,
                checks[3],
                None::<&str>,
            )?;

            // Non-mode items.
            let sep1 = PredefinedMenuItem::separator(app)?;
            let show_window =
                MenuItem::with_id(app, "show_window", "Show Window", true, None::<&str>)?;
            let captions =
                MenuItem::with_id(app, "captions", "Captions", true, None::<&str>)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Intervox", true, None::<&str>)?;

            let menu = Menu::with_items(
                app,
                &[
                    &mode_silence,
                    &mode_passthrough,
                    &mode_translate,
                    &mode_translate_orig,
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
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    use intervox_core::state::VirtualMicMode;
                    use tauri::Manager;

                    match event.id().as_ref() {
                        // ── Mode items ──────────────────────────────────────
                        id @ ("mode_silence"
                        | "mode_passthrough"
                        | "mode_translate"
                        | "mode_translate_orig") => {
                            let mode = match id {
                                "mode_silence" => VirtualMicMode::Silence,
                                "mode_passthrough" => VirtualMicMode::PassThrough,
                                "mode_translate" => VirtualMicMode::Translate,
                                _ => VirtualMicMode::TranslateWithOriginal,
                            };
                            let h = app.state::<AppHandle>();
                            let engine =
                                app.state::<std::sync::Arc<crate::engine::Engine>>();
                            commands::apply_mode(app, &h, &engine, mode);
                        }

                        // ── Show Window ─────────────────────────────────────
                        "show_window" => {
                            if let Some(win) = app.get_webview_window("main") {
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }

                        // ── Captions ─────────────────────────────────────────
                        "captions" => {
                            // Toggle the dedicated always-on-top captions window.
                            // If it is currently open, close it; otherwise open it.
                            use tauri::Manager;
                            if app.get_webview_window("captions").is_some() {
                                let _ = commands::do_close_captions_window(app);
                            } else {
                                let h = app.state::<commands::AppHandle>();
                                let always_on_top =
                                    h.config.lock().unwrap().captions.always_on_top;
                                let _ = commands::do_open_captions_window(app, always_on_top);
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
                mode_translate_orig,
            });

            // ── 5 s polling task (device list + driver status) ────────────────
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut tick =
                    tokio::time::interval(std::time::Duration::from_secs(5));
                loop {
                    tick.tick().await;

                    // ── device list ──────────────────────────────────────────
                    // enumerate() is sync and returns owned data — no cpal
                    // type crosses this await point.
                    let devices = crate::devices::enumerate();

                    // ── driver presence ──────────────────────────────────────
                    // Both calls are sync, read-only, and return owned bools.
                    // No MutexGuard or cpal type is held across the await above.
                    let installed = crate::driver_status::installed_on_disk()
                        && crate::driver_status::visible_to_coreaudio();

                    // Update managed state and capture a clone for the event.
                    // The MutexGuard is dropped before the next await point.
                    use tauri::Manager;
                    let status_clone = {
                        let app_handle = handle.state::<commands::AppHandle>();
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

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::set_virtual_mic_mode,
            commands::get_audio_devices,
            commands::set_source_mic,
            commands::set_monitor_output,
            commands::set_target_language,
            commands::set_mix_settings,
            commands::install_virtual_mic,
            commands::update_virtual_mic,
            commands::uninstall_virtual_mic,
            commands::get_driver_state,
            commands::open_audio_midi_setup,
            commands::open_system_mic_permission_settings,
            commands::get_mic_permission,
            commands::start_test_phrase,
            commands::clear_transcript_history,
            commands::stop_all_audio,
            commands::get_config,
            commands::get_account_status,
            commands::set_api_key,
            commands::verify_api_key,
            commands::clear_api_key,
            commands::set_source_language,
            commands::set_quality_mode,
            commands::set_mix_percent,
            commands::set_captions_config,
            commands::set_privacy_config,
            commands::set_shortcuts,
            commands::complete_onboarding,
            commands::open_captions_window,
            commands::close_captions_window,
            commands::open_accessibility_settings,
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
