//! What the window may ask for; granted one by one in capabilities/default.json.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lodav_core::dirs::Platform;
use lodav_core::editor::{LibreOffice, MsOffice, Prompts, PromptsError};
use lodav_core::{pending, set_aside as set_aside_session, Context, Editor, Lang, Message, Msg, Pending, SessionInfo};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::settings::{EditorChoice, Language};
use crate::{t, tray, Connector};

type Result<T> = std::result::Result<T, String>;

#[derive(Serialize)]
pub struct Overview {
    lang: &'static str,
    sessions: Vec<SessionInfo>,
    pending: Vec<Pending>,
    unread: usize,
}

#[tauri::command]
pub fn overview(connector: State<'_, Connector>) -> Overview {
    let ctx = &connector.ctx;
    Overview { lang: lang_tag(ctx.lang()), sessions: ctx.activity.list(), pending: pending(ctx), unread: ctx.messages.unread().len() }
}

#[derive(Serialize)]
pub struct Messages {
    /// Newest first.
    messages: Vec<Message>,
    /// The newest message seen before; later ones are new.
    seen: u64,
}

#[tauri::command]
pub fn messages(connector: State<'_, Connector>) -> Messages {
    let log = &connector.ctx.messages;
    let mut messages = log.list();
    messages.reverse();
    Messages { messages, seen: log.seen_id() }
}

#[tauri::command]
pub fn mark_seen(app: AppHandle, connector: State<'_, Connector>, id: u64) -> Result<()> {
    let ctx = &connector.ctx;
    ctx.messages.mark_seen(id).map_err(|e| ctx.failed("marking messages seen", &e))?;
    tray::refresh(&app);
    Ok(())
}

/// "Done editing": stop waiting for the editor to close the document.
#[tauri::command]
pub fn finish_session(connector: State<'_, Connector>, key: String) -> bool {
    log::info!("done editing {key}, as the user says");
    connector.ctx.activity.request_finish(&key)
}

/// Give up on an earlier session that keeps waiting for the server. False if it is running.
#[tauri::command]
pub async fn set_aside(connector: State<'_, Connector>, key: String) -> Result<bool> {
    blocking(&connector.ctx, move |ctx| {
        set_aside_session(ctx, &key).map_err(|e| match e.kind() {
            std::io::ErrorKind::TimedOut => ctx.tr(Msg::StillBusy, &[]),
            _ => ctx.failed("setting a session aside", &e),
        })
    })
    .await
}

/// Run blocking work (network, process lists) off the window's thread.
async fn blocking<T: Send + 'static>(
    ctx: &Arc<Context>,
    work: impl FnOnce(&Context) -> Result<T> + Send + 'static,
) -> Result<T> {
    let (ctx, inner) = (ctx.clone(), ctx.clone());
    let result = tauri::async_runtime::spawn_blocking(move || work(&inner)).await;
    result.map_err(|e| ctx.failed("a background task", &e))?
}

/// A version kept for the user, if a message names it and it is still there. The window
/// can't open anything else.
fn kept_file(ctx: &Context, path: &Path) -> Result<PathBuf> {
    let known = ctx.messages.list().into_iter().any(|m| m.kept.as_deref() == Some(path));
    if !known {
        return Err(ctx.failed("opening a file", &format!("{} is not a kept file", path.display())));
    }
    if !path.exists() {
        return Err(t(ctx.lang(), "The file is not there any more; it may have been moved or deleted.", "Die Datei ist nicht mehr da; vielleicht wurde sie verschoben oder gelöscht.").into());
    }
    Ok(path.to_path_buf())
}

#[tauri::command]
pub fn open_kept(app: AppHandle, connector: State<'_, Connector>, path: PathBuf) -> Result<()> {
    let ctx = &connector.ctx;
    let path = kept_file(ctx, &path)?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| ctx.failed("opening a kept file", &e))
}

#[tauri::command]
pub fn show_kept(app: AppHandle, connector: State<'_, Connector>, path: PathBuf) -> Result<()> {
    let ctx = &connector.ctx;
    let path = kept_file(ctx, &path)?;
    app.opener().reveal_item_in_dir(path).map_err(|e| ctx.failed("showing a kept file", &e))
}

pub fn open_user_folder(app: &AppHandle) {
    let folder = app.state::<Connector>().ctx.user_folder();
    let opened = folder.map_err(|e| e.to_string()).and_then(|folder| {
        app.opener().open_path(folder.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
    });
    if let Err(e) = opened {
        log::warn!("could not open the folder: {e}");
    }
}

#[tauri::command]
pub fn open_folder(app: AppHandle) {
    open_user_folder(&app);
}

#[tauri::command]
pub fn open_log_folder(app: AppHandle, connector: State<'_, Connector>) -> Result<()> {
    let ctx = &connector.ctx;
    let state = ctx.dirs.state.to_string_lossy().into_owned();
    app.opener().open_path(state, None::<&str>).map_err(|e| ctx.failed("opening the log folder", &e))
}

#[derive(Serialize)]
pub struct EditorOption {
    id: EditorChoice,
    name: &'static str,
    installed: bool,
}

#[derive(Serialize)]
pub struct SettingsView {
    lang: &'static str,
    /// Chosen in the settings; none: the system's.
    language: Option<Language>,
    /// The system's language, shown next to "automatic".
    system_lang: &'static str,
    version: &'static str,
    editor: EditorChoice,
    editors: Vec<EditorOption>,
    tray: bool,
    /// Why there is no tray icon to be had or seen: "library" (Linux, missing), "gnome"
    /// (shown only with an extension), or none.
    tray_note: Option<&'static str>,
    tray_available: bool,
    prompts_off: bool,
    /// LibreOffice has settings to change: it was started at least once.
    prompts_available: bool,
    autostart: bool,
    folder: PathBuf,
    /// False where the package manager brings updates.
    updates_supported: bool,
    updates_auto: bool,
    /// The version an update would install.
    update_available: Option<String>,
}

fn lang_tag(lang: Lang) -> &'static str {
    if lang == Lang::De { "de" } else { "en" }
}

