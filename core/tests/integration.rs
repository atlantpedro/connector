//! End-to-end tests of the sync loop against a local mock of the CMS WebDAV server.
//!
//! A fake editor stands in for LibreOffice: it writes LibreOffice's lock file, saves twice
//! and closes. So these tests run unchanged on Linux, macOS and Windows.

mod mock_dav;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use lodav_core::editor::LibreOffice;
use lodav_core::migrate::{migrate, Migrated, Migration};
use lodav_core::store::FileLock;
use lodav_core::uri::session_key;
use lodav_core::{
    open_uri, pending, recover_loop, recover_stale, recovery, session_lock, set_aside, Context, Dirs, Editor, Lang,
    Outcome, Phase, Stop, Timing,
};
use mock_dav::{etag, MockDav};
use tempfile::TempDir;

const ORIGINAL: &[u8] = b"PK original document bytes";

/// Opens the document, saves twice and closes it, like a user in LibreOffice would.
#[derive(Default)]
struct FakeEditor {
    /// "Save as" this name next to the document first.
    save_as: Option<String>,
    /// Never closes it: its lock file stays.
    stays_open: bool,
}

impl Editor for FakeEditor {
    fn name(&self) -> &str {
        "FakeOffice"
    }

    fn launch(&self, path: &Path, view: bool) -> io::Result<()> {
        if view {
            return Ok(()); // read-only: nothing to edit
        }
        let (doc, save_as, stays_open) = (path.to_path_buf(), self.save_as.clone(), self.stays_open);
        thread::spawn(move || {
            let lock = lock_file(&doc);
            fs::write(&lock, "fake office").unwrap();
            thread::sleep(Duration::from_millis(600));
            if let Some(other) = save_as {
                fs::write(doc.with_file_name(other), "odf").unwrap();
            }
            for n in 1..=2 {
                let mut content = fs::read(&doc).unwrap();
                content.extend(format!(" edit{n}").as_bytes());
                fs::write(&doc, content).unwrap();
                thread::sleep(Duration::from_millis(1200));
            }
            if !stays_open {
                fs::remove_file(&lock).unwrap();
            }
        });
        Ok(())
    }

    fn is_open(&self, path: &Path) -> bool {
        LibreOffice::new().is_open(path)
    }

    fn running(&self) -> bool {
        false
    }
}

fn lock_file(doc: &Path) -> PathBuf {
    doc.with_file_name(format!(".~lock.{}#", doc.file_name().unwrap().to_string_lossy()))
}

struct Fixture {
    server: MockDav,
    ctx: Context,
    notes: Arc<Mutex<Vec<String>>>,
    url: String,
    tmp: TempDir,
}

fn fixture() -> Fixture {
    fixture_with(FakeEditor::default())
}

fn fixture_with(editor: FakeEditor) -> Fixture {
    let server = MockDav::start(ORIGINAL);
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs {
        cache: tmp.path().join("cache"),
        state: tmp.path().join("state"),
        folder: tmp.path().join("Documents").join("Datenportal Connector"),
    };
    let notes = Arc::new(Mutex::new(Vec::new()));
    let sink = notes.clone();
    let mut ctx = Context::new(dirs, Arc::new(editor), Box::new(move |m| sink.lock().unwrap().push(m.text.clone())));
    ctx.timing = Timing {
        poll: Duration::from_millis(200),
        refresh_every: Duration::from_millis(500),
        start_grace: Duration::from_secs(20),
        retry_max: Duration::from_millis(500),
        ..Timing::default()
    };
    ctx.set_lang(Lang::En);
    // never through a proxy from the environment
    ctx.http = reqwest::blocking::Client::builder().no_proxy().build().unwrap();
    let url = server.url();
    Fixture { server, ctx, notes, url, tmp }
}

impl Fixture {
    fn open(&self, uri: &str) -> Outcome {
        open_uri(&self.ctx, uri, &Stop::new()).unwrap()
    }

    fn notes(&self) -> Vec<String> {
        self.notes.lock().unwrap().clone()
    }

    fn noted(&self, part: &str) -> bool {
        self.notes().iter().any(|n| n.contains(part))
    }

    fn session_files(&self) -> Vec<PathBuf> {
        files(&self.ctx.sessions(), |name| name.ends_with(".json"))
    }

    fn cache_entries(&self) -> Vec<PathBuf> {
        files(&self.ctx.dirs.cache, |_| true)
    }

