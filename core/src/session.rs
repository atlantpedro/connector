//! One document from download to unlock.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::activity::Phase;
use crate::dav::{self, Dav};
use crate::folder::{discard_working_copy, keep_name, keep_unsaved, move_file};
use crate::recover::{recover_stale, recovery, session_lock};
use crate::store::{signature, Signature, State, PART};
use crate::uri::{file_name, parse_office_uri, session_key, Mode};
use crate::{Context, Editor, Error, Msg, Outcome, Stop};

/// How long a click waits for a recovery pass of another thread or process.
const RECOVERY_WAIT: Duration = Duration::from_secs(120);

/// True if the working copy has changes the CMS doesn't have.
pub(crate) fn unsynced(local: &Path, uploaded: Option<Signature>) -> bool {
    // uploaded is None if the download failed; never push a stale local copy then
    matches!((uploaded, signature(local)), (Some(u), Some(s)) if s != u)
}

fn name_of(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// UNLOCK and check that the lock is gone. Only transient errors are returned, so the caller
/// can retry.
pub(crate) fn release(ctx: &Context, dav: &mut Dav, name: &str) -> dav::Result<()> {
    if dav.token.is_none() {
        return Ok(());
    }
    match dav.unlock() {
        Err(e) if !e.is_transient() => {
            log::warn!("unlock refused ({e}); the lock has probably expired");
            dav.token = None;
            return Ok(());
        }
        other => other?,
    }
    match dav.is_locked() {
        Ok(true) => ctx.notify(ctx.tr(Msg::StillLocked, &[("name", &name)]), true, None),
        Ok(false) => {}
        Err(e) if e.is_transient() => return Err(e),
        Err(e) => log::warn!("could not check that the lock is gone: {e}"),
    }
    Ok(())
}

/// Upload what the CMS doesn't have yet, then release the lock.
///
/// Fails with a transient error while the server can't be reached, with Conflict if someone
/// else took over the document while our lock had expired.
fn finish(ctx: &Context, dav: &mut Dav, local: &Path, uploaded: Option<Signature>) -> dav::Result<()> {
    if unsynced(local, uploaded) {
        match dav.refresh() {
            Err(dav::Error::Http { .. }) => dav.relock()?,
            other => other?,
        }
        dav.upload(local)?;
    }
    release(ctx, dav, &name_of(local))
}

/// Finish a session that is no longer watched. False if it has to be tried again later.
pub(crate) fn settle(ctx: &Context, state: &State) -> bool {
    let local = &state.local;
    let pending = unsynced(local, state.uploaded);
    let mut dav = Dav::new(ctx.http.clone(), &state.url, ctx.timing.settle_timeout);
    dav.token = state.token.clone();
    dav.etag = state.etag.clone();
    match finish(ctx, &mut dav, local, state.uploaded) {
        Err(e) if e.is_transient() => {
            log::warn!("server unreachable ({e}), trying again later");
            return false;
        }
        Err(e) => {
            log::warn!("could not upload {}: {e}", local.display());
            if pending {
                if let Err(e) = keep_unsaved(ctx, local, local) {
                    log::error!("could not keep {}: {e}", local.display());
                    return false;
                }
            }
            let _ = release(ctx, &mut dav, &name_of(local)); // else the lock times out on its own
        }
        Ok(()) => {
            if pending {
                ctx.notify(ctx.tr(Msg::Uploaded, &[("name", &name_of(local))]), false, None);
            }
            discard_working_copy(ctx, local);
        }
    }
    true
}

/// Signatures of the files next to the working copy that we and the editor didn't put there.
fn other_files(local: &Path) -> HashMap<String, Option<Signature>> {
    let own = name_of(local);
    let Ok(entries) = fs::read_dir(local.parent().unwrap_or(Path::new("."))) else { return HashMap::new() };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(name, _)| {
            !(*name == own
                || name.starts_with(['.', '~']) // lock and owner files, Word's temporary files
                || name.starts_with("view-") // lodav.py's view copies
                || name.contains(".unsaved-")
                || [PART, ".lodav-part", ".tmp", ".leftover"].iter().any(|s| name.ends_with(s)))
        })
        .map(|(name, path)| (name, signature(&path)))
        .collect()
}

