//! The Tauri app: its commands and `setup()`, which starts the photographer,
//! OCR and the video converter. The find bar's commands are in `search`, the
//! windows and menu in `windows`, and the `frames:` URI scheme in
//! `frame_scheme`.
//!
//! Commands with logic worth testing are thin shims over `*_impl` functions
//! that take plain arguments. `tauri::State` wraps a private reference with no
//! public constructor, so that split is what lets the tests call the logic
//! directly.

mod activity;
mod analytics;
mod app_state;
mod backlog;
mod boost;
mod cache_eviction;
mod converter;
mod database;
mod day_apps;
mod day_status;
mod diagnostics;
mod frame_scheme;
mod media;
mod menu_app;
mod ocr;
mod ocr_helper;
mod ocr_readahead;
mod ocr_tiles;
mod paths;
mod previews;
mod scrub_log;
mod search;
mod settings;
mod timelapse;
mod updater;
mod windows;

use frame_source::{
    DayPreviews, DaySummary, DayVideos, FrameSource, FrameTime, PendingFrames, PreviewStore, Tools,
};
use tauri::http::header;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use activity::{Activity, Snapshot};
use app_state::{AppStateStore, ViewerPosition};
use database::ScreenshotDatabase;
use settings::{Settings, SettingsStore};
use timelapse::Photographer;
use updater::UpdaterState;

// Shared state to manage the timelapse photographer
type PhotographerState = Arc<Mutex<Option<Photographer>>>;

/// Answers "frame N of day D" for the viewer. Managed once `setup` has
/// resolved the cache directory.
pub(crate) type FrameSourceState = Arc<FrameSource>;

/// What capture, conversion and OCR are doing, for the Activity window.
type ActivityState = Arc<Activity>;

/// Whether conversion and OCR are boosted (`boost.rs`).
type BoostState = Arc<boost::Boost>;

/// Decoded video frames are disposable, so the cache lives in the OS cache
/// directory rather than in the (possibly synced) library.
const FRAME_CACHE_CAP_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Preview packs are disposable too, and kept apart from the frame cache so
/// that scrubbing through decoded frames never evicts them. The whole
/// existing library comes to roughly 2.5 GB of them.
const PREVIEW_CAP_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Event sent with a day's date when read-ahead has decoded a chunk of it.
const FRAMES_DECODED: &str = "frames-decoded";

/// Event sent with a day's date when the preview builder has built a pack
/// for one of its videos.
const PREVIEWS_BUILT: &str = "previews-built";

/// Event sent with the new settings whenever one changes, so the main window
/// picks up a change made in the Settings window.
const SETTINGS_CHANGED: &str = "settings-changed";

/// The directory under `$HOME` this build uses. Rust is the single source of
/// truth for it: the frontend asks at startup rather than deriving its own
/// answer, so the capture loop and the viewer can never disagree.
#[tauri::command]
fn get_timelapse_root_name() -> String {
    paths::TIMELAPSE_DIR_NAME.to_string()
}

/// A read-only connection to the library database in `root`, or `None`
/// before it exists.
pub(crate) fn library_db(root: &Path) -> Result<Option<database::Reader>, String> {
    ScreenshotDatabase::reader(ScreenshotDatabase::path_in(root)).map_err(|e| e.to_string())
}

/// Whether the photographer has been started.
fn is_capturing(state: &PhotographerState) -> Result<bool, String> {
    let photographer_guard = state.lock().map_err(|e| e.to_string())?;
    Ok(photographer_guard.is_some())
}

/// Run blocking work (ffmpeg, SQLite) off the async runtime's worker threads.
pub(crate) async fn run_blocking<T: Send + 'static>(
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

/// Whether OCR and video conversion have finished each day, oldest first.
/// See `day_status`.
#[tauri::command]
async fn get_day_statuses() -> Result<Vec<day_status::DayStatus>, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    run_blocking(move || {
        // A database that can't be opened reads as one OCR hasn't touched.
        let db = library_db(&timelapse_root).unwrap_or(None);
        day_status::day_statuses(&timelapse_root, db.as_deref()).map_err(|e| e.to_string())
    })
    .await
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
    with_frame_source(source.inner(), move |s| {
        let started = Instant::now();
        let time = s.frame_time(&date, index);
        scrub_log::step("frameTime", started.elapsed());
        time
    })
    .await
}

