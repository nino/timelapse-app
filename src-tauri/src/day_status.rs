//! Whether OCR and video conversion have finished each day, for the dots in
//! the day picker.
//!
//! Built from what is cheap to read: the library's directory listings (file
//! names only, no stats) and the `ocr_progress` table. Nothing is probed or
//! decoded, so it can be asked every time the picker opens.
//!
//! - **Video** is done once a day has no screenshots left: the converter
//!   deletes an hour's PNGs only after encoding it, and the old script's
//!   days never had any left.
//! - **OCR** is done once the day's progress mark covers its highest
//!   remaining screenshot. A day without screenshots is done if OCR read it
//!   from screenshots before they went (the converter waits for OCR), or, if
//!   it is read from video, once OCR has marked it complete
//!   (`ScreenshotDatabase::record_video_day_complete`).

use crate::database::{OcrProgress, ScreenshotDatabase};
use frame_source::parse_screenshot_name;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStatus {
    pub date: String,
    pub ocr_done: bool,
    pub video_done: bool,
}

/// The status of each of `days` (folder names, `YYYY-MM-DD`) in `root`, in
/// the same order. `db` is `None` before the library has a database, when
/// OCR has done nothing.
pub fn day_statuses(
    root: &Path,
    db: Option<&ScreenshotDatabase>,
    days: &[String],
) -> Result<Vec<DayStatus>, String> {
    let progress: HashMap<String, (OcrProgress, bool)> = match db {
        Some(db) => db
            .all_ocr_progress()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(day, progress, complete)| (day, (progress, complete)))
            .collect(),
        None => HashMap::new(),
    };

    days.iter()
        .map(|date| {
            let last_screenshot = highest_screenshot(&root.join(date)).map_err(|e| e.to_string())?;
            let ocr_done = match (last_screenshot, progress.get(date)) {
                (Some(last), Some((OcrProgress::FrameNumber(done), _))) => *done >= last,
                (None, Some((OcrProgress::FrameNumber(_), _))) => true,
                (None, Some((OcrProgress::VideoPosition(_), complete))) => *complete,
                _ => false,
            };
            Ok(DayStatus {
                date: date.clone(),
                ocr_done,
                video_done: last_screenshot.is_none(),
            })
        })
        .collect()
}

/// The highest `NNNNN.png` number in `day_dir`; `None` if it has none or
/// doesn't exist.
fn highest_screenshot(day_dir: &Path) -> std::io::Result<Option<u32>> {
    let entries = match std::fs::read_dir(day_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| parse_screenshot_name(entry.file_name().to_str()?))
        .max())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    fn status(date: &str, ocr_done: bool, video_done: bool) -> DayStatus {
        DayStatus {
            date: date.into(),
            ocr_done,
            video_done,
        }
    }

    #[test]
    fn test_day_statuses() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let db = ScreenshotDatabase::new(root.join("screenshots.db")).unwrap();

        // Old script's day, read from video to the end.
        touch(&root.join("2024-01-01--20-00-00.mov"));
        db.record_video_ocr_frame("2024-01-01", 500, None).unwrap();
        db.record_video_day_complete("2024-01-01").unwrap();
        // Old script's day, OCR part way through.
        touch(&root.join("2024-01-02--20-00-00.mov"));
        db.record_video_ocr_frame("2024-01-02", 10, None).unwrap();
        // Old script's day OCR hasn't reached.
        touch(&root.join("2024-01-03--20-00-00.mov"));
        // Converted hour by hour; the PNGs went after OCR.
        touch(&root.join("2026-10-01--09-00-00--hourly.mov"));
        db.record_ocr_frame("2026-10-01", 3600, None).unwrap();
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

        let days: Vec<String> = [
            "2024-01-01", "2024-01-02", "2024-01-03", "2026-10-01", "2026-10-02", "2026-10-03",
        ]
        .map(String::from)
        .into();
        assert_eq!(
            day_statuses(root, Some(&db), &days).unwrap(),
            vec![
                status("2024-01-01", true, true),
                status("2024-01-02", false, true),
                status("2024-01-03", false, true),
                status("2026-10-01", true, true),
                status("2026-10-02", true, false),
                status("2026-10-03", false, false),
            ]
        );
    }

    #[test]
    fn test_day_statuses_without_a_database() {
        let dir = TempDir::new().unwrap();
        touch(&dir.path().join("2026-10-03/00001.png"));

        assert_eq!(
            day_statuses(dir.path(), None, &["2026-10-03".into()]).unwrap(),
            vec![status("2026-10-03", false, false)]
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

        db.record_video_day_complete("2026-10-03").unwrap();

        assert_eq!(db.ocr_done_through("2026-10-03").unwrap(), Some(1));
        assert_eq!(
            day_statuses(root, Some(&db), &["2026-10-03".into()]).unwrap(),
            vec![status("2026-10-03", false, false)]
        );
    }
}
