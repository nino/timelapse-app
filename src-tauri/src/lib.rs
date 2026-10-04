mod converter;
mod timelapse;
mod database;
mod paths;

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use timelapse::Photographer;

// Shared state to manage the timelapse photographer
type PhotographerState = Arc<Mutex<Option<Photographer>>>;

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
) -> Result<Option<(String, String)>, String> {
    let photographer_guard = state.lock().map_err(|e| e.to_string())?;

    if let Some(photographer) = &*photographer_guard {
        photographer
            .get_screenshot_metadata(frame_number)
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

/// Extract the frames of `<root>/<video_filename>` into
/// `<root>/.cache/<basename>/`, and return that cache folder's name — the
/// frontend reads the JPEGs out of it directly.
///
/// Takes the library root instead of resolving it so tests can aim it at a
/// `TempDir`; the command below passes the real one.
fn extract_video_frames_impl(root: &Path, video_filename: &str) -> Result<String, String> {
    let source_path = root.join(video_filename);

    // Create cache directory if it doesn't exist
    let cache_dir = root.join(".cache");
    std::fs::create_dir_all(&cache_dir).map_err(|e| format!("Failed to create cache directory: {}", e))?;

    // Generate cache folder name (remove .mov extension)
    let cache_folder_name = video_filename.trim_end_matches(".mov");
    let cache_folder_path = cache_dir.join(cache_folder_name);

    // Check if frame sequence already exists. The folder is only moved into
    // place after ffmpeg has finished, so if it is here at all it is complete.
    if cache_folder_path.is_dir() {
        let entries = std::fs::read_dir(&cache_folder_path)
            .map_err(|e| format!("Failed to read cache directory: {}", e))?;
        let has_frames = entries.count() > 0;
        if has_frames {
            println!("Using cached frame sequence: {:?}", cache_folder_path);
            return Ok(cache_folder_name.to_string());
        }
    }

    // Extract into a staging folder and publish it under the real name only
    // once ffmpeg has succeeded. Writing straight into the cache folder meant
    // an interrupted run left a partial frame set that every later call read as
    // a complete cache, with no way to recover but deleting it by hand.
    let staging_path = cache_dir.join(format!(".{}.partial", cache_folder_name));
    if staging_path.exists() {
        std::fs::remove_dir_all(&staging_path)
            .map_err(|e| format!("Failed to clear stale staging folder: {}", e))?;
    }
    std::fs::create_dir_all(&staging_path)
        .map_err(|e| format!("Failed to create staging folder: {}", e))?;

    println!("Extracting frames from video: {:?} -> {:?}", source_path, staging_path);

    // Run ffmpeg to extract frames as JPEG images
    // frame%06d.jpg creates frame000001.jpg, frame000002.jpg, etc.
    let output_pattern = staging_path.join("frame%06d.jpg");
    let spawned = Command::new("ffmpeg")
        .arg("-i")
        .arg(&source_path)
        .arg("-vf")
        .arg("fps=30") // Extract at 30 fps (adjust as needed)
        .arg("-q:v")
        .arg("2") // High quality JPEG (1-31, lower is better)
        .arg("-y")
        .arg(&output_pattern)
        .output();

    let output = match spawned {
        Ok(output) => output,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging_path);
            return Err(format!("Failed to execute ffmpeg: {}. Make sure ffmpeg is installed and in PATH.", e));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let _ = std::fs::remove_dir_all(&staging_path);
        return Err(format!("ffmpeg failed: {}", stderr));
    }

    publish_staged_frames(&staging_path, &cache_folder_path, video_filename)?;

    println!("Frame extraction complete: {:?}", cache_folder_path);

    Ok(cache_folder_name.to_string())
}

/// Move a finished staging folder into the place the frontend reads from.
///
/// Split out of `extract_video_frames_impl` so the empty-output guard and the
/// rename are reachable in tests: everything above the call site needs a real
/// video and a working ffmpeg before it gets this far.
fn publish_staged_frames(
    staging_path: &Path,
    cache_folder_path: &Path,
    video_filename: &str,
) -> Result<(), String> {
    // ffmpeg can exit 0 having written nothing — a zero-length or unreadable
    // stream does exactly that. Publishing that empty folder would count as a
    // cache hit forever after, stranding the UI on "Loading frames…".
    let frame_count = std::fs::read_dir(staging_path)
        .map_err(|e| format!("Failed to read staging folder: {}", e))?
        .count();
    if frame_count == 0 {
        let _ = std::fs::remove_dir_all(staging_path);
        return Err(format!("ffmpeg produced no frames for {}", video_filename));
    }

    // An empty folder left by an older run would make the rename fail, so clear
    // it first.
    if cache_folder_path.exists() {
        std::fs::remove_dir_all(cache_folder_path)
            .map_err(|e| format!("Failed to clear incomplete cache folder: {}", e))?;
    }
    std::fs::rename(staging_path, cache_folder_path)
        .map_err(|e| format!("Failed to publish extracted frames: {}", e))
}

#[tauri::command]
async fn extract_video_frames(video_filename: String) -> Result<String, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    extract_video_frames_impl(&timelapse_root, &video_filename)
}

