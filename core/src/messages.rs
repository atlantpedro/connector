//! The message log the window shows: every message the user got, so that none is lost with a
//! notification dismissed unread.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::store::write_atomic;

/// Bytes; older messages are dropped beyond that.
pub const MAX_BYTES: u64 = 200_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Milliseconds since the epoch, made unique; orders the messages.
    pub id: u64,
    /// Local time, RFC 3339.
    pub time: String,
    pub text: String,
    #[serde(default)]
    pub urgent: bool,
    /// A version set aside for the user that the message is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept: Option<PathBuf>,
}

pub struct MessageLog {
    path: PathBuf,
    seen: PathBuf,
    max_bytes: u64,
    last_id: Mutex<Option<u64>>,
}

impl MessageLog {
    pub fn new(state: &Path) -> MessageLog {
        MessageLog::with_limit(state, MAX_BYTES)
    }

    pub fn with_limit(state: &Path, max_bytes: u64) -> MessageLog {
        MessageLog {
            path: state.join("messages.jsonl"),
            seen: state.join("messages-seen"),
            max_bytes,
            last_id: Mutex::new(None),
        }
    }

    pub fn append(&self, text: &str, urgent: bool, kept: Option<&Path>) -> io::Result<Message> {
        let now = chrono::Local::now();
        let millis = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        self.add(Message {
            id: millis,
            time: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            text: text.to_string(),
            urgent,
            kept: kept.map(Path::to_path_buf),
        })
    }

    /// Add a message; its id is raised above every earlier one if needed.
    pub fn add(&self, mut message: Message) -> io::Result<Message> {
        let mut last = self.last_id.lock().unwrap();
        let last_id = match *last {
            Some(id) => id,
            None => self.list().last().map_or(0, |m| m.id),
        };
        message.id = message.id.max(last_id + 1);
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        self.trim()?;
        let mut file = OpenOptions::new().create(true).append(true).read(true).open(&self.path)?;
        // after a line cut short (power cut), start on a fresh one
        let mut line = if ends_open(&mut file)? { "\n".to_string() } else { String::new() };
        line += &serde_json::to_string(&message)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;
        *last = Some(message.id);
        Ok(message)
    }

    /// Oldest first.
    pub fn list(&self) -> Vec<Message> {
        let text = fs::read_to_string(&self.path).unwrap_or_default();
        // a line cut short by a power cut is skipped, not fatal
        text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
    }

    pub fn unread(&self) -> Vec<Message> {
        let seen = self.seen_id();
        self.list().into_iter().filter(|m| m.id > seen).collect()
    }

    /// The user has seen every message up to and including id.
    pub fn mark_seen(&self, id: u64) -> io::Result<()> {
        if id > self.seen_id() {
            write_atomic(&self.seen, &id.to_string())?;
        }
        Ok(())
    }

    /// The newest message the user has seen; 0 if none.
    pub fn seen_id(&self) -> u64 {
        fs::read_to_string(&self.seen).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
    }

    /// Drop the older half once the log grows beyond its limit.
    fn trim(&self) -> io::Result<()> {
        match fs::metadata(&self.path) {
            Ok(meta) if meta.len() > self.max_bytes => {}
            _ => return Ok(()),
        }
        let text = fs::read_to_string(&self.path)?;
        let half = text.len() / 2;
        let cut = text[half..].find('\n').map_or(text.len(), |i| half + i + 1);
        write_atomic(&self.path, &text[cut..])
    }
}

/// True if the file doesn't end with a newline (and isn't empty).
fn ends_open(file: &mut fs::File) -> io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8];
    file.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appended_in_order_and_unread_until_seen() {
        let tmp = tempfile::tempdir().unwrap();
        let log = MessageLog::new(tmp.path());
        let first = log.append("erste", false, None).unwrap();
        let second = log.append("zweite", true, Some(Path::new("/x/a.docx"))).unwrap();
        assert!(second.id > first.id);
        let texts: Vec<_> = log.list().into_iter().map(|m| m.text).collect();
        assert_eq!(texts, ["erste", "zweite"]);
        assert_eq!(log.unread().len(), 2);
        log.mark_seen(first.id).unwrap();
        assert_eq!(log.unread(), [second.clone()]);
        log.mark_seen(second.id).unwrap();
        assert!(log.unread().is_empty());
        // a later log on the same file goes on from the last id
        let again = MessageLog::new(tmp.path()).add(Message { id: 1, ..second.clone() }).unwrap();
        assert!(again.id > second.id);
    }

    #[test]
    fn old_messages_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let log = MessageLog::with_limit(tmp.path(), 2_000);
        for i in 0..100 {
            log.append(&format!("Meldung {i:03}"), false, None).unwrap();
        }
        let texts: Vec<_> = log.list().into_iter().map(|m| m.text).collect();
        assert!(fs::metadata(tmp.path().join("messages.jsonl")).unwrap().len() <= 2_200);
        assert!(!texts.contains(&"Meldung 000".to_string()));
        assert_eq!(texts.last().unwrap(), "Meldung 099");
    }

    #[test]
    fn broken_line_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let log = MessageLog::new(tmp.path());
        log.append("ganz", false, None).unwrap();
        OpenOptions::new().append(true).open(tmp.path().join("messages.jsonl")).unwrap().write_all(b"{\"id\": 9").unwrap();
        assert_eq!(log.list().len(), 1);
        log.append("danach", false, None).unwrap();
        let texts: Vec<_> = log.list().into_iter().map(|m| m.text).collect();
        assert_eq!(texts, ["ganz", "danach"]);
    }
}
