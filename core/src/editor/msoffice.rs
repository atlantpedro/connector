//! Microsoft Word and Excel (Windows, macOS): mark an open document with a `~$…` owner file.
//!
//! Untested on real Office so far.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::Editor;
use crate::host::{process_running, spawn_detached};

const EXCEL_EXTENSIONS: [&str; 7] = ["xls", "xlsx", "xlsm", "xlsb", "xltx", "xltm", "csv"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum App {
    Word,
    Excel,
}

impl App {
    fn for_document(path: &Path) -> App {
        let ext = path.extension().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
        if EXCEL_EXTENSIONS.contains(&ext.as_str()) { App::Excel } else { App::Word }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn exe(self) -> &'static str {
        match self {
            App::Word => "WINWORD.EXE",
            App::Excel => "EXCEL.EXE",
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn bundle(self) -> &'static str {
        match self {
            App::Word => "Microsoft Word.app",
            App::Excel => "Microsoft Excel.app",
        }
    }

    /// Where the program is installed, if it is.
    fn find(self) -> Option<PathBuf> {
        #[cfg(windows)]
        {
            use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
            use winreg::RegKey;
            let key = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{}", self.exe());
            for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
                let Ok(k) = RegKey::predef(root).open_subkey_with_flags(&key, KEY_READ) else { continue };
                if let Ok(exe) = k.get_value::<String, _>("") {
                    let exe = PathBuf::from(exe.trim_matches('"'));
                    if exe.exists() {
                        return Some(exe);
                    }
                }
            }
            None
        }
        #[cfg(target_os = "macos")]
        {
            [PathBuf::from("/Applications"), dirs::home_dir().unwrap_or_default().join("Applications")]
                .into_iter()
                .map(|apps| apps.join(self.bundle()))
                .find(|app| app.exists())
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        None
    }

    fn name(self) -> &'static str {
        match self {
            App::Word => "Microsoft Word",
            App::Excel => "Microsoft Excel",
        }
    }
}

pub struct MsOffice;

impl MsOffice {
    /// True if Word is installed; Excel is looked for when a spreadsheet is opened.
    pub fn installed() -> bool {
        App::Word.find().is_some()
    }
}

/// True if an owner file for name is among names. Word shortens long names ("~$st Dokument.docm"
/// for "Test Dokument.docm"); Excel puts "~$" before the full name.
fn has_owner_file<'a>(name: &str, names: impl IntoIterator<Item = &'a str>) -> bool {
    names.into_iter().any(|other| {
        other.strip_prefix("~$").is_some_and(|rest| {
            !rest.is_empty() && name.ends_with(rest) && rest.chars().count() + 2 >= name.chars().count()
        })
    })
}

impl Editor for MsOffice {
    fn name(&self) -> &str {
        "Microsoft Office"
    }

    fn launch(&self, path: &Path, _view: bool) -> io::Result<()> {
        // view copies are made read-only on disk, which Office respects
        let app = App::for_document(path);
        let program = app.find().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, app.name()))?;
        if cfg!(target_os = "macos") {
            let args = [OsStr::new("-a"), program.as_os_str(), path.as_os_str()];
            return spawn_detached("open", &args);
        }
        spawn_detached(&program.to_string_lossy(), &[path.as_os_str()])
    }

    fn is_open(&self, path: &Path) -> bool {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return false };
        let Ok(entries) = fs::read_dir(dir) else { return false };
        let names: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        has_owner_file(&name.to_string_lossy(), names.iter().map(String::as_str))
    }

    fn running(&self) -> bool {
        if cfg!(target_os = "macos") {
            process_running(&["Microsoft Word", "Microsoft Excel"])
        } else {
            process_running(&["WINWORD.EXE", "EXCEL.EXE"])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_files() {
        assert!(has_owner_file("Test Dokument.docm", ["~$st Dokument.docm"])); // Word, long name
        assert!(has_owner_file("Test Dokument.xlsx", ["~$Test Dokument.xlsx"])); // Excel
        assert!(has_owner_file("abc.docx", ["~$abc.docx"])); // Word, short name
        assert!(!has_owner_file("Test Dokument.docm", ["Test Dokument.docm", "~$other.docm", "~$"]));
        assert!(!has_owner_file("Test Dokument.docm", ["~$docm"]));
    }

    #[test]
    fn app_by_extension() {
        assert_eq!(App::for_document(Path::new("a.XLSX")), App::Excel);
        assert_eq!(App::for_document(Path::new("a.docm")), App::Word);
        assert_eq!(App::for_document(Path::new("a")), App::Word);
    }
}