    fn kept(&self) -> Vec<Vec<u8>> {
        files(&self.ctx.dirs.folder, |n| n.starts_with("Test Dokument (not uploaded ") && n.ends_with(").docm"))
            .iter()
            .map(|f| fs::read(f).unwrap())
            .collect()
    }

    fn local(&self) -> PathBuf {
        self.ctx.dirs.cache.join(session_key(&self.url)).join("Test Dokument.docm")
    }

    fn state_file(&self) -> PathBuf {
        self.ctx.sessions().join(format!("{}.json", session_key(&self.url)))
    }

    /// A session as lodav.py leaves it when the computer dies: locked, edited, not uploaded.
    fn dead_session(&self, content: &[u8]) -> PathBuf {
        dead_session(&self.server, &self.ctx, &self.ctx.dirs, content)
    }
}

fn files(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries.flatten().filter(|e| keep(&e.file_name().to_string_lossy())).map(|e| e.path()).collect()
}

fn dead_session(server: &MockDav, ctx: &Context, dirs: &Dirs, content: &[u8]) -> PathBuf {
    let url = server.url();
    let mut dav = lodav_core::dav::Dav::new(ctx.http.clone(), &url, Duration::from_secs(5));
    dav.lock().unwrap();
    fs::create_dir_all(dirs.sessions()).unwrap();
    let local = dirs.cache.join(session_key(&url)).join("Test Dokument.docm");
    fs::create_dir_all(local.parent().unwrap()).unwrap();
    fs::write(&local, content).unwrap();
    // the JSON exactly as lodav.py writes it
    let state = serde_json::json!({
        "url": url, "token": dav.token, "etag": etag(ORIGINAL), "pid": 999999999,
        "local": local, "uploaded": [0, 0],
    });
    fs::write(dirs.sessions().join(format!("{}.json", session_key(&url))), state.to_string()).unwrap();
    local
}

const EDITED: &[u8] = b"PK original document bytes edit1 edit2";

#[test]
fn edit_session_syncs_every_save_and_unlocks() {
    let f = fixture();
    assert_eq!(f.open(&format!("ms-word:ofe%7Cu%7C{}", f.url)), Outcome::Done);
    let s = f.server.server();
    assert_eq!(s.content, EDITED);
    assert!(s.puts >= 2);
    assert!(s.refreshes >= 1);
    assert_eq!(s.token, None, "lock must be released on close");
    assert!(f.session_files().is_empty());
    assert!(f.cache_entries().is_empty(), "working copy must not outlive the session");
    assert!(f.notes().is_empty(), "{:?}", f.notes());
}

#[test]
fn the_window_sees_the_session_and_its_phases() {
    let f = fixture();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    f.ctx.activity.set_listener(move |activity| {
        let mut seen = sink.lock().unwrap();
        for s in activity.list() {
            if seen.last() != Some(&s.phase) {
                seen.push(s.phase);
            }
        }
    });
    assert_eq!(f.open(&format!("ms-word:ofe|u|{}", f.url)), Outcome::Done);
    let seen = seen.lock().unwrap().clone();
    for phase in [Phase::Opening, Phase::Editing, Phase::Uploading, Phase::Closing] {
        assert!(seen.contains(&phase), "{seen:?}");
    }
    assert!(f.ctx.activity.is_empty());
}

#[test]
fn done_editing_ends_a_session_the_editor_keeps_open() {
    let f = fixture_with(FakeEditor { stays_open: true, ..FakeEditor::default() });
    let key = session_key(&f.url);
    thread::scope(|scope| {
        let session = scope.spawn(|| f.open(&format!("ms-word:ofe|u|{}", f.url)));
        // both saves uploaded (after ~3 s), and the session back from uploading the last one
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let editing = || f.ctx.activity.list().first().is_some_and(|s| s.phase == Phase::Editing);
        while f.server.server().content != EDITED || !editing() {
            if std::time::Instant::now() >= deadline {
                // end the session first: the scope waits for it, so panicking now would hang
                f.ctx.activity.request_finish(&key);
                panic!("saves never arrived: {:?}", f.ctx.activity.list());
            }
            thread::sleep(Duration::from_millis(100));
        }
        assert!(f.ctx.activity.request_finish(&key));
        assert_eq!(session.join().unwrap(), Outcome::Done);
    });
    assert_eq!(f.server.server().token, None, "lock must be released");
    assert!(f.session_files().is_empty());
    assert!(f.local().exists(), "still open in the editor: the working copy stays");
    assert!(!f.ctx.activity.request_finish(&key));
}

