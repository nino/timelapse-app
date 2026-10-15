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

use frame_source::{FrameRange, FrameSource};
use serde::Serialize;

use crate::database::{OcrProgress, ScreenshotDatabase};

/// The apps in front on one day, most-used first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayApps {
    pub apps: Vec<AppStretches>,
}

/// Where one app was in front.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppStretches {
    pub name: String,
    /// How many frames it was in front for.
    pub frames: usize,
    /// Day-wide frame indices, in order.
    pub ranges: Vec<FrameRange>,
}

/// Which app was in front for each frame of `date`. Empty when nothing on
/// that day says, as for days captured before either source existed.
pub fn day_apps(
    db: Option<&ScreenshotDatabase>,
    source: &FrameSource,
    date: &str,
) -> Result<DayApps, String> {
    let empty = DayApps { apps: Vec::new() };
    let Some(db) = db else {
        return Ok(empty);
    };
    let error = |e: rusqlite::Error| e.to_string();
    let front = db.front_windows_in_day(date).map_err(error)?;
    let menu = db.menu_apps_in_day(date).map_err(error)?;
    let has_guesses = menu.iter().any(|(_, app)| app.is_some());
    if front.is_empty() && !has_guesses {
        return Ok(empty);
    }
    let frame_count = source.day(date).map_err(|e| e.to_string())?.frame_count;
    if frame_count == 0 {
        return Ok(empty);
    }
    let progress = db.ocr_progress(date).map_err(error)?;
    let by_position = matches!(progress, Some(OcrProgress::VideoPosition(_)));
    let done_through = match progress {
        Some(OcrProgress::FrameNumber(n) | OcrProgress::VideoPosition(n)) => Some(n),
        None => None,
    };

    // Each OCR row runs to the next one (exclusive) or, if that one is gone,
    // the frame just before it (inclusive); the day's last row runs through
    // the last frame OCR handled. The same as `search_ocr_day_impl`.
    let menu_numbers: Vec<u32> = menu
        .iter()
        .enumerate()
        .flat_map(|(i, (n, _))| match menu.get(i + 1) {
            Some((next, _)) => [*n, *next, next - 1],
            None => {
                let last = done_through.unwrap_or(*n).max(*n);
                [*n, last, last]
            }
        })
        .collect();
    // Placed in one frame-source call where possible. A day OCR read from
    // video recorded 1-based positions, so a position is an index plus one.
    let mut numbers: Vec<u32> = front.iter().map(|(n, _)| *n).collect();
    if !by_position {
        numbers.extend(&menu_numbers);
    }
    let mut indices = if numbers.is_empty() {
        Vec::new()
    } else {
        source.indices_of_frames(date, &numbers).map_err(|e| e.to_string())?
    };
    let menu_indices: Vec<Option<usize>> = if by_position {
        menu_numbers.iter().map(|&p| (p as usize).checked_sub(1)).collect()
    } else {
        indices.split_off(front.len())
    };
    // The day may have grown between the two looks at it.
    let place = |index: &Option<usize>| index.filter(|&i| i < frame_count);

    let known = if has_guesses {
        db.front_app_names().map_err(error)?
    } else {
        Vec::new()
    };
    let mut names = Names::new(known);
    let mut app_of: Vec<Option<usize>> = vec![None; frame_count];

    // Menu bar guesses first, so the recorded windows overwrite them.
    for (i, ((_, app), found)) in menu.iter().zip(menu_indices.chunks(3)).enumerate() {
        let (Some(app), Some(start)) = (app, place(&found[0])) else {
            continue;
        };
        let has_next = i + 1 < menu.len();
        let end = match (place(&found[1]), place(&found[2])) {
            (Some(next), _) if has_next && next > start => next,
            (_, Some(last)) => last + 1,
            _ => start + 1,
        };
        let id = names.snapped(app);
        app_of[start..end.clamp(start + 1, frame_count)].fill(Some(id));
    }
    let windows = db.front_apps_of_day(date).map_err(error)?;
    for ((_, window), index) in front.iter().zip(&indices) {
        if let (Some(index), Some(app)) = (place(index), windows.get(window)) {
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
            apps[id].ranges.push(FrameRange { start, end: index });
        }
        start = index;
    }
    apps.retain(|app| app.frames > 0);
    apps.sort_by(|a, b| b.frames.cmp(&a.frames).then_with(|| a.name.cmp(&b.name)));
    Ok(DayApps { apps })
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

/// The recorded name `read` most likely is: itself; the one closest to it if
/// that is within a letter (two for longer names) and no other is as close;
/// or the only one it ends, as the menu bar shortens `Google Chrome` to
/// `Chrome`.
fn snap<'a>(read: &'a str, known: &'a [String]) -> Option<&'a str> {
    if known.iter().any(|name| name == read) {
        return Some(read);
    }
    let read_lower = read.to_lowercase();
    let mut ending = known
        .iter()
        .filter(|name| name.to_lowercase().ends_with(&format!(" {read_lower}")));
    if let (Some(name), None) = (ending.next(), ending.next()) {
        return Some(name);
    }
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
    fn a_guess_on_a_missing_frame_still_ends_the_run_before_it() {
        // Frame 3 was removed by hand: frames 1, 2, 4, 5 are indices 0-3.
        let (library, db) = library(&[1, 2, 4, 5]);
        ocr(&db, 1, "Safari");
        ocr(&db, 3, "Blender");
        ocr(&db, 4, "Mail");
        db.record_ocr_frame(DAY, 5, None).unwrap();
        let (_cache, source) = source_in(library.path());

        let apps = day_apps(Some(&db), &source, DAY).unwrap();
        assert_eq!(ranges(&apps, "Safari"), vec![(0, 2)]);
        assert_eq!(ranges(&apps, "Mail"), vec![(2, 4)]);
    }

    #[test]
    fn a_day_with_no_app_data_has_no_apps() {
        let (library, db) = library(&[1, 2]);
        capture(&db, 1, None);
        let (_cache, source) = source_in(library.path());
        let apps = day_apps(Some(&db), &source, DAY).unwrap();
        assert_eq!(apps, DayApps { apps: Vec::new() });
        assert_eq!(day_apps(None, &source, DAY).unwrap().apps, Vec::new());
    }

    #[test]
    fn snapping_takes_only_a_clear_near_miss() {
        let known: Vec<String> = ["Finder", "Toggl Track", "Mail", "Maps", "Google Chrome", "Microsoft Word", "Word Counter"]
            .map(String::from)
            .to_vec();
        assert_eq!(snap("Finderc", &known), Some("Finder"));
        assert_eq!(snap("finder", &known), Some("Finder"));
        assert_eq!(snap("Toggi Track", &known), Some("Toggl Track"));
        assert_eq!(snap("Blender", &known), None);
        assert_eq!(snap("Chrome", &known), Some("Google Chrome"));
        assert_eq!(snap("Word", &known), Some("Microsoft Word"));
        // As close to Mail as to Maps.
        assert_eq!(snap("Mais", &known), None);
    }
}
