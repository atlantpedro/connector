//! The ms-word:/ms-excel: links: taken over from the program that had them, handed back on
//! uninstall. Also clears away what lodav.py's installers registered, once lodav.py is gone.
//!
//! - Windows: the installer writes the scheme keys under HKCU and its pre-install hook saves
//!   what was there before (app/windows/hooks.nsh). `--setup` registers the app for
//!   Settings > Default apps; `--uninstall` puts the saved handlers back.
//! - macOS: the bundle declares the schemes; the first run makes the app their default.
//! - Linux: the first run makes the app the default in mimeapps.list and remembers the previous
//!   default for `--uninstall`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// As in tauri.conf.json; a test keeps them equal.
pub const APP_ID: &str = "io.github.atlantpedro.connector";
pub const SCHEMES: [&str; 2] = ["ms-word", "ms-excel"];

/// What the takeover changed, in the state dir.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Record {
    /// The handler set: a desktop file id on Linux, the bundle id on macOS.
    handler: String,
    /// Per MIME type, the previous default (Linux); none: there was none, or it was lodav.py.
    #[serde(default)]
    previous: std::collections::BTreeMap<String, Option<String>>,
}

fn record_path(state: &Path) -> PathBuf {
    state.join("handover.json")
}

fn read_record(state: &Path) -> Option<Record> {
    serde_json::from_str(&fs::read_to_string(record_path(state)).ok()?).ok()
}

fn write_record(state: &Path, record: &Record) -> io::Result<()> {
    fs::create_dir_all(state)?;
    lodav_core::store::write_atomic(&record_path(state), &serde_json::to_string_pretty(record)?)
}

/// At every start: take the links over the first time, keep the registration current, and
/// clear away lodav.py's.
pub fn at_start(state: &Path) {
    remove_lodav();
    if let Err(e) = take_over(state) {
        log::warn!("could not take over the Office links: {e}");
    }
}

/// `--setup`, run by the Windows installer once the app is in place.
pub fn setup(state: &Path) -> io::Result<()> {
    remove_lodav();
    take_over(state)?;
    #[cfg(windows)]
    windows::ask_for_default();
    Ok(())
}

/// `--uninstall`: hand the links back to whatever had them before.
pub fn hand_back(state: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    linux::hand_back(state)?;
    #[cfg(windows)]
    windows::hand_back()?;
    let _ = fs::remove_file(record_path(state));
    Ok(())
}

fn take_over(state: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    return linux::take_over(state);
    #[cfg(target_os = "macos")]
    return macos::take_over(state);
    #[cfg(windows)]
    {
        let _ = state;
        return windows::register();
    }
    #[allow(unreachable_code)]
    {
        let _ = state;
        Ok(())
    }
}

/// lodav.py's link handler, start at login and script; its folders are moved over by
/// `migrate`. lodav.py running at this moment is harmless: it finishes its session.
fn remove_lodav() {
    #[cfg(target_os = "linux")]
    linux::remove_lodav();
    #[cfg(target_os = "macos")]
    macos::remove_lodav();
    #[cfg(windows)]
    windows::remove_lodav();
}

/// Desktop file ids that are this app or lodav.py, never to be handed back to.
/// (de.wntd.…: the app id of development builds before the first release.)
const OURS: [&str; 4] = [
    "lodav.desktop",
    "de.wntd.datenportal-connector.desktop",
    "Datenportal Connector.desktop",
    "io.github.atlantpedro.connector.desktop",
];

/// The first default for mime in a mimeapps.list.
pub fn default_in(text: &str, mime: &str) -> Option<String> {
    let mut in_defaults = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_defaults = line == "[Default Applications]";
        } else if in_defaults {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == mime {
                    return value.split(';').map(str::trim).find(|v| !v.is_empty()).map(String::from);
                }
            }
        }
    }
    None
}

