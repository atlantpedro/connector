//! LibreOffice: found per platform, marks an open document with `.~lock.<name>#`.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::Editor;
use crate::host::{self, process_running, spawn_detached, which};
use crate::dirs::Platform;
use crate::store::part_path;

pub struct LibreOffice {
    /// Program and leading arguments; found on each launch if not given.
    command: Option<Vec<String>>,
}

impl LibreOffice {
    pub fn new() -> LibreOffice {
        LibreOffice { command: None }
    }

    /// Use this command instead of looking for LibreOffice.
    pub fn with_command(command: Vec<String>) -> LibreOffice {
        LibreOffice { command: Some(command) }
    }

    /// The command that starts LibreOffice, if it is installed.
    pub fn find() -> Option<Vec<String>> {
        #[cfg(windows)]
        if let Some(exe) = windows_soffice() {
            return Some(vec![exe.to_string_lossy().into_owned()]);
        }
        #[cfg(target_os = "macos")]
        for apps in [PathBuf::from("/Applications"), dirs::home_dir().unwrap_or_default().join("Applications")] {
            let exe = apps.join("LibreOffice.app/Contents/MacOS/soffice");
            if exe.exists() {
                return Some(vec![exe.to_string_lossy().into_owned()]);
            }
        }
        for cmd in ["libreoffice", "soffice"] {
            if let Some(path) = which(cmd) {
                return Some(vec![path.to_string_lossy().into_owned()]);
            }
        }
        if cfg!(target_os = "linux") {
            if host::succeeds(host::command("flatpak").args(["info", "org.libreoffice.LibreOffice"])) {
                return Some(["flatpak", "run", "org.libreoffice.LibreOffice"].map(String::from).to_vec());
            }
        }
        None
    }
}

impl Default for LibreOffice {
    fn default() -> Self {
        LibreOffice::new()
    }
}

pub fn lock_file(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".~lock.{name}#"))
}

impl Editor for LibreOffice {
    fn name(&self) -> &str {
        "LibreOffice"
    }

    fn launch(&self, path: &Path, view: bool) -> io::Result<()> {
        let command = match &self.command {
            Some(c) => c.clone(),
            None => LibreOffice::find().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LibreOffice"))?,
        };
        let (program, lead) = command.split_first().ok_or_else(|| io::Error::other("empty command"))?;
        let mut args: Vec<&OsStr> = lead.iter().map(OsStr::new).collect();
        if view {
            args.push(OsStr::new("--view"));
        }
        args.push(path.as_os_str());
        spawn_detached(program, &args)
    }

    fn is_open(&self, path: &Path) -> bool {
        lock_file(path).exists()
    }

    fn running(&self) -> bool {
        process_running(if cfg!(target_os = "macos") { &["soffice"] } else { &["soffice.bin"] })
    }
}

#[cfg(windows)]
fn windows_soffice() -> Option<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    use winreg::RegKey;
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for view in [0, KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let Ok(key) = RegKey::predef(root).open_subkey_with_flags(r"SOFTWARE\LibreOffice\UNO\InstallPath", KEY_READ | view)
            else {
                continue;
            };
            if let Ok(dir) = key.get_value::<String, _>("") {
                let exe = Path::new(&dir).join("soffice.exe");
                if exe.exists() {
                    return Some(exe);
                }
            }
        }
    }
    ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|dir| Path::new(&dir).join(r"LibreOffice\program\soffice.exe"))
        .find(|exe| exe.exists())
}

/// LibreOffice settings the user may turn off, as (key, registry path, property):
///   format: the "Keep current format?" question on every save of a Word file
///   macros: the "Macros disabled" bar on documents with macros (the macros stay disabled)
const PROMPTS: [(&str, &str, &str); 2] = [
    ("format", "/org.openoffice.Office.Common/Save/Document", "WarnAlienFormat"),
    ("macros", "/org.openoffice.Office.UI.Infobar/Enabled", "MacrosDisabled"),
];

