//! Datenportal Connector: open CMS documents from ms-word:/ms-excel: links in LibreOffice or
//! Microsoft Office.
//!
//! Instead of letting the editor talk WebDAV itself (which fails against the SiteFusion/Milton
//! server from the second save on), a session takes the WebDAV lock, downloads the file, lets
//! the editor work on a local copy, uploads it on every save and unlocks when the document is
//! closed. This crate has no GUI; the app drives it and shows what it reports.

use std::fmt::Display;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;

pub mod activity;
pub mod dav;
pub mod dirs;
pub mod editor;
mod folder;
pub mod host;
pub mod i18n;
pub mod messages;
pub mod migrate;
mod recover;
mod session;
pub mod store;
pub mod uri;

pub use activity::{Activity, Phase, SessionInfo};
pub use dirs::Dirs;
pub use editor::Editor;
pub use i18n::{Lang, Msg};
pub use messages::{Message, MessageLog};
pub use recover::{pending, recover_loop, recover_stale, recovery, session_lock, set_aside, Pending};
pub use session::open_uri;

/// How often things happen; shortened by the tests.
#[derive(Clone, Debug)]
pub struct Timing {
    /// How often the working copy and the editor's lock file are looked at.
    pub poll: Duration,
    /// Must stay well below the server's lock timeout (~120 s observed).
    pub refresh_every: Duration,
    /// How long the editor may take to open the document.
    pub start_grace: Duration,
    /// Longest wait between retries while the server can't be reached.
    pub retry_max: Duration,
    /// Failed uploads before the user is told; a short network blip stays silent.
    pub warn_after: u32,
    /// Per WebDAV request during a session.
    pub request_timeout: Duration,
    /// Per WebDAV request when finishing an earlier session: a click may be waiting for it.
    pub settle_timeout: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            poll: Duration::from_secs(1),
            refresh_every: Duration::from_secs(60),
            start_grace: Duration::from_secs(90),
            retry_max: Duration::from_secs(60),
            warn_after: 3,
            request_timeout: Duration::from_secs(60),
            settle_timeout: Duration::from_secs(20),
        }
    }
}

pub type Notifier = Box<dyn Fn(&Message) + Send + Sync>;

/// Everything a session needs from its surroundings.
pub struct Context {
    pub dirs: Dirs,
    pub timing: Timing,
    /// The user's language; the setting can change it while sessions run.
    lang: RwLock<Lang>,
    /// For documents opened from now on; a running session keeps the one it started with.
    editor: RwLock<Arc<dyn Editor>>,
    pub messages: MessageLog,
    pub activity: Activity,
    pub http: reqwest::blocking::Client,
    /// Shows a message to the user, e.g. as a notification; it is in the log already.
    notifier: Notifier,
}

impl Context {
    pub fn new(dirs: Dirs, editor: Arc<dyn Editor>, notifier: Notifier) -> Context {
        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("Datenportal-Connector/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client");
        Context {
            messages: MessageLog::new(&dirs.state),
            dirs,
            timing: Timing::default(),
            lang: RwLock::new(Lang::detect()),
            editor: RwLock::new(editor),
            activity: Activity::new(),
            http,
            notifier,
        }
    }

    pub fn lang(&self) -> Lang {
        *self.lang.read().unwrap()
    }

    pub fn set_lang(&self, lang: Lang) {
        *self.lang.write().unwrap() = lang;
    }

    pub fn editor(&self) -> Arc<dyn Editor> {
        self.editor.read().unwrap().clone()
    }

    pub fn set_editor(&self, editor: Arc<dyn Editor>) {
        *self.editor.write().unwrap() = editor;
    }

    pub fn sessions(&self) -> PathBuf {
        self.dirs.sessions()
    }

    pub fn tr(&self, msg: Msg, values: &[(&str, &dyn Display)]) -> String {
        msg.fill(self.lang(), values)
    }

    /// Tell the user: logged, kept in the message log and shown.
    pub fn notify(&self, text: String, urgent: bool, kept: Option<&Path>) {
        if urgent {
            log::warn!("{text}");
        } else {
            log::info!("{text}");
        }
        match self.messages.append(&text, urgent, kept) {
            Ok(message) => (self.notifier)(&message),
            Err(e) => {
                log::warn!("could not record the message: {e}");
                (self.notifier)(&Message { id: 0, time: String::new(), text, urgent, kept: kept.map(Path::to_path_buf) });
            }
        }
    }

