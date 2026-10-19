//! How far OCR and video conversion have got with each day, for the marks
//! in the day picker, and how much of each is left, for the Activity window.
//!
//! Built from what is cheap to read: the library's directory listings (file
//! names only, no stats) and the `ocr_progress` table. Nothing is probed or
//! decoded, so the picker can ask every few seconds while it is open.
//!
//! - **Video** is done once a day has no screenshots left: the converter
//!   deletes an hour's PNGs only after encoding it, and the old script's
//!   days never had any left. It is partial while a day has both screenshots
//!   and a video (some hours converted).
//! - **OCR** is done once the day's progress mark covers its highest
//!   remaining screenshot. A day without screenshots is done if the mark
//!   covers the frames its hourly videos were made from (`video_frames`),
//!   or, if it is read from video, once OCR has marked it complete
//!   (`ScreenshotDatabase::set_video_day_complete`). It is partial once the
//!   progress mark is past zero, or OCR has sampled some of a day read from
//!   video, but the day isn't done.
//!
//! A day with neither screenshots nor videos (an empty folder while the
//! screen is locked) gets no status, and nor does a day whose folder can't
//! be read. If the database can't be read, every day shows OCR as not done
//! rather than the picker showing nothing.

use crate::converter::is_day_folder_name;
use crate::database::{OcrProgress, ScreenshotDatabase};
use frame_source::{parse_screenshot_name, parse_video_name};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStatus {
    pub date: String,
    pub ocr: Stage,
    pub video: Stage,
}

/// How far one job has got with a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Todo,
    Partial,
    Done,
}

/// What OCR and video conversion have left to do across the library, for the
/// Activity window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemainingWork {
    pub ocr: Backlog,
    pub video: Backlog,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backlog {
    /// Days with work left.
    pub days: usize,
    /// Frames left in those days, except `uncounted_days`.
    pub frames: u64,
    /// Days among `days` whose frames aren't counted: days that only exist
    /// as video, before OCR's first pass over them has counted their frames.
    pub uncounted_days: usize,
}

/// Per day: the progress mark, whether a day read from video is complete,
/// and how far OCR's sampling pass has got through such a day.
type Progress = HashMap<String, (OcrProgress, bool, u32)>;

/// The status of every day in `root` that has frames, oldest first. `db` is
/// `None` before the library has a database, when OCR has done nothing.
pub fn day_statuses(root: &Path, db: Option<&ScreenshotDatabase>) -> std::io::Result<Vec<DayStatus>> {
    let (progress, days) = scan(root, db)?;
    Ok(days
        .into_iter()
        .map(|(date, folder)| DayStatus {
            ocr: ocr_stage(&date, &folder, &progress, db),
            video: video_stage(&folder),
            date,
        })
        .collect())
}

/// What is left to do, by the same rules as `day_statuses`.
///
/// - **Video**: every screenshot still in a day folder, today's included.
/// - **OCR**: screenshots above the day's progress mark; for a day whose
///   screenshots are gone, the frames its hourly videos were made from above
///   the mark (an index count); and for a day read from video,
///   `video_frames_left`, which OCR counts at the start of each pass over
///   them (`Activity::ocr_video_frames_left`). Probing those videos here
///   would mean ffmpeg reading each one, so a day OCR hasn't counted yet is
///   left as an uncounted day.
pub fn remaining_work(
    root: &Path,
    db: Option<&ScreenshotDatabase>,
    video_frames_left: Option<&HashMap<String, u64>>,
) -> std::io::Result<RemainingWork> {
    let (progress, days) = scan(root, db)?;
    let mut work = RemainingWork::default();
    for (date, folder) in &days {
        if !folder.screenshots.is_empty() {
            work.video.days += 1;
            work.video.frames += folder.screenshots.len() as u64;
        }
        if ocr_stage(date, folder, &progress, db) == Stage::Done {
            continue;
        }
        work.ocr.days += 1;
        let frames = match (folder.last_screenshot(), progress.get(date)) {
            (Some(_), Some((OcrProgress::FrameNumber(done), _, _))) => {
                Some(folder.screenshots.iter().filter(|&&n| n > *done).count() as u64)
            }
            (Some(_), _) => Some(folder.screenshots.len() as u64),
            (None, Some((OcrProgress::FrameNumber(done), _, _))) => {
                db.and_then(|db| db.video_frames_after(date, *done).ok())
            }
            (None, _) => video_frames_left.and_then(|left| left.get(date).copied()),
        };
        match frames {
            Some(frames) => work.ocr.frames += frames,
            None => work.ocr.uncounted_days += 1,
        }
    }
    Ok(work)
}