#[test]
fn set_aside_keeps_the_version_and_forgets_the_session() {
    let f = fixture();
    f.dead_session(b"edited, but the server refuses it");
    f.server.set_down(true);
    assert_eq!(recover_stale(&f.ctx, None), ["Test Dokument.docm"]);
    let waiting = pending(&f.ctx);
    assert_eq!(waiting.len(), 1);
    assert!(waiting[0].unsynced);

    assert!(set_aside(&f.ctx, &waiting[0].key).unwrap());

    assert_eq!(f.kept(), [b"edited, but the server refuses it"]);
    assert!(f.session_files().is_empty());
    assert!(pending(&f.ctx).is_empty());
    assert!(f.cache_entries().is_empty());
    assert!(f.noted("Your version is kept at"), "{:?}", f.notes());
}

#[test]
fn failures_are_explained_in_the_users_language() {
    let f = fixture();
    f.ctx.set_lang(Lang::De);
    assert_eq!(lodav_core::open(&f.ctx, "ms-word:ofe|u|ftp://h/a.docx", &Stop::new()), None);
    f.server.set_down(true);
    assert_eq!(lodav_core::open(&f.ctx, &format!("ms-word:ofe|u|{}", f.url), &Stop::new()), None);
    assert_eq!(
        f.notes(),
        [
            "Das Dokument konnte nicht geöffnet werden: Der Link aus dem CMS konnte nicht gelesen werden.",
            "Das Dokument konnte nicht geöffnet werden: Der Server war nicht erreichbar.",
        ]
    );
}

#[test]
fn closed_while_offline_uploads_once_back() {
    let f = fixture();
    f.server.outage(0.3, 7.0); // the fake editor saves twice and closes after ~3s
    assert_eq!(f.open(&format!("ms-word:ofe|u|{}", f.url)), Outcome::Done);
    assert_eq!(f.server.server().content, EDITED);
    assert_eq!(f.server.server().token, None, "lock must be released once the server is back");
    assert!(f.session_files().is_empty());
    assert!(f.cache_entries().is_empty());
    assert!(f.noted("closed, but its changes are not in the CMS"), "{:?}", f.notes());
    assert!(f.notes().contains(&"Changes to Test Dokument.docm have been uploaded to the CMS.".to_string()));
}

#[test]
fn changed_by_someone_else_while_offline_keeps_version() {
    let f = fixture();
    f.server.outage(0.3, 5.0);
    let state = f.server.state.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        let mut s = state.lock().unwrap();
        s.token = None; // our lock expired
        s.content = b"PK someone else's version".to_vec();
    });
    assert_eq!(f.open(&format!("ms-word:ofe|u|{}", f.url)), Outcome::Done);
    assert_eq!(f.server.server().content, b"PK someone else's version");
    assert_eq!(f.server.server().token, None);
    assert_eq!(f.kept(), [EDITED]);
    assert!(f.noted("changed or locked in the CMS by someone else"), "{:?}", f.notes());
}

#[test]
fn save_in_other_format_warns() {
    let f = fixture_with(FakeEditor { save_as: Some("Test Dokument.odt".into()), ..FakeEditor::default() });
    assert_eq!(f.open(&format!("ms-word:ofe|u|{}", f.url)), Outcome::Done);
    let notes = f.notes();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("Test Dokument.odt") && notes[0].contains("NOT uploaded"));
    assert_eq!(f.server.server().content, EDITED);
    assert_eq!(fs::read(f.ctx.dirs.folder.join("Test Dokument.odt")).unwrap(), b"odf");
    assert!(f.cache_entries().is_empty());
    // in the message log, though not shown
    let log = f.ctx.messages.list();
    let moved = log.iter().find(|m| m.text.contains("Test Dokument.odt was moved to")).unwrap();
    assert_eq!(moved.kept.as_deref(), Some(f.ctx.dirs.folder.join("Test Dokument.odt").as_path()));
}

#[test]
fn locked_by_someone_else_opens_read_only() {
    let f = fixture();
    f.server.server().token = Some("opaquelocktoken:other".into());
    assert_eq!(f.open(&format!("ms-word:ofe|u|{}", f.url)), Outcome::LockedReadOnly);
    assert_eq!(f.server.server().puts, 0);
    assert_eq!(f.server.server().token.as_deref(), Some("opaquelocktoken:other"));
    assert!(f.noted("locked"));
}

