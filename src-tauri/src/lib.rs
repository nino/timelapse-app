mod activity;
mod app_state;
mod boost;
mod converter;
mod timelapse;
mod database;
mod diagnostics;
mod ocr;
mod ocr_tiles;
mod paths;
mod settings;
mod updater;

use frame_source::{DaySummary, FrameSource, FrameTime, PendingFrames, Tools};
use tauri::http::{header, Response, StatusCode};

use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem, WINDOW_SUBMENU_ID};
use tauri::{AppHandle, Emitter, Manager, Runtime, State, WebviewUrl, WebviewWindowBuilder};
use activity::{Activity, Snapshot};
use app_state::{AppStateStore, Rect, ViewerPosition};
use database::{OcrHit, OcrProgress, ScreenshotDatabase};
use serde::Serialize;
use settings::{Settings, SettingsStore};
use timelapse::Photographer;
use updater::UpdaterState;

// Shared state to manage the timelapse photographer
type PhotographerState = Arc<Mutex<Option<Photographer>>>;

/// Answers "frame N of day D" for the viewer. Managed once `setup` has
/// resolved the cache directory.
type FrameSourceState = Arc<FrameSource>;

/// What capture, conversion and OCR are doing, for the Activity window.
type ActivityState = Arc<Activity>;

/// Whether conversion and OCR are boosted (`boost.rs`).
type BoostState = Arc<boost::Boost>;

/// Label of the main window, set in `tauri.conf.json`.
const MAIN_WINDOW: &str = "main";
/// Label of the Activity window, and id of the menu item that opens it.
const ACTIVITY_WINDOW: &str = "activity";
/// Label of the Settings window, and id of the menu item that opens it.
const SETTINGS_WINDOW: &str = "settings";
/// Label of the About window, and id of the menu item that opens it.
const ABOUT_WINDOW: &str = "about";
/// Id of the "Check for Updates…" menu item.
const CHECK_FOR_UPDATES: &str = "check-for-updates";

/// Decoded video frames are disposable, so the cache lives in the OS cache
/// directory rather than in the (possibly synced) library.
const FRAME_CACHE_CAP_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Event sent with a day's date when read-ahead has decoded a chunk of it.
const FRAMES_DECODED: &str = "frames-decoded";

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// The directory under `$HOME` this build uses. Rust is the single source of
/// truth for it: the frontend asks at startup rather than deriving its own
/// answer, so the capture loop and the viewer can never disagree.
#[tauri::command]
fn get_timelapse_root_name() -> String {
    paths::TIMELAPSE_DIR_NAME.to_string()
}

// The state-backed commands below are thin shims over these functions.
// `tauri::State` is a newtype around a private reference with no public
// constructor, so it cannot be built outside a running Tauri app — keeping the
// logic in plain functions is what makes it reachable from unit tests.

fn start_timelapse_impl(
    state: &PhotographerState,
    make_photographer: impl FnOnce() -> Result<Photographer, String>,
) -> Result<String, String> {
    let mut photographer_guard = state.lock().map_err(|e| e.to_string())?;

    if photographer_guard.is_none() {
        let photographer = make_photographer()?;
        photographer.start();
        *photographer_guard = Some(photographer);
        Ok("Timelapse started successfully".to_string())
    } else {
        Err("Timelapse is already running".to_string())
    }
}

fn stop_timelapse_impl(state: &PhotographerState) -> Result<String, String> {
    let mut photographer_guard = state.lock().map_err(|e| e.to_string())?;

    if let Some(photographer) = photographer_guard.take() {
        photographer.stop();
        Ok("Timelapse stopped successfully".to_string())
    } else {
        Err("Timelapse is not running".to_string())
    }
}

fn is_timelapse_running_impl(state: &PhotographerState) -> Result<bool, String> {
    let photographer_guard = state.lock().map_err(|e| e.to_string())?;
    Ok(photographer_guard.is_some())
}

fn get_error_logs_impl(
    state: &PhotographerState,
) -> Result<Vec<timelapse::ErrorLogEntry>, String> {
    let photographer_guard = state.lock().map_err(|e| e.to_string())?;

    if let Some(photographer) = &*photographer_guard {
        Ok(photographer.get_error_logs())
    } else {
        Ok(Vec::new())
    }
}

fn clear_error_logs_impl(state: &PhotographerState) -> Result<String, String> {
    let photographer_guard = state.lock().map_err(|e| e.to_string())?;

    if let Some(photographer) = &*photographer_guard {
        photographer.clear_error_logs();
        Ok("Error logs cleared successfully".to_string())
    } else {
        Err("Timelapse is not running".to_string())
    }
}

fn get_screenshot_metadata_impl(
    state: &PhotographerState,
    frame_number: u32,
    day: Option<&str>,
) -> Result<Option<(String, String)>, String> {
    let photographer_guard = state.lock().map_err(|e| e.to_string())?;

    if let Some(photographer) = &*photographer_guard {
        photographer
            .get_screenshot_metadata(frame_number, day)
            .map_err(|e| e.to_string())
    } else {
        Err("Timelapse is not running".to_string())
    }
}