/// What the refresher and the main loop share; holding it serializes their WebDAV calls.
struct Shared {
    dav: Dav,
    uploaded: Option<Signature>,
    conflict: Option<String>,
}

/// The main loop's own bookkeeping.
struct Progress {
    /// Failed uploads/unlocks in a row.
    failures: u32,
    retry_at: Instant,
    /// The user has been told the CMS lacks their changes.
    told: bool,
    /// Files already next to the working copy when the session started.
    others: HashMap<String, Option<Signature>>,
}

/// One document from download to unlock.
///
/// The session outlives the editor window: closing the document while the server can't be
/// reached keeps the session going until the edits are uploaded and the lock is released.
/// A shutdown ends it early; the next run (at the latest the one at login) picks up from
/// the session file.
struct Session<'a> {
    ctx: &'a Context,
    /// The one the session started with, also if the setting changes meanwhile.
    editor: Arc<dyn Editor>,
    key: String,
    local: PathBuf,
    state_file: PathBuf,
    shared: Mutex<Shared>,
    /// Files saved in another format that the user was warned about.
    warned: Mutex<BTreeSet<String>>,
    locked: AtomicBool,
    /// Everything uploaded and the lock released.
    done: AtomicBool,
}

impl<'a> Session<'a> {
    fn new(ctx: &'a Context, editor: Arc<dyn Editor>, url: &str, local: PathBuf) -> Session<'a> {
        let key = session_key(url);
        Session {
            ctx,
            editor,
            state_file: ctx.sessions().join(format!("{key}.json")),
            key,
            local,
            shared: Mutex::new(Shared {
                dav: Dav::new(ctx.http.clone(), url, ctx.timing.request_timeout),
                uploaded: None,
                conflict: None,
            }),
            warned: Mutex::new(BTreeSet::new()),
            locked: AtomicBool::new(false),
            done: AtomicBool::new(false),
        }
    }

    fn name(&self) -> String {
        name_of(&self.local)
    }

    fn state(&self, s: &Shared) -> State {
        State {
            url: s.dav.url.clone(),
            token: s.dav.token.clone(),
            etag: s.dav.etag.clone(),
            pid: Some(std::process::id()),
            local: self.local.clone(),
            uploaded: s.uploaded,
        }
    }

    fn save_state(&self, s: &Shared) {
        if let Err(e) = self.state(s).write(&self.state_file) {
            log::error!("could not save the session state: {e}");
        }
    }

    fn run(&self, stop: &Stop) -> Result<(), Error> {
        {
            let mut s = self.shared.lock().unwrap();
            s.dav.lock()?;
            self.locked.store(true, Ordering::SeqCst);
            self.save_state(&s);
            // a working copy can only be here if an earlier session ended without cleaning up
            // (e.g. its session file was lost); never download over it
            let mut leftover = None;
            if self.local.exists() {
                let path = self.local.with_file_name(format!("{}.leftover", self.name()));
                fs::rename(&self.local, &path)?;
                leftover = Some(path);
            }
            s.dav.download(&self.local)?;
            if let Some(leftover) = leftover {
                if fs::read(&leftover)? == fs::read(&self.local)? {
                    fs::remove_file(&leftover)?;
                } else {
                    keep_unsaved(self.ctx, &leftover, &self.local)?;
                }
            }
            s.uploaded = signature(&self.local);
            self.save_state(&s);
        }
        let halt = Stop::new();
        std::thread::scope(|scope| {
            scope.spawn(|| self.refresher(&halt));
            let result = self.watch(stop);
            halt.set();
            result
        })
    }

    fn refresher(&self, halt: &Stop) {
        while !halt.wait(self.ctx.timing.refresh_every) {
            let mut s = self.shared.lock().unwrap();
            // done: unlocked just now; without a token, a refresh would end up locking again
            if s.conflict.is_some() || self.done.load(Ordering::SeqCst) {
                return;
            }
            match self.confirm_lock(&mut s) {
                Ok(()) => log::debug!("lock refreshed"),
                Err(e) if e.is_transient() => log::info!("lock refresh failed, server unreachable: {e}"),
                Err(dav::Error::Conflict(_)) => return,
                Err(e) => log::warn!("re-locking failed ({e}), trying again later"),
            }
        }
    }

    /// Refresh our lock; if it expired, lock again unless someone else locked or changed the
    /// document meanwhile (then the session has lost it: Conflict).
    fn confirm_lock(&self, s: &mut Shared) -> dav::Result<()> {
        match s.dav.refresh() {
            Err(dav::Error::Http { status, .. }) => {
                log::warn!("lock lost (HTTP {status}), re-locking");
                match s.dav.relock() {
                    Ok(()) => {
                        self.save_state(s);
                        Ok(())
                    }
                    Err(dav::Error::Conflict(c)) => {
                        self.lost(s, c.clone());
                        Err(dav::Error::Conflict(c))
                    }
                    Err(e) => Err(e),
                }
            }
            other => other,
        }
    }

    fn lost(&self, s: &mut Shared, conflict: String) {
        log::warn!("giving up on uploads: {conflict}");
        s.conflict = Some(conflict);
        self.save_state(s);
        self.ctx.notify(self.ctx.tr(Msg::Conflict, &[("name", &self.name())]), true, None);
    }

    fn failed(&self, what: &str, error: &dav::Error, p: &mut Progress) {
        p.failures += 1;
        let backoff = Duration::from_secs(1 << p.failures.min(16)).min(self.ctx.timing.retry_max);
        p.retry_at = Instant::now() + backoff;
        log::warn!("{what} attempt {} failed: {error}", p.failures);
    }

    fn upload(&self, sig: Signature, p: &mut Progress) {
        {
            self.ctx.activity.set_phase(&self.key, Phase::Uploading);
            let mut s = self.shared.lock().unwrap();
            // Never PUT without our lock: after an outage the token may be stale, and the
            // server accepts a PUT on an unlocked document, overwriting whatever is there.
            let result = self.confirm_lock(&mut s).and_then(|()| s.dav.upload(&self.local));
            if let Err(dav::Error::Conflict(_)) = result {
                return; // lost; the user has been told
            }
            if let Err(e) = result {
                drop(s);
                self.failed("upload", &e, p);
                if p.failures == self.ctx.timing.warn_after && !p.told {
                    p.told = true;
                    self.ctx.notify(self.ctx.tr(Msg::UploadPending, &[("name", &self.name())]), true, None);
                }
                return;
            }
            s.uploaded = Some(sig);
            p.failures = 0;
            self.save_state(&s);
        }
        self.ctx.activity.uploaded(&self.key);
        log::info!("uploaded {} ({} bytes)", self.name(), sig.1);
        if p.told {
            p.told = false;
            self.ctx.notify(self.ctx.tr(Msg::Uploaded, &[("name", &self.name())]), false, None);
        }
    }

    fn unlock(&self, p: &mut Progress) -> bool {
        let mut s = self.shared.lock().unwrap();
        if let Err(e) = release(self.ctx, &mut s.dav, &self.name()) {
            drop(s);
            self.failed("unlock", &e, p);
            return false;
        }
        self.done.store(true, Ordering::SeqCst);
        true
    }

    /// "Save as" in another format (e.g. ODF) writes a second file that never reaches the CMS.
    fn warn_other_format(&self, p: &Progress) {
        let mut warned = self.warned.lock().unwrap();
        for (name, sig) in other_files(&self.local) {
            if !warned.contains(&name) && p.others.get(&name) != Some(&sig) {
                let path = self.local.with_file_name(&name);
                let values: [(&str, &dyn std::fmt::Display); 3] =
                    [("other", &name), ("name", &self.name()), ("path", &path.display())];
                self.ctx.notify(self.ctx.tr(Msg::OtherFormat, &values), true, None);
                warned.insert(name);
            }
        }
    }

    fn uploaded(&self) -> Option<Signature> {
        self.shared.lock().unwrap().uploaded
    }

    fn conflict(&self) -> bool {
        self.shared.lock().unwrap().conflict.is_some()
    }

    fn phase(&self, p: &Progress, opened: bool, is_open: bool) -> Phase {
        if self.conflict() {
            Phase::Conflict
        } else if p.failures > 0 {
            Phase::Waiting
        } else if !opened {
            Phase::Opening
        } else if is_open {
            Phase::Editing
        } else {
            Phase::Closing
        }
    }

    /// Upload every save until the document is closed and the CMS has it all.
    fn watch(&self, stop: &Stop) -> Result<(), Error> {
        let mut p = Progress { failures: 0, retry_at: Instant::now(), told: false, others: other_files(&self.local) };
        launch(&*self.editor, &self.local, false)?;
        let (mut opened, mut is_open) = (false, false);
        let mut pending = None;
        let started = Instant::now();
        while !stop.wait(self.ctx.timing.poll) {
            let was_open = is_open;
            // "Done editing": taken as closed, for when the editor's lock file misleads us
            let finishing = self.ctx.activity.finish_requested(&self.key);
            is_open = !finishing && self.editor.is_open(&self.local);
            opened |= is_open || finishing;
            let sig = signature(&self.local);
            let can_upload = {
                let s = self.shared.lock().unwrap();
                // no token: the lock was lost and re-locking (which checks for conflicts) is pending
                sig.is_some() && sig != s.uploaded && s.conflict.is_none() && s.dav.token.is_some()
            };
            if can_upload {
                // upload once the file has been unchanged for a full poll interval
                if sig == pending && Instant::now() >= p.retry_at {
                    self.upload(sig.unwrap(), &mut p);
                }
                pending = sig;
            } else {
                pending = None;
            }
            self.warn_other_format(&p);
            self.ctx.activity.set_phase(&self.key, self.phase(&p, opened, is_open));
            if !opened {
                if started.elapsed() > self.ctx.timing.start_grace && !self.editor.running() {
                    log::warn!("{} did not open {}", self.editor.name(), self.local.display());
                    break;
                }
                continue;
            }
            if is_open {
                continue;
            }
            // closed; keep going until the CMS has everything (or the document is opened again)
            if was_open {
                log::info!("document closed");
                if p.failures > 0 && !self.conflict() {
                    p.told = true;
                    self.ctx.notify(self.ctx.tr(Msg::ClosedPending, &[("name", &self.name())]), true, None);
                }
            }
            if self.conflict() {
                break;
            }
            if sig.is_some() && sig != self.uploaded() {
                continue;
            }
            if Instant::now() >= p.retry_at && self.unlock(&mut p) {
                break;
            }
        }
        self.warn_other_format(&p);
        Ok(())
    }

    /// Move files saved in another format to the user's folder, unless still open.
    fn keep_other_files(&self) {
        for name in self.warned.lock().unwrap().iter() {
            let f = self.local.with_file_name(name);
            if !f.exists() || self.editor.is_open(&f) {
                continue; // still being edited: the notification named this path
            }
            let moved = self.ctx.user_folder().and_then(|folder| {
                let kept = keep_name(&folder, name);
                move_file(&f, &kept).map(|()| kept)
            });
            match moved {
                Ok(kept) => {
                    let text = self.ctx.tr(Msg::MovedTo, &[("file", name), ("path", &kept.display())]);
                    self.ctx.record(text, Some(&kept));
                }
                Err(e) => log::warn!("could not move {}: {e}", f.display()),
            }
        }
    }

    fn close(&self) {
        self.keep_other_files();
        let s = self.shared.lock().unwrap();
        if self.done.load(Ordering::SeqCst) {
            discard_working_copy(self.ctx, &self.local);
        } else if !self.locked.load(Ordering::SeqCst) {
            return; // never got the lock, nothing to sync or release
        } else if s.conflict.is_some() {
            if unsynced(&self.local, s.uploaded) {
                if let Err(e) = keep_unsaved(self.ctx, &self.local, &self.local) {
                    log::error!("could not keep {}: {e}", self.local.display());
                    return; // the session file stays, so the next run tries again
                }
            } else {
                discard_working_copy(self.ctx, &self.local);
            }
        } else {
            // interrupted, e.g. by a shutdown: one last try, otherwise the next run finishes it
            self.save_state(&s);
            let state = self.state(&s);
            drop(s);
            if !settle(self.ctx, &state) {
                return;
            }
        }
        let _ = fs::remove_file(&self.state_file);
    }
}

fn launch(editor: &dyn Editor, path: &Path, view: bool) -> Result<(), Error> {
    editor.launch(path, view).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            // a custom error names the missing program; an OS error doesn't
            Error::EditorNotFound(if e.get_ref().is_some() { e.to_string() } else { editor.name().to_string() })
        } else {
            Error::Io(e)
        }
    })
}

