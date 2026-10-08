//! The user's folder, where versions that didn't reach the CMS are kept.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{Context, Msg};

/// name in folder, or name with a number added if that is taken.
pub fn keep_name(folder: &Path, name: &str) -> PathBuf {
    let path = folder.join(name);
    let (stem, suffix) = split_name(name);
    let mut candidate = path;
    let mut n = 2;
    while candidate.exists() {
        candidate = folder.join(format!("{stem} ({n}){suffix}"));
        n += 1;
    }
    candidate
}

/// "a.docx" -> ("a", ".docx")
pub fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Move a file, also to another drive (the Documents folder may be on one).
pub fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)?;
    fs::remove_file(from)
}

/// Move path, a copy of working copy local with edits the CMS doesn't have, to the user's
/// folder and tell the user.
pub fn keep_unsaved(ctx: &Context, path: &Path, local: &Path) -> io::Result<PathBuf> {
    let name = local.file_name().unwrap_or_default().to_string_lossy();
    let (stem, suffix) = split_name(&name);
    let time = chrono::Local::now().format("%Y-%m-%d %H-%M").to_string();
    let kept_name = ctx.tr(Msg::KeptName, &[("stem", &stem), ("time", &time), ("suffix", &suffix)]);
    let kept = keep_name(&ctx.user_folder()?, &kept_name);
    move_file(path, &kept)?;
    let text = ctx.tr(Msg::NotUploadedKept, &[("name", &name), ("path", &kept.display())]);
    ctx.notify(text, true, Some(&kept));
    Ok(kept)
}

/// Delete a working copy whose changes are all in the CMS.
///
/// Left behind, the editor's "Recent Documents" would reopen it later; edits made there
/// never reach the CMS and are overwritten by the next download.
pub fn discard_working_copy(ctx: &Context, local: &Path) {
    // also an editor other than the current one: the setting may have changed meanwhile
    if ctx.editor().is_open(local) || crate::editor::open_in_any(local) {
        return; // opened again meanwhile
    }
    let _ = fs::remove_file(local);
    if let Some(dir) = local.parent() {
        let _ = fs::remove_dir(dir); // fails while view copies or other files are in there
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_name_never_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.docm"), "x").unwrap();
        fs::write(tmp.path().join("a (2).docm"), "x").unwrap();
        assert_eq!(keep_name(tmp.path(), "a.docm"), tmp.path().join("a (3).docm"));
        assert_eq!(keep_name(tmp.path(), "b"), tmp.path().join("b"));
    }

    #[test]
    fn split() {
        assert_eq!(split_name("a.b.docx"), ("a.b", ".docx"));
        assert_eq!(split_name(".hidden"), (".hidden", ""));
        assert_eq!(split_name("plain"), ("plain", ""));
    }
}
