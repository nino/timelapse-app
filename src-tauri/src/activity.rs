//! What the background work is doing right now, for the Activity window.
//!
//! The photographer, the video converter and OCR each report into one shared
//! `Activity` as they go, and the window reads a `Snapshot` of it. Reporting
//! only touches memory, so the window can ask as often as it likes without the
//! workers scanning the library or the database on its behalf.

use chrono::{DateTime, Local};
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a power-source reading is reused. Reading it runs `pmset`.
const POWER_READING_TTL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Whether the machine is on AC power. Conversion and OCR run only then.
    /// `None` until first asked.
    pub on_ac_power: Option<bool>,
    pub capture: CaptureStatus,
    pub conversion: ConversionStatus,
    pub ocr: OcrStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    /// Whether the photographer is running.
    pub running: bool,
    /// The most recently saved screenshot.
    pub last_frame: Option<FrameRef>,
    /// Screenshots saved since the app started.
    pub frames_saved: u64,
    /// When the last all-black screen was dropped (the screen was off or
    /// locked); capture then waits 10 s.
    pub last_black_at: Option<DateTime<Local>>,
    pub last_error: Option<Failure>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameRef {
    pub day: String,
    pub number: u32,
    pub at: DateTime<Local>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub at: DateTime<Local>,
    pub message: String,
}

