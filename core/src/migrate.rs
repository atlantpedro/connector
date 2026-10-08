//! Moving over from lodav.py, the Python version: its folders, unfinished sessions and
//! messages.

use std::fs;
use std::io;
use std::path::Path;

use chrono::{Local, NaiveDateTime, TimeZone};

use crate::folder::{keep_name, move_file};
use crate::store::{FileLock, State};
use crate::{Context, Dirs, Message};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Migrated {
    pub sessions: usize,
    pub files: usize,
    pub messages: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Migration {
    /// No lodav.py folders (any more).
    Nothing,
    /// A lodav.py process still holds a session; try again on a later run.
    Postponed,
    Done(Migrated),
}

/// Move lodav.py's folders (as given by legacy) over to ctx's.
pub fn migrate(ctx: &Context, legacy: &Dirs) -> io::Result<Migration> {
    if !legacy.state.exists() && !legacy.cache.exists() && !legacy.folder.exists() {
        return Ok(Migration::Nothing);
    }
    if lodav_running(&legacy.sessions()) {
        log::info!("lodav.py is still running a session; moving its folders later");
        return Ok(Migration::Postponed);
    }
    let mut done = Migrated::default();
    fs::create_dir_all(ctx.sessions())?;
    fs::create_dir_all(&ctx.dirs.cache)?;

    if let Ok(entries) = fs::read_dir(legacy.sessions()) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.extension().is_some_and(|e| e == "json") {
                let target = ctx.sessions().join(&name);
                if target.exists() {
                    continue; // can't both be real; keep the old one where it is
                }
                match State::read(&path) {
                    Ok(mut state) => {
                        if let Ok(rel) = state.local.strip_prefix(&legacy.cache).map(Path::to_path_buf) {
                            state.local = ctx.dirs.cache.join(&rel);
                            // the document's working directory, <cache>/<key>
                            if let Some(dir) = rel.components().next() {
                                let to = ctx.dirs.cache.join(dir);
                                if !to.exists() {
                                    move_dir(&legacy.cache.join(dir), &to)?;
                                }
                            }
                        }
                        state.write(&target)?;
                        fs::remove_file(&path)?;
                        done.sessions += 1;
                    }
                    // kept for a look, as recover_stale does with an unreadable file
                    Err(_) => fs::rename(&path, ctx.sessions().join(format!("{}.broken", name.to_string_lossy())))?,
                }
            } else if name.to_string_lossy().ends_with(".broken") {
                let target = ctx.sessions().join(&name);
                if !target.exists() {
                    move_file(&path, &target)?;
                }
            } else if path.extension().is_some_and(|e| e == "lock") {
                let _ = fs::remove_file(&path);
            }
        }
    }

    // working copies without a session file are kept when their document is opened again
    if let Ok(entries) = fs::read_dir(&legacy.cache) {
        for entry in entries.flatten() {
            let to = ctx.dirs.cache.join(entry.file_name());
            if !to.exists() {
                move_dir(&entry.path(), &to)?;
            }
        }
    }

    for marker in ["office-prompts.json", "format-warning.json"] {
        let from = legacy.state.join(marker);
        let to = ctx.dirs.state.join(marker);
        if from.exists() && !to.exists() {
            move_file(&from, &to)?;
        }
    }
    let _ = fs::remove_file(legacy.state.join("lodav.log"));

    if let Ok(entries) = fs::read_dir(&legacy.folder) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name == "Messages.txt" || name == "Meldungen.txt" {
                done.messages += import_messages(ctx, &path)?;
                fs::remove_file(&path)?;
            } else {
                let to = keep_name(&ctx.user_folder()?, &name);
                if path.is_dir() { move_dir(&path, &to)? } else { move_file(&path, &to)? }
                done.files += 1;
            }
        }
    }

    // gone once empty; whatever couldn't be moved stays where it is
    for dir in [legacy.sessions(), legacy.state.clone(), legacy.cache.clone(), legacy.folder.clone()] {
        let _ = fs::remove_dir(dir);
    }
    if let (Some(a), Some(b)) = (legacy.state.parent(), legacy.cache.parent()) {
        if a == b {
            let _ = fs::remove_dir(a); // Windows: %LOCALAPPDATA%\lodav
        }
    }
    log::info!("moved over from lodav.py: {done:?}");
    Ok(Migration::Done(done))
}

/// True if a lodav.py process holds a session or recovery lock in sessions.
fn lodav_running(sessions: &Path) -> bool {
    let Ok(entries) = fs::read_dir(sessions) else { return false };
    entries.flatten().any(|e| {
        let path = e.path();
        path.extension().is_some_and(|x| x == "lock") && !FileLock::new(path).try_acquire().unwrap_or(false)
    })
}

/// Move a directory, also to another drive.
fn move_dir(from: &Path, to: &Path) -> io::Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)?.flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() { move_dir(&entry.path(), &target)? } else { move_file(&entry.path(), &target)? }
    }
    fs::remove_dir(from)
}

/// Add the messages in lodav.py's Messages.txt to the log, as seen. Returns how many.
fn import_messages(ctx: &Context, path: &Path) -> io::Result<usize> {
    let text = fs::read_to_string(path)?;
    let mut last = None;
    let mut count = 0;
    // "<YYYY-MM-DD HH:MM>  <message>", separated by blank lines, after a header paragraph
    for block in text.trim_start_matches('\u{feff}').split("\n\n") {
        let block = block.trim();
        if block.len() < 18 || !block.is_char_boundary(16) || !block.is_char_boundary(18) || &block[16..18] != "  " {
            continue;
        }
        let Ok(time) = NaiveDateTime::parse_from_str(&block[..16], "%Y-%m-%d %H:%M") else { continue };
        let Some(time) = Local.from_local_datetime(&time).earliest() else { continue };
        let message = Message {
            id: time.timestamp_millis().max(0) as u64,
            time: time.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            text: block[18..].to_string(),
            urgent: false,
            kept: None,
        };
        last = Some(ctx.messages.add(message)?.id);
        count += 1;
    }
    if let Some(id) = last {
        ctx.messages.mark_seen(id)?;
    }
    Ok(count)
}