/// Download a read-only copy and open it. View copies get their own folder, so that the
/// editor's lock files for them can't be taken for the working copy's.
fn view(ctx: &Context, url: &str, local: &Path) -> Result<(), Error> {
    let dir = local.parent().unwrap_or(Path::new(".")).join("view");
    fs::create_dir_all(&dir)?;
    let mut path = dir.join(name_of(local));
    if path.exists() {
        let mut perms = fs::metadata(&path)?.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = fs::set_permissions(&path, perms);
        if fs::remove_file(&path).is_err() {
            path = keep_name(&dir, &name_of(local)); // still open (Windows)
        }
    }
    Dav::new(ctx.http.clone(), url, ctx.timing.request_timeout).download(&path)?;
    let mut perms = fs::metadata(&path)?.permissions();
    perms.set_readonly(true);
    fs::set_permissions(&path, perms)?;
    launch(&*ctx.editor(), &path, true)
}

/// Open a CMS link: edit the document until it is closed and in the CMS, or view it.
pub fn open_uri(ctx: &Context, uri: &str, stop: &Stop) -> Result<Outcome, Error> {
    let (mode, url) = parse_office_uri(uri).map_err(Error::Uri)?;
    fs::create_dir_all(ctx.sessions())?;
    let key = session_key(&url);
    let workdir = ctx.dirs.cache.join(&key);
    fs::create_dir_all(&workdir)?;
    let local = workdir.join(file_name(&url));
    let mut lock = session_lock(ctx, &key);

    {
        // the recovery lock is held until this document's session lock is ours, so that
        // the recovery started at login can't take this document over in between
        let _recovery = recovery(ctx, RECOVERY_WAIT)?;
        recover_stale(ctx, None);
        if mode == Mode::Edit {
            if !lock.try_acquire()? {
                log::info!("already being edited in another session, reopening");
                launch(&*ctx.editor(), &local, false)?;
                return Ok(Outcome::Reopened);
            }
            if ctx.sessions().join(format!("{key}.json")).exists() {
                // left by a session recover_stale could not finish
                lock.release();
                ctx.notify(ctx.tr(Msg::EarlierBlocked, &[("name", &name_of(&local))]), true, None);
                return Ok(Outcome::Blocked);
            }
        }
    }

    if mode == Mode::View {
        view(ctx, &url, &local)?;
        return Ok(Outcome::Viewed);
    }

    let editor = ctx.editor();
    let tracked = ctx.activity.track(&key, &name_of(&local), editor.name());
    let session = Session::new(ctx, editor, &url, local.clone());
    let result = session.run(stop);
    session.close();
    lock.release();
    drop(tracked);
    match result {
        Err(Error::Dav(dav::Error::Http { status: 423, .. })) => {
            ctx.notify(ctx.tr(Msg::LockedReadOnly, &[("name", &name_of(&local))]), true, None);
            view(ctx, &url, &local)?;
            Ok(Outcome::LockedReadOnly)
        }
        Err(e) => Err(e),
        Ok(()) if stop.is_set() => Ok(Outcome::Interrupted),
        Ok(()) => Ok(Outcome::Done),
    }
}
