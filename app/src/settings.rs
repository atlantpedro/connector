//! The user's choices, kept in settings.json in the state dir. Unset means the default.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lodav_core::editor::{LibreOffice, MsOffice};
use lodav_core::store::write_atomic;
use lodav_core::{Editor, Lang};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditorChoice {
    LibreOffice,
    MsOffice,
}

impl EditorChoice {
    pub fn editor(self) -> Arc<dyn Editor> {
        match self {
            EditorChoice::LibreOffice => Arc::new(LibreOffice::new()),
            EditorChoice::MsOffice => Arc::new(MsOffice),
        }
    }

    /// LibreOffice if installed, else Microsoft Office if installed, else LibreOffice (whose
    /// absence the user is told about on the first click).
    pub fn default_choice() -> EditorChoice {
        if LibreOffice::find().is_none() && MsOffice::installed() { EditorChoice::MsOffice } else { EditorChoice::LibreOffice }
    }
}

/// A language chosen in the settings, over the system's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    De,
    En,
}

impl Language {
    pub fn lang(self) -> Lang {
        match self {
            Language::De => Lang::De,
            Language::En => Lang::En,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Unset: the system's language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<EditorChoice>,
    /// Unset: on where the desktop shows tray icons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tray: Option<bool>,
    /// Start at login, which finishes interrupted sessions. Unset: on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autostart: Option<bool>,
    /// Look for updates once a day. Unset: on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updates: Option<bool>,
}

fn path(state: &Path) -> PathBuf {
    state.join("settings.json")
}

impl Settings {
    pub fn load(state: &Path) -> Settings {
        let Ok(text) = fs::read_to_string(path(state)) else { return Settings::default() };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            log::warn!("settings.json unreadable ({e}), using the defaults");
            Settings::default()
        })
    }

    pub fn save(&self, state: &Path) -> std::io::Result<()> {
        fs::create_dir_all(state)?;
        write_atomic(&path(state), &serde_json::to_string_pretty(self)?)
    }

    pub fn lang(&self) -> Lang {
        self.language.map_or_else(Lang::detect, Language::lang)
    }

    pub fn editor(&self) -> EditorChoice {
        self.editor.unwrap_or_else(EditorChoice::default_choice)
    }
}