pub fn office_profiles() -> Vec<PathBuf> {
    let home = lodav_core::dirs::home();
    let env = |key: &str| {
        // in the Flatpak it names the sandbox's config dir, not LibreOffice's
        if key == "XDG_CONFIG_HOME" && lodav_core::host::in_flatpak() {
            return None;
        }
        std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
    };
    Prompts::profiles(Platform::current(), &env, &home)
}

#[tauri::command]
pub async fn settings(app: AppHandle, connector: State<'_, Connector>) -> Result<SettingsView> {
    let (choice, language) = {
        let settings = connector.settings.lock().unwrap();
        (settings.editor(), settings.language)
    };
    let tray_on = tray::shown(&app);
    let autostart = crate::autostart::is_enabled(&app);
    let updates_auto = connector.settings.lock().unwrap().updates != Some(false);
    let update_available = app.state::<crate::updater::Updates>().version();
    blocking(&connector.ctx, move |ctx| {
        let tray_available = tray::available();
        let gnome = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').any(|d| d.eq_ignore_ascii_case("GNOME"));
        Ok(SettingsView {
            lang: lang_tag(ctx.lang()),
            language,
            system_lang: lang_tag(Lang::detect()),
            version: env!("CARGO_PKG_VERSION"),
            editor: choice,
            editors: vec![
                EditorOption { id: EditorChoice::LibreOffice, name: "LibreOffice", installed: LibreOffice::find().is_some() },
                EditorOption { id: EditorChoice::MsOffice, name: "Microsoft Office", installed: MsOffice::installed() },
            ],
            tray: tray_on,
            tray_note: if !tray_available {
                Some("library")
            } else if gnome {
                Some("gnome")
            } else {
                None
            },
            tray_available,
            prompts_off: Prompts::new(&ctx.dirs.state).are_off(),
            prompts_available: !office_profiles().is_empty(),
            autostart,
            folder: ctx.dirs.folder.clone(),
            updates_supported: crate::updater::supported(),
            updates_auto,
            update_available,
        })
    })
    .await
}

fn save(connector: &Connector, change: impl FnOnce(&mut crate::settings::Settings)) -> Result<()> {
    let mut settings = connector.settings.lock().unwrap();
    change(&mut settings);
    settings.save(&connector.ctx.dirs.state).map_err(|e| connector.ctx.failed("saving the settings", &e))
}

/// For documents opened from now on.
#[tauri::command]
pub fn set_editor(connector: State<'_, Connector>, editor: EditorChoice) -> Result<()> {
    save(&connector, |s| s.editor = Some(editor))?;
    let editor: Arc<dyn Editor> = editor.editor();
    log::info!("editor: {}", editor.name());
    connector.ctx.set_editor(editor);
    Ok(())
}

/// None: the system's language. Returns the language now in use.
#[tauri::command]
pub fn set_language(app: AppHandle, connector: State<'_, Connector>, language: Option<Language>) -> Result<&'static str> {
    let mut lang = Lang::detect();
    save(&connector, |s| {
        s.language = language;
        lang = s.lang();
    })?;
    log::info!("language: {}", lang_tag(lang));
    connector.ctx.set_lang(lang);
    tray::refresh(&app);
    Ok(lang_tag(lang))
}

#[tauri::command]
pub fn set_tray(app: AppHandle, connector: State<'_, Connector>, on: bool) -> Result<()> {
    save(&connector, |s| s.tray = Some(on))?;
    tray::set(&app, on);
    Ok(())
}

/// Turn LibreOffice's format question and macro bar off, or restore them.
#[tauri::command]
pub async fn set_prompts(connector: State<'_, Connector>, off: bool) -> Result<()> {
    blocking(&connector.ctx, move |ctx| {
        let prompts = Prompts::new(&ctx.dirs.state);
        let running = LibreOffice::new().running();
        let result = if off { prompts.turn_off(&office_profiles(), running) } else { prompts.restore(running) };
        match result {
            Ok(changed) => {
                log::info!("LibreOffice prompts {}: {changed:?}", if off { "off" } else { "restored" });
                Ok(())
            }
            Err(PromptsError::NoProfiles) => Err(ctx.tr(Msg::NoLibreOfficeSettings, &[])),
            Err(PromptsError::Running) => Err(ctx.tr(Msg::LibreOfficeRunning, &[])),
            Err(PromptsError::Io(e)) => Err(ctx.failed("changing LibreOffice's settings", &e)),
        }
    })
    .await
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, connector: State<'_, Connector>, on: bool) -> Result<()> {
    save(&connector, |s| s.autostart = Some(on))?;
    let result = crate::autostart::set(&app, on);
    result.map_err(|e| connector.ctx.failed("changing the start at login", &e))
}

#[tauri::command]
pub fn set_updates(connector: State<'_, Connector>, on: bool) -> Result<()> {
    save(&connector, |s| s.updates = Some(on))
}

/// The version an update would install, if any.
#[tauri::command]
pub async fn check_update(app: AppHandle, connector: State<'_, Connector>) -> Result<Option<String>> {
    crate::updater::check(&app).await.map_err(|e| connector.ctx.failed("looking for updates", &e))
}

/// Install the update found and start again; refused while documents are open.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<()> {
    crate::updater::install(&app).await
}