    /// Keep a message in the log only, without showing it.
    pub fn record(&self, text: String, kept: Option<&Path>) {
        log::info!("{text}");
        if let Err(e) = self.messages.append(&text, false, kept) {
            log::warn!("could not record the message: {e}");
        }
    }

    /// Why something failed, in the user's language; the technical details go to the log.
    pub fn explain(&self, e: &Error) -> String {
        match e {
            Error::Uri(_) => self.tr(Msg::BadLink, &[]),
            Error::EditorNotFound(name) => self.tr(Msg::EditorNotFound, &[("editor", name)]),
            Error::Dav(dav::Error::Http { status, .. }) => self.tr(Msg::ServerRefused, &[("status", status)]),
            Error::Dav(dav::Error::Conflict(_)) => self.tr(Msg::ServerRefused, &[("status", &409)]),
            Error::Dav(_) => self.tr(Msg::ServerUnreachable, &[]),
            Error::Io(e) => match e.kind() {
                io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => self.tr(Msg::NoPermission, &[]),
                io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => self.tr(Msg::DiskFull, &[]),
                io::ErrorKind::TimedOut => self.tr(Msg::StillBusy, &[]),
                _ => self.tr(Msg::LocalFiles, &[]),
            },
        }
    }

    /// For a failed action the user asked for: logs the details and says so in their language.
    pub fn failed(&self, what: &str, e: &dyn Display) -> String {
        log::warn!("{what} failed: {e}");
        self.tr(Msg::Failed, &[])
    }

    /// The user's folder, created if needed.
    pub fn user_folder(&self) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.dirs.folder)?;
        Ok(self.dirs.folder.clone())
    }
}

/// Set to end sessions early, e.g. when the computer shuts down. Sessions save their state
/// and the next run finishes them.
#[derive(Clone, Default)]
pub struct Stop(Arc<(Mutex<bool>, Condvar)>);

impl Stop {
    pub fn new() -> Stop {
        Stop::default()
    }

    pub fn set(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }

    pub fn is_set(&self) -> bool {
        *self.0.0.lock().unwrap()
    }

    /// Wait up to timeout; true if stopped.
    pub fn wait(&self, timeout: Duration) -> bool {
        let guard = self.0.0.lock().unwrap();
        let (guard, _) = self.0.1.wait_timeout_while(guard, timeout, |stopped| !*stopped).unwrap();
        *guard
    }
}

/// How a click on a CMS link ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Edited, uploaded and unlocked (or set aside after a conflict).
    Done,
    /// Opened read-only, as asked for.
    Viewed,
    /// Locked by someone else; opened read-only.
    LockedReadOnly,
    /// Being edited in a session already; the editor was asked to show it again.
    Reopened,
    /// An earlier session's edits wait for the server; not opened.
    Blocked,
    /// Ended early by Stop; the next run finishes it.
    Interrupted,
}

#[derive(Debug)]
pub enum Error {
    Uri(String),
    Dav(dav::Error),
    Io(io::Error),
    /// The editor (or, for Microsoft Office, the program for this document) isn't installed.
    EditorNotFound(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Uri(e) => write!(f, "{e}"),
            Error::Dav(e) => write!(f, "{e}"),
            Error::Io(e) => write!(f, "{e}"),
            Error::EditorNotFound(name) => write!(f, "{name} not found"),
        }
    }
}

impl std::error::Error for Error {}

impl From<dav::Error> for Error {
    fn from(e: dav::Error) -> Error {
        Error::Dav(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}

/// Open a CMS link, telling the user if that fails. Returns once the session is over.
pub fn open(ctx: &Context, uri: &str, stop: &Stop) -> Option<Outcome> {
    match open_uri(ctx, uri, stop) {
        Ok(outcome) => Some(outcome),
        Err(e) => {
            log::error!("failed: {e}");
            ctx.notify(ctx.tr(Msg::CouldNotOpen, &[("error", &ctx.explain(&e))]), true, None);
            None
        }
    }
}
