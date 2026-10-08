//! The optional tray icon: shows whether a document is open and whether something needs
//! attention; its menu reaches the sessions, the messages, the folder and the settings.

use lodav_core::dirs::APP_NAME;
use lodav_core::{pending, Lang, Phase};
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

use crate::{quit, show_window, t, Connector};

const ID: &str = "main";

/// False where the icon can't be shown at all. On Linux it needs libayatana-appindicator
/// (or libappindicator); without it, Tauri would panic.
pub fn available() -> bool {
    #[cfg(target_os = "linux")]
    {
        ["libayatana-appindicator3.so.1", "libappindicator3.so.1"]
            .iter()
            // SAFETY: these libraries run no code of concern when loaded; tray-icon loads them too
            .any(|name| unsafe { libloading::Library::new(name) }.is_ok())
    }
    #[cfg(not(target_os = "linux"))]
    true
}

/// On where the desktop shows tray icons. GNOME shows them only with an extension.
pub fn on_by_default() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    available() && !desktop.split(':').any(|d| d.eq_ignore_ascii_case("GNOME"))
}

pub fn shown(app: &AppHandle) -> bool {
    app.tray_by_id(ID).is_some()
}

/// Show or remove the icon.
pub fn set(app: &AppHandle, on: bool) {
    if !on {
        app.remove_tray_by_id(ID);
        return;
    }
    if shown(app) || !available() {
        return;
    }
    let built = TrayIconBuilder::with_id(ID)
        .icon(icon(Look::Idle))
        .tooltip(APP_NAME)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu)
        .build(app);
    match built {
        Ok(_) => update(app),
        Err(e) => log::warn!("could not show the tray icon: {e}"),
    }
}

/// Bring icon and menu up to date; from any thread.
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    // menus are built on the main thread; never wait for it here, the caller may hold locks
    let _ = app.run_on_main_thread(move || update(&handle));
}

enum Look {
    Idle,
    Busy,
    Attention,
}

fn icon(look: Look) -> Image<'static> {
    let bytes: &[u8] = match look {
        Look::Idle => include_bytes!("../icons/tray/tray.png"),
        Look::Busy => include_bytes!("../icons/tray/tray-busy.png"),
        Look::Attention => include_bytes!("../icons/tray/tray-attention.png"),
    };
    Image::from_bytes(bytes).expect("tray icon")
}

pub fn phase_text(lang: Lang, phase: Phase) -> &'static str {
    match phase {
        Phase::Opening => t(lang, "opening", "wird geöffnet"),
        Phase::Editing => t(lang, "open", "geöffnet"),
        Phase::Uploading => t(lang, "uploading", "wird hochgeladen"),
        Phase::Waiting => t(lang, "waiting for the server", "wartet auf den Server"),
        Phase::Closing => t(lang, "finishing", "wird abgeschlossen"),
        Phase::Conflict => t(lang, "changed by someone else", "von jemand anderem geändert"),
    }
}

fn update(app: &AppHandle) {
    let (Some(tray), Some(connector)) = (app.tray_by_id(ID), app.try_state::<Connector>()) else { return };
    let ctx = &connector.ctx;
    let lang = ctx.lang();
    let sessions = ctx.activity.list();
    let waiting = pending(ctx);
    let unread = ctx.messages.unread();

    let attention = unread.iter().any(|m| m.urgent)
        || !waiting.is_empty()
        || sessions.iter().any(|s| matches!(s.phase, Phase::Waiting | Phase::Conflict));
    let look = if attention {
        Look::Attention
    } else if sessions.is_empty() {
        Look::Idle
    } else {
        Look::Busy
    };
    let _ = tray.set_icon(Some(icon(look)));
    let tooltip = match sessions.len() {
        0 => APP_NAME.to_string(),
        1 => format!("{APP_NAME}: {} ({})", sessions[0].name, phase_text(lang, sessions[0].phase)),
        n => format!("{APP_NAME}: {n} {}", t(lang, "documents open", "Dokumente geöffnet")),
    };
    let _ = tray.set_tooltip(Some(tooltip));

    match menu(app, lang, &sessions, &waiting, unread.len()) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(e) => log::warn!("could not build the tray menu: {e}"),
    }
}

fn menu(
    app: &AppHandle,
    lang: Lang,
    sessions: &[lodav_core::SessionInfo],
    waiting: &[lodav_core::Pending],
    unread: usize,
) -> tauri::Result<Menu<Wry>> {
    let mut items: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();
    for s in sessions {
        let label = format!("{} ({})", s.name, phase_text(lang, s.phase));
        let done = MenuItem::with_id(
            app,
            format!("finish:{}", s.key),
            t(lang, "Done editing", "Fertig bearbeitet"),
            !s.finishing,
            None::<&str>,
        )?;
        items.push(Box::new(Submenu::with_items(app, label, true, &[&done])?));
    }
    for p in waiting {
        let label = format!("{} ({})", p.name, phase_text(lang, Phase::Waiting));
        items.push(Box::new(MenuItem::with_id(app, format!("pending:{}", p.key), label, true, None::<&str>)?));
    }
    if !items.is_empty() {
        items.push(Box::new(PredefinedMenuItem::separator(app)?));
    }
    let messages = match unread {
        0 => t(lang, "Messages", "Meldungen").to_string(),
        n => format!("{} ({n} {})", t(lang, "Messages", "Meldungen"), t(lang, "new", "neu")),
    };
    items.push(Box::new(MenuItem::with_id(app, "messages", messages, true, None::<&str>)?));
    items.push(Box::new(MenuItem::with_id(app, "folder", t(lang, "Open folder", "Ordner öffnen"), true, None::<&str>)?));
    items.push(Box::new(MenuItem::with_id(app, "settings", t(lang, "Settings", "Einstellungen"), true, None::<&str>)?));
    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(MenuItem::with_id(app, "quit", t(lang, "Quit", "Beenden"), true, None::<&str>)?));
    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|i| i.as_ref()).collect();
    Menu::with_items(app, &refs)
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    let connector = app.state::<Connector>();
    if let Some(key) = id.strip_prefix("finish:") {
        connector.ctx.activity.request_finish(key);
    } else if id.starts_with("pending:") {
        show_window(app, "sessions");
    } else {
        match id {
            "messages" => show_window(app, "messages"),
            "settings" => show_window(app, "settings"),
            "folder" => crate::commands::open_user_folder(app),
            "quit" => quit(app),
            _ => {}
        }
    }
}
