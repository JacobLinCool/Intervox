#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if intervox_tauri_lib::run_cli_probe_if_requested() {
        return;
    }
    intervox_tauri_lib::run();
}