/// How long frames took from the page asking for them to having them on
/// screen, in milliseconds, for the scrub log (`scrub_log`): `milliseconds`
/// for `<img>` frames, `seeks` for the beta video player's seeks.
#[tauri::command]
fn report_frames_shown(milliseconds: Vec<f64>, seeks: Option<Vec<f64>>) {
    let steps = [("shown", milliseconds), ("videoSeek", seeks.unwrap_or_default())];
    for (step, samples) in steps {
        for ms in samples {
            if ms.is_finite() && ms >= 0.0 {
                scrub_log::step(step, Duration::from_secs_f64(ms / 1000.0));
            }
        }
    }
}

/// Frame ranges of `date` that still have to be decoded from video.
#[tauri::command]
async fn get_pending_frames(
    source: State<'_, FrameSourceState>,
    date: String,
) -> Result<PendingFrames, String> {
    with_frame_source(source.inner(), move |s| s.pending(&date)).await
}

/// The stretches of `date` served from video and the video each comes from,
/// for the beta video player, which plays them from `videos:` URLs.
#[tauri::command]
async fn get_day_videos(
    source: State<'_, FrameSourceState>,
    date: String,
) -> Result<DayVideos, String> {
    with_frame_source(source.inner(), move |s| s.day_videos(&date)).await
}

/// The preview packs of `date`'s videos, for showing while the scrubber is
/// dragged. Each is served at `frames://localhost/previews/<name>`.
#[tauri::command]
async fn get_day_previews(source: State<'_, FrameSourceState>, date: String) -> Result<DayPreviews, String> {
    with_frame_source(source.inner(), move |s| s.day_previews(&date)).await
}

/// Which app was in front across `date`, for the viewer's app filter.
#[tauri::command]
async fn get_day_apps(
    source: State<'_, FrameSourceState>,
    date: String,
) -> Result<day_apps::DayApps, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    let source = Arc::clone(source.inner());
    run_blocking(move || {
        let db = library_db(&timelapse_root)?;
        day_apps::day_apps(db.as_deref(), &source, &date)
    })
    .await
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
    let capturing = is_capturing(photographer.inner())?;
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