/// What a background loop is doing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Not started yet (the app starts the workers a second after launch).
    #[default]
    Starting,
    /// Working through a batch or a pass.
    Working,
    /// Nothing to do; looks again at `next_check_at`.
    Idle,
    /// Done with a batch and waiting before the next one may start.
    Resting,
    /// Paused until the machine is on AC power.
    OnBattery,
    /// Not available on this platform (OCR outside macOS).
    Unavailable,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionStatus {
    pub state: State,
    /// The batch being encoded.
    pub current: Option<Encoding>,
    /// When the converter next wakes up to delete converted PNGs and look for
    /// an hour to convert.
    pub next_check_at: Option<DateTime<Local>>,
    /// Ended hours (or parts of one) that OCR has read and that wait for
    /// their turn, not counting the one being encoded. Counted when the
    /// converter last looked on AC power.
    pub ready: usize,
    /// Ended hours (or parts) that wait for OCR before they can be converted.
    pub waiting_for_ocr: usize,
    /// How the last batch went.
    pub last: Option<Outcome>,
    /// Videos published since the app started.
    pub videos_made: u64,
    /// Converted PNGs deleted since the app started.
    pub frames_deleted: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Encoding {
    pub video: String,
    pub day: String,
    pub hour: u32,
    pub frames: usize,
    pub started_at: DateTime<Local>,
    /// Frames ffmpeg has encoded so far, from its `-progress` output.
    pub frames_done: usize,
    /// When the encode was paused (unplugged), while it is.
    pub paused_since: Option<DateTime<Local>>,
    /// Seconds spent paused before `paused_since`.
    pub paused_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub video: String,
    pub finished_at: DateTime<Local>,
    pub took_secs: u64,
    /// `None` when the video was published; otherwise why not.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrStatus {
    pub state: State,
    /// The frame last handled in the current pass.
    pub current: Option<FrameRef>,
    /// Frames the current pass still has to handle. Frames captured since the
    /// pass started are picked up by the next one.
    pub remaining: usize,
    /// Frames run through text recognition since the app started.
    pub recognized: u64,
    /// Frames skipped since the app started because the screen had not
    /// changed.
    pub skipped: u64,
    /// When OCR next looks for new frames, while idle or on battery.
    pub next_check_at: Option<DateTime<Local>>,
    pub last_error: Option<Failure>,
    /// Whether the current pass reads days that only exist as video. Their
    /// `current.number` is a 1-based position in the day's videos.
    pub reading_video: bool,
    /// Days that only exist as video and still have frames to read, as of the
    /// last pass over them. `None` until OCR first gets to them.
    pub video_days_left: Option<usize>,
}

/// The shared report. Every method takes the lock briefly and never blocks on
/// anything else.
#[derive(Default)]
pub struct Activity {
    snapshot: Mutex<Snapshot>,
    power_read_at: Mutex<Option<Instant>>,
}

impl Activity {
    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            f(&mut snapshot);
        }
    }

    /// The current report, with the power source read through `read_power`
    /// unless a recent reading can be reused.
    pub fn snapshot(&self, capture_running: bool, read_power: impl FnOnce() -> bool) -> Snapshot {
        let stale = self
            .power_read_at
            .lock()
            .map(|at| at.map_or(true, |at| at.elapsed() >= POWER_READING_TTL))
            .unwrap_or(true);
        if stale {
            let on_ac = read_power();
            self.update(|s| s.on_ac_power = Some(on_ac));
            if let Ok(mut at) = self.power_read_at.lock() {
                *at = Some(Instant::now());
            }
        }

        let mut snapshot = self.snapshot.lock().map(|s| s.clone()).unwrap_or_default();
        snapshot.capture.running = capture_running;
        snapshot
    }

    // Capture.

    pub fn frame_saved(&self, day: &str, number: u32) {
        self.update(|s| {
            s.capture.frames_saved += 1;
            s.capture.last_frame = Some(FrameRef { day: day.to_string(), number, at: Local::now() });
        });
    }

    pub fn black_frame_dropped(&self) {
        self.update(|s| s.capture.last_black_at = Some(Local::now()));
    }

    pub fn capture_failed(&self, message: String) {
        self.update(|s| s.capture.last_error = Some(Failure { at: Local::now(), message }));
    }

    // Conversion.

    /// The converter counted the hours waiting to be converted.
    pub fn conversion_backlog(&self, ready: usize, waiting_for_ocr: usize) {
        self.update(|s| {
            s.conversion.ready = ready;
            s.conversion.waiting_for_ocr = waiting_for_ocr;
        });
    }

    pub fn frames_deleted(&self, count: usize) {
        self.update(|s| s.conversion.frames_deleted += count as u64);
    }

    pub fn encoding_started(&self, encoding: Encoding) {
        self.update(|s| {
            s.conversion.state = State::Working;
            s.conversion.current = Some(encoding);
            s.conversion.next_check_at = None;
        });
    }

    /// ffmpeg has encoded `frames_done` frames of the current batch.
    pub fn encoding_progress(&self, frames_done: usize) {
        self.update(|s| {
            if let Some(current) = s.conversion.current.as_mut() {
                current.frames_done = frames_done.min(current.frames);
            }
        });
    }

    /// The encode is paused until the machine is back on AC power.
    pub fn encoding_paused(&self) {
        self.update(|s| {
            if let Some(current) = s.conversion.current.as_mut() {
                current.paused_since.get_or_insert_with(Local::now);
                s.conversion.state = State::OnBattery;
            }
        });
    }

    /// The paused encode carries on.
    pub fn encoding_resumed(&self) {
        self.update(|s| {
            if let Some(current) = s.conversion.current.as_mut() {
                if let Some(since) = current.paused_since.take() {
                    current.paused_secs += (Local::now() - since).num_seconds().max(0) as u64;
                }
                s.conversion.state = State::Working;
            }
        });
    }

    pub fn encoding_finished(&self, video: String, took: Duration, error: Option<String>) {
        self.update(|s| {
            s.conversion.current = None;
            if error.is_none() {
                s.conversion.videos_made += 1;
            }
            s.conversion.last = Some(Outcome {
                video,
                finished_at: Local::now(),
                took_secs: took.as_secs(),
                error,
            });
        });
    }

    /// The converter goes to sleep for `wait`, in `state`.
    pub fn converter_sleeps(&self, state: State, wait: Duration) {
        self.update(|s| {
            s.conversion.state = state;
            s.conversion.next_check_at = Some(after(wait));
        });
    }

    // OCR.

    pub fn ocr_unavailable(&self) {
        self.update(|s| s.ocr.state = State::Unavailable);
    }

    pub fn ocr_pass_started(&self, frames: usize) {
        self.update(|s| {
            s.ocr.state = State::Working;
            s.ocr.remaining = frames;
            s.ocr.next_check_at = None;
            s.ocr.reading_video = false;
        });
    }

    /// A pass over days that only exist as video starts, with `days_left`
    /// of them still unread.
    pub fn ocr_video_pass_started(&self, days_left: usize) {
        self.update(|s| {
            s.ocr.state = State::Working;
            s.ocr.remaining = 0;
            s.ocr.next_check_at = None;
            s.ocr.reading_video = true;
            s.ocr.video_days_left = Some(days_left);
        });
    }

    pub fn ocr_frame_handled(&self, day: &str, number: u32, recognized: bool) {
        self.update(|s| {
            s.ocr.current = Some(FrameRef { day: day.to_string(), number, at: Local::now() });
            s.ocr.remaining = s.ocr.remaining.saturating_sub(1);
            if recognized {
                s.ocr.recognized += 1;
            } else {
                s.ocr.skipped += 1;
            }
        });
    }

    pub fn ocr_failed(&self, message: String) {
        self.update(|s| s.ocr.last_error = Some(Failure { at: Local::now(), message }));
    }

    /// OCR goes to sleep for `wait`, in `state`.
    pub fn ocr_sleeps(&self, state: State, wait: Duration) {
        self.update(|s| {
            s.ocr.state = state;
            s.ocr.next_check_at = Some(after(wait));
        });
    }
}