#[tauri::command]
async fn start_timelapse(
    state: State<'_, PhotographerState>,
    activity: State<'_, ActivityState>,
) -> Result<String, String> {
    let activity = Arc::clone(activity.inner());
    start_timelapse_impl(state.inner(), || {
        Photographer::new()
            .map(|photographer| photographer.reporting_to(activity))
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
async fn stop_timelapse(state: State<'_, PhotographerState>) -> Result<String, String> {
    stop_timelapse_impl(state.inner())
}

#[tauri::command]
async fn is_timelapse_running(state: State<'_, PhotographerState>) -> Result<bool, String> {
    is_timelapse_running_impl(state.inner())
}

#[tauri::command]
async fn get_error_logs(
    state: State<'_, PhotographerState>,
) -> Result<Vec<timelapse::ErrorLogEntry>, String> {
    get_error_logs_impl(state.inner())
}

#[tauri::command]
async fn clear_error_logs(state: State<'_, PhotographerState>) -> Result<String, String> {
    clear_error_logs_impl(state.inner())
}

/// Run blocking work (ffmpeg, SQLite) off the async runtime's worker threads.
async fn run_blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

/// Run a blocking frame-source call (it may shell out to ffmpeg) off the
/// async runtime's worker threads.
async fn with_frame_source<T: Send + 'static>(
    source: &FrameSourceState,
    f: impl FnOnce(&FrameSource) -> Result<T, frame_source::Error> + Send + 'static,
) -> Result<T, String> {
    let source = Arc::clone(source);
    run_blocking(move || f(&source).map_err(|e| e.to_string())).await
}

/// Every day with frames, oldest first.
#[tauri::command]
async fn list_days(source: State<'_, FrameSourceState>) -> Result<Vec<String>, String> {
    with_frame_source(source.inner(), |s| s.days()).await
}

#[tauri::command]
async fn get_day(source: State<'_, FrameSourceState>, date: String) -> Result<DaySummary, String> {
    with_frame_source(source.inner(), move |s| s.day(&date)).await
}

#[tauri::command]
async fn get_frame_time(
    source: State<'_, FrameSourceState>,
    date: String,
    index: usize,
) -> Result<Option<FrameTime>, String> {
    with_frame_source(source.inner(), move |s| s.frame_time(&date, index)).await
}

/// Frame ranges of `date` that still have to be decoded from video.
#[tauri::command]
async fn get_pending_frames(
    source: State<'_, FrameSourceState>,
    date: String,
) -> Result<PendingFrames, String> {
    with_frame_source(source.inner(), move |s| s.pending(&date)).await
}

/// The response for a `frames://localhost/<YYYY-MM-DD>/<index>` request.
fn frame_response(source: Option<&FrameSource>, path: &str) -> Response<Vec<u8>> {
    let reply = |status: StatusCode, message: String| {
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(message.into_bytes())
            .unwrap()
    };
    let Some(source) = source else {
        return reply(StatusCode::SERVICE_UNAVAILABLE, "frame source not ready".into());
    };
    let mut parts = path.trim_start_matches('/').split('/');
    let (Some(date), Some(index), None) = (parts.next(), parts.next(), parts.next()) else {
        return reply(StatusCode::BAD_REQUEST, format!("expected /<day>/<index>, got {path}"));
    };
    let Ok(index) = index.parse::<usize>() else {
        return reply(StatusCode::BAD_REQUEST, format!("not a frame index: {index}"));
    };
    match source.frame(date, index) {
        Ok(frame) => Response::builder()
            .header(header::CONTENT_TYPE, frame.mime)
            // The same URL can name a different file if a black frame is
            // deleted mid-day; the frame source's own cache is what makes
            // repeat requests cheap.
            .header(header::CACHE_CONTROL, "no-store")
            .body(frame.bytes)
            .unwrap(),
        Err(e @ (frame_source::Error::NotADay(_) | frame_source::Error::OutOfRange { .. })) => {
            reply(StatusCode::NOT_FOUND, e.to_string())
        }
        Err(e) => reply(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[tauri::command]
async fn get_screenshot_metadata(
    state: State<'_, PhotographerState>,
    frame_number: u32,
    day: Option<String>,
) -> Result<Option<(String, String)>, String> {
    get_screenshot_metadata_impl(state.inner(), frame_number, day.as_deref())
}

/// Search the OCR text of every screenshot in `<root>`, newest first.
fn search_ocr_impl(root: &Path, query: &str, limit: u32) -> Result<Vec<OcrHit>, String> {
    let db = ScreenshotDatabase::new(root.join("screenshots.db")).map_err(|e| e.to_string())?;
    db.search_ocr(query, limit).map_err(|e| e.to_string())
}

#[tauri::command]
async fn search_ocr(query: String, limit: Option<u32>) -> Result<Vec<OcrHit>, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    search_ocr_impl(&timelapse_root, &query, limit.unwrap_or(100))
}

/// Where one OCR match sits on a day's scrubber.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DayMatch {
    /// The OCR'd frame whose text matched.
    index: usize,
    /// One past the last frame that frame's text stands for (OCR skips frames
    /// that look the same as the last one it read).
    end_index: usize,
    /// The OCR'd frame's number, to ask `get_match_lines` for its lines.
    frame: u32,
}

/// The read-only library database in `root`, or `None` before it exists.
fn library_db(root: &Path) -> Result<Option<ScreenshotDatabase>, String> {
    ScreenshotDatabase::open_read_only(root.join("screenshots.db")).map_err(|e| e.to_string())
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
async fn search_ocr_day(
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
struct LineBox {
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
async fn get_match_lines(date: String, frame: u32, query: String) -> Result<Vec<LineBox>, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    run_blocking(move || match_lines_impl(&timelapse_root, &date, frame, &query)).await
}

/// How many times the searched text came onto the screen on one day.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct DayCount {
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
async fn count_ocr_matches(query: String) -> Result<Vec<DayCount>, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    run_blocking(move || count_ocr_matches_impl(&timelapse_root, &query)).await
}

/// Changes whenever OCR has recorded something new; see
/// `ScreenshotDatabase::ocr_version`.
#[tauri::command]
async fn get_ocr_version() -> Result<String, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    run_blocking(move || match library_db(&timelapse_root)? {
        Some(db) => db.ocr_version().map_err(|e| e.to_string()),
        None => Ok(String::new()),
    })
    .await
}

/// Delete every directory directly under `<root>/.cache` whose mtime is more
/// than 15 days old, and report how many went.
///
/// This is the only `remove_dir_all` in the app, so it takes the library root
/// as an argument: that is what lets the tests exercise it against a `TempDir`
/// instead of the real library.
fn evict_old_cache_impl(root: &Path) -> Result<String, String> {
    let cache_dir = root.join(".cache");

    if !cache_dir.exists() {
        return Ok("Cache directory does not exist".to_string());
    }

    let now = std::time::SystemTime::now();
    let fifteen_days = std::time::Duration::from_secs(15 * 24 * 60 * 60);

    let entries = std::fs::read_dir(&cache_dir)
        .map_err(|e| format!("Failed to read cache directory: {}", e))?;

    let mut removed_count = 0;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        // Get the modification time of the directory
        let metadata = std::fs::metadata(&path)
            .map_err(|e| format!("Failed to get metadata for {:?}: {}", path, e))?;

        let modified = metadata.modified()
            .map_err(|e| format!("Failed to get modified time for {:?}: {}", path, e))?;

        // Check if older than 15 days
        if let Ok(age) = now.duration_since(modified) {
            if age > fifteen_days {
                println!("Removing old cache folder: {:?} (age: {} days)", path, age.as_secs() / 86400);
                std::fs::remove_dir_all(&path)
                    .map_err(|e| format!("Failed to remove directory {:?}: {}", path, e))?;
                removed_count += 1;
            }
        }
    }

    Ok(format!("Removed {} old cache folders", removed_count))
}

#[tauri::command]
async fn evict_old_cache() -> Result<String, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    evict_old_cache_impl(&timelapse_root)
}

/// What capture, conversion and OCR are doing right now. It only reads
/// memory (and, every few seconds, the power source), so the Activity window
/// can poll it.
#[tauri::command]
async fn get_activity(
    activity: State<'_, ActivityState>,
    photographer: State<'_, PhotographerState>,
    boost: State<'_, BoostState>,
) -> Result<Snapshot, String> {
    let capturing = is_timelapse_running_impl(photographer.inner())?;
    let activity = Arc::clone(activity.inner());
    let low_power_until = boost.low_power_until();
    let boost = boost.status();
    run_blocking(move || {
        let mut snapshot = activity.snapshot(capturing, converter::on_ac_power);
        snapshot.boost = boost;
        snapshot.low_power_until = low_power_until;
        Ok(snapshot)
    })
    .await
}

/// Clear a "Last error" line of the Activity window: the error from `source`
/// that happened `at` (RFC 3339, as `get_activity` reported it). A newer
/// error from the same source stays.
#[tauri::command]
fn dismiss_error(
    activity: State<'_, ActivityState>,
    source: activity::ErrorSource,
    at: String,
) -> Result<(), String> {
    let at = chrono::DateTime::parse_from_rfc3339(&at).map_err(|e| e.to_string())?;
    activity.dismiss_error(source, at);
    Ok(())
}

/// Run conversion and OCR at full speed for `minutes`, on battery too if
/// `allow_battery`, replacing any boost in progress. Returns the new boost.
#[tauri::command]
fn start_boost(
    boost: State<'_, BoostState>,
    minutes: u64,
    allow_battery: bool,
) -> Result<Option<boost::BoostStatus>, String> {
    if minutes == 0 {
        return Err("A boost needs a length".to_string());
    }
    boost.start(std::time::Duration::from_secs(minutes.saturating_mul(60)), allow_battery);
    diagnostics::info("boost", "Boost started")
        .data(serde_json::json!({ "minutes": minutes, "allowBattery": allow_battery }))
        .record();
    Ok(boost.status())
}

/// End the boost in progress, if any.
#[tauri::command]
fn stop_boost(boost: State<'_, BoostState>) {
    boost.stop();
    diagnostics::info("boost", "Boost stopped").record();
}

/// Keep conversion and OCR off for `minutes`, even on AC power, replacing
/// any boost in progress. Returns when low-power mode ends.
#[tauri::command]
fn start_low_power(
    boost: State<'_, BoostState>,
    minutes: u64,
) -> Result<Option<chrono::DateTime<chrono::Local>>, String> {
    if minutes == 0 {
        return Err("Low-power mode needs a length".to_string());
    }
    boost.start_low_power(std::time::Duration::from_secs(minutes.saturating_mul(60)));
    diagnostics::info("boost", "Low-power mode started")
        .data(serde_json::json!({ "minutes": minutes }))
        .record();
    Ok(boost.low_power_until())
}

/// End low-power mode, if it is on.
#[tauri::command]
fn stop_low_power(boost: State<'_, BoostState>) {
    boost.stop_low_power();
    diagnostics::info("boost", "Low-power mode stopped").record();
}

/// Let the boost in progress run on battery, or not. Returns the boost.
#[tauri::command]
fn set_boost_allow_battery(boost: State<'_, BoostState>, allow_battery: bool) -> Option<boost::BoostStatus> {
    boost.set_allow_battery(allow_battery);
    diagnostics::info("boost", "Boost on battery changed")
        .data(serde_json::json!({ "allowBattery": allow_battery }))
        .record();
    boost.status()
}

/// Records an error the page caught (an uncaught exception or a rejected
/// promise) in the diagnostics log.
#[tauri::command]
fn log_frontend_error(window: tauri::Window, message: String, detail: Option<String>) {
    diagnostics::error("frontend", message)
        .data(serde_json::json!({ "window": window.label(), "detail": detail }))
        .record();
}

/// The app's settings. Changes apply at once and are saved straight away.
#[tauri::command]
fn get_settings(settings: State<'_, SettingsStore>) -> Settings {
    settings.get()
}

#[tauri::command]
fn set_update_automatically(
    enabled: bool,
    settings: State<'_, SettingsStore>,
    updater: State<'_, UpdaterState>,
) -> Result<Settings, String> {
    let saved = settings.update(|s| s.update_automatically = enabled);
    updater.setting_changed();
    saved
}

/// What the viewer showed when the app last quit.
#[tauri::command]
fn get_viewer_position(state: State<'_, AppStateStore>) -> ViewerPosition {
    state.get().viewer
}

/// Remembers what the viewer shows, to show it again after a relaunch. The
/// page calls this only once the viewer has settled, not on every frame of a
/// scrub.
#[tauri::command]
fn set_viewer_position(position: ViewerPosition, state: State<'_, AppStateStore>) {
    state.update(|s| s.viewer = position);
}

/// The usable area of each screen (without the menu bar and Dock), in
/// logical pixels, with the primary screen first.
fn screens<R: Runtime>(app: &AppHandle<R>) -> Vec<Rect> {
    let primary = app.primary_monitor().ok().flatten().map(|m| m.name().cloned());
    let mut monitors = app.available_monitors().unwrap_or_default();
    if let Some(primary) = primary {
        monitors.sort_by_key(|m| m.name() != primary.as_ref());
    }
    monitors
        .iter()
        .map(|m| {
            let scale = m.scale_factor();
            let area = m.work_area();
            Rect {
                x: area.position.x as f64 / scale,
                y: area.position.y as f64 / scale,
                width: area.size.width as f64 / scale,
                height: area.size.height as f64 / scale,
            }
        })
        .collect()
}

/// Where to open the window `label`: where it was last, moved onto a screen
/// if that place is out of reach now. `None` the first time.
fn saved_place<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<Rect> {
    let saved = app.try_state::<AppStateStore>()?.window(label)?;
    let rect = Rect {
        x: saved.x,
        y: saved.y,
        width: saved.width,
        height: saved.height,
    };
    Some(app_state::fit(rect, &screens(app)))
}

/// Records where `window` is and how big, and that it is open. A minimised
/// or full-screen window keeps the place it had before.
fn remember_place<R: Runtime>(window: &tauri::Window<R>) {
    let Some(state) = window.try_state::<AppStateStore>() else { return };
    let place = || -> tauri::Result<Option<Rect>> {
        if window.is_minimized()? || window.is_fullscreen()? {
            return Ok(None);
        }
        let scale = window.scale_factor()?;
        let position = window.outer_position()?.to_logical::<f64>(scale);
        let size = window.inner_size()?.to_logical::<f64>(scale);
        if size.width <= 0.0 || size.height <= 0.0 {
            return Ok(None);
        }
        Ok(Some(Rect {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }))
    };
    match place() {
        Ok(Some(rect)) => state.set_window(window.label(), rect),
        Ok(None) => {}
        Err(e) => eprintln!("Could not read where the {} window is: {}", window.label(), e),
    }
}

/// Keeps track of where the app's windows are and which are open.
fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if ![MAIN_WINDOW, ACTIVITY_WINDOW, SETTINGS_WINDOW, ABOUT_WINDOW].contains(&window.label()) {
        return;
    }
    match event {
        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => remember_place(window),
        // Quitting closes no window this way, so whatever is open when the
        // app quits stays marked open.
        tauri::WindowEvent::CloseRequested { .. } => {
            if let Some(state) = window.try_state::<AppStateStore>() {
                state.set_open(window.label(), false);
            }
        }
        _ => {}
    }
}

