//! The find bar's commands: where OCR read the searched text on a day's
//! scrubber, on which other days, and which lines of a frame hold it.

use crate::database::{self, OcrProgress};
use crate::{library_db, paths, run_blocking, FrameSourceState};
use frame_source::FrameSource;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use tauri::State;

/// Where one OCR match sits on a day's scrubber.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DayMatch {
    /// The OCR'd frame whose text matched.
    index: usize,
    /// One past the last frame that frame's text stands for (OCR skips frames
    /// that look the same as the last one it read).
    end_index: usize,
    /// The OCR'd frame's number, to ask `get_match_lines` for its lines.
    frame: u32,
}

/// Every place on `date`'s scrubber where OCR read text matching `query`, in
/// order. Matches whose frame the day no longer has are left out.
fn search_ocr_day_impl(
    root: &Path,
    source: &FrameSource,
    date: &str,
    query: &str,
) -> Result<Vec<DayMatch>, String> {
    let Some(db) = library_db(root)? else {
        return Ok(Vec::new());
    };
    let found = db.search_ocr_in_day(date, query).map_err(|e| e.to_string())?;
    if found.is_empty() {
        return Ok(Vec::new());
    }

    // For each match: its own frame, then where its run ends. That is the next
    // OCR'd frame (exclusive) or, if that one is gone, the frame just before
    // it (inclusive); for the day's last OCR'd frame, the last frame OCR
    // handled (inclusive).
    let numbers: Vec<u32> = found
        .iter()
        .flat_map(|m| match m.next_frame {
            Some(next) => [m.frame_number, next, next - 1],
            None => {
                let last = m.done_through.unwrap_or(m.frame_number);
                [m.frame_number, last, last]
            }
        })
        .collect();
    // A day OCR read from video (see `ocr::OcrWorker::run_video_pass`)
    // recorded 1-based positions in the day, so a position is an index plus
    // one. A position past the day's end means its videos changed since.
    let by_position = matches!(db.ocr_progress(date), Ok(Some(OcrProgress::VideoPosition(_))));
    let indices = if by_position {
        let count = source.day(date).map_err(|e| e.to_string())?.frame_count;
        numbers
            .iter()
            .map(|&position| (position as usize).checked_sub(1).filter(|&index| index < count))
            .collect()
    } else {
        source
            .indices_of_frames(date, &numbers)
            .map_err(|e| e.to_string())?
    };

    let mut matches: Vec<DayMatch> = found
        .iter()
        .zip(indices.chunks(3))
        .filter_map(|(m, found)| {
            let index = found[0]?;
            let end_index = match (m.next_frame, found[1], found[2]) {
                (Some(_), Some(next), _) => next,
                (_, _, Some(last)) | (None, Some(last), _) => last + 1,
                _ => index + 1,
            };
            Some(DayMatch {
                index,
                end_index: end_index.max(index + 1),
                frame: m.frame_number,
            })
        })
        .collect();
    matches.sort_by_key(|m| m.index);
    Ok(matches)
}

#[tauri::command]
pub(crate) async fn search_ocr_day(
    source: State<'_, FrameSourceState>,
    date: String,
    query: String,
) -> Result<Vec<DayMatch>, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    let source = Arc::clone(source.inner());
    run_blocking(move || search_ocr_day_impl(&timelapse_root, &source, &date, &query)).await
}

/// A line of recognized text, normalized to the frame's size with the origin
/// at the top-left corner, ready to draw over the image.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct LineBox {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// The lines of OCR'd frame `frame` of `date` holding a word of `query`.
fn match_lines_impl(root: &Path, date: &str, frame: u32, query: &str) -> Result<Vec<LineBox>, String> {
    let Some(db) = library_db(root)? else {
        return Ok(Vec::new());
    };
    let Some((text, boxes)) = db.ocr_lines(date, frame).map_err(|e| e.to_string())? else {
        return Ok(Vec::new());
    };
    let texts: Vec<&str> = text.split('\n').collect();
    // Line N of the text has box N; a row that lost its boxes has none.
    if texts.len() != boxes.len() {
        return Ok(Vec::new());
    }
    let matching = database::lines_matching(&texts, query).map_err(|e| e.to_string())?;
    // Vision's boxes have their origin at the bottom-left.
    Ok(matching
        .into_iter()
        .map(|i| boxes[i])
        .map(|[x, y, width, height]| LineBox {
            x,
            y: 1.0 - y - height,
            width,
            height,
        })
        .collect())
}

#[tauri::command]
pub(crate) async fn get_match_lines(date: String, frame: u32, query: String) -> Result<Vec<LineBox>, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    run_blocking(move || match_lines_impl(&timelapse_root, &date, frame, &query)).await
}

/// How many times the searched text came onto the screen on one day.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct DayCount {
    day: String,
    count: u32,
}

fn count_ocr_matches_impl(root: &Path, query: &str) -> Result<Vec<DayCount>, String> {
    let Some(db) = library_db(root)? else {
        return Ok(Vec::new());
    };
    let counts = db.count_ocr_matches(query).map_err(|e| e.to_string())?;
    Ok(counts
        .into_iter()
        .map(|(day, count)| DayCount { day, count })
        .collect())
}

/// Days whose OCR text matches `query`, newest first.
#[tauri::command]
pub(crate) async fn count_ocr_matches(query: String) -> Result<Vec<DayCount>, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    run_blocking(move || count_ocr_matches_impl(&timelapse_root, &query)).await
}

