//! A log of what the app did and what went wrong, kept in
//! `<library>/diagnostics.db` so it can be read after the fact.
//!
//! Events are rare (state changes, errors, finished batches, hourly
//! summaries), never one per frame. Recording one only sends it down a
//! channel; a thread of its own writes whatever has queued up in one
//! transaction, so no worker ever waits on the disk for it. Before `init`
//! (and in tests) events go nowhere.
//!
//! Rows older than `MAX_AGE`, and beyond the newest `MAX_ROWS`, are deleted
//! when the log opens and every `PRUNE_EVERY` after that.

use chrono::{DateTime, Local};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const FILE_NAME: &str = "diagnostics.db";

const MAX_AGE: chrono::TimeDelta = chrono::TimeDelta::days(30);
const MAX_ROWS: i64 = 100_000;
const PRUNE_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// At most this many events are written per `BURST_WINDOW`, so a loop that
/// fails on every frame can't fill the disk. The rest are counted and the
/// count written once the window ends.
const BURST_LIMIT: u32 = 600;
const BURST_WINDOW: Duration = Duration::from_secs(60 * 60);

/// How often a `Tally` writes its summary.
pub const TALLY_PERIOD: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

/// One row of the log. Build it with `info`/`warn`/`error`, add a duration or
/// data, and `record` it.
#[derive(Debug, Clone)]
pub struct Event {
    at: DateTime<Local>,
    level: Level,
    source: &'static str,
    message: String,
    duration: Option<Duration>,
    data: Option<Value>,
}

pub fn info(source: &'static str, message: impl Into<String>) -> Event {
    Event::new(Level::Info, source, message)
}

pub fn warn(source: &'static str, message: impl Into<String>) -> Event {
    Event::new(Level::Warn, source, message)
}

pub fn error(source: &'static str, message: impl Into<String>) -> Event {
    Event::new(Level::Error, source, message)
}

impl Event {
    fn new(level: Level, source: &'static str, message: impl Into<String>) -> Self {
        Event { at: Local::now(), level, source, message: message.into(), duration: None, data: None }
    }

    /// How long the thing being reported took.
    pub fn took(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Details, stored as JSON.
    pub fn data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// Queue the event for the log. Never blocks.
    pub fn record(self) {
        if let Some(sink) = SINK.get() {
            let _ = sink.send(Message::Event(self));
        }
    }
}

enum Message {
    Event(Event),
    Flush(Sender<()>),
}

static SINK: OnceLock<Sender<Message>> = OnceLock::new();

/// Open `<root>/diagnostics.db` and start writing to it. Later calls do
/// nothing.
pub fn init(root: &Path) -> rusqlite::Result<()> {
    if SINK.get().is_some() {
        return Ok(());
    }
    let log = Log::open(&root.join(FILE_NAME))?;
    let (sender, receiver) = mpsc::channel();
    if SINK.set(sender).is_ok() {
        let spawned = std::thread::Builder::new()
            .name("diagnostics".into())
            .spawn(move || log.run(receiver));
        if let Err(e) = spawned {
            eprintln!("Could not start the diagnostics log: {}", e);
        }
    }
    Ok(())
}

/// Wait up to `timeout` for every event recorded so far to be written. For
/// the moment before the app quits.
pub fn flush(timeout: Duration) {
    if let Some(sink) = SINK.get() {
        let (done, wait) = mpsc::channel();
        if sink.send(Message::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(timeout);
        }
    }
}

/// Record panics, which otherwise only reach stderr, before the default hook
/// prints them.
pub fn record_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        error("panic", info.to_string()).data(json!({ "thread": thread })).record();
        flush(Duration::from_secs(1));
        previous(info);
    }));
}

struct Log {
    conn: Connection,
    pruned_at: Instant,
    burst_started: Instant,
    burst_count: u32,
    dropped: u32,
}

