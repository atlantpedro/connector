//! What the running sessions are doing, for the window and the tray icon.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// Locking, downloading, waiting for the editor to show the document.
    Opening,
    /// Open in the editor; the CMS has every save.
    Editing,
    /// A save is on its way to the CMS.
    Uploading,
    /// A save or the unlock waits for the server.
    Waiting,
    /// Closed; the last save and the unlock are under way.
    Closing,
    /// Someone else took the document over; saves don't reach the CMS any more.
    Conflict,
}

/// One running session as the window shows it.
#[derive(Clone, Debug, Serialize)]
pub struct SessionInfo {
    pub key: String,
    pub name: String,
    pub editor: String,
    pub phase: Phase,
    /// Local time, RFC 3339.
    pub started: String,
    /// When the last save reached the CMS; local time, RFC 3339.
    pub uploaded: Option<String>,
    /// The user asked to stop waiting for the editor ("Done editing").
    pub finishing: bool,
}

type Listener = Arc<dyn Fn(&Activity) + Send + Sync>;

/// The running sessions. Whoever shows them is told of every change.
#[derive(Default)]
pub struct Activity {
    sessions: Mutex<BTreeMap<String, SessionInfo>>,
    listener: Mutex<Option<Listener>>,
}

fn now() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

impl Activity {
    pub fn new() -> Activity {
        Activity::default()
    }

    /// Called after every change, from the thread that made it.
    pub fn set_listener(&self, listener: impl Fn(&Activity) + Send + Sync + 'static) {
        *self.listener.lock().unwrap() = Some(Arc::new(listener));
    }

    /// Something changed that the listener should know about, e.g. an earlier session finished.
    pub fn changed(&self) {
        // called unlocked: the listener may well change something itself
        let listener = self.listener.lock().unwrap().clone();
        if let Some(listener) = listener {
            listener(self);
        }
    }

    /// Oldest first.
    pub fn list(&self) -> Vec<SessionInfo> {
        let mut list: Vec<_> = self.sessions.lock().unwrap().values().cloned().collect();
        list.sort_by(|a, b| a.started.cmp(&b.started));
        list
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.lock().unwrap().is_empty()
    }

    /// Stop waiting for the editor to close this document: upload the last save, unlock and
    /// end the session. False if there is no such session.
    pub fn request_finish(&self, key: &str) -> bool {
        self.update(key, |s| s.finishing = true)
    }

    pub(crate) fn finish_requested(&self, key: &str) -> bool {
        self.sessions.lock().unwrap().get(key).is_some_and(|s| s.finishing)
    }

    /// Show a session until the guard is dropped.
    pub(crate) fn track<'a>(&'a self, key: &str, name: &str, editor: &str) -> Tracked<'a> {
        let info = SessionInfo {
            key: key.to_string(),
            name: name.to_string(),
            editor: editor.to_string(),
            phase: Phase::Opening,
            started: now(),
            uploaded: None,
            finishing: false,
        };
        self.sessions.lock().unwrap().insert(key.to_string(), info);
        self.changed();
        Tracked { activity: self, key: key.to_string() }
    }

    pub(crate) fn set_phase(&self, key: &str, phase: Phase) {
        self.update(key, |s| s.phase = phase);
    }

    pub(crate) fn uploaded(&self, key: &str) {
        self.update(key, |s| s.uploaded = Some(now()));
    }

    /// Change a session; the listener is told only if something changed.
    fn update(&self, key: &str, change: impl FnOnce(&mut SessionInfo)) -> bool {
        let changed = {
            let mut sessions = self.sessions.lock().unwrap();
            let Some(session) = sessions.get_mut(key) else { return false };
            let before = (session.phase, session.uploaded.clone(), session.finishing);
            change(session);
            before != (session.phase, session.uploaded.clone(), session.finishing)
        };
        if changed {
            self.changed();
        }
        true
    }
}

pub(crate) struct Tracked<'a> {
    activity: &'a Activity,
    key: String,
}

impl Drop for Tracked<'_> {
    fn drop(&mut self) {
        self.activity.sessions.lock().unwrap().remove(&self.key);
        self.activity.changed();
    }
}