/// Opens the main window, from its entry in `tauri.conf.json` (which has
/// `create: false` so it can be placed before it appears).
fn open_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
        .cloned()
        .unwrap_or_default();
    let mut builder = WebviewWindowBuilder::from_config(app, &config)?;
    if let Some(place) = saved_place(app, MAIN_WINDOW) {
        builder = builder
            .position(place.x, place.y)
            .inner_size(place.width, place.height);
    }
    let window = builder.build()?;
    remember_place(&window.as_ref().window());
    Ok(())
}

/// Puts the windows back the way they were when the app quit. The Activity
/// and Settings windows open without taking focus, and before the main
/// window, so the main window ends up in front and focused, as on a first
/// launch.
fn restore_windows<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let saved = app.state::<AppStateStore>().get();
    let was_open = |label: &str| saved.windows.get(label).is_some_and(|w| w.open);
    if was_open(ACTIVITY_WINDOW) {
        if let Err(e) = show_activity_window(app, Opening::Restored) {
            eprintln!("Could not reopen the Activity window: {}", e);
        }
    }
    if was_open(SETTINGS_WINDOW) {
        if let Err(e) = show_settings_window(app, Opening::Restored) {
            eprintln!("Could not reopen the Settings window: {}", e);
        }
    }
    open_main_window(app)
}

