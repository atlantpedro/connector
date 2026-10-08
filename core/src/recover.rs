//! Finishing sessions that ended early: shutdowns, forced reboots, power cuts, crashes.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::time::Duration;

use serde::Serialize;

use crate::dav::Dav;
use crate::folder::{discard_working_copy, keep_unsaved};
use crate::session::{release, settle, unsynced};
use crate::store::{FileLock, State};
use crate::{Context, Msg, Stop};

/// Held by whoever runs the session for this document.
pub fn session_lock(ctx: &Context, key: &str) -> FileLock {
    FileLock::new(ctx.sessions().join(format!("{key}.lock")))
}

/// One recovery pass at a time, across all threads and processes; dropping it ends the pass.
pub struct Recovery(#[allow(dead_code)] FileLock);

pub fn recovery(ctx: &Context, wait: Duration) -> io::Result<Recovery> {
    fs::create_dir_all(ctx.sessions())?;
    let mut lock = FileLock::new(ctx.sessions().join("recovery.lock"));
    if !lock.acquire(wait)? {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "another recovery pass takes too long"));
    }
    Ok(Recovery(lock))
}

/// Upload the edits and release the locks of sessions that ended early.
///
/// Returns the documents that still wait for the server. If told is given, the user is told
/// (once per document) that we keep trying. Call it while holding [`recovery`].
pub fn recover_stale(ctx: &Context, mut told: Option<&mut HashSet<String>>) -> Vec<String> {
    let mut waiting = Vec::new();
    let Ok(entries) = fs::read_dir(ctx.sessions()) else { return waiting };
    let mut files: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    for state_file in files {
        let key = state_file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let mut lock = crate::session_lock(ctx, &key);
        if !lock.try_acquire().unwrap_or(false) {
            continue; // its session is still running
        }
        let state = match State::read(&state_file) {
            Ok(state) => state,
            Err(e) => {
                // a working copy left behind is kept when its document is opened again
                log::warn!("unreadable session file {} ({e}), set aside", state_file.display());
                let mut broken = state_file.clone().into_os_string();
                broken.push(".broken");
                let _ = fs::rename(&state_file, broken);
                continue;
            }
        };
        log::info!("recovering session for {}", state.url);
        if settle(ctx, &state) {
            let _ = fs::remove_file(&state_file);
            ctx.activity.changed();
            continue;
        }
        let name = state.local.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if let Some(told) = told.as_deref_mut() {
            if told.insert(key) {
                ctx.notify(ctx.tr(Msg::EarlierPending, &[("name", &name)]), true, None);
            }
        }
        waiting.push(name);
    }
    waiting
}

/// Run at login: finish what earlier sessions left behind, however long that takes.
/// False if stopped first.
pub fn recover_loop(ctx: &Context, stop: &Stop) -> bool {
    let mut told = HashSet::new();
    let mut delay = Duration::from_secs(5).min(ctx.timing.retry_max);
    loop {
        match recovery(ctx, Duration::from_secs(120)) {
            Ok(_pass) => {
                if recover_stale(ctx, Some(&mut told)).is_empty() {
                    return true;
                }
            }
            Err(e) => log::warn!("{e}"),
        }
        if stop.wait(delay) {
            return false;
        }
        delay = (delay * 2).min(ctx.timing.retry_max);
    }
}

/// An earlier session that waits for the server, as the window shows it.
#[derive(Clone, Debug, Serialize)]
pub struct Pending {
    pub key: String,
    pub name: String,
    /// Edits the CMS doesn't have; without them, only the unlock is missing.
    pub unsynced: bool,
    /// When the session last saved its state; local time, RFC 3339.
    pub since: Option<String>,
}

/// Earlier sessions that are not finished yet, oldest first. Running sessions are left out.
pub fn pending(ctx: &Context) -> Vec<Pending> {
    let running: Vec<String> = ctx.activity.list().into_iter().map(|s| s.key).collect();
    let Ok(entries) = fs::read_dir(ctx.sessions()) else { return Vec::new() };
    let mut list: Vec<(std::time::SystemTime, Pending)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|path| {
            let key = path.file_stem()?.to_string_lossy().into_owned();
            if running.contains(&key) {
                return None;
            }
            let state = State::read(&path).ok()?;
            let modified = fs::metadata(&path).and_then(|m| m.modified()).ok()?;
            let since = chrono::DateTime::<chrono::Local>::from(modified);
            let pending = Pending {
                key,
                name: state.local.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                unsynced: unsynced(&state.local, state.uploaded),
                since: Some(since.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)),
            };
            Some((modified, pending))
        })
        .collect();
    list.sort_by_key(|(modified, _)| *modified);
    list.into_iter().map(|(_, p)| p).collect()
}

/// Stop trying to finish an earlier session, e.g. when the server keeps refusing it: keep the
/// edits the CMS lacks in the user's folder, try once more to unlock, and forget the session.
/// False if the session is running right now.
pub fn set_aside(ctx: &Context, key: &str) -> io::Result<bool> {
    let _pass = recovery(ctx, Duration::from_secs(60))?;
    let mut lock = session_lock(ctx, key);
    if !lock.try_acquire()? {
        return Ok(false);
    }
    let state_file = ctx.sessions().join(format!("{key}.json"));
    let state = State::read(&state_file)?;
    let name = state.local.file_name().unwrap_or_default().to_string_lossy().into_owned();
    log::info!("setting aside the session for {}", state.url);
    if unsynced(&state.local, state.uploaded) {
        keep_unsaved(ctx, &state.local, &state.local)?;
    }
    discard_working_copy(ctx, &state.local);
    let mut dav = Dav::new(ctx.http.clone(), &state.url, ctx.timing.settle_timeout);
    dav.token = state.token.clone();
    if let Err(e) = release(ctx, &mut dav, &name) {
        log::warn!("could not unlock {name}: {e}");
    }
    fs::remove_file(&state_file)?;
    ctx.record(ctx.tr(Msg::SetAside, &[("name", &name)]), None);
    ctx.activity.changed();
    Ok(true)
}