#[test]
fn view_takes_no_lock_and_is_read_only() {
    let f = fixture();
    assert_eq!(f.open(&format!("ms-word:ofv|u|{}", f.url)), Outcome::Viewed);
    assert_eq!(f.server.server().lock_count, 0);
    assert_eq!(f.server.server().puts, 0);
    let view = f.local().parent().unwrap().join("view").join("Test Dokument.docm");
    assert_eq!(fs::read(&view).unwrap(), ORIGINAL);
    assert!(fs::metadata(&view).unwrap().permissions().readonly());
    // viewing again replaces the read-only copy
    assert_eq!(f.open(&format!("ms-word:ofv|u|{}", f.url)), Outcome::Viewed);
}

#[test]
fn recover_uploads_and_unlocks_dead_session() {
    let f = fixture();
    let local = f.dead_session(b"edited but never uploaded");
    recover_stale(&f.ctx, None);
    assert_eq!(f.server.server().content, b"edited but never uploaded");
    assert_eq!(f.server.server().token, None);
    assert!(f.session_files().is_empty());
    assert!(!local.exists());
}

#[test]
fn recover_relocks_after_the_lock_expired() {
    let f = fixture();
    f.dead_session(b"edited before the lock expired");
    f.server.server().token = None; // expired; refreshing it gets HTTP 500
    assert!(recover_stale(&f.ctx, None).is_empty());
    assert_eq!(f.server.server().content, b"edited before the lock expired");
    assert_eq!(f.server.server().token, None);
    assert!(f.session_files().is_empty());
}

#[test]
fn recover_without_base_etag_keeps_version() {
    let f = fixture();
    let local = f.dead_session(b"edited in an old lodav");
    let mut state: serde_json::Value = serde_json::from_str(&fs::read_to_string(f.state_file()).unwrap()).unwrap();
    state.as_object_mut().unwrap().remove("etag"); // session files from before ETags were recorded
    fs::write(f.state_file(), state.to_string()).unwrap();
    f.server.server().token = None;

    assert!(recover_stale(&f.ctx, None).is_empty());

    assert_eq!(f.server.server().content, ORIGINAL, "may hold changes by someone else");
    assert_eq!(f.server.server().token, None);
    assert!(f.session_files().is_empty());
    assert!(!local.exists());
    assert_eq!(f.kept(), [b"edited in an old lodav"]);
}

#[test]
fn recover_while_offline_keeps_session_and_blocks_reopen() {
    let f = fixture();
    let local = f.dead_session(b"edited but never uploaded");
    f.server.set_down(true);

    assert_eq!(f.open(&format!("ms-word:ofe|u|{}", f.url)), Outcome::Blocked);

    assert_eq!(fs::read(&local).unwrap(), b"edited but never uploaded");
    assert_eq!(f.session_files().len(), 1);
    assert!(f.noted("not in the CMS yet"), "{:?}", f.notes());
    f.server.set_down(false);
    recover_stale(&f.ctx, None);
    assert_eq!(f.server.server().content, b"edited but never uploaded");
    assert_eq!(f.server.server().token, None);
}

#[test]
fn running_session_is_left_alone() {
    let f = fixture();
    let local = f.dead_session(b"being edited");
    let mut alive = session_lock(&f.ctx, &session_key(&f.url));
    assert!(alive.try_acquire().unwrap()); // stands in for the session that owns it

    {
        let _pass = recovery(&f.ctx, Duration::from_secs(5)).unwrap();
        assert!(recover_stale(&f.ctx, None).is_empty());
    }

    assert_eq!(fs::read(&local).unwrap(), b"being edited");
    assert_eq!(f.server.server().content, ORIGINAL);
    assert!(f.server.server().token.is_some());
}

#[test]
fn recover_loop_keeps_trying_until_the_server_is_back() {
    let f = fixture();
    f.dead_session(b"edited before the power cut");
    f.server.outage(0.0, 3.0);
    thread::sleep(Duration::from_millis(50));

    assert!(recover_loop(&f.ctx, &Stop::new()));

    assert_eq!(f.server.server().content, b"edited before the power cut");
    assert_eq!(f.server.server().token, None);
    assert!(f.noted("from an earlier session"), "{:?}", f.notes());
    assert!(f.notes().contains(&"Changes to Test Dokument.docm have been uploaded to the CMS.".to_string()));
}

#[test]
fn recover_loop_ends_when_stopped() {
    let f = fixture();
    f.dead_session(b"edited before the power cut");
    f.server.set_down(true);
    let stop = Stop::new();
    let s = stop.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        s.set();
    });
    assert!(!recover_loop(&f.ctx, &stop));
    assert_eq!(f.session_files().len(), 1);
}