/// Whether a window opens because someone asked for it, or because it was
/// open when the app last quit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Opening {
    Asked,
    Restored,
}

/// What has to happen before the app quits or relaunches, however it does:
/// ffmpeg is a separate process and would outlive the app, and the window
/// state is otherwise written a second after the last change.
pub(crate) fn before_quit<R: Runtime>(app: &AppHandle<R>) {
    converter::kill_running_encode();
    if let Some(state) = app.try_state::<AppStateStore>() {
        state.save_now();
    }
    diagnostics::info("app", "Quit").record();
    diagnostics::flush(std::time::Duration::from_secs(1));
}

/// The standard menu bar, with "Check for Updates…" and "Settings…" (⌘,)
/// under About in the app menu, and "Activity" added to the Window menu.
/// About opens the app's own About window, which has the change log, instead
/// of the standard About panel.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn app_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::default(app)?;
    // On macOS the first submenu is the app menu, which starts with About.
    if let Some(MenuItemKind::Submenu(app_submenu)) = menu.items()?.first() {
        app_submenu.remove_at(0)?;
        let about = format!("About {}", app.package_info().name);
        app_submenu.insert(&MenuItem::with_id(app, ABOUT_WINDOW, about, true, None::<&str>)?, 0)?;
        app_submenu.insert(
            &MenuItem::with_id(
                app,
                CHECK_FOR_UPDATES,
                "Check for Updates…",
                true,
                None::<&str>,
            )?,
            1,
        )?;
        app_submenu.insert(&PredefinedMenuItem::separator(app)?, 2)?;
        app_submenu.insert(
            &MenuItem::with_id(app, SETTINGS_WINDOW, "Settings…", true, Some("CmdOrCtrl+,"))?,
            3,
        )?;
    }
    if let Some(MenuItemKind::Submenu(window)) = menu.get(WINDOW_SUBMENU_ID) {
        window.append(&PredefinedMenuItem::separator(app)?)?;
        window.append(&MenuItem::with_id(app, ACTIVITY_WINDOW, "Activity", true, None::<&str>)?)?;
    }
    Ok(menu)
}

