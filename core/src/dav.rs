//! The few WebDAV calls a session needs. Auth is the token in the URL path.

use std::fmt;
use std::fs::{self, File};
use std::path::Path;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Method;

use crate::store::part_path;

/// Seconds requested per LOCK.
pub const LOCK_TIMEOUT: u32 = 180;

const LOCKINFO: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
    <D:lockinfo xmlns:D=\"DAV:\"><D:lockscope><D:exclusive/></D:lockscope>\
    <D:locktype><D:write/></D:locktype><D:owner>Datenportal Connector</D:owner></D:lockinfo>";
const LOCKDISCOVERY: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
    <D:propfind xmlns:D=\"DAV:\"><D:prop><D:lockdiscovery/></D:prop></D:propfind>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The server refused (HTTP 4xx, or a refresh of an expired lock).
    Http { status: u16, what: String },
    /// HTTP 5xx: server or proxy trouble, retried like a network error.
    Server { status: u16, what: String },
    /// The server can't be reached, or a local file can't be read: worth retrying.
    Network(String),
    /// Our lock expired and the document was locked or changed by someone else meanwhile.
    Conflict(String),
}

impl Error {
    /// Worth trying again later: the server can't be reached or has trouble.
    pub fn is_transient(&self) -> bool {
        matches!(self, Error::Network(_) | Error::Server { .. })
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Http { status, .. } | Error::Server { status, .. } => Some(*status),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Http { status, what } | Error::Server { status, what } => write!(f, "{what}: HTTP {status}"),
            Error::Network(e) => write!(f, "{e}"),
            Error::Conflict(e) => write!(f, "conflict: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Error {
        Error::Network(format!("{e}"))
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Network(format!("{e}"))
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Dav {
    pub url: String,
    pub token: Option<String>,
    /// Of the server copy our working copy is based on.
    pub etag: Option<String>,
    pub content_type: String,
    client: Client,
    timeout: Duration,
}

impl Dav {
    pub fn new(client: Client, url: &str, timeout: Duration) -> Dav {
        Dav {
            url: url.to_string(),
            token: None,
            etag: None,
            content_type: "application/octet-stream".to_string(),
            client,
            timeout,
        }
    }

    fn request(&self, method: &str, body: Option<Vec<u8>>, headers: &[(&str, &str)]) -> Result<(u16, HeaderMap, Vec<u8>)> {
        let method = Method::from_bytes(method.as_bytes()).expect("valid method");
        let mut req = self.client.request(method.clone(), &self.url).timeout(self.timeout);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        if let Some(body) = body {
            req = req.body(body);
        }
        let resp = req.send()?;
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        let body = resp.bytes()?.to_vec();
        if status >= 500 {
            return Err(Error::Server { status, what: method.to_string() });
        }
        if status >= 400 {
            return Err(Error::Http { status, what: method.to_string() });
        }
        Ok((status, headers, body))
    }

    fn if_header(&self) -> Option<String> {
        self.token.as_ref().map(|t| format!("({t})"))
    }

    pub fn download(&mut self, dest: &Path) -> Result<()> {
        let mut resp = self.client.get(&self.url).timeout(self.timeout).send()?;
        let status = resp.status().as_u16();
        if status >= 500 {
            return Err(Error::Server { status, what: "GET".into() });
        }
        if status >= 400 {
            return Err(Error::Http { status, what: "GET".into() });
        }
        let tmp = part_path(dest);
        let mut file = File::create(&tmp)?;
        resp.copy_to(&mut file)?;
        file.sync_all()?;
        drop(file);
        if let Some(ct) = header(resp.headers(), "Content-Type") {
            self.content_type = ct;
        }
        self.etag = header(resp.headers(), "ETag");
        fs::rename(&tmp, dest)?;
        Ok(())
    }

    pub fn lock(&mut self) -> Result<()> {
        let timeout = format!("Second-{LOCK_TIMEOUT}");
        let headers = [("Content-Type", "application/xml; charset=utf-8"), ("Timeout", timeout.as_str()), ("Depth", "0")];
        let (_, headers, _) = self.request("LOCK", Some(LOCKINFO.into()), &headers)?;
        let token = header(&headers, "Lock-Token").unwrap_or_default();
        let token = token.trim();
        if token.is_empty() {
            return Err(Error::Http { status: 0, what: "LOCK returned no Lock-Token".into() });
        }
        self.token = Some(if token.starts_with('<') { token.to_string() } else { format!("<{token}>") });
        Ok(())
    }

    pub fn refresh(&self) -> Result<()> {
        let timeout = format!("Second-{LOCK_TIMEOUT}");
        let if_header = self.if_header();
        let mut headers = vec![("Timeout", timeout.as_str())];
        if let Some(h) = &if_header {
            headers.push(("If", h));
        }
        match self.request("LOCK", None, &headers) {
            // Milton answers the refresh of an expired lock with 500, not 412. Taken for an
            // outage, it was retried forever; relock() tells the two apart with a fresh LOCK.
            Err(Error::Server { status, .. }) => Err(Error::Http { status, what: "LOCK refresh".into() }),
            other => other.map(|_| ()),
        }
    }

    /// Lock again after our lock expired, unless someone else locked or changed the document.
    pub fn relock(&mut self) -> Result<()> {
        self.token = None;
        match self.lock() {
            Err(Error::Http { status: 423, .. }) => return Err(Error::Conflict("locked by someone else".into())),
            other => other?,
        }
        let etag = self.current_etag()?;
        // no base ETag (a session file from an older lodav): changes by others can't be ruled out
        if etag.is_none() || etag != self.etag {
            if let Err(e) = self.unlock() {
                log::warn!("could not release the lock after a conflict: {e}");
            }
            return Err(Error::Conflict(format!("changed on the server (ETag {:?} -> {:?})", self.etag, etag)));
        }
        Ok(())
    }

    pub fn current_etag(&self) -> Result<Option<String>> {
        Ok(header(&self.request("HEAD", None, &[])?.1, "ETag"))
    }

    pub fn upload(&mut self, path: &Path) -> Result<()> {
        let data = fs::read(path)?;
        let if_header = self.if_header();
        let mut headers = vec![("Content-Type", self.content_type.as_str())];
        if let Some(h) = &if_header {
            headers.push(("If", h));
        }
        self.request("PUT", Some(data), &headers)?;
        // the server rewrites every upload, so its ETag is only known by asking
        self.etag = self.current_etag()?;
        Ok(())
    }

    pub fn unlock(&mut self) -> Result<()> {
        if let Some(token) = self.token.clone() {
            self.request("UNLOCK", None, &[("Lock-Token", &token)])?;
            self.token = None;
        }
        Ok(())
    }

    pub fn is_locked(&self) -> Result<bool> {
        // Milton leaves lockdiscovery out of an allprop PROPFIND, so ask for it explicitly
        let headers = [("Depth", "0"), ("Content-Type", "application/xml; charset=utf-8")];
        let (_, _, body) = self.request("PROPFIND", Some(LOCKDISCOVERY.into()), &headers)?;
        Ok(body.windows(b"activelock".len()).any(|w| w == b"activelock"))
    }
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name).and_then(|v: &HeaderValue| v.to_str().ok()).map(str::to_string)
}