/// Every day in `root` that has frames, oldest first, with what its folder
/// holds, and every day's OCR progress.
fn scan(root: &Path, db: Option<&ScreenshotDatabase>) -> std::io::Result<(Progress, Vec<(String, DayFolder)>)> {
    let progress: Progress = match db.map(|db| db.all_ocr_progress()) {
        Some(Ok(rows)) => rows
            .into_iter()
            .map(|row| (row.day, (row.progress, row.complete, row.sampled_through)))
            .collect(),
        Some(Err(error)) => {
            eprintln!("Day statuses: cannot read OCR progress: {}", error);
            HashMap::new()
        }
        None => HashMap::new(),
    };

    // One listing of the root gives both the day folders and the days with
    // a video there.
    let mut folders = BTreeSet::new();
    let mut video_days = BTreeSet::new();
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((progress, Vec::new())),
        Err(e) => return Err(e),
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if is_day_folder_name(&name) {
            folders.insert(name);
        } else if let Some(day) = video_day(&name) {
            video_days.insert(day);
        }
    }

    let dates: BTreeSet<&String> = folders.iter().chain(&video_days).collect();
    let days = dates
        .into_iter()
        .filter_map(|date| {
            let mut folder = if folders.contains(date) {
                read_day_folder(&root.join(date), date).ok()?
            } else {
                DayFolder::default()
            };
            folder.has_videos |= video_days.contains(date);
            if folder.screenshots.is_empty() && !folder.has_videos {
                return None;
            }
            Some((date.clone(), folder))
        })
        .collect();
    Ok((progress, days))
}

/// How much of `date` OCR has read.
fn ocr_stage(date: &str, folder: &DayFolder, progress: &Progress, db: Option<&ScreenshotDatabase>) -> Stage {
    let Some((mark, complete, sampled_through)) = progress.get(date) else {
        return Stage::Todo;
    };
    let done = match (folder.last_screenshot(), mark) {
        (Some(last), OcrProgress::FrameNumber(done)) => *done >= last,
        (None, OcrProgress::FrameNumber(done)) => db
            .and_then(|db| db.last_video_frame(date).ok())
            .is_some_and(|last| last.is_some_and(|last| *done >= last)),
        (None, OcrProgress::VideoPosition(_)) => *complete,
        (Some(_), OcrProgress::VideoPosition(_)) => false,
    };
    let started = match mark {
        OcrProgress::FrameNumber(done) => *done > 0,
        OcrProgress::VideoPosition(done) => *done > 0 || *sampled_through > 0,
    };
    if done {
        Stage::Done
    } else if started {
        Stage::Partial
    } else {
        Stage::Todo
    }
}

/// How much of a day the converter has encoded.
fn video_stage(folder: &DayFolder) -> Stage {
    if folder.screenshots.is_empty() {
        Stage::Done
    } else if folder.has_videos {
        Stage::Partial
    } else {
        Stage::Todo
    }
}

/// The day of a video named `name`.
fn video_day(name: &str) -> Option<String> {
    let (start, _) = parse_video_name(name)?;
    Some(start.date().format("%Y-%m-%d").to_string())
}

#[derive(Default)]
struct DayFolder {
    /// The `NNNNN.png` numbers, in no particular order.
    screenshots: Vec<u32>,
    /// Whether the day has a video, in the folder or the root.
    has_videos: bool,
}