#[tauri::command]
async fn get_screenshot_metadata(
    state: State<'_, PhotographerState>,
    frame_number: u32,
) -> Result<Option<(String, String)>, String> {
    get_screenshot_metadata_impl(state.inner(), frame_number)
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
        .manage(photographer_state)
        .setup(|app| {
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
                // power. It runs for the life of the app, independently of the
                // photographer, so it has no state or commands of its own yet.
                match paths::timelapse_root() {
                    Some(root) => converter::Converter::new_in(root).start(),
                    None => eprintln!("Unable to find home directory; video conversion is off"),
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
            extract_video_frames,
            evict_old_cache,
            get_screenshot_metadata
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

        let result = get_screenshot_metadata_impl(&state, 1);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Timelapse is not running");
    }

    #[test]
    fn test_get_screenshot_metadata_unknown_frame() {
        let (_temp_dir, state) = running_state();

        // Nothing has been captured into this temp library yet.
        let result = get_screenshot_metadata_impl(&state, 1);
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

    #[test]
    fn test_extract_video_frames_reuses_populated_cache() {
        let (temp_dir, cache_dir) = temp_library();
        let cached = cache_dir.join("2026-08-27");
        fs::create_dir_all(&cached).unwrap();
        fs::write(cached.join("frame000001.jpg"), b"already extracted").unwrap();

        // No such video exists, and ffmpeg may not be installed — reaching
        // either would fail, so an Ok here is proof the cache branch was taken.
        let result = extract_video_frames_impl(temp_dir.path(), "2026-08-27.mov");

        assert_eq!(result.unwrap(), "2026-08-27");
        assert_eq!(
            fs::read(cached.join("frame000001.jpg")).unwrap(),
            b"already extracted",
            "the cached frames should be left untouched"
        );
        assert_eq!(
            fs::read_dir(&cached).unwrap().count(),
            1,
            "no new frames should have been written"
        );
    }

    // The mirror of the test above: an *empty* cache folder is not a hit, so
    // this falls through to ffmpeg and fails there — either because ffmpeg is
    // missing, or because it is present and the source video is not.
    #[test]
    fn test_extract_video_frames_empty_cache_folder_is_not_a_hit() {
        let (temp_dir, cache_dir) = temp_library();
        let empty = cache_dir.join("2026-08-27");
        fs::create_dir_all(&empty).unwrap();

        let result = extract_video_frames_impl(temp_dir.path(), "2026-08-27.mov");

        assert!(result.is_err(), "an empty cache folder must not count as cached");
    }

    #[test]
    fn test_extract_video_frames_creates_cache_dir_when_missing() {
        // No `.cache` at all, and no video to extract.
        let temp_dir = TempDir::new().unwrap();

        let result = extract_video_frames_impl(temp_dir.path(), "2026-08-27.mov");

        assert!(result.is_err(), "there is no source video to extract");
        assert!(
            temp_dir.path().join(".cache").is_dir(),
            "the cache directory should have been created before ffmpeg ran"
        );
    }

    // Extraction stages into `.<name>.partial` and only renames it into place
    // once ffmpeg succeeds, so a failed run must not leave anything behind that
    // a later call would read back as a finished cache.
    #[test]
    fn test_extract_video_frames_publishes_nothing_when_ffmpeg_fails() {
        let (temp_dir, cache_dir) = temp_library();

        let result = extract_video_frames_impl(temp_dir.path(), "2026-08-27.mov");

        assert!(result.is_err(), "there is no source video to extract");
        assert!(
            !cache_dir.join("2026-08-27").exists(),
            "a failed extraction must not publish a cache folder"
        );
        assert!(
            !cache_dir.join(".2026-08-27.partial").exists(),
            "the staging folder should have been cleaned up"
        );
    }

    // The regression this replaces: frames written directly into the cache
    // folder made any interrupted run look like a complete one forever.
    #[test]
    fn test_extract_video_frames_reruns_after_a_partial_extraction() {
        let (temp_dir, cache_dir) = temp_library();

        // Simulate an interrupted run under the new scheme: frames are in
        // staging, and nothing has been published.
        let staging = cache_dir.join(".2026-08-27.partial");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("frame000001.jpg"), b"half a run").unwrap();

        let result = extract_video_frames_impl(temp_dir.path(), "2026-08-27.mov");

        assert!(
            result.is_err(),
            "a partial extraction must not count as cached; this should reach ffmpeg and fail"
        );
        assert!(
            !cache_dir.join("2026-08-27").exists(),
            "the partial frames must never be published under the real name"
        );
    }

    #[test]
    fn test_publish_staged_frames_moves_the_folder_into_place() {
        let (_temp_dir, cache_dir) = temp_library();
        let staging = cache_dir.join(".2026-08-27.partial");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("frame000001.jpg"), b"a frame").unwrap();
        let published = cache_dir.join("2026-08-27");

        publish_staged_frames(&staging, &published, "2026-08-27.mov").unwrap();

        assert!(!staging.exists(), "staging should have been renamed away");
        assert_eq!(
            fs::read(published.join("frame000001.jpg")).unwrap(),
            b"a frame"
        );
    }

    // ffmpeg reports success on some inputs without writing a single frame.
    // Publishing that would be a permanent empty cache hit.
    #[test]
    fn test_publish_staged_frames_rejects_an_empty_staging_folder() {
        let (_temp_dir, cache_dir) = temp_library();
        let staging = cache_dir.join(".2026-08-27.partial");
        fs::create_dir_all(&staging).unwrap();
        let published = cache_dir.join("2026-08-27");

        let result = publish_staged_frames(&staging, &published, "2026-08-27.mov");

        assert!(result.is_err(), "an empty extraction must not be published");
        assert!(!published.exists(), "nothing should have been published");
        assert!(!staging.exists(), "the empty staging folder should be gone");
    }

    #[test]
    fn test_publish_staged_frames_replaces_an_empty_leftover_folder() {
        let (_temp_dir, cache_dir) = temp_library();
        let staging = cache_dir.join(".2026-08-27.partial");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("frame000001.jpg"), b"a frame").unwrap();

        // An earlier version created this eagerly and left it behind on failure.
        let published = cache_dir.join("2026-08-27");
        fs::create_dir_all(&published).unwrap();

        publish_staged_frames(&staging, &published, "2026-08-27.mov").unwrap();

        assert_eq!(
            fs::read(published.join("frame000001.jpg")).unwrap(),
            b"a frame"
        );
    }
}
