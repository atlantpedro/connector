//! Session files and the OS file locks that tell live sessions from orphaned ones.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Suffix of files being written; never taken for anything else.
pub const PART: &str = ".connector-part";

/// What a working copy looked like: (mtime in ns, size). Equal signatures mean no change.
pub type Signature = (i64, u64);

pub fn signature(path: &Path) -> Option<Signature> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
    Some((mtime, meta.len()))
}

/// A session as written to `sessions/<key>.json`; the same JSON as lodav.py's.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct State {
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
    /// ETag of the server copy the working copy is based on.
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub local: PathBuf,
    /// Signature of the working copy as last uploaded (or downloaded).
    #[serde(default)]
    pub uploaded: Option<Signature>,
}

impl State {
    pub fn read(path: &Path) -> io::Result<State> {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(io::Error::other)
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        write_atomic(path, &serde_json::to_string(self)?)
    }
}

/// Replace a file so that a power cut leaves either the old or the new content.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let tmp = part_path(path);
    {
        let mut f = File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

pub fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(PART);
    path.with_file_name(name)
}

/// An OS file lock. The OS releases it when the process ends, however it ends (crash, kill,
/// power cut), so a held lock is proof of a live owner, unlike a pid that may be reused.
/// lodav.py's flock / msvcrt locks on the same file exclude it too.
pub struct FileLock {
    path: PathBuf,
    file: Option<File>,
}

impl FileLock {
    pub fn new(path: PathBuf) -> FileLock {
        FileLock { path, file: None }
    }

    pub fn try_acquire(&mut self) -> io::Result<bool> {
        self.acquire(Duration::ZERO)
    }

    pub fn acquire(&mut self, wait: Duration) -> io::Result<bool> {
        let deadline = Instant::now() + wait;
        let file = OpenOptions::new().create(true).append(true).read(true).open(&self.path)?;
        loop {
            match file.try_lock() {
                Ok(()) => {
                    self.file = Some(file);
                    return Ok(true);
                }
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(TryLockError::WouldBlock) => return Ok(false),
                Err(TryLockError::Error(e)) => return Err(e),
            }
        }
    }

    pub fn release(&mut self) {
        self.file = None; // closing drops the lock
    }

    pub fn held(&self) -> bool {
        self.file.is_some()
    }
}
