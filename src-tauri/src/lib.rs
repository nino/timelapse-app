mod converter;
mod timelapse;
mod database;
mod ocr;
mod paths;
mod updater;

use frame_source::{DaySummary, FrameSource, FrameTime, PendingFrames, Tools};
use tauri::http::{header, Response, StatusCode};

use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use database::{OcrHit, ScreenshotDatabase};
use serde::Serialize;
use timelapse::Photographer;

// Shared state to manage the timelapse photographer
type PhotographerState = Arc<Mutex<Option<Photographer>>>;

/// Answers "frame N of day D" for the viewer. Managed once `setup` has
/// resolved the cache directory.
type FrameSourceState = Arc<FrameSource>;

/// Decoded video frames are disposable, so the cache lives in the OS cache
/// directory rather than in the (possibly synced) library.
const FRAME_CACHE_CAP_BYTES: u64 = 2 * 1024 * 1024 * 1024;

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
async fn start_timelapse(state: State<'_, PhotographerState>) -> Result<String, String> {
    start_timelapse_impl(state.inner(), || {
        Photographer::new().map_err(|e| e.to_string())
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
    let indices = source
        .indices_of_frames(date, &numbers)
        .map_err(|e| e.to_string())?;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let photographer_state: PhotographerState = Arc::new(Mutex::new(None));

    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(photographer_state)
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
                        });
                    match source {
                        Ok(source) => {
                            app.manage::<FrameSourceState>(Arc::new(source));
                        }
                        Err(e) => eprintln!("Failed to set up the frame source: {}", e),
                    }
                }
                None => eprintln!("Unable to find home directory"),
            }

            // Start timelapse automatically when app is ready
            let photographer_state = app.state::<PhotographerState>();
            let state_clone = Arc::clone(&photographer_state.inner());

            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                // Evict old cache entries on startup
                match evict_old_cache().await {
                    Ok(msg) => println!("Cache eviction: {}", msg),
                    Err(e) => eprintln!("Failed to evict old cache: {}", e),
                }

                match Photographer::new() {
                    Ok(photographer) => {
                        photographer.start();
                        let mut guard = state_clone.lock().unwrap();
                        *guard = Some(photographer);
                        println!("Timelapse started automatically on app startup");
                    }
                    Err(e) => {
                        eprintln!("Failed to start timelapse automatically: {}", e);
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
                        .start();

                        if ocr::start_background_ocr(root) {
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
            get_ocr_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