/// The mimeapps.list text with mime's default set to id, or removed.
pub fn with_default(text: &str, mime: &str, id: Option<&str>) -> String {
    let mut out: Vec<String> = Vec::new();
    let (mut in_defaults, mut seen_section, mut done) = (false, false, false);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_defaults && !done {
                if let Some(id) = id {
                    out.push(format!("{mime}={id};"));
                }
                done = true;
            }
            in_defaults = trimmed == "[Default Applications]";
            seen_section |= in_defaults;
        } else if in_defaults && trimmed.split_once('=').is_some_and(|(key, _)| key.trim() == mime) {
            if !done {
                if let Some(id) = id {
                    out.push(format!("{mime}={id};"));
                }
                done = true;
            }
            continue;
        }
        out.push(line.to_string());
    }
    if !done {
        if let Some(id) = id {
            if !seen_section {
                if out.last().is_some_and(|l| !l.trim().is_empty()) {
                    out.push(String::new());
                }
                out.push("[Default Applications]".into());
            }
            out.push(format!("{mime}={id};"));
        }
    }
    if out.is_empty() {
        return String::new();
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use lodav_core::dirs::home;
    use lodav_core::host::in_flatpak;

    fn env_dir(key: &str, fallback: &str) -> PathBuf {
        // in the Flatpak, XDG_* name the sandbox's dirs
        let set = if in_flatpak() { None } else { std::env::var_os(key).filter(|v| !v.is_empty()) };
        set.map(PathBuf::from).unwrap_or_else(|| home().join(fallback))
    }

    fn mimeapps() -> PathBuf {
        env_dir("XDG_CONFIG_HOME", ".config").join("mimeapps.list")
    }

    fn applications() -> PathBuf {
        env_dir("XDG_DATA_HOME", ".local/share").join("applications")
    }

    fn mime(scheme: &str) -> String {
        format!("x-scheme-handler/{scheme}")
    }

    /// Write in place: in the Flatpak, mimeapps.list is a single file shared into the sandbox,
    /// which a rename can't replace.
    fn write_mimeapps(text: &str) -> io::Result<()> {
        let path = mimeapps();
        if in_flatpak() {
            return fs::write(path, text);
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        lodav_core::store::write_atomic(&path, text)
    }

    pub fn take_over(state: &Path) -> io::Result<()> {
        let handler = handler()?;
        if read_record(state).is_some_and(|r| r.handler == handler) {
            return Ok(()); // done before; the user may have chosen another handler since
        }
        let text = fs::read_to_string(mimeapps()).unwrap_or_default();
        let mut record = Record { handler: handler.clone(), ..Record::default() };
        let mut new = text.clone();
        for scheme in SCHEMES {
            let mime = mime(scheme);
            let previous = default_in(&text, &mime).filter(|p| !OURS.contains(&p.as_str()));
            record.previous.insert(mime.clone(), previous);
            new = with_default(&new, &mime, Some(&handler));
        }
        write_mimeapps(&new)?;
        write_record(state, &record)?;
        log::info!("took over the Office links: {handler}, previously {:?}", record.previous);
        Ok(())
    }

    pub fn hand_back(state: &Path) -> io::Result<()> {
        let Some(record) = read_record(state) else { return Ok(()) };
        let text = fs::read_to_string(mimeapps()).unwrap_or_default();
        let mut new = text.clone();
        for (mime, previous) in &record.previous {
            if default_in(&text, mime).as_deref() == Some(record.handler.as_str()) {
                new = with_default(&new, mime, previous.as_deref());
            }
        }
        if new != text {
            write_mimeapps(&new)?;
        }
        if !in_flatpak() {
            let _ = fs::remove_file(applications().join(format!("{APP_ID}.desktop")));
            let _ = fs::remove_file(icon_path());
            update_desktop_database();
        }
        // the start at login: the Flatpak's own (see autostart.rs), else as the plugin names it
        let autostart = match std::env::var("FLATPAK_ID") {
            Ok(id) => format!("{id}.desktop"),
            Err(_) => "Datenportal Connector.desktop".into(),
        };
        let _ = fs::remove_file(env_dir("XDG_CONFIG_HOME", ".config").join("autostart").join(autostart));
        log::info!("handed the Office links back: {:?}", record.previous);
        Ok(())
    }

    fn icon_path() -> PathBuf {
        env_dir("XDG_DATA_HOME", ".local/share").join(format!("icons/hicolor/128x128/apps/{APP_ID}.png"))
    }

    /// The desktop file that handles the links. The Flatpak exports its own; otherwise one is
    /// written for the binary or AppImage that runs now, also when a package brought a menu
    /// entry: that one is named after the product, with a space.
    fn handler() -> io::Result<String> {
        if let Some(id) = std::env::var_os("FLATPAK_ID") {
            return Ok(format!("{}.desktop", id.to_string_lossy()));
        }
        let exe = std::env::var_os("APPIMAGE").map(PathBuf::from).map_or_else(std::env::current_exe, Ok)?;
        let packaged = data_dirs().iter().any(|d| d.join("applications/Datenportal Connector.desktop").exists());
        let icon = if packaged {
            "datenportal-connector".to_string()
        } else {
            let path = icon_path();
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(&path, include_bytes!("../icons/128x128.png"))?;
            APP_ID.to_string()
        };
        let desktop = desktop_entry(&exe, &icon, packaged);
        let dir = applications();
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{APP_ID}.desktop"));
        if fs::read_to_string(&path).ok().as_deref() != Some(desktop.as_str()) {
            fs::write(&path, desktop)?;
            update_desktop_database();
        }
        Ok(format!("{APP_ID}.desktop"))
    }

    fn data_dirs() -> Vec<PathBuf> {
        let dirs = std::env::var("XDG_DATA_DIRS").ok().filter(|d| !d.is_empty());
        dirs.as_deref().unwrap_or("/usr/local/share:/usr/share").split(':').map(PathBuf::from).collect()
    }

    fn update_desktop_database() {
        let _ = lodav_core::host::succeeds(std::process::Command::new("update-desktop-database").arg(applications()));
    }

    pub fn remove_lodav() {
        if in_flatpak() {
            return; // can't see the host's files; lodav.py's handler is replaced in mimeapps.list
        }
        let desktop = applications().join("lodav.desktop");
        let is_lodav = fs::read_to_string(&desktop).is_ok_and(|t| t.contains("/.local/bin/lodav"));
        if !is_lodav {
            return;
        }
        log::info!("removing lodav.py's link handler and start at login");
        let _ = fs::remove_file(&desktop);
        let script = home().join(".local/bin/lodav");
        if fs::read_to_string(&script).is_ok_and(|t| t.starts_with("#!") && t.contains("python")) {
            let _ = fs::remove_file(script);
        }
        let _ = fs::remove_file(env_dir("XDG_CONFIG_HOME", ".config").join("autostart/lodav-recover.desktop"));
        update_desktop_database();
    }

    #[cfg(test)]
    #[test]
    fn desktop_entry_quotes_the_path() {
        let entry = desktop_entry(Path::new("/opt/My \"Apps\"/connector"), "x", true);
        assert!(entry.contains(r#"Exec="/opt/My \\"Apps\\"/connector" %u"#), "{entry}");
        assert!(entry.contains("TryExec=/opt/My \"Apps\"/connector\n"));
        assert!(entry.contains("NoDisplay=true"));
    }
}

/// A desktop file for exe. A packaged menu entry exists already: then this one only handles
/// the links. TryExec makes desktops ignore it once exe is gone.
#[cfg(any(target_os = "linux", test))]
pub fn desktop_entry(exe: &Path, icon: &str, packaged: bool) -> String {
    let exe = exe.to_string_lossy();
    // quoted per the Desktop Entry spec, then escaped as a string value
    let quoted = exe.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$");
    let exec = format!("\"{quoted}\" %u").replace('\\', "\\\\");
    let try_exec = exe.replace('\\', "\\\\");
    format!(
        "[Desktop Entry]
Type=Application
Name=Datenportal Connector
Comment=Edit Datenportal CMS documents in LibreOffice or Microsoft Office
Comment[de]=Datenportal-CMS-Dokumente in LibreOffice oder Microsoft Office bearbeiten
Keywords=CMS;LibreOffice;Datenportal;
Icon={icon}
Categories=Office;
Exec={exec}
TryExec={try_exec}
MimeType=x-scheme-handler/ms-word;x-scheme-handler/ms-excel;
Terminal=false
StartupNotify=false
NoDisplay={packaged}
"
    )
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use lodav_core::dirs::home;
    use std::process::Command;

    pub fn take_over(state: &Path) -> io::Result<()> {
        if read_record(state).is_some_and(|r| r.handler == APP_ID) {
            return Ok(());
        }
        for scheme in SCHEMES {
            // LaunchServices has no command line tool for this; JavaScript for Automation does
            let script = format!(
                "ObjC.import('CoreServices'); $.LSSetDefaultHandlerForURLScheme($('{scheme}'), $('{APP_ID}'))"
            );
            let output = Command::new("osascript").args(["-l", "JavaScript", "-e", &script]).output()?;
            let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if status != "0" {
                return Err(io::Error::other(format!("LSSetDefaultHandlerForURLScheme({scheme}): {status}")));
            }
        }
        write_record(state, &Record { handler: APP_ID.into(), ..Record::default() })?;
        log::info!("took over the Office links");
        Ok(())
    }

    pub fn remove_lodav() {
        let app = home().join("Applications/Datenportal LibreOffice Bridge.app");
        let agent = home().join("Library/LaunchAgents/de.lodav.handler.recover.plist");
        if !app.exists() && !agent.exists() {
            return;
        }
        log::info!("removing lodav.py's link handler and start at login");
        if app.exists() {
            let lsregister = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
            let _ = lodav_core::host::succeeds(Command::new(lsregister).arg("-u").arg(&app));
            let _ = fs::remove_dir_all(&app);
        }
        if agent.exists() {
            let uid = Command::new("id").arg("-u").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
            if let Ok(uid) = uid {
                let _ = lodav_core::host::succeeds(Command::new("launchctl").arg("bootout").arg(format!("gui/{uid}")).arg(&agent));
            }
            let _ = fs::remove_file(&agent);
        }
        let _ = fs::remove_file(home().join("Library/Application Support/lodav/lodav.py"));
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const APP_NAME: &str = lodav_core::dirs::APP_NAME;
    /// Where the installer's hook saved the previous handlers, and the app's capabilities.
    const APP_KEY: &str = r"Software\Datenportal Connector";
    const PROG_ID: &str = "DatenportalConnector.Link";
    const REGISTERED_APPS: &str = r"Software\RegisteredApplications";

    fn hkcu() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }

    fn command() -> io::Result<String> {
        Ok(format!("\"{}\" \"%1\"", std::env::current_exe()?.display()))
    }

    fn is_ours(command: &str) -> bool {
        command.to_ascii_lowercase().contains("datenportal-connector.exe")
    }

    /// Registered as an application, Windows lists the app in Settings > Default apps for the
    /// two link types; without it, a user who chose another program there can't choose this one.
    pub fn register() -> io::Result<()> {
        let hkcu = hkcu();
        let command = command()?;
        let (prog, _) = hkcu.create_subkey(format!(r"Software\Classes\{PROG_ID}"))?;
        prog.set_value("", &APP_NAME)?;
        prog.set_value("URL Protocol", &"")?;
        hkcu.create_subkey(format!(r"Software\Classes\{PROG_ID}\shell\open\command"))?.0.set_value("", &command)?;
        let (caps, _) = hkcu.create_subkey(format!(r"{APP_KEY}\Capabilities"))?;
        caps.set_value("ApplicationName", &APP_NAME)?;
        caps.set_value("ApplicationDescription", &"Opens Office links from the Datenportal CMS.")?;
        let (urls, _) = hkcu.create_subkey(format!(r"{APP_KEY}\Capabilities\URLAssociations"))?;
        for scheme in SCHEMES {
            urls.set_value(scheme, &PROG_ID)?;
            // lodav.py marked the keys it took over; the installer's hook kept what they say
            if let Ok(key) = hkcu.create_subkey(format!(r"Software\Classes\{scheme}")).map(|(k, _)| k) {
                for name in ["lodav", "lodavPreviousName", "lodavPreviousCommand"] {
                    let _ = key.delete_value(name);
                }
            }
        }
        hkcu.create_subkey(REGISTERED_APPS)?.0.set_value(APP_NAME, &format!(r"{APP_KEY}\Capabilities"))?;
        notify_associations();
        Ok(())
    }

    /// Windows keeps the user's choice of program per link type, and programs can't change
    /// it. If another one is chosen, open the settings where the user can pick this app.
    pub fn ask_for_default() {
        let other_choice = SCHEMES.iter().any(|scheme| {
            let path = format!(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\{scheme}\UserChoice");
            let choice: Option<String> = hkcu().open_subkey(path).and_then(|k| k.get_value("ProgId")).ok();
            choice.is_some_and(|c| c != PROG_ID)
        });
        if other_choice {
            let encoded = APP_NAME.replace(' ', "%20");
            let url = format!("ms-settings:defaultapps?registeredAppUser={encoded}");
            let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
        }
    }

    /// Put back the handler the installer's hook saved for each scheme, and remove the app's
    /// registration.
    pub fn hand_back() -> io::Result<()> {
        let hkcu = hkcu();
        for scheme in SCHEMES {
            let saved = hkcu.open_subkey(format!(r"{APP_KEY}\Handover\{scheme}")).ok();
            let get = |name: &str| -> String { saved.as_ref().and_then(|k| k.get_value(name).ok()).unwrap_or_default() };
            let path = format!(r"Software\Classes\{scheme}");
            let current: String = hkcu.open_subkey(format!(r"{path}\shell\open\command")).and_then(|k| k.get_value("")).unwrap_or_default();
            if !current.is_empty() && !is_ours(&current) {
                continue; // changed by someone else since; theirs now
            }
            // what lodav.py had found before it took over, else what the hook found
            let (name, previous) = match get("LodavMarker").as_str() {
                "takeover" => (get("LodavPreviousName"), get("LodavPreviousCommand")),
                "created" => (String::new(), String::new()),
                _ if get("Command").to_ascii_lowercase().contains("lodav.py") => (String::new(), String::new()),
                _ => (get("Name"), get("Command")),
            };
            if previous.is_empty() || is_ours(&previous) {
                let _ = hkcu.delete_subkey_all(&path);
                continue;
            }
            let (key, _) = hkcu.create_subkey(&path)?;
            key.set_value("", &name)?;
            key.set_value("URL Protocol", &"")?;
            hkcu.create_subkey(format!(r"{path}\shell\open\command"))?.0.set_value("", &previous)?;
            let _ = hkcu.delete_subkey_all(format!(r"{path}\DefaultIcon")); // the installer's, ours
        }
        let _ = hkcu.delete_subkey_all(APP_KEY);
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{PROG_ID}"));
        if let Ok(apps) = hkcu.create_subkey(REGISTERED_APPS).map(|(k, _)| k) {
            let _ = apps.delete_value(APP_NAME);
        }
        notify_associations();
        Ok(())
    }

    pub fn remove_lodav() {
        let hkcu = hkcu();
        let had_lodav = hkcu.open_subkey(r"Software\lodav").is_ok();
        let _ = hkcu.delete_subkey_all(r"Software\lodav");
        let _ = hkcu.delete_subkey_all(r"Software\Classes\lodav.link");
        if let Ok(apps) = hkcu.create_subkey(REGISTERED_APPS).map(|(k, _)| k) {
            let _ = apps.delete_value(lodav_core::dirs::LEGACY_APP_NAME);
        }
        if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
            let programs = appdata.join(r"Microsoft\Windows\Start Menu\Programs");
            let name = lodav_core::dirs::LEGACY_APP_NAME;
            let _ = fs::remove_file(programs.join(format!("{name}.lnk")));
            let _ = fs::remove_file(programs.join(format!(r"Startup\{name} (recovery).lnk")));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            let _ = fs::remove_file(local.join(r"lodav\lodav.py"));
        }
        if had_lodav {
            log::info!("removed lodav.py's link handler and start at login");
            notify_associations();
        }
    }

    /// Without this, Windows keeps using its cached list of link handlers until the next
    /// sign-in.
    fn notify_associations() {
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn SHChangeNotify(event: i32, flags: u32, item1: *const std::ffi::c_void, item2: *const std::ffi::c_void);
        }
        const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
        // SAFETY: documented call with no items
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, 0, std::ptr::null(), std::ptr::null()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_matches_the_config() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], APP_ID);
        assert!(OURS.contains(&format!("{APP_ID}.desktop").as_str()));
    }

    #[test]
    fn mimeapps_defaults() {
        let text = "[Added Associations]\nx-scheme-handler/ms-word=a.desktop;\n\n[Default Applications]\nx-scheme-handler/ms-word=lo.desktop;other.desktop;\ntext/html=firefox.desktop\n";
        assert_eq!(default_in(text, "x-scheme-handler/ms-word").as_deref(), Some("lo.desktop"));
        assert_eq!(default_in(text, "x-scheme-handler/ms-excel"), None);

        let ours = with_default(text, "x-scheme-handler/ms-word", Some("us.desktop"));
        assert_eq!(default_in(&ours, "x-scheme-handler/ms-word").as_deref(), Some("us.desktop"));
        assert!(ours.contains("[Added Associations]\nx-scheme-handler/ms-word=a.desktop;"), "{ours}");
        assert!(ours.contains("text/html=firefox.desktop"));
        let both = with_default(&ours, "x-scheme-handler/ms-excel", Some("us.desktop"));
        assert_eq!(default_in(&both, "x-scheme-handler/ms-excel").as_deref(), Some("us.desktop"));

        let back = with_default(&both, "x-scheme-handler/ms-word", Some("lo.desktop"));
        let back = with_default(&back, "x-scheme-handler/ms-excel", None);
        assert_eq!(default_in(&back, "x-scheme-handler/ms-word").as_deref(), Some("lo.desktop"));
        assert_eq!(default_in(&back, "x-scheme-handler/ms-excel"), None);
        assert!(back.contains("text/html=firefox.desktop"));
    }

    #[test]
    fn mimeapps_without_a_defaults_section() {
        let text = "[Added Associations]\nimage/png=eog.desktop;";
        let new = with_default(text, "x-scheme-handler/ms-word", Some("us.desktop"));
        assert_eq!(new, "[Added Associations]\nimage/png=eog.desktop;\n\n[Default Applications]\nx-scheme-handler/ms-word=us.desktop;\n");
        assert_eq!(with_default("", "x-scheme-handler/ms-word", None), "");
        let fresh = with_default("", "x-scheme-handler/ms-word", Some("us.desktop"));
        assert_eq!(default_in(&fresh, "x-scheme-handler/ms-word").as_deref(), Some("us.desktop"));
    }
}
