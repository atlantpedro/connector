const COMMANDS: &[&str] = &[
    "overview",
    "messages",
    "mark_seen",
    "finish_session",
    "set_aside",
    "open_kept",
    "show_kept",
    "open_folder",
    "open_log_folder",
    "settings",
    "set_editor",
    "set_language",
    "set_tray",
    "set_prompts",
    "set_autostart",
    "set_updates",
    "check_update",
    "install_update",
];

fn main() {
    // the window may only call these, as granted in capabilities/default.json
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)))
        .expect("failed to run the tauri build script");
}
