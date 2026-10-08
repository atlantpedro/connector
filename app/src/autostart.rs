//! The start at login, which finishes interrupted sessions. Through tauri-plugin-autostart,
//! except in the Flatpak: there the plugin would write into the sandbox's own config dir, so
//! the entry goes straight into the host's autostart folder, which the manifest shares.

use std::fs;
use std::path::PathBuf;

use lodav_core::host::in_flatpak;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

fn flatpak_entry() -> Option<(PathBuf, String)> {
    let id = std::env::var("FLATPAK_ID").ok()?;
    let path = lodav_core::dirs::home().join(format!(".config/autostart/{id}.desktop"));
    let entry = format!(
        "[Desktop Entry]
Type=Application
Name=Datenportal Connector
Exec=flatpak run --command=datenportal-connector {id} --recover
X-Flatpak={id}
NoDisplay=true
Terminal=false
"
    );
    Some((path, entry))
}

pub fn is_enabled(app: &AppHandle) -> bool {
    if in_flatpak() {
        return flatpak_entry().is_some_and(|(path, _)| path.exists());
    }
    app.autolaunch().is_enabled().unwrap_or(false)
}

pub fn set(app: &AppHandle, on: bool) -> Result<(), String> {
    if let Some((path, entry)) = flatpak_entry() {
        let result = if on {
            path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&path, entry))
        } else {
            fs::remove_file(&path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
        };
        return result.map_err(|e| e.to_string());
    }
    let autolaunch = app.autolaunch();
    let result = if on { autolaunch.enable() } else { autolaunch.disable() };
    result.map_err(|e| e.to_string())
}
