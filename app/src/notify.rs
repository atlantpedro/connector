//! Messages from the core, shown as notifications. They are in the message log already.
//! Urgent ones also open the window on the messages, since a notification is easily missed.

use lodav_core::dirs::APP_NAME;
use lodav_core::{Message, Notifier};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::{show_window, tray};

pub fn notifier(app: &AppHandle) -> Notifier {
    let app = app.clone();
    Box::new(move |message: &Message| {
        show(&app, &message.text);
        let _ = app.emit("messages-changed", ());
        tray::refresh(&app);
        if message.urgent {
            show_window(&app, "messages");
        }
    })
}

/// A notification only, without a trace in the message log.
pub fn show(app: &AppHandle, text: &str) {
    if let Err(e) = app.notification().builder().title(APP_NAME).body(text).show() {
        log::warn!("could not show a notification: {e}");
    }
}