/// Bring the Settings window to the front, opening it if it isn't open. Like
/// a macOS settings window it has no Save button and doesn't resize.
fn show_settings_window<R: Runtime>(app: &AppHandle<R>, opening: Opening) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    // It opens where it was last, at the height it had then, which is the
    // height its content needed then. Only the width is fixed.
    let place = saved_place(app, SETTINGS_WINDOW);
    let restored = opening == Opening::Restored;
    // Asked for, it opens hidden, and `grow_settings_window` shows it once
    // it is sized to its content, so it never visibly jumps. Shown anyway
    // after a second in case the page never gets that far. Showing a window
    // focuses it, so one reopened at launch is visible from the start
    // instead, without focus.
    let mut builder =
        WebviewWindowBuilder::new(app, SETTINGS_WINDOW, WebviewUrl::App("index.html".into()))
            .title("Settings")
            .inner_size(SETTINGS_WIDTH, place.map_or(SETTINGS_HEIGHT, |p| p.height))
            .resizable(false)
            .minimizable(false)
            .maximizable(false)
            .visible(restored)
            .focused(!restored);
    if let Some(place) = place {
        builder = builder.position(place.x, place.y);
    }
    let window = builder.build()?;
    remember_place(&window.as_ref().window());
    if !restored {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let _ = window.show();
        });
    }
    Ok(())
}

/// Width of the Settings window, in logical pixels.
const SETTINGS_WIDTH: f64 = 440.0;
/// Its height the first time it opens, before it is fitted to its content.
const SETTINGS_HEIGHT: f64 = 140.0;

/// Makes the Settings window `by` logical pixels taller (shorter when
/// negative) and shows it. The page asks for the difference between its
/// content and its viewport rather than for a height, because what
/// `set_size` sets on macOS is not the viewport's height (on Nino's Mac a
/// 170pt window showed about 138pt of page), so only a change is reliable.
/// The top-left corner stays where it is, so a window reopened where it was
/// last stays there.
#[tauri::command]
fn grow_settings_window(by: f64, window: tauri::WebviewWindow) -> Result<(), String> {
    let resize = || -> tauri::Result<()> {
        if by.abs() >= 0.5 {
            let corner = window.outer_position()?;
            let size = window.inner_size()?.to_logical::<f64>(window.scale_factor()?);
            window.set_size(tauri::LogicalSize::new(size.width, (size.height + by).round()))?;
            window.set_position(corner)?;
        }
        // Already visible means it was reopened at launch, without focus;
        // showing it again would take focus from the main window.
        if !window.is_visible()? {
            window.show()?;
        }
        Ok(())
    };
    resize().map_err(|e| e.to_string())
}

/// Bring the Activity window to the front, opening it if it isn't open. It
/// loads the same page as the main window, which picks the view by the
/// window's label.
fn show_activity_window<R: Runtime>(app: &AppHandle<R>, opening: Opening) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(ACTIVITY_WINDOW) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    let mut builder =
        WebviewWindowBuilder::new(app, ACTIVITY_WINDOW, WebviewUrl::App("index.html".into()))
            .title("Activity")
            .inner_size(460.0, 700.0)
            .min_inner_size(360.0, 360.0)
            .focused(opening == Opening::Asked);
    if let Some(place) = saved_place(app, ACTIVITY_WINDOW) {
        builder = builder
            .position(place.x, place.y)
            .inner_size(place.width, place.height);
    }
    let window = builder.build()?;
    remember_place(&window.as_ref().window());
    Ok(())
}