fn after(wait: Duration) -> DateTime<Local> {
    Local::now() + chrono::Duration::from_std(wait).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_a_recent_power_reading() {
        let activity = Activity::default();
        let mut reads = 0;
        activity.snapshot(true, || {
            reads += 1;
            true
        });
        let snapshot = activity.snapshot(true, || {
            reads += 1;
            false
        });
        assert_eq!(reads, 1);
        assert_eq!(snapshot.on_ac_power, Some(true));
        assert!(snapshot.capture.running);
    }

    #[test]
    fn tells_video_passes_from_screenshot_passes() {
        let activity = Activity::default();
        assert_eq!(activity.snapshot(false, || true).ocr.video_days_left, None);
        activity.ocr_video_pass_started(12);
        let ocr = activity.snapshot(false, || true).ocr;
        assert!(ocr.reading_video);
        assert_eq!(ocr.video_days_left, Some(12));
        activity.ocr_pass_started(3);
        let ocr = activity.snapshot(false, || true).ocr;
        assert!(!ocr.reading_video);
        // Still the last count, until the next video pass.
        assert_eq!(ocr.video_days_left, Some(12));
    }

    #[test]
    fn counts_ocr_progress_down() {
        let activity = Activity::default();
        activity.ocr_pass_started(3);
        activity.ocr_frame_handled("2024-01-01", 1, true);
        activity.ocr_frame_handled("2024-01-01", 2, false);
        let ocr = activity.snapshot(false, || true).ocr;
        assert_eq!(ocr.state, State::Working);
        assert_eq!(ocr.remaining, 1);
        assert_eq!((ocr.recognized, ocr.skipped), (1, 1));
        assert_eq!(ocr.current.map(|f| f.number), Some(2));
    }

    #[test]
    fn records_how_an_encode_went() {
        let activity = Activity::default();
        activity.encoding_started(Encoding {
            video: "v.mov".into(),
            day: "2024-01-01".into(),
            hour: 9,
            frames: 10,
            started_at: Local::now(),
            frames_done: 0,
            paused_since: None,
            paused_secs: 0,
        });
        assert_eq!(activity.snapshot(false, || true).conversion.state, State::Working);
        activity.encoding_progress(4);
        assert_eq!(activity.snapshot(false, || true).conversion.current.unwrap().frames_done, 4);

        activity.encoding_paused();
        let conversion = activity.snapshot(false, || false).conversion;
        assert_eq!(conversion.state, State::OnBattery);
        assert!(conversion.current.unwrap().paused_since.is_some());
        activity.encoding_resumed();
        let conversion = activity.snapshot(false, || true).conversion;
        assert_eq!(conversion.state, State::Working);
        assert!(conversion.current.unwrap().paused_since.is_none());

        activity.encoding_finished("v.mov".into(), Duration::from_secs(3), None);
        activity.converter_sleeps(State::Resting, Duration::from_secs(60));
        let conversion = activity.snapshot(false, || true).conversion;
        assert_eq!(conversion.state, State::Resting);
        assert!(conversion.current.is_none());
        assert_eq!(conversion.videos_made, 1);
        assert_eq!(conversion.last.unwrap().took_secs, 3);
        assert!(conversion.next_check_at.unwrap() > Local::now());
    }
}
