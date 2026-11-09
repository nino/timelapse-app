//! Once an hour, how far behind OCR and video conversion are: a "Backlog"
//! row in the diagnostics log, also sent to PostHog for anyone who sends
//! error reports (`analytics.rs`; counts and ages only, nothing from the
//! library).
//!
//! It holds what is left (`day_status::remaining_work`), how old the oldest
//! screenshot each job still has to do is, and what the workers could do at
//! the time (power source, boost, low-power mode). An hour is converted only
//! once it has ended and OCR has read it, so a video lag of up to about two
//! hours is normal; a lag that keeps growing from one row to the next means
//! the work isn't keeping up. OCR's lag leaves out days that only exist as
//! video, which nothing waits on; their frames are in `ocrFrames`.

use crate::activity::Activity;
use crate::boost::Boost;
use crate::day_status::{remaining_work_and_oldest, Oldest, RemainingWork};
use crate::database::ScreenshotDatabase;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// How often the backlog is logged.
const PERIOD: Duration = Duration::from_secs(60 * 60);

/// How long after launch the first row is logged: long enough for the
/// workers to have looked at the library, soon enough that a short session
/// still gets one.
const FIRST: Duration = Duration::from_secs(5 * 60);

/// Log the backlog of the library at `root` every `PERIOD`, for the life of
/// the app.
pub fn start(root: PathBuf, activity: Arc<Activity>, boost: Arc<Boost>) {
    let spawned = std::thread::Builder::new().name("backlog".into()).spawn(move || {
        std::thread::sleep(FIRST);
        loop {
            report(&root, &activity, &boost);
            std::thread::sleep(PERIOD);
        }
    });
    if let Err(error) = spawned {
        crate::diagnostics::warn("backlog", format!("Cannot start: {}", error)).record();
    }
}

fn report(root: &Path, activity: &Activity, boost: &Boost) {
    let db = ScreenshotDatabase::open_read_only(root.join("screenshots.db")).unwrap_or(None);
    let video_frames_left = activity.ocr_video_frames_left();
    let (work, oldest) = match remaining_work_and_oldest(root, db.as_ref(), video_frames_left.as_ref()) {
        Ok(found) => found,
        Err(error) => {
            crate::diagnostics::warn("backlog", format!("Cannot list the library: {}", error)).record();
            return;
        }
    };
    let snapshot = activity.snapshot(true, crate::converter::on_ac_power);
    let mut data = backlog_data(&work, &oldest, |day, number| lag_minutes(root, day, number, SystemTime::now()));
    if let Value::Object(fields) = &mut data {
        fields.insert("hoursReady".into(), json!(snapshot.conversion.ready));
        fields.insert("hoursWaitingForOcr".into(), json!(snapshot.conversion.waiting_for_ocr));
        fields.insert("onAcPower".into(), json!(snapshot.on_ac_power));
        fields.insert("boosted".into(), json!(boost.is_on()));
        fields.insert("lowPower".into(), json!(boost.is_low_power()));
    }
    crate::diagnostics::info("backlog", "Backlog").data(data.clone()).record();
    crate::analytics::capture("backlog", data);
}

/// The counts and lags, `lag` giving a screenshot's age in minutes.
fn backlog_data(work: &RemainingWork, oldest: &Oldest, lag: impl Fn(&str, u32) -> Option<u64>) -> Value {
    let lag_of = |frame: &Option<(String, u32)>| frame.as_ref().and_then(|(day, number)| lag(day, *number));
    json!({
        "ocrFrames": work.ocr.frames,
        "ocrDays": work.ocr.days,
        "ocrUncountedDays": work.ocr.uncounted_days,
        "ocrLagMinutes": lag_of(&oldest.ocr),
        "videoFrames": work.video.frames,
        "videoDays": work.video.days,
        "videoLagMinutes": lag_of(&oldest.video),
    })
}

/// How many minutes before `now` screenshot `number` of `day` was written,
/// or `None` if it is gone.
fn lag_minutes(root: &Path, day: &str, number: u32, now: SystemTime) -> Option<u64> {
    let modified = std::fs::metadata(root.join(day).join(format!("{:05}.png", number)))
        .and_then(|metadata| metadata.modified())
        .ok()?;
    Some(now.duration_since(modified).unwrap_or_default().as_secs() / 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::day_status::Backlog;
    use tempfile::TempDir;

    #[test]
    fn logs_what_is_left_and_how_old_the_oldest_frame_is() {
        let work = RemainingWork {
            ocr: Backlog { days: 2, frames: 120, uncounted_days: 1 },
            video: Backlog { days: 1, frames: 4000, uncounted_days: 0 },
        };
        let oldest = Oldest { ocr: Some(("2026-11-09".into(), 3500)), video: Some(("2026-11-09".into(), 1)) };
        let data = backlog_data(&work, &oldest, |_, number| Some(if number == 1 { 150 } else { 12 }));
        assert_eq!(
            data,
            json!({
                "ocrFrames": 120,
                "ocrDays": 2,
                "ocrUncountedDays": 1,
                "ocrLagMinutes": 12,
                "videoFrames": 4000,
                "videoDays": 1,
                "videoLagMinutes": 150,
            })
        );

        let caught_up = backlog_data(&RemainingWork::default(), &Oldest::default(), |_, _| panic!("nothing to age"));
        assert_eq!(caught_up["ocrLagMinutes"], Value::Null);
        assert_eq!(caught_up["videoLagMinutes"], Value::Null);
    }

    #[test]
    fn lag_is_the_age_of_the_screenshot_file() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("2026-11-09")).unwrap();
        std::fs::write(dir.path().join("2026-11-09/00042.png"), b"").unwrap();
        let later = SystemTime::now() + Duration::from_secs(90 * 60);
        assert_eq!(lag_minutes(dir.path(), "2026-11-09", 42, later), Some(90));
        assert_eq!(lag_minutes(dir.path(), "2026-11-09", 43, later), None);
    }
}