/// Changes whenever OCR has recorded something new; see
/// `ScreenshotDatabase::ocr_version`.
#[tauri::command]
pub(crate) async fn get_ocr_version() -> Result<String, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    run_blocking(move || match library_db(&timelapse_root)? {
        Some(db) => db.ocr_version().map_err(|e| e.to_string()),
        None => Ok(String::new()),
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::ScreenshotDatabase;
    use frame_source::Tools;
    use std::fs;
    use tempfile::TempDir;

    fn frame_source_in(root: &Path) -> (TempDir, FrameSource) {
        let cache = TempDir::new().unwrap();
        let source =
            FrameSource::new(root.to_path_buf(), cache.path().to_path_buf(), u64::MAX, Tools::new(paths::ffmpeg()))
                .unwrap();
        (cache, source)
    }

    #[test]
    fn test_search_ocr_day_places_matches_on_the_scrubber() {
        let library = TempDir::new().unwrap();
        let day_dir = library.path().join("2026-10-04");
        fs::create_dir(&day_dir).unwrap();
        // Frame 4 was removed by hand, so frames 1-7 are indices 0-5.
        for n in [1, 2, 3, 5, 6, 7] {
            fs::write(day_dir.join(format!("{n:05}.png")), b"png").unwrap();
        }
        let db = ScreenshotDatabase::new(library.path().join("screenshots.db")).unwrap();
        db.record_ocr_frame("2026-10-04", 2, Some(("cargo build", &[]))).unwrap();
        db.record_ocr_frame("2026-10-04", 3, None).unwrap();
        db.record_ocr_frame("2026-10-04", 4, Some(("cargo check", &[]))).unwrap();
        db.record_ocr_frame("2026-10-04", 5, Some(("bun test", &[]))).unwrap();
        db.record_ocr_frame("2026-10-04", 6, Some(("cargo test", &[]))).unwrap();
        db.record_ocr_frame("2026-10-04", 7, None).unwrap();
        let (_cache, source) = frame_source_in(library.path());

        let matches = search_ocr_day_impl(library.path(), &source, "2026-10-04", "cargo").unwrap();
        assert_eq!(
            matches,
            vec![
                // Its run would end at frame 4, which is gone, so it runs
                // through frame 3, the last one before it.
                DayMatch { index: 1, end_index: 3, frame: 2 },
                // Frame 4 is left out; frame 6 runs to the last frame OCR handled.
                DayMatch { index: 4, end_index: 6, frame: 6 },
            ]
        );
        assert!(search_ocr_day_impl(library.path(), &source, "2026-10-04", "")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_search_ocr_day_places_video_positions_on_the_scrubber() {
        let library = TempDir::new().unwrap();
        // A day that only exists as one of the old script's videos, 6 frames.
        let made = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i", "color=c=gray:s=32x32:r=15"])
            .args(["-frames:v", "6", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
            .arg(library.path().join("2024-12-20--23-00-00.mov"))
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !made {
            eprintln!("skipping: ffmpeg with libx264 is not available");
            return;
        }
        let db = ScreenshotDatabase::new(library.path().join("screenshots.db")).unwrap();
        db.record_video_ocr_frame("2024-12-20", 1, Some(("cargo build", &[]))).unwrap();
        db.record_video_ocr_frame("2024-12-20", 3, Some(("bun test", &[]))).unwrap();
        db.record_video_ocr_frame("2024-12-20", 5, Some(("cargo test", &[]))).unwrap();
        db.record_video_ocr_frame("2024-12-20", 6, None).unwrap();
        let (_cache, source) = frame_source_in(library.path());

        let matches = search_ocr_day_impl(library.path(), &source, "2024-12-20", "cargo").unwrap();
        assert_eq!(
            matches,
            vec![
                DayMatch { index: 0, end_index: 2, frame: 1 },
                // Runs to the last frame OCR handled, the day's last.
                DayMatch { index: 4, end_index: 6, frame: 5 },
            ]
        );
    }

    #[test]
    fn test_search_without_a_database_finds_nothing() {
        let library = TempDir::new().unwrap();
        let (_cache, source) = frame_source_in(library.path());
        assert!(search_ocr_day_impl(library.path(), &source, "2026-10-04", "cargo")
            .unwrap()
            .is_empty());
        assert!(count_ocr_matches_impl(library.path(), "cargo").unwrap().is_empty());
        assert!(match_lines_impl(library.path(), "2026-10-04", 1, "cargo").unwrap().is_empty());
        // Searching must not create the database either.
        assert!(!library.path().join("screenshots.db").exists());
    }

    #[test]
    fn test_match_lines_flips_vision_boxes() {
        let library = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(library.path().join("screenshots.db")).unwrap();
        let boxes = [[0.125, 0.5, 0.5, 0.25], [0.0, 0.0, 1.0, 0.1]];
        db.record_ocr_frame("2026-10-04", 2, Some(("$ Cargo build\nFinished", &boxes))).unwrap();

        let found = match_lines_impl(library.path(), "2026-10-04", 2, "cargo").unwrap();
        assert_eq!(found.len(), 1);
        // Stored to a 65535th, so compare to well under a pixel.
        let LineBox { x, y, width, height } = found[0];
        for (got, want) in [(x, 0.125), (y, 0.25), (width, 0.5), (height, 0.25)] {
            assert!((got - want).abs() < 1e-4, "{got} vs {want}");
        }
        assert!(match_lines_impl(library.path(), "2026-10-04", 3, "cargo").unwrap().is_empty());
    }

    #[test]
    fn test_count_ocr_matches_reads_the_library_database() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("screenshots.db")).unwrap();
        db.record_ocr_frame("2024-01-01", 4, Some(("cargo test", &[]))).unwrap();

        assert_eq!(
            count_ocr_matches_impl(temp_dir.path(), "cargo").unwrap(),
            vec![DayCount { day: "2024-01-01".to_string(), count: 1 }]
        );
    }
}
