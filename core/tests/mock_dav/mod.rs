//! A local mock of the CMS WebDAV server, with the behaviour that matters:
//! exclusive locks with a token, PUT refused on a locked resource without that token,
//! lockdiscovery only reported when asked for explicitly, HTTP 500 when an expired lock is
//! refreshed.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

#[derive(Default)]
pub struct Server {
    pub content: Vec<u8>,
    pub token: Option<String>,
    pub lock_count: u32,
    pub puts: u32,
    pub refreshes: u32,
}

pub struct MockDav {
    pub state: Arc<Mutex<Server>>,
    down: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    pub port: u16,
}

pub fn etag(content: &[u8]) -> String {
    let hex: String = Sha256::digest(content)[..6].iter().map(|b| format!("{b:02x}")).collect();
    format!("\"{hex}\"")
}

impl MockDav {
    pub fn start(content: &[u8]) -> MockDav {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(Server { content: content.to_vec(), ..Server::default() }));
        let down = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, d, st) = (state.clone(), down.clone(), stop.clone());
        thread::spawn(move || {
            while !st.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if d.load(Ordering::SeqCst) {
                            continue; // network outage: drop the connection without an answer
                        }
                        let s = s.clone();
                        thread::spawn(move || handle(stream, &s));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        MockDav { state, down, stop, port }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/webdav/Asset/tok-184/1/Test%20Dokument.docm", self.port)
    }

    pub fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }

    pub fn server(&self) -> std::sync::MutexGuard<'_, Server> {
        self.state.lock().unwrap()
    }

    /// Cut the connection for a while, starting after down_after seconds.
    pub fn outage(&self, down_after: f64, up_after: f64) {
        let down = self.down.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs_f64(down_after));
            down.store(true, Ordering::SeqCst);
            thread::sleep(Duration::from_secs_f64(up_after - down_after));
            down.store(false, Ordering::SeqCst);
        });
    }
}

impl Drop for MockDav {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn handle(stream: TcpStream, state: &Mutex<Server>) {
    stream.set_nonblocking(false).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let method = line.split_whitespace().next().unwrap_or("").to_string();
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0; len];
    reader.read_exact(&mut body).unwrap();
    let header = |k: &str| headers.get(k).cloned().unwrap_or_default();

    let mut s = state.lock().unwrap();
    let holds_lock = s.token.as_ref().is_some_and(|t| header("if").contains(t.as_str()));
    let (status, extra, out): (u16, Vec<(String, String)>, Vec<u8>) = match method.as_str() {
        "GET" => (
            200,
            vec![("Content-Type".into(), "application/vnd.ms-word.document.macroenabled.12".into()), ("ETag".into(), etag(&s.content))],
            s.content.clone(),
        ),
        "HEAD" => (200, vec![("ETag".into(), etag(&s.content))], Vec::new()),
        "LOCK" if body.is_empty() => {
            // refresh
            if holds_lock {
                s.refreshes += 1;
                (200, vec![], vec![])
            } else {
                (500, vec![], vec![]) // what Milton answers for an expired lock
            }
        }
        "LOCK" if s.token.is_some() => (423, vec![], vec![]),
        "LOCK" => {
            s.lock_count += 1;
            let token = format!("opaquelocktoken:336|{}", s.lock_count);
            s.token = Some(token.clone());
            (200, vec![("Lock-Token".into(), format!("<{token}>"))], b"<d:prop/>".to_vec())
        }
        "PUT" if s.token.is_some() && !holds_lock => (423, vec![], vec![]),
        "PUT" => {
            s.content = body.clone();
            s.puts += 1;
            (204, vec![], vec![])
        }
        "UNLOCK" if s.token.as_ref().is_some_and(|t| header("lock-token") == format!("<{t}>")) => {
            s.token = None;
            (204, vec![], vec![])
        }
        "UNLOCK" => (409, vec![], vec![]),
        "PROPFIND" => {
            let asked = body.windows(13).any(|w| w == b"lockdiscovery");
            let active: &[u8] = if s.token.is_some() && asked { b"<d:activelock/>" } else { b"" };
            (207, vec![], [b"<d:multistatus>".as_slice(), active, b"</d:multistatus>"].concat())
        }
        _ => (405, vec![], vec![]),
    };
    let length = if method == "HEAD" { s.content.len() } else { out.len() };
    drop(s);
    let mut response = format!("HTTP/1.1 {status} X\r\n");
    for (k, v) in extra {
        response += &format!("{k}: {v}\r\n");
    }
    response += &format!("Content-Length: {length}\r\nConnection: close\r\n\r\n");
    let mut stream = stream;
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(&out);
}
