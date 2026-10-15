//! Which app was in front across one day, as stretches of the day's
//! scrubber, for the viewer's app filter.
//!
//! A frame's app comes from the window the capture loop recorded
//! (`screenshots.window_id`) and, for frames taken before that was recorded,
//! from the app OCR read in the menu bar (`ocr_frames.menu_app`, see
//! `menu_app`). The menu bar guess stands for every frame up to the day's
//! next OCR row, like the OCR text it was read from. OCR misspells names now
//! and then (`Finderc`), so a guess one or two letters away from a name the
//! capture loop has recorded is taken to be that name.

use std::collections::HashMap;

use frame_source::FrameSource;
use serde::Serialize;

use crate::database::{OcrProgress, ScreenshotDatabase};

/// The apps in front on one day, most-used first.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayApps {
    /// The day's frame count when the stretches were found.
    pub frame_count: usize,
    pub apps: Vec<AppStretches>,
}

/// Where one app was in front.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppStretches {
    pub name: String,
    /// How many frames it was in front for.
    pub frames: usize,
    /// Day-wide frame indices `start..end` (end exclusive), in order.
    pub ranges: Vec<Stretch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Stretch {
    pub start: usize,
    pub end: usize,
}

/// Which app was in front for each frame of `date`. Empty when nothing on
/// that day says, as for days captured before either source existed.
pub fn day_apps(
    db: Option<&ScreenshotDatabase>,
    source: &FrameSource,
    date: &str,
) -> Result<DayApps, String> {
    let frame_count = source.day(date).map_err(|e| e.to_string())?.frame_count;
    let empty = DayApps { frame_count, apps: Vec::new() };
    let Some(db) = db else {
        return Ok(empty);
    };
    if frame_count == 0 {
        return Ok(empty);
    }
    let error = |e: rusqlite::Error| e.to_string();
    let front = db.front_apps_in_day(date).map_err(error)?;
    let menu = db.menu_apps_in_day(date).map_err(error)?;
    if front.is_empty() && menu.iter().all(|(_, app)| app.is_none()) {
        return Ok(empty);
    }
    let progress = db.ocr_progress(date).map_err(error)?;

    // Every frame number to place on the scrubber, in one frame-source call:
    // the recorded frames, then the OCR rows, then where OCR has got to.
    let by_position = matches!(progress, Some(OcrProgress::VideoPosition(_)));
    let done_through = match progress {
        Some(OcrProgress::FrameNumber(n) | OcrProgress::VideoPosition(n)) => Some(n),
        None => None,
    };
    let mut numbers: Vec<u32> = front.iter().map(|(n, _)| *n).collect();
    let front_indices = place(source, date, &numbers)?;
    numbers = menu.iter().map(|(n, _)| *n).chain(done_through).collect();
    // A day OCR read from video recorded 1-based positions in the day, so a
    // position is an index plus one.
    let menu_indices = if by_position {
        numbers
            .iter()
            .map(|&p| (p as usize).checked_sub(1).filter(|&i| i < frame_count))
            .collect()
    } else {
        place(source, date, &numbers)?
    };

    let mut names = Names::new(db.front_app_names().map_err(error)?);
    let mut app_of: Vec<Option<usize>> = vec![None; frame_count];

    // Menu bar guesses first, so the recorded windows overwrite them.
    let mut runs: Vec<(usize, Option<&str>)> = menu
        .iter()
        .zip(&menu_indices)
        .filter_map(|((_, app), index)| Some(((*index)?, app.as_deref())))
        .collect();
    runs.sort_by_key(|(index, _)| *index);
    // The day's last row runs through the last frame OCR has handled.
    let last_end = done_through
        .and(menu_indices.get(menu.len()).copied().flatten())
        .map(|index| index + 1);
    for (i, &(start, app)) in runs.iter().enumerate() {
        let end = match runs.get(i + 1) {
            Some(&(next, _)) => next,
            None => last_end.unwrap_or(start + 1).max(start + 1),
        };
        if let Some(app) = app {
            let id = names.snapped(app);
            app_of[start..end.min(frame_count)].fill(Some(id));
        }
    }
    for ((_, app), index) in front.iter().zip(&front_indices) {
        if let Some(index) = *index {
            app_of[index] = Some(names.exact(app));
        }
    }

    let mut apps: Vec<AppStretches> = names
        .list
        .into_iter()
        .map(|name| AppStretches { name, frames: 0, ranges: Vec::new() })
        .collect();
    let mut start = 0;
    for index in 1..=frame_count {
        if index < frame_count && app_of[index] == app_of[start] {
            continue;
        }
        if let Some(id) = app_of[start] {
            apps[id].frames += index - start;
            apps[id].ranges.push(Stretch { start, end: index });
        }
        start = index;
    }
    apps.retain(|app| app.frames > 0);
    apps.sort_by(|a, b| b.frames.cmp(&a.frames).then_with(|| a.name.cmp(&b.name)));
    Ok(DayApps { frame_count, apps })
}