#[test]
fn broken_session_file_never_costs_the_edits() {
    let f = fixture();
    let local = f.dead_session(b"edited before the power cut");
    fs::write(f.state_file(), r#"{"url": "http://half-writ"#).unwrap();
    f.server.server().token = None; // the lock expired while the computer was off

    assert_eq!(f.open(&format!("ms-word:ofe|u|{}", f.url)), Outcome::Done);

    assert_eq!(f.kept(), [b"edited before the power cut"]);
    let mut broken = f.state_file().into_os_string();
    broken.push(".broken");
    assert!(Path::new(&broken).exists());
    assert!(!local.exists());
}

#[test]
fn leftover_identical_to_the_server_copy_is_dropped_silently() {
    let f = fixture();
    fs::create_dir_all(f.local().parent().unwrap()).unwrap();
    fs::write(f.local(), ORIGINAL).unwrap();

    assert_eq!(f.open(&format!("ms-word:ofe|u|{}", f.url)), Outcome::Done);

    assert!(f.kept().is_empty());
    assert!(f.notes().is_empty(), "{:?}", f.notes());
}

fn legacy_dirs(f: &Fixture) -> Dirs {
    Dirs {
        cache: f.tmp.path().join("lodav-cache"),
        state: f.tmp.path().join("lodav-state"),
        folder: f.tmp.path().join("Documents").join("Datenportal LibreOffice Bridge"),
    }
}

#[test]
fn moves_over_from_lodav_py() {
    let f = fixture();
    let legacy = legacy_dirs(&f);
    let old_local = dead_session(&f.server, &f.ctx, &legacy, b"edited in lodav.py");
    FileLock::new(legacy.sessions().join(format!("{}.lock", session_key(&f.url)))).try_acquire().unwrap();
    fs::write(legacy.state.join("lodav.log"), "log").unwrap();
    fs::write(legacy.state.join("office-prompts.json"), "{}").unwrap();
    fs::create_dir_all(&legacy.folder).unwrap();
    fs::write(legacy.folder.join("Kept (nicht hochgeladen 2026-10-01 10-00).docm"), "kept").unwrap();
    fs::write(
        legacy.folder.join("Meldungen.txt"),
        "\u{feff}Meldungen der Datenportal LibreOffice Bridge, die neueste steht am Ende.\n\n\
         2026-10-01 09:59  Erste Meldung\n\n2026-10-01 10:00  Zweite Meldung\n\n",
    )
    .unwrap();

    let moved = migrate(&f.ctx, &legacy).unwrap();

    assert_eq!(moved, Migration::Done(Migrated { sessions: 1, files: 1, messages: 2 }));
    for dir in [&legacy.cache, &legacy.state, &legacy.folder] {
        assert!(!dir.exists(), "{} left behind", dir.display());
    }
    assert!(!old_local.exists());
    assert_eq!(fs::read(f.local()).unwrap(), b"edited in lodav.py");
    assert!(f.ctx.dirs.state.join("office-prompts.json").exists());
    assert!(f.ctx.dirs.folder.join("Kept (nicht hochgeladen 2026-10-01 10-00).docm").exists());
    let texts: Vec<_> = f.ctx.messages.list().into_iter().map(|m| m.text).collect();
    assert_eq!(texts, ["Erste Meldung", "Zweite Meldung"]);
    assert!(f.ctx.messages.unread().is_empty(), "imported as read");

    // and the session lodav.py left unfinished is finished
    assert!(recover_stale(&f.ctx, None).is_empty());
    assert_eq!(f.server.server().content, b"edited in lodav.py");
    assert_eq!(f.server.server().token, None);
    assert!(!f.local().exists());

    assert_eq!(migrate(&f.ctx, &legacy).unwrap(), Migration::Nothing);
}

#[test]
fn moving_over_waits_while_lodav_py_runs() {
    let f = fixture();
    let legacy = legacy_dirs(&f);
    let old_local = dead_session(&f.server, &f.ctx, &legacy, b"being edited in lodav.py");
    let mut running = FileLock::new(legacy.sessions().join(format!("{}.lock", session_key(&f.url))));
    assert!(running.try_acquire().unwrap());

    assert_eq!(migrate(&f.ctx, &legacy).unwrap(), Migration::Postponed);
    assert!(old_local.exists());

    running.release();
    assert!(matches!(migrate(&f.ctx, &legacy).unwrap(), Migration::Done(_)));
    assert!(!old_local.exists());
}