impl DayFolder {
    fn last_screenshot(&self) -> Option<u32> {
        self.screenshots.iter().copied().max()
    }
}

/// What day folder `day_dir` (of `date`) holds, from its file names alone.
fn read_day_folder(day_dir: &Path, date: &str) -> std::io::Result<DayFolder> {
    let mut folder = DayFolder::default();
    for entry in std::fs::read_dir(day_dir)?.filter_map(|entry| entry.ok()) {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if let Some(number) = parse_screenshot_name(&name) {
            folder.screenshots.push(number);
        } else if video_day(&name).as_deref() == Some(date) {
            folder.has_videos = true;
        }
    }
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use tempfile::TempDir;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    use Stage::{Done, Partial, Todo};

    fn status(date: &str, ocr: Stage, video: Stage) -> DayStatus {
        DayStatus { date: date.into(), ocr, video }
    }

    #[test]
    fn test_day_statuses() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let db = ScreenshotDatabase::new(root.join("screenshots.db")).unwrap();

        // Old script's day, read from video to the end.
        touch(&root.join("2024-01-01--20-00-00.mov"));
        db.record_video_ocr_frame("2024-01-01", 500, None).unwrap();
        db.set_video_day_complete("2024-01-01", true).unwrap();
        // Old script's day, OCR part way through.
        touch(&root.join("2024-01-02--20-00-00.mov"));
        db.record_video_ocr_frame("2024-01-02", 10, None).unwrap();
        // Old script's day OCR hasn't reached.
        touch(&root.join("2024-01-03--20-00-00.mov"));
        // Old script's day OCR has only sampled.
        touch(&root.join("2024-01-04--20-00-00.mov"));
        db.record_video_ocr_sample("2024-01-04", 40, None).unwrap();
        // Converted hour by hour; the PNGs went after OCR.
        touch(&root.join("2026-10-01--09-00-00--hourly.mov"));
        let frames = [(3599, Local::now()), (3600, Local::now())];
        db.record_video_frames("2026-10-01--09-00-00--hourly.mov", "2026-10-01", &frames).unwrap();
        db.record_ocr_frame("2026-10-01", 3600, None).unwrap();
        // Converted, but its progress was reset after Vision failed, so its
        // text is missing.
        touch(&root.join("2026-10-08--09-00-00--hourly.mov"));
        db.record_video_frames("2026-10-08--09-00-00--hourly.mov", "2026-10-08", &frames).unwrap();
        db.record_ocr_frame("2026-10-08", 0, None).unwrap();
        // One hour converted, the rest OCR'd but not converted yet.
        touch(&root.join("2026-10-02--09-00-00--hourly.mov"));
        touch(&root.join("2026-10-02/03601.png"));
        touch(&root.join("2026-10-02/03602.png"));
        db.record_ocr_frame("2026-10-02", 3602, None).unwrap();
        // Today: screenshots only, OCR behind.
        touch(&root.join("2026-10-03/00001.png"));
        touch(&root.join("2026-10-03/00002.png"));
        touch(&root.join("2026-10-03/.00003.png.tmp"));
        db.record_ocr_frame("2026-10-03", 1, None).unwrap();

        assert_eq!(
            day_statuses(root, Some(&db)).unwrap(),
            vec![
                status("2024-01-01", Done, Done),
                status("2024-01-02", Partial, Done),
                status("2024-01-03", Todo, Done),
                status("2024-01-04", Partial, Done),
                status("2026-10-01", Done, Done),
                status("2026-10-02", Done, Partial),
                status("2026-10-03", Partial, Todo),
                status("2026-10-08", Todo, Done),
            ]
        );
    }

    #[test]
    fn test_remaining_work() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let db = ScreenshotDatabase::new(root.join("screenshots.db")).unwrap();

        // Old script's days: one read to the end, one part way (OCR counted
        // its frames), one OCR hasn't counted yet.
        touch(&root.join("2024-01-01--20-00-00.mov"));
        db.record_video_ocr_frame("2024-01-01", 500, None).unwrap();
        db.set_video_day_complete("2024-01-01", true).unwrap();
        touch(&root.join("2024-01-02--20-00-00.mov"));
        db.record_video_ocr_frame("2024-01-02", 10, None).unwrap();
        touch(&root.join("2024-01-03--20-00-00.mov"));
        // Converted, but its progress was reset, so two frames are unread.
        touch(&root.join("2026-10-08--09-00-00--hourly.mov"));
        let frames = [(3599, Local::now()), (3600, Local::now())];
        db.record_video_frames("2026-10-08--09-00-00--hourly.mov", "2026-10-08", &frames).unwrap();
        db.record_ocr_frame("2026-10-08", 0, None).unwrap();
        // OCR'd, not converted yet.
        touch(&root.join("2026-10-02/03601.png"));
        touch(&root.join("2026-10-02/03602.png"));
        db.record_ocr_frame("2026-10-02", 3602, None).unwrap();
        // Today: OCR behind by two, and 3 screenshots to convert.
        touch(&root.join("2026-10-03/00001.png"));
        touch(&root.join("2026-10-03/00002.png"));
        touch(&root.join("2026-10-03/00004.png"));
        db.record_ocr_frame("2026-10-03", 1, None).unwrap();
        // A day OCR hasn't started.
        touch(&root.join("2026-10-04/00001.png"));

        let counted = HashMap::from([("2024-01-02".to_string(), 90)]);
        assert_eq!(
            remaining_work(root, Some(&db), Some(&counted)).unwrap(),
            RemainingWork {
                ocr: Backlog { days: 5, frames: 90 + 2 + 2 + 1, uncounted_days: 1 },
                video: Backlog { days: 3, frames: 6, uncounted_days: 0 },
            }
        );
        // Before OCR has counted any old video.
        assert_eq!(remaining_work(root, Some(&db), None).unwrap().ocr.uncounted_days, 2);
    }

    #[test]
    fn test_days_without_frames_have_no_status() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("2026-10-03")).unwrap();
        touch(&dir.path().join("2026-10-04/00001.png"));
        // A video in its day's folder, rather than the root, counts too,
        // but only for its own day.
        touch(&dir.path().join("2026-10-05/2026-10-05--09-00-00--hourly.mov"));
        touch(&dir.path().join("2026-10-06/2026-10-05--10-00-00--hourly.mov"));

        assert_eq!(
            day_statuses(dir.path(), None).unwrap(),
            vec![status("2026-10-04", Todo, Todo), status("2026-10-05", Todo, Done)]
        );
    }

    #[test]
    fn test_day_statuses_without_a_database() {
        let dir = TempDir::new().unwrap();
        touch(&dir.path().join("2026-10-03/00001.png"));

        assert_eq!(
            day_statuses(dir.path(), None).unwrap(),
            vec![status("2026-10-03", Todo, Todo)]
        );
    }

    #[test]
    fn test_marking_complete_leaves_screenshot_days_alone() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let db = ScreenshotDatabase::new(root.join("screenshots.db")).unwrap();
        touch(&root.join("2026-10-03/00001.png"));
        touch(&root.join("2026-10-03/00002.png"));
        db.record_ocr_frame("2026-10-03", 1, None).unwrap();

        db.set_video_day_complete("2026-10-03", true).unwrap();
        // Nor does it invent a row for a day OCR hasn't started.
        db.set_video_day_complete("2026-10-04", true).unwrap();

        assert_eq!(db.ocr_done_through("2026-10-03").unwrap(), Some(1));
        assert_eq!(db.ocr_progress("2026-10-04").unwrap(), None);
        assert_eq!(
            day_statuses(root, Some(&db)).unwrap(),
            vec![status("2026-10-03", Partial, Todo)]
        );
    }
}