/// Scrubber indices of frames `numbers` of `date`.
fn place(source: &FrameSource, date: &str, numbers: &[u32]) -> Result<Vec<Option<usize>>, String> {
    if numbers.is_empty() {
        return Ok(Vec::new());
    }
    source.indices_of_frames(date, numbers).map_err(|e| e.to_string())
}

/// App names numbered in the order they are first seen, with OCR's
/// misspellings snapped to names the capture loop recorded.
struct Names {
    known: Vec<String>,
    list: Vec<String>,
    ids: HashMap<String, usize>,
    snaps: HashMap<String, String>,
}

impl Names {
    fn new(known: Vec<String>) -> Self {
        Self { known, list: Vec::new(), ids: HashMap::new(), snaps: HashMap::new() }
    }

    fn exact(&mut self, name: &str) -> usize {
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        self.list.push(name.to_string());
        self.ids.insert(name.to_string(), self.list.len() - 1);
        self.list.len() - 1
    }

    fn snapped(&mut self, read: &str) -> usize {
        if !self.snaps.contains_key(read) {
            let name = snap(read, &self.known).unwrap_or(read).to_string();
            self.snaps.insert(read.to_string(), name);
        }
        let name = self.snaps[read].clone();
        self.exact(&name)
    }
}

/// The recorded name `read` most likely is: itself, the same name in other
/// case, or the one closest to it if that is within a letter (two for
/// longer names) and no other is as close.
fn snap<'a>(read: &'a str, known: &'a [String]) -> Option<&'a str> {
    if known.iter().any(|name| name == read) {
        return Some(read);
    }
    let read_lower = read.to_lowercase();
    let mut best: Option<(usize, &str)> = None;
    let mut tied = false;
    for name in known {
        let distance = edit_distance(&read_lower, &name.to_lowercase());
        match best {
            Some((d, _)) if distance > d => {}
            Some((d, _)) if distance == d => tied = true,
            _ => {
                best = Some((distance, name));
                tied = false;
            }
        }
    }
    let (distance, name) = best?;
    let allowed = if read.chars().count() >= 8 { 2 } else { 1 };
    (distance <= allowed && !tied).then_some(name)
}