impl Log {
    fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        // A plain rollback journal, not WAL, so the file on its own is the
        // whole log and can be copied while the app runs.
        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS events (
                 id INTEGER PRIMARY KEY,
                 at TEXT NOT NULL,
                 level TEXT NOT NULL,
                 source TEXT NOT NULL,
                 message TEXT NOT NULL,
                 duration_ms INTEGER,
                 data TEXT
             );
             CREATE INDEX IF NOT EXISTS events_at ON events(at);",
        )?;
        let log = Log { conn, pruned_at: Instant::now(), burst_started: Instant::now(), burst_count: 0, dropped: 0 };
        log.prune(Local::now())?;
        Ok(log)
    }

    fn run(mut self, receiver: Receiver<Message>) {
        loop {
            let first = match receiver.recv_timeout(PRUNE_EVERY) {
                Ok(message) => Some(message),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            let mut events = Vec::new();
            let mut flushes = Vec::new();
            for message in first.into_iter().chain(receiver.try_iter()) {
                match message {
                    Message::Event(event) => events.push(event),
                    Message::Flush(done) => flushes.push(done),
                }
            }
            let events = self.limit(events, Instant::now());
            if let Err(e) = self.write(&events) {
                eprintln!("Could not write the diagnostics log: {}", e);
            }
            if self.pruned_at.elapsed() >= PRUNE_EVERY {
                self.pruned_at = Instant::now();
                if let Err(e) = self.prune(Local::now()) {
                    eprintln!("Could not prune the diagnostics log: {}", e);
                }
            }
            for done in flushes {
                let _ = done.send(());
            }
        }
    }

    /// Drop what is over `BURST_LIMIT` for the current window, and once a
    /// window with drops ends, say how many went.
    fn limit(&mut self, events: Vec<Event>, now: Instant) -> Vec<Event> {
        let mut kept = Vec::with_capacity(events.len());
        if now.duration_since(self.burst_started) >= BURST_WINDOW {
            if self.dropped > 0 {
                kept.push(
                    warn("diagnostics", format!("Dropped {} events over the limit of {} an hour", self.dropped, BURST_LIMIT))
                        .data(json!({ "dropped": self.dropped })),
                );
            }
            self.burst_started = now;
            self.burst_count = 0;
            self.dropped = 0;
        }
        for event in events {
            if self.burst_count < BURST_LIMIT {
                self.burst_count += 1;
                kept.push(event);
            } else {
                self.dropped += 1;
            }
        }
        kept
    }

    fn write(&mut self, events: &[Event]) -> rusqlite::Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO events (at, level, source, message, duration_ms, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for event in events {
                insert.execute(params![
                    event.at.to_rfc3339_opts(chrono::SecondsFormat::Millis, false),
                    event.level.as_str(),
                    event.source,
                    event.message,
                    event.duration.map(|d| d.as_millis() as i64),
                    event.data.as_ref().map(|d| d.to_string()),
                ])?;
            }
        }
        tx.commit()
    }

    /// Delete rows older than `MAX_AGE` before `now`, then all but the newest
    /// `MAX_ROWS`.
    fn prune(&self, now: DateTime<Local>) -> rusqlite::Result<usize> {
        // Times are stored with their offset, so compare them as instants.
        let cutoff = (now - MAX_AGE).timestamp();
        let old = self.conn.execute("DELETE FROM events WHERE unixepoch(at) < ?1", [cutoff])?;
        let excess = self.conn.execute(
            "DELETE FROM events WHERE id <= (SELECT id FROM events ORDER BY id DESC LIMIT 1 OFFSET ?1)",
            [MAX_ROWS],
        )?;
        Ok(old + excess)
    }
}

/// Counts and timings gathered over `TALLY_PERIOD` and written as one event,
/// for things that happen too often to log one by one.
pub struct Tally {
    source: &'static str,
    since: Instant,
    counts: BTreeMap<&'static str, u64>,
    timings: BTreeMap<&'static str, Timing>,
}

#[derive(Default)]
struct Timing {
    count: u64,
    total: Duration,
    max: Duration,
}

impl Tally {
    pub fn new(source: &'static str) -> Self {
        Tally { source, since: Instant::now(), counts: BTreeMap::new(), timings: BTreeMap::new() }
    }

    pub fn count(&mut self, name: &'static str, n: u64) {
        *self.counts.entry(name).or_default() += n;
    }

    pub fn time(&mut self, name: &'static str, took: Duration) {
        let timing = self.timings.entry(name).or_default();
        timing.count += 1;
        timing.total += took;
        timing.max = timing.max.max(took);
    }