/// What OCR and video conversion have left to do, for the Activity window.
/// Lists the library and reads an index or two, so the window asks for it
/// far less often than `get_activity`. See `day_status::remaining_work`.
#[tauri::command]
async fn get_remaining_work(activity: State<'_, ActivityState>) -> Result<day_status::RemainingWork, String> {
    let timelapse_root = paths::timelapse_root().ok_or("Unable to find home directory")?;
    let video_frames_left = activity.ocr_video_frames_left();
    run_blocking(move || {
        let db = library_db(&timelapse_root).unwrap_or(None);
        day_status::remaining_work(&timelapse_root, db.as_deref(), video_frames_left.as_ref()).map_err(|e| e.to_string())
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

/// Sends a bug report (`kind` "bug") or feature request ("feature") to
/// PostHog and says whether it arrived (`analytics::feedback`).
#[tauri::command]
async fn send_feedback(kind: String, message: String) -> Result<(), String> {
    analytics::feedback(&kind, &message).await
}

/// The app's settings. Changes apply at once and are saved straight away.
#[tauri::command]
fn get_settings(settings: State<'_, SettingsStore>) -> Settings {
    settings.get()
}

/// Tells every window the settings have changed (`SETTINGS_CHANGED`), so the
/// main window picks up a change made in the Settings window.
fn settings_changed(app: &AppHandle, saved: Settings) {
    if let Err(e) = app.emit(SETTINGS_CHANGED, saved) {
        eprintln!("Could not report the settings change: {}", e);
    }
}

#[tauri::command]
fn set_update_automatically(
    enabled: bool,
    app: AppHandle,
    settings: State<'_, SettingsStore>,
    updater: State<'_, UpdaterState>,
) -> Result<Settings, String> {
    let saved = settings.update(|s| s.update_automatically = enabled);
    updater.setting_changed();
    if let Ok(saved) = saved {
        settings_changed(&app, saved);
    }
    saved
}

/// The version of an update installed and waiting for a relaunch, if any.
/// The main window shows an Update button for it, as it does on `UPDATE_READY`.
#[tauri::command]
fn get_installed_update(updater: State<'_, UpdaterState>) -> Option<String> {
    updater::installed_version(&updater)
}

/// The Update button: quit and open the installed update.
#[tauri::command]
fn relaunch_into_update(app: AppHandle) {
    updater::relaunch_into_update(&app);
}

#[tauri::command]
fn set_send_error_reports(
    enabled: bool,
    app: AppHandle,
    settings: State<'_, SettingsStore>,
) -> Result<Settings, String> {
    match settings.update(|s| s.send_error_reports = enabled) {
        Ok(saved) => {
            analytics::set_enabled(enabled);
            settings_changed(&app, saved);
            Ok(saved)
        }
        Err(e) => {
            // The page puts the switch back when saving fails, so take the
            // change back here too rather than send with the switch shown off.
            let _ = settings.update(|s| s.send_error_reports = !enabled);
            Err(e)
        }
    }
}

/// Turns the beta video player on or off, and tells the main window.
#[tauri::command]
fn set_video_player_beta(
    enabled: bool,
    app: AppHandle,
    settings: State<'_, SettingsStore>,
) -> Result<Settings, String> {
    match settings.update(|s| s.video_player_beta = enabled) {
        Ok(saved) => {
            settings_changed(&app, saved);
            Ok(saved)
        }
        Err(e) => {
            // The page puts the switch back when saving fails, so take the
            // change back here too, or the main window (never told) and the
            // store would disagree with what the switch shows.
            let _ = settings.update(|s| s.video_player_beta = !enabled);
            Err(e)
        }
    }
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

/// What has to happen before the app quits or relaunches, however it does:
/// ffmpeg is a separate process and would outlive the app, and the window
/// state is otherwise written a second after the last change.
pub(crate) fn before_quit<R: Runtime>(app: &AppHandle<R>) {
    converter::kill_running_encode();
    frame_source::previews::kill_running_build();
    if let Some(state) = app.try_state::<AppStateStore>() {
        state.save_now();
    }
    diagnostics::info("app", "Quit").record();
    diagnostics::flush(std::time::Duration::from_secs(1));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// If this process was started as the OCR helper (see `ocr_helper`), serve
/// text recognition until the app closes its stdin, and return `true`: the
/// caller then exits instead of starting the app.
pub fn run_ocr_helper_if_asked() -> bool {
    if std::env::args().nth(1).as_deref() != Some(ocr_helper::HELPER_ARG) {
        return false;
    }
    ocr::run_helper();
    true
}

/// Raise this process's limit on open files from macOS's default of 256 for
/// apps started from Finder, which the app's own files, ffmpeg's pipes and
/// WebKit's share, to what the system allows. Scrubbing once ran past 256;
/// this is headroom, not the fix for that.
#[cfg(unix)]
fn raise_open_file_limit() {
    // OPEN_MAX: macOS refuses a soft limit above it even when the hard
    // limit is unlimited.
    const MOST: libc::rlim_t = 10_240;
    // SAFETY: getrlimit and setrlimit only read and write the struct.
    unsafe {
        let mut limit: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
            return;
        }
        let wanted = limit.rlim_max.min(MOST);
        if limit.rlim_cur < wanted {
            limit.rlim_cur = wanted;
            libc::setrlimit(libc::RLIMIT_NOFILE, &limit);
        }
    }
}

pub fn run() {
    #[cfg(unix)]
    raise_open_file_limit();
    let photographer_state: PhotographerState = Arc::new(Mutex::new(None));
    let activity: ActivityState = Arc::default();

    let builder = tauri::Builder::default();
    // Elsewhere a menu would add a menu bar to the main window.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(windows::app_menu);

    builder
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(photographer_state)
        .manage(activity)
        .manage(BoostState::default())
        .manage(UpdaterState::default())
        .on_menu_event(windows::on_menu_event)
        .on_window_event(windows::on_window_event)
        .register_asynchronous_uri_scheme_protocol("frames", |ctx, request, responder| {
            let source = ctx
                .app_handle()
                .try_state::<FrameSourceState>()
                .map(|state| Arc::clone(state.inner()));
            let path = request.uri().path().to_owned();
            // Decoding a chunk of video can take a moment; keep it off the
            // webview's thread.
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(frame_scheme::frame_response(source.as_deref(), &path));
            });
        })
        .register_asynchronous_uri_scheme_protocol("videos", |ctx, request, responder| {
            let source = ctx
                .app_handle()
                .try_state::<FrameSourceState>()
                .map(|state| Arc::clone(state.inner()));
            let name = request.uri().path().trim_start_matches('/').to_owned();
            let range = request
                .headers()
                .get(header::RANGE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            // A range is a few megabytes read from disk; keep it off the
            // webview's thread all the same.
            tauri::async_runtime::spawn_blocking(move || {
                let path = source.and_then(|source| source.video_path(&name));
                responder.respond(media::video_response(path.as_deref(), range.as_deref()));
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
            windows::restore_windows(app.handle())?;
            let version = app.package_info().version.to_string();
            let reports = app.state::<SettingsStore>().get().send_error_reports;
            analytics::init(app.path().app_config_dir().ok().as_deref(), &version, reports);
            analytics::capture("app started", serde_json::json!({}));
            updater::spawn(app.handle().clone());

            // Shared by the frame source, which serves them, the converter,
            // which writes them for new videos, and the preview builder.
            let mut preview_store: Option<Arc<PreviewStore>> = None;

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
                            source
                                .read_ahead(move |date| {
                                    if let Err(e) = app.emit(FRAMES_DECODED, date) {
                                        eprintln!("Could not report decoded frames: {}", e);
                                    }
                                })
                                .time_steps(scrub_log::step)
                        });
                    // Per profile too. Without it, scrubbing works as before.
                    preview_store = app
                        .path()
                        .app_cache_dir()
                        .map_err(|e| e.to_string())
                        .and_then(|dir| {
                            let dir = dir.join(paths::TIMELAPSE_DIR_NAME).join("previews");
                            PreviewStore::open(dir, PREVIEW_CAP_BYTES).map_err(|e| e.to_string())
                        })
                        .inspect_err(|e| {
                            diagnostics::warn("previews", format!("Failed to open the preview store: {}", e)).record();
                        })
                        .ok()
                        .map(Arc::new);
                    let source = source.map(|source| match &preview_store {
                        Some(store) => source.with_previews(Arc::clone(store)),
                        None => source,
                    });
                    match source {
                        Ok(source) => {
                            let source = Arc::new(source);
                            app.manage::<FrameSourceState>(Arc::clone(&source));
                            if preview_store.is_some() {
                                let app = app.handle().clone();
                                let boost = Arc::clone(app.state::<BoostState>().inner());
                                previews::start(source, boost, move |date| {
                                    if let Err(e) = app.emit(PREVIEWS_BUILT, date) {
                                        eprintln!("Could not report built previews: {}", e);
                                    }
                                });
                            }
                        }
                        Err(e) => {
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
            let state_clone = Arc::clone(photographer_state.inner());
            let activity = Arc::clone(app.state::<ActivityState>().inner());
            let boost = Arc::clone(app.state::<BoostState>().inner());
            let converter_previews = preview_store.clone();

            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                // Evict old cache entries on startup
                if let Some(root) = paths::timelapse_root() {
                    match cache_eviction::evict_old_cache_impl(&root) {
                        Ok(msg) => println!("Cache eviction: {}", msg),
                        Err(e) => eprintln!("Failed to evict old cache: {}", e),
                    }
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
                        .with_previews(converter_previews)
                        .start();

                        backlog::start(root.clone(), Arc::clone(&activity), Arc::clone(&boost));
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
            get_timelapse_root_name,
            list_days,
            get_day,
            get_day_statuses,
            get_frame_time,
            report_frames_shown,
            get_pending_frames,
            get_day_videos,
            get_day_previews,
            search::search_ocr_day,
            get_day_apps,
            search::get_match_lines,
            search::count_ocr_matches,
            search::get_ocr_version,
            get_activity,
            get_remaining_work,
            dismiss_error,
            start_boost,
            stop_boost,
            start_low_power,
            stop_low_power,
            set_boost_allow_battery,
            log_frontend_error,
            send_feedback,
            get_settings,
            set_update_automatically,
            get_installed_update,
            relaunch_into_update,
            set_send_error_reports,
            set_video_player_beta,
            get_viewer_position,
            set_viewer_position,
            windows::grow_settings_window
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                before_quit(app);
            }
        });
}