#[derive(Debug)]
pub enum PromptsError {
    /// LibreOffice has never been started, so there are no settings to change.
    NoProfiles,
    /// LibreOffice rewrites its settings when it exits, undoing the change.
    Running,
    Io(io::Error),
}

impl From<io::Error> for PromptsError {
    fn from(e: io::Error) -> PromptsError {
        PromptsError::Io(e)
    }
}

/// The LibreOffice prompts in PROMPTS, turned off and restored in every profile of the user.
/// Only what we changed is restored; that is remembered per profile in the state dir.
pub struct Prompts {
    marker: PathBuf,
    old_marker: PathBuf,
}

fn prompt_item(path: &str, prop: &str, value: &str) -> String {
    format!(r#"<item oor:path="{path}"><prop oor:name="{prop}" oor:op="fuse"><value>{value}</value></prop></item>"#)
}

/// The value of the first setting of this prompt in text, if any.
fn prompt_value(text: &str, path: &str, prop: &str) -> Option<bool> {
    let t = text.find(&prompt_item(path, prop, "true"));
    let f = text.find(&prompt_item(path, prop, "false"));
    match (t, f) {
        (Some(t), Some(f)) => Some(t < f),
        (Some(_), None) => Some(true),
        (None, Some(_)) => Some(false),
        (None, None) => None,
    }
}

fn remove_prompt(text: &str, path: &str, prop: &str) -> String {
    let mut text = text.to_string();
    for value in ["true", "false"] {
        let item = prompt_item(path, prop, value);
        text = text.replace(&format!("{item}\n"), "").replace(&item, "");
    }
    text
}

impl Prompts {
    pub fn new(state: &Path) -> Prompts {
        Prompts { marker: state.join("office-prompts.json"), old_marker: state.join("format-warning.json") }
    }

    /// The registrymodifications.xcu of each LibreOffice profile this user has.
    pub fn profiles(platform: Platform, env: &dyn Fn(&str) -> Option<PathBuf>, home: &Path) -> Vec<PathBuf> {
        let dirs = match platform {
            Platform::Windows => vec![env("APPDATA").unwrap_or_else(|| home.join(r"AppData\Roaming")).join("LibreOffice")],
            Platform::MacOs => vec![home.join("Library/Application Support/LibreOffice")],
            Platform::Linux => vec![
                env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")).join("libreoffice"),
                home.join(".var/app/org.libreoffice.LibreOffice/config/libreoffice"),
                home.join("snap/libreoffice/current/.config/libreoffice"),
            ],
        };
        dirs.into_iter().map(|d| d.join("4").join("user").join("registrymodifications.xcu")).filter(|f| f.exists()).collect()
    }

    /// True if we turned prompts off and haven't restored them.
    pub fn are_off(&self) -> bool {
        !self.changed().is_empty()
    }

    fn changed(&self) -> BTreeMap<String, Vec<String>> {
        let mut changed: BTreeMap<String, Vec<String>> =
            fs::read_to_string(&self.marker).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        // written by the first version of this feature in lodav.py
        if let Some(files) = fs::read_to_string(&self.old_marker).ok().and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok()) {
            for f in files {
                changed.entry(f).or_insert_with(|| vec!["format".into()]);
            }
        }
        changed
    }

    /// Turn the prompts off in these profiles. Returns the files changed.
    pub fn turn_off(&self, profiles: &[PathBuf], running: bool) -> Result<Vec<PathBuf>, PromptsError> {
        if profiles.is_empty() {
            return Err(PromptsError::NoProfiles);
        }
        self.apply(profiles.to_vec(), running, true)
    }

    /// Undo what turn_off changed. Returns the files changed.
    pub fn restore(&self, running: bool) -> Result<Vec<PathBuf>, PromptsError> {
        let files = self.changed().into_keys().map(PathBuf::from).collect();
        self.apply(files, running, false)
    }

    fn apply(&self, files: Vec<PathBuf>, running: bool, off: bool) -> Result<Vec<PathBuf>, PromptsError> {
        let mut changed = self.changed();
        if !files.is_empty() && running {
            return Err(PromptsError::Running);
        }
        let mut written = Vec::new();
        for f in files {
            let key = f.to_string_lossy().into_owned();
            let old = fs::read_to_string(&f).unwrap_or_default();
            let text = if off {
                let Some(end) = old.rfind("</oor:items>") else {
                    log::warn!("not changed, unexpected content: {}", f.display());
                    continue;
                };
                let (mut text, tail) = (old[..end].to_string(), &old[end..]);
                for (name, path, prop) in PROMPTS {
                    if prompt_value(&text, path, prop) == Some(false) {
                        continue; // turned off by the user; theirs to keep
                    }
                    text = remove_prompt(&text, path, prop) + &prompt_item(path, prop, "false") + "\n";
                    let keys = changed.entry(key.clone()).or_default();
                    if !keys.iter().any(|k| k == name) {
                        keys.push(name.to_string());
                        keys.sort();
                    }
                }
                text + tail
            } else {
                let mut text = old.clone();
                for name in changed.remove(&key).unwrap_or_default() {
                    if let Some((_, path, prop)) = PROMPTS.iter().find(|(n, _, _)| *n == name) {
                        text = remove_prompt(&text, path, prop);
                    }
                }
                text
            };
            if text == old {
                continue;
            }
            let tmp = part_path(&f);
            fs::write(&tmp, &text)?;
            fs::rename(&tmp, &f)?;
            written.push(f);
        }
        if changed.is_empty() {
            let _ = fs::remove_file(&self.marker);
        } else {
            fs::write(&self.marker, serde_json::to_string(&changed).map_err(io::Error::other)?)?;
        }
        let _ = fs::remove_file(&self.old_marker);
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_off_and_restore() {
        let tmp = tempfile::tempdir().unwrap();
        let other = r#"<item oor:path="/org.openoffice.Office.Calc/Content/Update"><prop oor:name="Link" oor:op="fuse"><value>2</value></prop></item>"#;
        let format_off = prompt_item(PROMPTS[0].1, PROMPTS[0].2, "false");
        let macros_off = prompt_item(PROMPTS[1].1, PROMPTS[1].2, "false");
        // the format question was already turned off by the user: it must be left alone
        let original = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<oor:items>\n{other}\n{format_off}\n</oor:items>");
        let profile = tmp.path().join("registrymodifications.xcu");
        fs::write(&profile, &original).unwrap();
        let prompts = Prompts::new(tmp.path());

        assert!(matches!(prompts.turn_off(&[profile.clone()], true), Err(PromptsError::Running)));
        assert_eq!(prompts.turn_off(&[profile.clone()], false).unwrap(), [profile.clone()]);
        let text = fs::read_to_string(&profile).unwrap();
        assert!(text.ends_with(&format!("{macros_off}\n</oor:items>")));
        assert_eq!(text.matches("WarnAlienFormat").count(), 1);
        assert!(text.contains(other));
        assert!(prompts.are_off());
        assert!(prompts.turn_off(&[profile.clone()], false).unwrap().is_empty()); // nothing left to change
        assert_eq!(fs::read_to_string(&profile).unwrap(), text);

        assert_eq!(prompts.restore(false).unwrap(), [profile.clone()]);
        assert_eq!(fs::read_to_string(&profile).unwrap(), original);
        assert!(!prompts.are_off());
        assert!(!tmp.path().join("office-prompts.json").exists());
    }

    #[test]
    fn no_profiles() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(Prompts::new(tmp.path()).turn_off(&[], false), Err(PromptsError::NoProfiles)));
    }

    #[test]
    fn lock_file_name() {
        assert_eq!(lock_file(Path::new("/w/a b.docx")), Path::new("/w/.~lock.a b.docx#"));
    }
}
