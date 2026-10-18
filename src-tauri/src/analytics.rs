//! Error reports and a launch event, sent to PostHog, only for someone who
//! turned on "Send error reports" in Settings (`Settings::send_error_reports`,
//! off by default). Nothing is sent, or queued for later, while it is off, and
//! the app never asks anyone to turn it on.
//!
//! Only release builds whose compile environment set `POSTHOG_KEY` and
//! `POSTHOG_HOST` can send anything (the release workflow sets both; a
//! PostHog project key is public); dev builds, tests and local `tauri build`s
//! never do.
//!
//! What is sent: "app started" with the version, OS and architecture, and
//! every error the diagnostics log records (`diagnostics::Level::Error`:
//! background failures, panics and the page's uncaught errors), as a PostHog
//! exception with its source and message (home folder replaced by `~`,
//! capped at `MAX_MESSAGE` characters). Nothing from the library leaves the
//! machine: no frames, no OCR text, no window titles. Events carry a random id,
//! made on the first send and kept in the config directory until reports are
//! turned off, and create no person profile.
//!
//! Like the diagnostics log, `capture` only sends an event down a channel; a
//! thread of its own posts it, so no worker ever waits on the network. At most
//! `LIMIT` events go out per `WINDOW`, so a loop that fails on every frame
//! can't flood the project; the rest are dropped.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const KEY: Option<&str> = option_env!("POSTHOG_KEY");
const HOST: Option<&str> = option_env!("POSTHOG_HOST");

const LIMIT: u32 = 30;
const WINDOW: Duration = Duration::from_secs(60 * 60);
const TIMEOUT: Duration = Duration::from_secs(10);

/// The file in the config directory holding this install's random id.
const ID_FILE: &str = "analytics-id";

enum Message {
    Event { name: String, properties: Value },
    Flush(Sender<()>),
    /// Reports were turned off: forget the install id.
    Forget,
}

static SINK: OnceLock<Sender<Message>> = OnceLock::new();

/// Whether the person opted in. Mirrors `Settings::send_error_reports`.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Follow the "Send error reports" setting. Turning it off drops whatever is
/// still queued and deletes the install id.
pub fn set_enabled(enabled: bool) {
    let was = ENABLED.swap(enabled, Ordering::SeqCst);
    if was && !enabled {
        if let Some(sink) = SINK.get() {
            let _ = sink.send(Message::Forget);
        }
    }
}

/// Start the sender, if this build can send. `enabled` is the saved setting.
/// Later calls do nothing.
pub fn init(config_dir: Option<&Path>, version: &str, enabled: bool) {
    set_enabled(enabled);
    let (Some(key), Some(host)) = (configured(KEY), configured(HOST)) else {
        return;
    };
    if cfg!(debug_assertions) || SINK.get().is_some() {
        return;
    }
    // reqwest is built without a TLS provider of its own (as the updater
    // plugin uses it); install the one the updater would.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let poster = Poster {
        client: match reqwest::Client::builder().timeout(TIMEOUT).build() {
            Ok(client) => client,
            Err(e) => {
                eprintln!("Could not start analytics: {}", e);
                return;
            }
        },
        url: format!("{}/i/v0/e/", host.trim_end_matches('/')),
        key: key.to_string(),
        id_file: config_dir.map(|dir| dir.join(ID_FILE)),
        distinct_id: None,
        common: common_properties(version),
    };
    let (send, receive) = mpsc::channel();
    if SINK.set(send).is_ok() {
        let spawned = std::thread::Builder::new()
            .name("analytics".into())
            .spawn(move || poster.run(receive));
        if let Err(e) = spawned {
            eprintln!("Could not start analytics: {}", e);
        }
    }
}

/// Queue an event. Never blocks; does nothing unless `init` started sending
/// and reports are turned on.
pub fn capture(name: &str, properties: Value) {
    if !ENABLED.load(Ordering::SeqCst) {
        return;
    }
    if let Some(sink) = SINK.get() {
        let _ = sink.send(Message::Event { name: name.to_string(), properties });
    }
}

