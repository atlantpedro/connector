//! Where working copies, state and the user's folder live.

use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "Datenportal Connector";
/// Name of the internal folders.
pub const DIR_NAME: &str = "datenportal-connector";
/// What lodav.py, the Python version, called them.
pub const LEGACY_DIR_NAME: &str = "lodav";
pub const LEGACY_APP_NAME: &str = "Datenportal LibreOffice Bridge";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOs,
    Windows,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

#[derive(Clone, Debug)]
pub struct Dirs {
    /// One subdirectory per document, holding the working copy.
    pub cache: PathBuf,
    /// Session files, the message log, settings.
    pub state: PathBuf,
    /// `<Documents>/Datenportal Connector`: versions set aside for the user.
    pub folder: PathBuf,
}

impl Dirs {
    pub fn current() -> Dirs {
        Dirs::for_user(DIR_NAME, APP_NAME)
    }

    /// The folders lodav.py used, to move them over.
    pub fn legacy() -> Dirs {
        Dirs::for_user(LEGACY_DIR_NAME, LEGACY_APP_NAME)
    }

    fn for_user(dir_name: &str, folder_name: &str) -> Dirs {
        let home = home();
        let env = |key: &str| std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from);
        let (cache, state) = app_dirs(Platform::current(), &env, &home, dir_name);
        Dirs { cache, state, folder: documents_dir(&home).join(folder_name) }
    }

    pub fn sessions(&self) -> PathBuf {
        self.state.join("sessions")
    }
}

/// The user's home folder.
pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// Cache and state dir following each platform's conventions.
pub fn app_dirs(platform: Platform, env: &dyn Fn(&str) -> Option<PathBuf>, home: &Path, name: &str) -> (PathBuf, PathBuf) {
    match platform {
        Platform::Windows => {
            let base = env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local")).join(name);
            (base.join("cache"), base.join("state"))
        }
        Platform::MacOs => {
            let library = home.join("Library");
            (library.join("Caches").join(name), library.join("Application Support").join(name))
        }
        Platform::Linux => (
            env("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache")).join(name),
            env("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local").join("state")).join(name),
        ),
    }
}

/// The user's Documents folder, also when it is localized (~/Dokumente) or redirected.
pub fn documents_dir(home: &Path) -> PathBuf {
    // xdg-user-dirs points it at $HOME when no Documents folder is configured
    dirs::document_dir().filter(|d| d != home).unwrap_or_else(|| home.join("Documents"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<PathBuf> {
        move |key| vars.iter().find(|(k, _)| *k == key).map(|(_, v)| PathBuf::from(v))
    }

    #[test]
    fn linux_xdg() {
        let env = env_of(&[("XDG_CACHE_HOME", "/c"), ("XDG_STATE_HOME", "/s")]);
        let dirs = app_dirs(Platform::Linux, &env, Path::new("/home/u"), DIR_NAME);
        assert_eq!(dirs, (Path::new("/c").join(DIR_NAME), Path::new("/s").join(DIR_NAME)));
    }

    #[test]
    fn linux_defaults() {
        let dirs = app_dirs(Platform::Linux, &env_of(&[]), Path::new("/home/u"), DIR_NAME);
        let home = Path::new("/home/u");
        assert_eq!(dirs, (home.join(".cache").join(DIR_NAME), home.join(".local").join("state").join(DIR_NAME)));
    }

    #[test]
    fn macos() {
        let (cache, state) = app_dirs(Platform::MacOs, &env_of(&[]), Path::new("/Users/u"), DIR_NAME);
        let library = Path::new("/Users/u/Library");
        assert_eq!(cache, library.join("Caches").join(DIR_NAME));
        assert_eq!(state, library.join("Application Support").join(DIR_NAME));
    }

    #[test]
    fn windows() {
        let env = env_of(&[("LOCALAPPDATA", "/appdata")]);
        let (cache, state) = app_dirs(Platform::Windows, &env, Path::new("/home/u"), DIR_NAME);
        assert_eq!(cache, Path::new("/appdata").join("datenportal-connector").join("cache"));
        assert_eq!(state, Path::new("/appdata").join("datenportal-connector").join("state"));
    }
}
