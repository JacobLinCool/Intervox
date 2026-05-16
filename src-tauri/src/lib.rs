mod commands;

use commands::AppHandle;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppHandle::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::set_virtual_mic_mode,
            commands::get_audio_devices,
            commands::set_source_mic,
            commands::set_monitor_output,
            commands::set_target_language,
            commands::set_mix_settings,
            commands::install_virtual_mic,
            commands::uninstall_virtual_mic,
            commands::open_system_mic_permission_settings,
            commands::start_test_phrase,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Intervox");
}