/// Queue an error as a PostHog exception.
pub fn exception(source: &str, message: &str) {
    if SINK.get().is_some() {
        let home = dirs::home_dir();
        let message = scrub(message, home.as_deref().and_then(Path::to_str));
        capture("$exception", exception_properties(source, &message));
    }
}

/// Longest message sent, in characters.
const MAX_MESSAGE: usize = 1000;

/// Paths in error messages name the account (`/Users/<name>/…`), so the home
/// folder becomes `~`; and a message is cut at `MAX_MESSAGE` characters.
fn scrub(message: &str, home: Option<&str>) -> String {
    let message = match home.map(|h| h.trim_end_matches('/')).filter(|h| h.len() > 1) {
        Some(home) => message.replace(home, "~"),
        None => message.to_string(),
    };
    message.chars().take(MAX_MESSAGE).collect()
}

/// Wait up to `timeout` for every event queued so far to be sent. For a panic,
/// which may be about to take the process down.
pub fn flush(timeout: Duration) {
    if let Some(sink) = SINK.get() {
        let (done, wait) = mpsc::channel();
        if sink.send(Message::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(timeout);
        }
    }
}

fn configured(value: Option<&'static str>) -> Option<&'static str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// This install's random id, created on first use. Without a config directory
/// (or if it can't be written) each run gets a fresh one.
fn install_id(path: Option<&Path>) -> String {
    if let Some(id) = path.and_then(|p| std::fs::read_to_string(p).ok()) {
        let id = id.trim();
        if !id.is_empty() {
            return id.to_string();
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(path) = path {
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, &id));
        if let Err(e) = written {
            eprintln!("Could not save the analytics id: {}", e);
        }
    }
    id
}

fn common_properties(version: &str) -> Map<String, Value> {
    let mut properties = Map::new();
    properties.insert("$lib".into(), json!("timelapse-app"));
    properties.insert("app_version".into(), json!(version));
    properties.insert("os".into(), json!(std::env::consts::OS));
    properties.insert("arch".into(), json!(std::env::consts::ARCH));
    // Anonymous events: no person profile per install.
    properties.insert("$process_person_profile".into(), json!(false));
    properties
}

fn exception_properties(source: &str, message: &str) -> Value {
    json!({
        "$exception_list": [{ "type": source, "value": message }],
        "$exception_level": "error",
        "source": source,
    })
}

/// The body PostHog's capture endpoint takes for one event.
fn payload(key: &str, distinct_id: &str, common: &Map<String, Value>, name: &str, properties: Value) -> Value {
    let mut merged = common.clone();
    if let Value::Object(own) = properties {
        merged.extend(own);
    }
    json!({
        "api_key": key,
        "event": name,
        "distinct_id": distinct_id,
        "properties": merged,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })
}

/// Lets `LIMIT` events through per `WINDOW`.
struct Limiter {
    started: Instant,
    count: u32,
}

impl Limiter {
    fn new(now: Instant) -> Self {
        Limiter { started: now, count: 0 }
    }

    fn allow(&mut self, now: Instant) -> bool {
        if now.duration_since(self.started) >= WINDOW {
            self.started = now;
            self.count = 0;
        }
        self.count += 1;
        self.count <= LIMIT
    }
}

struct Poster {
    client: reqwest::Client,
    url: String,
    key: String,
    id_file: Option<PathBuf>,
    /// Read or made on the first send, so nobody who never opts in gets one.
    distinct_id: Option<String>,
    common: Map<String, Value>,
}

impl Poster {
    fn run(mut self, receiver: Receiver<Message>) {
        let mut limiter = Limiter::new(Instant::now());
        for message in receiver {
            match message {
                // Checked again here: reports may have been turned off since
                // the event was queued.
                Message::Event { name, properties } => {
                    if ENABLED.load(Ordering::SeqCst) && limiter.allow(Instant::now()) {
                        self.send(&name, properties);
                    }
                }
                Message::Flush(done) => {
                    let _ = done.send(());
                }
                Message::Forget => {
                    self.distinct_id = None;
                    if let Some(path) = &self.id_file {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        }
    }

    fn send(&mut self, name: &str, properties: Value) {
        let id_file = self.id_file.as_deref();
        let distinct_id = self.distinct_id.get_or_insert_with(|| install_id(id_file));
        let body = payload(&self.key, distinct_id, &self.common, name, properties);
        // Built inside `block_on`: with a timeout set, `send` starts a tokio
        // timer straight away, which panics on this plain thread.
        let request = async { self.client.post(&self.url).json(&body).send().await };
        if let Err(e) = tauri::async_runtime::block_on(request).and_then(|r| r.error_for_status()) {
            // Not a diagnostics error: that would be sent here again.
            eprintln!("Could not send {} to PostHog: {}", name, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_merges_common_and_own_properties() {
        let common = common_properties("1.2.3");
        let body = payload("phc_key", "id-1", &common, "$exception", exception_properties("ocr", "boom"));
        assert_eq!(body["api_key"], "phc_key");
        assert_eq!(body["event"], "$exception");
        assert_eq!(body["distinct_id"], "id-1");
        let properties = &body["properties"];
        assert_eq!(properties["app_version"], "1.2.3");
        assert_eq!(properties["$process_person_profile"], false);
        assert_eq!(properties["$exception_list"][0]["type"], "ocr");
        assert_eq!(properties["$exception_list"][0]["value"], "boom");
    }

    #[test]
    fn limiter_caps_each_window() {
        let start = Instant::now();
        let mut limiter = Limiter::new(start);
        assert!((0..LIMIT).all(|_| limiter.allow(start)));
        assert!(!limiter.allow(start));
        assert!(limiter.allow(start + WINDOW));
    }

    #[test]
    fn install_id_is_kept() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(ID_FILE);
        let first = install_id(Some(&path));
        assert_eq!(install_id(Some(&path)), first);
        assert_ne!(install_id(None), first);
    }

    #[test]
    fn scrub_hides_the_home_folder_and_caps_length() {
        assert_eq!(
            scrub("cannot open /Users/nino/Timelapse/x.png", Some("/Users/nino/")),
            "cannot open ~/Timelapse/x.png"
        );
        assert_eq!(scrub("no home", None), "no home");
        // A home of "/" would turn every slash into "~".
        assert_eq!(scrub("/a/b", Some("/")), "/a/b");
        assert_eq!(scrub(&"x".repeat(5000), None).chars().count(), MAX_MESSAGE);
    }

    /// Sends one event to a local server, from a plain thread as `run` does.
    #[test]
    fn send_posts_from_a_plain_thread() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/i/v0/e/", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 4096];
            // Read the headers, then as much body as they announce.
            let complete = |request: &[u8]| {
                let text = String::from_utf8_lossy(request);
                let Some(end) = text.find("\r\n\r\n") else {
                    return false;
                };
                let length = text[..end]
                    .lines()
                    .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().to_string()))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                request.len() >= end + 4 + length
            };
            while !complete(&request) {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}")
                .unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let mut poster = Poster {
            client: reqwest::Client::builder().timeout(TIMEOUT).build().unwrap(),
            url,
            key: "phc_test".into(),
            id_file: None,
            distinct_id: None,
            common: common_properties("1.2.3"),
        };
        std::thread::spawn(move || poster.send("$exception", exception_properties("test", "boom")))
            .join()
            .expect("send must not panic off the async runtime");
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /i/v0/e/ "), "{request}");
        assert!(request.contains("\"api_key\":\"phc_test\""), "{request}");
    }

    #[test]
    fn blank_configuration_is_unset() {
        assert_eq!(configured(Some("  ")), None);
        assert_eq!(configured(None), None);
        assert_eq!(configured(Some("phc_x")), Some("phc_x"));
    }
}