/// Levenshtein distance, by characters.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != cb)).min(above + 1).min(row[j] + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::FrontWindow;
    use chrono::{Local, Utc};
    use frame_source::Tools;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    const DAY: &str = "2026-10-04";

    fn library(frames: &[u32]) -> (TempDir, ScreenshotDatabase) {
        let library = TempDir::new().unwrap();
        let day_dir = library.path().join(DAY);
        fs::create_dir(&day_dir).unwrap();
        for n in frames {
            fs::write(day_dir.join(format!("{n:05}.png")), b"png").unwrap();
        }
        let db = ScreenshotDatabase::new(library.path().join("screenshots.db")).unwrap();
        (library, db)
    }

    fn source_in(root: &Path) -> (TempDir, FrameSource) {
        let cache = TempDir::new().unwrap();
        let source = FrameSource::new(
            root.to_path_buf(),
            cache.path().to_path_buf(),
            u64::MAX,
            Tools::new(crate::paths::ffmpeg()),
        )
        .unwrap();
        (cache, source)
    }

    fn capture(db: &ScreenshotDatabase, n: u32, app: Option<&str>) {
        let window = app.map(|app| FrontWindow {
            app_name: app.to_string(),
            bundle_id: None,
            app_path: format!("/Applications/{app}.app"),
            title: String::new(),
        });
        db.insert_capture(DAY, n, Utc::now(), Local::now(), window.as_ref()).unwrap();
    }

    /// An OCR row whose menu bar names `app`.
    fn ocr(db: &ScreenshotDatabase, n: u32, app: &str) {
        let text = format!("{app}\nFile Edit View");
        let boxes = [[0.05, 0.97, 0.05, 0.02], [0.11, 0.97, 0.2, 0.02]];
        db.record_ocr_frame(DAY, n, Some((&text, &boxes))).unwrap();
    }

    fn ranges(apps: &DayApps, name: &str) -> Vec<(usize, usize)> {
        apps.apps
            .iter()
            .find(|app| app.name == name)
            .map(|app| app.ranges.iter().map(|r| (r.start, r.end)).collect())
            .unwrap_or_default()
    }

    #[test]
    fn recorded_windows_give_each_frame_its_app() {
        // Frame 4 was removed by hand, so frames 1-7 are indices 0-5.
        let (library, db) = library(&[1, 2, 3, 5, 6, 7]);
        for (n, app) in [(1, "Blender"), (2, "Blender"), (3, "Safari"), (5, "Blender"), (6, "Blender"), (7, "Safari")] {
            capture(&db, n, Some(app));
        }
        let (_cache, source) = source_in(library.path());

        let apps = day_apps(Some(&db), &source, DAY).unwrap();
        assert_eq!(apps.frame_count, 6);
        assert_eq!(apps.apps[0].name, "Blender");
        assert_eq!(apps.apps[0].frames, 4);
        assert_eq!(ranges(&apps, "Blender"), vec![(0, 2), (3, 5)]);
        assert_eq!(ranges(&apps, "Safari"), vec![(2, 3), (5, 6)]);
    }

    #[test]
    fn menu_bar_fills_frames_without_a_recorded_window() {
        let (library, db) = library(&[1, 2, 3, 4, 5, 6]);
        // Frames 1-4 predate window recording; OCR read them, frame 3's menu
        // bar misspelt.
        for n in 1..=4 {
            capture(&db, n, None);
        }
        capture(&db, 5, Some("Finder"));
        capture(&db, 6, Some("Blender"));
        ocr(&db, 1, "Blender");
        ocr(&db, 3, "Finderc");
        db.record_ocr_frame(DAY, 4, None).unwrap();
        let (_cache, source) = source_in(library.path());

        let apps = day_apps(Some(&db), &source, DAY).unwrap();
        assert_eq!(ranges(&apps, "Blender"), vec![(0, 2), (5, 6)]);
        // Frame 4 looked the same as frame 3, so it is Finder too.
        assert_eq!(ranges(&apps, "Finder"), vec![(2, 5)]);
        assert!(ranges(&apps, "Finderc").is_empty());
    }

    #[test]
    fn a_day_with_no_app_data_has_no_apps() {
        let (library, db) = library(&[1, 2]);
        capture(&db, 1, None);
        let (_cache, source) = source_in(library.path());
        let apps = day_apps(Some(&db), &source, DAY).unwrap();
        assert_eq!(apps, DayApps { frame_count: 2, apps: Vec::new() });
        assert_eq!(day_apps(None, &source, DAY).unwrap().apps, Vec::new());
    }

    #[test]
    fn snapping_takes_only_a_clear_near_miss() {
        let known = vec!["Finder".to_string(), "Toggl Track".to_string(), "Mail".to_string(), "Maps".to_string()];
        assert_eq!(snap("Finderc", &known), Some("Finder"));
        assert_eq!(snap("finder", &known), Some("Finder"));
        assert_eq!(snap("Toggi Track", &known), Some("Toggl Track"));
        assert_eq!(snap("Blender", &known), None);
        // As close to Mail as to Maps.
        assert_eq!(snap("Mais", &known), None);
    }
}