    /// Record the summary and start a new one, once `TALLY_PERIOD` has gone
    /// by. A period with nothing in it records nothing.
    pub fn report_if_due(&mut self) {
        if let Some(event) = self.take_if_due(Instant::now()) {
            event.record();
        }
    }

    fn take_if_due(&mut self, now: Instant) -> Option<Event> {
        let period = now.duration_since(self.since);
        if period < TALLY_PERIOD {
            return None;
        }
        let counts = std::mem::take(&mut self.counts);
        let timings = std::mem::take(&mut self.timings);
        self.since = now;
        if counts.is_empty() && timings.is_empty() {
            return None;
        }
        let timings: BTreeMap<_, _> = timings
            .into_iter()
            .map(|(name, t)| {
                let avg = t.total.as_secs_f64() * 1000.0 / t.count as f64;
                (name, json!({ "count": t.count, "avgMs": avg.round(), "maxMs": t.max.as_millis() as u64 }))
            })
            .collect();
        Some(
            info(self.source, "Summary")
                .took(period)
                .data(json!({ "counts": counts, "timings": timings })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn rows(log: &Log) -> Vec<(String, String, String, Option<i64>, Option<String>)> {
        let mut stmt = log
            .conn
            .prepare("SELECT level, source, message, duration_ms, data FROM events ORDER BY id")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn writes_events() {
        let dir = TempDir::new().unwrap();
        let mut log = Log::open(&dir.path().join(FILE_NAME)).unwrap();
        log.write(&[
            info("app", "Started"),
            error("converter", "ffmpeg failed").took(Duration::from_millis(1500)).data(json!({ "frames": 3 })),
        ])
        .unwrap();
        assert_eq!(
            rows(&log),
            vec![
                ("info".into(), "app".into(), "Started".into(), None, None),
                (
                    "error".into(),
                    "converter".into(),
                    "ffmpeg failed".into(),
                    Some(1500),
                    Some(r#"{"frames":3}"#.into())
                ),
            ]
        );
    }

    #[test]
    fn prunes_old_rows() {
        let dir = TempDir::new().unwrap();
        let mut log = Log::open(&dir.path().join(FILE_NAME)).unwrap();
        let mut old = info("app", "old");
        old.at = Local::now() - chrono::TimeDelta::days(31);
        log.write(&[old, info("app", "new")]).unwrap();
        assert_eq!(log.prune(Local::now()).unwrap(), 1);
        assert_eq!(rows(&log).len(), 1);
        assert_eq!(rows(&log)[0].2, "new");
    }

    #[test]
    fn limits_bursts_and_says_how_many_went() {
        let dir = TempDir::new().unwrap();
        let mut log = Log::open(&dir.path().join(FILE_NAME)).unwrap();
        let start = log.burst_started;
        let events = (0..BURST_LIMIT + 5).map(|i| warn("ocr", format!("{}", i))).collect();
        assert_eq!(log.limit(events, start).len(), BURST_LIMIT as usize);
        assert!(log.limit(vec![warn("ocr", "more")], start).is_empty());

        let next = log.limit(vec![info("ocr", "later")], start + BURST_WINDOW);
        assert_eq!(next.len(), 2);
        assert_eq!(next[0].data, Some(json!({ "dropped": 6 })));
        assert_eq!(next[1].message, "later");
    }

    #[test]
    fn tallies_once_a_period() {
        let mut tally = Tally::new("capture");
        let start = tally.since;
        tally.count("saved", 2);
        tally.time("capture", Duration::from_millis(10));
        tally.time("capture", Duration::from_millis(30));
        assert!(tally.take_if_due(start + Duration::from_secs(1)).is_none());

        let event = tally.take_if_due(start + TALLY_PERIOD).unwrap();
        assert_eq!(event.source, "capture");
        assert_eq!(
            event.data,
            Some(json!({
                "counts": { "saved": 2 },
                "timings": { "capture": { "count": 2, "avgMs": 20.0, "maxMs": 30 } },
            }))
        );
        // Nothing since: nothing to say.
        assert!(tally.take_if_due(start + TALLY_PERIOD * 2).is_none());
    }
}
