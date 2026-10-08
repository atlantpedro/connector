//! Updates from GitHub Releases: a signed latest.json names the new version and its files.
//! For Windows, macOS and the AppImage; the Flatpak (built without the `updater` feature) and
//! the .deb are updated by their package managers.

use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::{t, Connector};

/// The update found by the last check.
#[derive(Default)]
pub struct Updates {
    #[cfg(feature = "updater")]
    available: std::sync::Mutex<Option<tauri_plugin_updater::Update>>,
}

impl Updates {
    pub fn version(&self) -> Option<String> {
        #[cfg(feature = "updater")]
        return self.available.lock().unwrap().as_ref().map(|u| u.version.clone());
        #[cfg(not(feature = "updater"))]
        None
    }
}

/// True where this app updates itself.
pub fn supported() -> bool {
    cfg!(feature = "updater")
        && !cfg!(debug_assertions)
        && (cfg!(windows) || cfg!(target_os = "macos") || std::env::var_os("APPIMAGE").is_some())
}

const EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// Look for updates now and once a day, as long as the setting allows.
pub fn start(app: &AppHandle) {
    if !supported() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        let auto = app.state::<Connector>().settings.lock().unwrap().updates != Some(false);
        if auto {
            if let Err(e) = tauri::async_runtime::block_on(check(&app)) {
                log::info!("update check failed: {e}");
            }
        }
        std::thread::sleep(EVERY);
    });
}

/// The version available, if any. Tells the user once per version.
pub async fn check(app: &AppHandle) -> Result<Option<String>, String> {
    #[cfg(feature = "updater")]
    {
        use tauri::Emitter;
        use tauri_plugin_updater::UpdaterExt;
        let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
        let version = update.as_ref().map(|u| u.version.clone());
        *app.state::<Updates>().available.lock().unwrap() = update;
        if let Some(version) = &version {
            log::info!("update available: {version}");
            tell_once(app, version);
        }
        let _ = app.emit("update-changed", ());
        Ok(version)
    }
    #[cfg(not(feature = "updater"))]
    {
        let _ = app;
        Ok(None)
    }
}

#[cfg(feature = "updater")]
fn tell_once(app: &AppHandle, version: &str) {
    use lodav_core::dirs::APP_NAME;
    let ctx = &app.state::<Connector>().ctx;
    let marker = ctx.dirs.state.join("update-told");
    if std::fs::read_to_string(&marker).is_ok_and(|v| v.trim() == version) {
        return;
    }
    let rest = t(ctx.lang(), "is available. Install it under Settings.", "ist verfügbar. Sie können es unter Einstellungen installieren.");
    let text = format!("{APP_NAME} {version} {rest}");
    ctx.notify(text, false, None);
    let _ = std::fs::write(marker, version);
}

/// Download and install the update found, then start again. Refused while a document is
/// open: its saves would wait for the next start.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let connector = app.state::<Connector>();
    let lang = connector.ctx.lang();
    if !connector.ctx.activity.is_empty() {
        return Err(t(lang, "Close your documents first, then install the update.", "Schließen Sie zuerst Ihre Dokumente und installieren Sie dann das Update.").into());
    }
    #[cfg(feature = "updater")]
    {
        let update = app.state::<Updates>().available.lock().unwrap().clone();
        let Some(update) = update else { return Err(t(lang, "No update is available.", "Es ist kein Update verfügbar.").into()) };
        log::info!("installing {}", update.version);
        let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| connector.ctx.failed("downloading the update", &e))?;
        // earlier sessions still waiting save their state; the next start finishes them
        connector.stop.set();
        crate::wait_for_sessions(app, Duration::from_secs(10));
        // on Windows, this starts the installer and ends the app
        update.install(bytes).map_err(|e| connector.ctx.failed("installing the update", &e))?;
        app.restart();
    }
    #[allow(unreachable_code)]
    Err(connector.ctx.failed("installing the update", &"built without the updater"))
}