/// Bring the About window to the front, opening it if it isn't open. It opens
/// where it was last, but unlike the Activity and Settings windows it isn't
/// reopened at launch.
fn show_about_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(ABOUT_WINDOW) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    let mut builder =
        WebviewWindowBuilder::new(app, ABOUT_WINDOW, WebviewUrl::App("index.html".into()))
            .title(format!("About {}", app.package_info().name))
            .inner_size(420.0, 560.0)
            .min_inner_size(320.0, 320.0)
            .minimizable(false)
            .maximizable(false);
    if let Some(place) = saved_place(app, ABOUT_WINDOW) {
        builder = builder
            .position(place.x, place.y)
            .inner_size(place.width, place.height);
    }
    let window = builder.build()?;
    remember_place(&window.as_ref().window());
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let photographer_state: PhotographerState = Arc::new(Mutex::new(None));
    let activity: ActivityState = Arc::default();

    let builder = tauri::Builder::default();
    // Elsewhere a menu would add a menu bar to the main window.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(app_menu);

    builder
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(photographer_state)
        .manage(activity)
        .manage(BoostState::default())
        .manage(UpdaterState::default())
        .on_menu_event(|app, event| {
            let opened = match event.id().as_ref() {
                ACTIVITY_WINDOW => show_activity_window(app, Opening::Asked),
                SETTINGS_WINDOW => show_settings_window(app, Opening::Asked),
                ABOUT_WINDOW => show_about_window(app),
                CHECK_FOR_UPDATES => {
                    updater::check_from_menu(app.clone());
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(e) = opened {
                eprintln!("Could not open the {} window: {}", event.id().as_ref(), e);
            }
        })
        .on_window_event(on_window_event)
        .register_asynchronous_uri_scheme_protocol("frames", |ctx, request, responder| {
            let source = ctx
                .app_handle()
                .try_state::<FrameSourceState>()
                .map(|state| Arc::clone(state.inner()));
            let path = request.uri().path().to_owned();
            // Decoding a chunk of video can take a moment; keep it off the
            // webview's thread.
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(frame_response(source.as_deref(), &path));
            });
        })
        .setup(|app| {
            app.manage(SettingsStore::load(app.path().app_config_dir().ok()));
            // Per profile, like the library: the viewer position names a day
            // in one library, and a dev build shouldn't move the release
            // build's windows.
            app.manage(AppStateStore::load(
                app.path()
                    .app_config_dir()
                    .ok()
                    .map(|dir| dir.join(paths::TIMELAPSE_DIR_NAME)),
            ));
            restore_windows(app.handle())?;
            updater::spawn(app.handle().clone());

            // Create the library up front. The frontend calls readDir on it
            // during its first render, which happens well before the delayed
            // task below builds the Photographer — without this, a first run
            // against a fresh library (every machine, the first time it uses
            // the dev root) dead-ends on the "Error loading folders" screen.
            match paths::timelapse_root() {
                Some(root) => {
                    if let Err(e) = std::fs::create_dir_all(&root) {
                        eprintln!("Failed to create {:?}: {}", root, e);
                    }
                    match diagnostics::init(&root) {
                        Ok(()) => {
                            diagnostics::record_panics();
                            diagnostics::info("app", "Started")
                                .data(serde_json::json!({
                                    "version": app.package_info().version.to_string(),
                                    "debug": cfg!(debug_assertions),
                                    "os": std::env::consts::OS,
                                    "arch": std::env::consts::ARCH,
                                }))
                                .record();
                        }
                        Err(e) => eprintln!("Could not open the diagnostics log: {}", e),
                    }
                    // Per-profile, like the library, so a dev build never
                    // serves frames cached from the real one. A failure here
                    // only breaks viewing; capture below still starts.
                    let source = app
                        .path()
                        .app_cache_dir()
                        .map_err(|e| e.to_string())
                        .and_then(|dir| {
                            let dir = dir.join(paths::TIMELAPSE_DIR_NAME).join("frames");
                            FrameSource::new(root, dir, FRAME_CACHE_CAP_BYTES, Tools::new(paths::ffmpeg()))
                                .map_err(|e| e.to_string())
                        })
                        .map(|source| {
                            // Tell the viewer, so the scrubber stops drawing
                            // the read-ahead stretch as not decoded yet.
                            let app = app.handle().clone();
                            source.read_ahead(move |date| {
                                if let Err(e) = app.emit(FRAMES_DECODED, date) {
                                    eprintln!("Could not report decoded frames: {}", e);
                                }
                            })
                        });
                    match source {
                        Ok(source) => {
                            app.manage::<FrameSourceState>(Arc::new(source));
                        }
                        Err(e) => {
                            eprintln!("Failed to set up the frame source: {}", e);
                            diagnostics::error("app", format!("Failed to set up the frame source: {}", e)).record();
                        }
                    }
                }
                None => eprintln!("Unable to find home directory"),
            }

            // Scratch space for OCR's decoded video frames, per profile like
            // the frame cache.
            let ocr_dir = app
                .path()
                .app_cache_dir()
                .ok()
                .map(|dir| dir.join(paths::TIMELAPSE_DIR_NAME).join("ocr"));

            // Start timelapse automatically when app is ready
            let photographer_state = app.state::<PhotographerState>();
            let state_clone = Arc::clone(&photographer_state.inner());
            let activity = Arc::clone(app.state::<ActivityState>().inner());
            let boost = Arc::clone(app.state::<BoostState>().inner());

            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                // Evict old cache entries on startup
                match evict_old_cache().await {
                    Ok(msg) => println!("Cache eviction: {}", msg),
                    Err(e) => eprintln!("Failed to evict old cache: {}", e),
                }

                match Photographer::new() {
                    Ok(photographer) => {
                        let photographer = photographer.reporting_to(Arc::clone(&activity));
                        photographer.start();
                        let mut guard = state_clone.lock().unwrap();
                        *guard = Some(photographer);
                        println!("Timelapse started automatically on app startup");
                    }
                    Err(e) => {
                        eprintln!("Failed to start timelapse automatically: {}", e);
                        diagnostics::error("capture", format!("Failed to start: {}", e)).record();
                    }
                }

                // Turn finished hours of screenshots into videos while on AC
                // power, deleting an hour's PNGs only once OCR has read them.
                // Both run for the life of the app, independently of the
                // photographer, so they have no state or commands of their
                // own yet.
                match paths::timelapse_root() {
                    Some(root) => {
                        converter::Converter::with_ocr_check(
                            root.clone(),
                            ocr::ocr_check(&root),
                        )
                        .reporting_to(Arc::clone(&activity))
                        .boosted_by(Arc::clone(&boost))
                        .start();

                        if ocr::start_background_ocr(root, ocr_dir, activity, boost) {
                            println!("OCR started");
                        } else {
                            println!("OCR is not available on this platform");
                        }
                    }
                    None => eprintln!("Unable to find home directory; video conversion and OCR are off"),
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_timelapse_root_name,
            start_timelapse,
            stop_timelapse,
            is_timelapse_running,
            get_error_logs,
            clear_error_logs,
            evict_old_cache,
            get_screenshot_metadata,
            list_days,
            get_day,
            get_frame_time,
            get_pending_frames,
            search_ocr,
            search_ocr_day,
            get_match_lines,
            count_ocr_matches,
            get_ocr_version,
            get_activity,
            dismiss_error,
            start_boost,
            stop_boost,
            start_low_power,
            stop_low_power,
            set_boost_allow_battery,
            log_frontend_error,
            get_settings,
            set_update_automatically,
            get_viewer_position,
            set_viewer_position,
            grow_settings_window
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                before_quit(app);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File, FileTimes};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    /// A photographer rooted in a temp dir, so nothing here reads or writes the
    /// real `~/Timelapse`. The `TempDir` is returned so the caller keeps it
    /// alive for the duration of the test.
    fn temp_photographer() -> (TempDir, Photographer) {
        let temp_dir = TempDir::new().unwrap();
        let photographer = Photographer::new_in(temp_dir.path().to_path_buf()).unwrap();
        (temp_dir, photographer)
    }

    /// State holding an already-registered (but never started) photographer.
    fn running_state() -> (TempDir, PhotographerState) {
        let (temp_dir, photographer) = temp_photographer();
        (temp_dir, Arc::new(Mutex::new(Some(photographer))))
    }

    fn idle_state() -> PhotographerState {
        Arc::new(Mutex::new(None))
    }

    #[test]
    fn test_greet() {
        let result = greet("Alice");
        assert_eq!(result, "Hello, Alice! You've been greeted from Rust!");

        let result = greet("Bob");
        assert_eq!(result, "Hello, Bob! You've been greeted from Rust!");
    }

    // `start` calls `tokio::spawn`, which panics outside a runtime. What keeps
    // the spawned capture loop from taking a screenshot is the
    // `stop_timelapse_impl` below: it clears `running` before this test reaches
    // any yield point, so the loop exits the first time it is polled. Do not
    // add an `.await` between the start and the stop.
    #[tokio::test]
    async fn test_start_timelapse_success() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().to_path_buf();
        let state = idle_state();

        let result = start_timelapse_impl(&state, || {
            Photographer::new_in(root).map_err(|e| e.to_string())
        });

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Timelapse started successfully");

        // Verify photographer was created
        assert!(state.lock().unwrap().is_some());

        stop_timelapse_impl(&state).unwrap();
    }

    #[test]
    fn test_start_timelapse_already_running() {
        let (_temp_dir, state) = running_state();

        // The factory must not run when one is already registered.
        let result = start_timelapse_impl(&state, || {
            panic!("should not build a second photographer");
        });

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Timelapse is already running");
    }

    #[test]
    fn test_stop_timelapse_success() {
        let (_temp_dir, state) = running_state();

        let result = stop_timelapse_impl(&state);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Timelapse stopped successfully");

        // Verify photographer was removed
        assert!(state.lock().unwrap().is_none());
    }

    #[test]
    fn test_stop_timelapse_not_running() {
        let state = idle_state();

        let result = stop_timelapse_impl(&state);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Timelapse is not running");
    }

    #[test]
    fn test_is_timelapse_running() {
        let state = idle_state();

        // Initially not running
        assert!(!is_timelapse_running_impl(&state).unwrap());

        // Register a photographer
        let (_temp_dir, photographer) = temp_photographer();
        *state.lock().unwrap() = Some(photographer);
        assert!(is_timelapse_running_impl(&state).unwrap());

        // Stop it again
        stop_timelapse_impl(&state).unwrap();
        assert!(!is_timelapse_running_impl(&state).unwrap());
    }

    #[test]
    fn test_get_error_logs_when_running() {
        let (_temp_dir, state) = running_state();

        let result = get_error_logs_impl(&state);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_get_error_logs_when_not_running() {
        let state = idle_state();

        let result = get_error_logs_impl(&state);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_clear_error_logs_success() {
        let (_temp_dir, state) = running_state();

        let result = clear_error_logs_impl(&state);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Error logs cleared successfully");
    }

    #[test]
    fn test_clear_error_logs_not_running() {
        let state = idle_state();

        let result = clear_error_logs_impl(&state);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Timelapse is not running");
    }

    #[test]
    fn test_get_screenshot_metadata_not_running() {
        let state = idle_state();

        let result = get_screenshot_metadata_impl(&state, 1, None);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Timelapse is not running");
    }

    #[test]
    fn test_get_screenshot_metadata_unknown_frame() {
        let (_temp_dir, state) = running_state();

        // Nothing has been captured into this temp library yet.
        let result = get_screenshot_metadata_impl(&state, 1, None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    /// A temp library root with an empty `.cache` inside it. The `TempDir` is
    /// returned so the caller keeps it alive for the duration of the test.
    fn temp_library() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join(".cache");
        fs::create_dir_all(&cache_dir).unwrap();
        (temp_dir, cache_dir)
    }

    /// A cache folder holding one file, aged to `mtime_age`.
    ///
    /// Order matters: writing the file bumps the directory's mtime, so the
    /// backdating has to come last.
    fn cache_folder(cache_dir: &Path, name: &str, mtime_age: Duration) -> PathBuf {
        let path = cache_dir.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("frame000001.jpg"), b"not really a jpeg").unwrap();
        set_age(&path, mtime_age);
        path
    }

    /// Backdate `path`'s mtime by `age`. Eviction compares mtime against the
    /// clock, so stamping it explicitly is what keeps these tests off the
    /// wall-clock — no sleeping, and no dependence on when they run.
    fn set_age(path: &Path, age: Duration) {
        let when = SystemTime::now() - age;
        File::open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(when))
            .unwrap();
    }

    #[test]
    fn test_evict_old_cache_removes_stale_folder() {
        let (temp_dir, cache_dir) = temp_library();
        let stale = cache_folder(&cache_dir, "2020-01-01", 30 * DAY);

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Removed 1 old cache folders");
        assert!(!stale.exists(), "a 30-day-old cache folder should be gone");
    }

    #[test]
    fn test_evict_old_cache_keeps_recent_folder() {
        let (temp_dir, cache_dir) = temp_library();
        let recent = cache_folder(&cache_dir, "2026-08-01", 14 * DAY);

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Removed 0 old cache folders");
        assert!(recent.exists(), "a 14-day-old cache folder should survive");
        assert!(recent.join("frame000001.jpg").exists());
    }

    // The cutoff is `age > 15 days`. Both folders here sit an hour away from it
    // so the outcome cannot hinge on how long the test itself takes to run.
    #[test]
    fn test_evict_old_cache_cutoff_is_fifteen_days() {
        let (temp_dir, cache_dir) = temp_library();
        let just_inside = cache_folder(&cache_dir, "inside", 15 * DAY - Duration::from_secs(3600));
        let just_outside = cache_folder(&cache_dir, "outside", 15 * DAY + Duration::from_secs(3600));

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Removed 1 old cache folders");
        assert!(just_inside.exists(), "14d23h is inside the window");
        assert!(!just_outside.exists(), "15d1h is outside the window");
    }

    #[test]
    fn test_evict_old_cache_skips_plain_files() {
        let (temp_dir, cache_dir) = temp_library();
        let loose_file = cache_dir.join("stale-note.txt");
        fs::write(&loose_file, b"old, but not a directory").unwrap();
        set_age(&loose_file, 30 * DAY);

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Removed 0 old cache folders");
        assert!(loose_file.exists(), "eviction only ever removes directories");
    }

    #[test]
    fn test_evict_old_cache_counts_only_what_it_removed() {
        let (temp_dir, cache_dir) = temp_library();
        let stale_a = cache_folder(&cache_dir, "stale-a", 20 * DAY);
        let stale_b = cache_folder(&cache_dir, "stale-b", 90 * DAY);
        let fresh = cache_folder(&cache_dir, "fresh", Duration::from_secs(60));
        let loose_file = cache_dir.join("stale-note.txt");
        fs::write(&loose_file, b"old, but not a directory").unwrap();
        set_age(&loose_file, 30 * DAY);

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Removed 2 old cache folders");
        assert!(!stale_a.exists());
        assert!(!stale_b.exists());
        assert!(fresh.exists());
        assert!(loose_file.exists());
    }

    #[test]
    fn test_evict_old_cache_without_cache_dir() {
        // A library that has never had frames extracted from it.
        let temp_dir = TempDir::new().unwrap();

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Cache directory does not exist");
    }

    #[test]
    fn test_evict_old_cache_leaves_the_rest_of_the_library_alone() {
        let (temp_dir, cache_dir) = temp_library();
        cache_folder(&cache_dir, "stale", 30 * DAY);

        // A day of screenshots, as old as the cache folder next to it.
        let day_dir = temp_dir.path().join("2020-01-01");
        fs::create_dir_all(&day_dir).unwrap();
        fs::write(day_dir.join("00001.png"), b"screenshot").unwrap();
        set_age(&day_dir, 30 * DAY);

        evict_old_cache_impl(temp_dir.path()).unwrap();

        assert!(
            day_dir.join("00001.png").exists(),
            "eviction must never reach outside .cache"
        );
    }

    fn frame_source_in(root: &Path) -> (TempDir, FrameSource) {
        let cache = TempDir::new().unwrap();
        let source =
            FrameSource::new(root.to_path_buf(), cache.path().to_path_buf(), u64::MAX, Tools::new(paths::ffmpeg()))
                .unwrap();
        (cache, source)
    }

    #[test]
    fn test_frame_response_serves_a_screenshot() {
        let library = TempDir::new().unwrap();
        fs::create_dir(library.path().join("2026-10-04")).unwrap();
        fs::write(library.path().join("2026-10-04/00001.png"), b"png bytes").unwrap();
        let (_cache, source) = frame_source_in(library.path());

        let response = frame_response(Some(&source), "/2026-10-04/0");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(response.body(), b"png bytes");
    }

    #[test]
    fn test_frame_response_rejects_bad_requests() {
        let library = TempDir::new().unwrap();
        let (_cache, source) = frame_source_in(library.path());

        assert_eq!(
            frame_response(None, "/2026-10-04/0").status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        for (path, status) in [
            ("/2026-10-04", StatusCode::BAD_REQUEST),
            ("/2026-10-04/abc", StatusCode::BAD_REQUEST),
            ("/2026-10-04/0/extra", StatusCode::BAD_REQUEST),
            ("/..%2Fetc/0", StatusCode::NOT_FOUND),
            ("/2026-10-04/0", StatusCode::NOT_FOUND),
        ] {
            assert_eq!(frame_response(Some(&source), path).status(), status, "{path}");
        }
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

    #[test]
    fn test_search_ocr_reads_the_library_database() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("screenshots.db")).unwrap();
        db.record_ocr_frame("2024-01-01", 4, Some(("cargo test", &[]))).unwrap();

        let hits = search_ocr_impl(temp_dir.path(), "cargo", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].day.as_str(), hits[0].frame_number), ("2024-01-01", 4));

        assert!(search_ocr_impl(temp_dir.path(), "nothing", 10).unwrap().is_empty());
    }
}
