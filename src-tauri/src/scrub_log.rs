//! How long scrubbing takes, step by step: one "Scrub" row in the
//! diagnostics log per stretch of scrubbing (or playback), also sent to
//! PostHog for anyone who sends error reports (`analytics.rs`; timings and
//! counts only, nothing from the library).
//!
//! The frame source reports each step of serving a frame (`plan`, `readPng`,
//! `videoFrame`, `dbTime`, see `FrameSource::time_steps`), the `frames:`
//! handler and `get_frame_time` their whole requests (`frameRequest`,
//! `frameTime`), and the page how long each frame took from asking for it to
//! having it on screen (`shown`), which includes the webview decoding it. The
//! beta video player, which asks the frame source for nothing, reports how
//! long each seek took to show its frame (`videoSeek`). A stretch ends once
//! nothing has been asked for in `IDLE`; stretches with fewer than
//! `MIN_FRAMES` frames are not logged.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long without a frame request ends a stretch of scrubbing.
const IDLE: Duration = Duration::from_secs(3);

/// Stretches with fewer frame requests than this (a click, an arrow key) are
/// not worth a row.
const MIN_FRAMES: usize = 5;

/// At most this many timings are kept per step and stretch, so a long
/// playback can't grow without bound.
const MAX_SAMPLES: usize = 20_000;

#[derive(Default)]
struct Stretch {
    started: Option<Instant>,
    last: Option<Instant>,
    steps: BTreeMap<&'static str, Vec<Duration>>,
    counts: BTreeMap<&'static str, u64>,
    /// Whether a thread is waiting to end this stretch.
    watched: bool,
}

fn stretch() -> &'static Mutex<Stretch> {
    static STRETCH: OnceLock<Mutex<Stretch>> = OnceLock::new();
    STRETCH.get_or_init(Mutex::default)
}

/// Record that step `name` took `took`.
pub fn step(name: &'static str, took: Duration) {
    let now = Instant::now();
    let mut stretch = stretch().lock().unwrap();
    let samples = stretch.steps.entry(name).or_default();
    if samples.len() < MAX_SAMPLES {
        samples.push(took);
    }
    stretch.started.get_or_insert(now);
    stretch.last = Some(now);
    if !stretch.watched {
        stretch.watched = true;
        let spawned = std::thread::Builder::new().name("scrub-log".into()).spawn(watch);
        if spawned.is_err() {
            stretch.watched = false;
        }
    }
}

/// Add `n` to counter `name` (frames by kind, and so on).
pub fn count(name: &'static str, n: u64) {
    *stretch().lock().unwrap().counts.entry(name).or_default() += n;
}

/// Wait for the stretch to go idle, then log it.
fn watch() {
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let mut stretch = stretch().lock().unwrap();
        if stretch.last.is_some_and(|last| last.elapsed() < IDLE) {
            continue;
        }
        let ended = std::mem::take(&mut *stretch);
        drop(stretch);
        if let Some(data) = summary(&ended) {
            let took = match (ended.started, ended.last) {
                (Some(started), Some(last)) => last.duration_since(started),
                _ => Duration::ZERO,
            };
            let mut data = data;
            if let Value::Object(fields) = &mut data {
                fields.insert("onAcPower".into(), json!(crate::converter::on_ac_power()));
            }
            crate::diagnostics::info("scrub", "Scrub").took(took).data(data.clone()).record();
            crate::analytics::capture("scrub", data);
        }
        return;
    }
}

/// The stretch as logged: per step its count and 50th/90th percentile and
/// longest time in milliseconds, plus the counters. `None` for a stretch too
/// short to log.
fn summary(stretch: &Stretch) -> Option<Value> {
    // Frames served by the frame source, or shown by the beta video player
    // seeking (which asks the frame source for nothing).
    let frames: usize = ["frameRequest", "videoSeek"]
        .iter()
        .map(|step| stretch.steps.get(step).map_or(0, Vec::len))
        .sum();
    if frames < MIN_FRAMES {
        return None;
    }
    let mut steps = Map::new();
    for (name, samples) in &stretch.steps {
        let mut ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
        ms.sort_by(f64::total_cmp);
        let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
        let round = |v: f64| (v * 10.0).round() / 10.0;
        steps.insert(
            (*name).into(),
            json!({
                "count": ms.len(),
                "p50Ms": round(at(0.5)),
                "p90Ms": round(at(0.9)),
                "maxMs": round(at(1.0)),
            }),
        );
    }
    Some(json!({ "steps": steps, "counts": stretch.counts }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn summarises_steps_as_percentiles_and_skips_short_stretches() {
        let mut stretch = Stretch::default();
        stretch.steps.insert("frameRequest", vec![ms(1); 4]);
        assert!(summary(&stretch).is_none());

        stretch.steps.insert("frameRequest", (1..=10).map(ms).collect());
        stretch.steps.insert("plan", vec![ms(3)]);
        stretch.counts.insert("pngFrames", 10);
        let data = summary(&stretch).unwrap();
        assert_eq!(
            data["steps"]["frameRequest"],
            json!({ "count": 10, "p50Ms": 6.0, "p90Ms": 9.0, "maxMs": 10.0 })
        );
        assert_eq!(data["steps"]["plan"]["p90Ms"], json!(3.0));
        assert_eq!(data["counts"]["pngFrames"], json!(10));
    }

    #[test]
    fn counts_video_player_seeks_as_frames() {
        let mut stretch = Stretch::default();
        stretch.steps.insert("videoSeek", vec![ms(30); 4]);
        assert!(summary(&stretch).is_none());
        stretch.steps.insert("videoSeek", vec![ms(30); 5]);
        assert_eq!(summary(&stretch).unwrap()["steps"]["videoSeek"]["p50Ms"], json!(30.0));
    }
}
