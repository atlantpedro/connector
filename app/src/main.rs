//! Datenportal Connector: the app around lodav-core. It receives the CMS links, runs the
//! sessions, shows messages, and stays out of the way otherwise.

// no console window next to the app on Windows
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod commands;
mod handover;
mod links;
mod notify;
mod settings;
mod tray;
mod updater;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lodav_core::dirs::APP_NAME;
use lodav_core::migrate::{migrate, Migration};
use lodav_core::{Context, Dirs, Lang, Stop};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

use settings::Settings;

/// What the app shares between its threads.
pub struct Connector {
    pub ctx: Arc<Context>,
    /// Set when the app quits: sessions save their state and the next run finishes them.
    pub stop: Stop,
    /// Sessions and recovery passes running.
    busy: Mutex<usize>,
    pub settings: Mutex<Settings>,
}

/// Held while a session or recovery pass runs; the app quits after the last one, unless a
/// window or the tray icon is open.
pub struct Busy(AppHandle);

impl Busy {
    pub fn new(app: &AppHandle) -> Busy {
        *app.state::<Connector>().busy.lock().unwrap() += 1;
        Busy(app.clone())
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        let connector = self.0.state::<Connector>();
        let mut busy = connector.busy.lock().unwrap();
        *busy -= 1;
        if *busy == 0 && self.0.webview_windows().is_empty() && !tray::shown(&self.0) && !connector.stop.is_set() {
            log::info!("nothing left to do, quitting");
            self.0.exit(0);
        }
    }
}

pub fn t(lang: Lang, en: &'static str, de: &'static str) -> &'static str {
    if lang == Lang::De { de } else { en }
}

/// Open the window, or bring it to the front, on this view: sessions, messages or settings.
pub fn show_window(app: &AppHandle, view: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("navigate", view);
        return;
    }
    let built = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(format!("index.html#{view}").into()))
        .title(APP_NAME)
        .inner_size(760.0, 540.0)
        .min_inner_size(420.0, 360.0)
        .build();
    if let Err(e) = built {
        log::error!("could not open the window: {e}");
    }
}

/// Quit, unless a document is still open: its saves would not reach the CMS until the next
/// start.
pub fn quit(app: &AppHandle) {
    let connector = app.state::<Connector>();
    let sessions = connector.ctx.activity.list();
    if sessions.is_empty() {
        app.exit(0);
        return;
    }
    let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
    let lang = connector.ctx.lang();
    let text = format!(
        "{} {}",
        t(lang, "Still open:", "Noch geöffnet:"),
        names.join(", ")
    ) + t(
        lang,
        ". Close the documents (or click “Done editing”) first, then quit.",
        ". Schließen Sie zuerst die Dokumente (oder klicken Sie auf „Fertig bearbeitet“) und beenden Sie dann.",
    );
    notify::show(app, &text);
    show_window(app, "sessions");
}

/// Wait up to timeout for running sessions and recovery passes to end, once told to stop.
pub fn wait_for_sessions(app: &AppHandle, timeout: Duration) {
    let connector = app.state::<Connector>();
    let deadline = Instant::now() + timeout;
    while *connector.busy.lock().unwrap() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// What installers run: `--setup` after installing, `--uninstall` before removing the app.
/// Handled before anything else, so that a running instance doesn't get them. The exit code
/// is 3 if LibreOffice runs and its prompts can't be put back yet, else 1 on errors.
fn installer_step(dirs: &Dirs, arg: &str) -> Option<i32> {
    let result = match arg {
        "--setup" => handover::setup(&dirs.state),
        "--uninstall" => {
            use lodav_core::editor::{LibreOffice, Prompts};
            use lodav_core::Editor;
            let prompts = Prompts::new(&dirs.state);
            if prompts.are_off() && LibreOffice::new().running() {
                eprintln!("LibreOffice is running; close it so that its settings can be put back");
                return Some(3);
            }
            if let Err(e) = prompts.restore(false) {
                eprintln!("could not put LibreOffice's settings back: {e:?}");
            }
            handover::hand_back(&dirs.state)
        }
        _ => return None,
    };
    Some(match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{arg} failed: {e}");
            1
        }
    })
}

fn main() {
    let dirs = Dirs::current();
    if let Some(code) = std::env::args().nth(1).and_then(|arg| installer_step(&dirs, &arg)) {
        std::process::exit(code);
    }
    let log_dir = dirs.state.clone();
    tauri::Builder::default()
        // first, so that a second start hands its arguments over before doing anything else
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| links::handle_args(app, &argv)))
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .target(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Folder {
                    path: log_dir,
                    file_name: Some("datenportal-connector".into()),
                }))
                .target(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(updater::Updates::default())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--recover"]),
        ))
        .setup(move |app| {
            #[cfg(feature = "updater")]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            let handle = app.handle().clone();
            let settings = Settings::load(&dirs.state);
            let ctx = Context::new(dirs, settings.editor().editor(), notify::notifier(&handle));
            ctx.set_lang(settings.lang());
            log::info!("{APP_NAME} {} started, editor: {}", env!("CARGO_PKG_VERSION"), ctx.editor().name());
            // not by development builds, which would take the links from the installed app
            if !cfg!(debug_assertions) {
                handover::at_start(&ctx.dirs.state);
            }
            match migrate(&ctx, &Dirs::legacy()) {
                Ok(Migration::Done(moved)) => log::info!("moved over from lodav.py: {moved:?}"),
                Ok(_) => {}
                Err(e) => log::error!("could not move lodav.py's folders over: {e}"),
            }
            let changes = handle.clone();
            ctx.activity.set_listener(move |_| {
                let _ = changes.emit("sessions-changed", ());
                tray::refresh(&changes);
            });
            let (tray_on, autostart) = (settings.tray.unwrap_or_else(tray::on_by_default), settings.autostart);
            app.manage(Connector { ctx: Arc::new(ctx), stop: Stop::new(), busy: Mutex::new(0), settings: Mutex::new(settings) });
            links::register(&handle);
            tray::set(&handle, tray_on);
            updater::start(&handle);
            // finishing earlier sessions at login; not set up by development builds
            if autostart == Some(false) || !cfg!(debug_assertions) {
                let wanted = autostart != Some(false);
                if autostart::is_enabled(&handle) != wanted {
                    if let Err(e) = autostart::set(&handle, wanted) {
                        log::warn!("could not change the start at login: {e}");
                    }
                }
            }
            links::handle_args(&handle, &std::env::args().collect::<Vec<_>>());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::overview,
            commands::messages,
            commands::mark_seen,
            commands::finish_session,
            commands::set_aside,
            commands::open_kept,
            commands::show_kept,
            commands::open_folder,
            commands::open_log_folder,
            commands::settings,
            commands::set_editor,
            commands::set_language,
            commands::set_tray,
            commands::set_prompts,
            commands::set_autostart,
            commands::set_updates,
            commands::check_update,
            commands::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the app")
        .run(|app, event| match event {
            // the last window was closed: keep running while sessions are, or the tray icon is
            RunEvent::ExitRequested { code: None, api, .. } => {
                if *app.state::<Connector>().busy.lock().unwrap() > 0 || tray::shown(app) {
                    api.prevent_exit();
                }
            }
            RunEvent::Exit => {
                // e.g. a logout: give sessions a moment to save their state and try a last upload
                app.state::<Connector>().stop.set();
                wait_for_sessions(app, Duration::from_secs(10));
            }
            _ => {}
        });
}
