//! CMS links in, sessions out.

use std::thread;

use tauri::{AppHandle, Manager};

use crate::{show_window, Busy, Connector};

/// What the app was started with: a link, `--recover` (at login), or nothing (from the menu).
/// Also called with the arguments of a second start, which then ends.
pub fn handle_args(app: &AppHandle, args: &[String]) {
    let link = args.iter().skip(1).find(|a| {
        let lower = a.to_ascii_lowercase();
        lower.starts_with("ms-word:") || lower.starts_with("ms-excel:")
    });
    if let Some(link) = link {
        open_link(app, link.clone());
    } else if args.iter().any(|a| a == "--recover") {
        recover(app);
    } else {
        show_window(app, "sessions");
    }
}

/// macOS hands links over as Apple Events, not as arguments.
pub fn register(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        let handle = app.clone();
        app.deep_link().on_open_url(move |event| {
            for url in event.urls() {
                open_link(&handle, url.to_string());
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

pub fn open_link(app: &AppHandle, uri: String) {
    let busy = Busy::new(app);
    let app = app.clone();
    thread::spawn(move || {
        let connector = app.state::<Connector>();
        log::info!("opening {uri}");
        let outcome = lodav_core::open(&connector.ctx, &uri, &connector.stop);
        log::info!("{uri}: {outcome:?}");
        drop(busy);
    });
}

/// Finish what earlier sessions left behind, however long that takes.
pub fn recover(app: &AppHandle) {
    let busy = Busy::new(app);
    let app = app.clone();
    thread::spawn(move || {
        let connector = app.state::<Connector>();
        lodav_core::recover_loop(&connector.ctx, &connector.stop);
        drop(busy);
    });
}
