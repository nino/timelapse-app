//! Error reports and a launch event, sent to PostHog.
//!
//! Only release builds whose compile environment set `POSTHOG_KEY` and
//! `POSTHOG_HOST` send anything (the release workflow sets both from
//! repository secrets); dev builds, tests and local `tauri build`s never do.
//!
//! What is sent: "app started" with the version, OS and architecture, and
//! every error the diagnostics log records (`diagnostics::Level::Error`:
//! background failures, panics and the page's uncaught errors), as a PostHog
//! exception with its source and message. Nothing from the library leaves the
//! machine: no frames, no OCR text, no window titles. Events carry a random id
//! kept in the config directory and create no person profile.
//!
//! Like the diagnostics log, `capture` only sends an event down a channel; a
//! thread of its own posts it, so no worker ever waits on the network. At most
//! `LIMIT` events go out per `WINDOW`, so a loop that fails on every frame
//! can't flood the project; the rest are dropped.

use serde_json::{json, Map, Value};
use std::path::Path;
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
}

static SINK: OnceLock<Sender<Message>> = OnceLock::new();

/// Start sending, if this build is meant to. Later calls do nothing.
pub fn init(config_dir: Option<&Path>, version: &str) {
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
        distinct_id: install_id(config_dir),
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

/// Queue an event. Never blocks; does nothing unless `init` started sending.
pub fn capture(name: &str, properties: Value) {
    if let Some(sink) = SINK.get() {
        let _ = sink.send(Message::Event { name: name.to_string(), properties });
    }
}

/// Queue an error as a PostHog exception.
pub fn exception(source: &str, message: &str) {
    if SINK.get().is_some() {
        capture("$exception", exception_properties(source, message));
    }
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
fn install_id(config_dir: Option<&Path>) -> String {
    let path = config_dir.map(|dir| dir.join(ID_FILE));
    if let Some(id) = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
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
    distinct_id: String,
    common: Map<String, Value>,
}

impl Poster {
    fn run(self, receiver: Receiver<Message>) {
        let mut limiter = Limiter::new(Instant::now());
        for message in receiver {
            match message {
                Message::Event { name, properties } => {
                    if limiter.allow(Instant::now()) {
                        self.send(&name, properties);
                    }
                }
                Message::Flush(done) => {
                    let _ = done.send(());
                }
            }
        }
    }

    fn send(&self, name: &str, properties: Value) {
        let body = payload(&self.key, &self.distinct_id, &self.common, name, properties);
        let request = self.client.post(&self.url).json(&body).send();
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
        let first = install_id(Some(dir.path()));
        assert_eq!(install_id(Some(dir.path())), first);
        assert_ne!(install_id(None), first);
    }

    #[test]
    fn blank_configuration_is_unset() {
        assert_eq!(configured(Some("  ")), None);
        assert_eq!(configured(None), None);
        assert_eq!(configured(Some("phc_x")), Some("phc_x"));
    }
}
