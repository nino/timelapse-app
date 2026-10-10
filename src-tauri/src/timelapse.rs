use active_win_pos_rs::get_active_window;
use chrono::{DateTime, Utc, Local};
use fast_image_resize::images::{CroppedImageMut, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::{ImageFormat, RgbImage};
use screenshots::Screen;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tokio::time::{sleep, Duration};
use crate::activity::{Activity, FAILURES_BEFORE_BACKOFF};
use crate::database::{FrontWindow, ScreenshotDatabase};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorLogEntry {
    pub timestamp: DateTime<Utc>,
    pub error_message: String,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Unable to find home dir")]
    UnableToFindHomeDir,

    #[error("Unable to create screenshot because: {reason}")]
    UnableToCreateScreenshot { reason: String },

    #[error("Unable to resize screenshot {path} because: {reason}")]
    UnableToResizeScreenshot { path: String, reason: String },

    #[error("Unable to write screenshot {path} because: {reason}")]
    UnableToWriteScreenshot { path: String, reason: String },

    #[error("Unable convert screenshot path to string")]
    UnableToConvertScreenshotPathToString,

    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    #[error("IO Error")]
    IoError(#[from] std::io::Error),
}

pub struct Photographer {
    timelapse_root_path: PathBuf,
    running: Arc<AtomicBool>,
    error_logs: Arc<Mutex<Vec<ErrorLogEntry>>>,
    db: Arc<Mutex<ScreenshotDatabase>>,
    activity: Arc<Activity>,
}

impl Photographer {
    pub fn new() -> Result<Photographer, Error> {
        let timelapse_root_path =
            crate::paths::timelapse_root().ok_or(Error::UnableToFindHomeDir)?;

        Self::new_in(timelapse_root_path)
    }

    /// Build a photographer rooted at an arbitrary directory instead of
    /// `~/Timelapse`. Tests use this so they never read or write the real
    /// screenshot library.
    pub fn new_in(timelapse_root_path: PathBuf) -> Result<Photographer, Error> {
        // Create the Timelapse directory if it doesn't exist
        std::fs::create_dir_all(&timelapse_root_path)?;

        // Initialize the database
        let db_path = timelapse_root_path.join("screenshots.db");
        let db = ScreenshotDatabase::new(db_path)?;

        Ok(Photographer {
            timelapse_root_path,
            running: Arc::new(AtomicBool::new(false)),
            error_logs: Arc::new(Mutex::new(Vec::new())),
            db: Arc::new(Mutex::new(db)),
            activity: Arc::default(),
        })
    }

    /// Report captures to `activity`, for the Activity window.
    pub fn reporting_to(mut self, activity: Arc<Activity>) -> Self {
        self.activity = activity;
        self
    }

    pub fn start(&self) -> Arc<AtomicBool> {
        let running = Arc::clone(&self.running);
        running.store(true, Ordering::SeqCst);

        let timelapse_root_path = self.timelapse_root_path.clone();
        let running_clone = Arc::clone(&running);
        let error_logs_clone = Arc::clone(&self.error_logs);
        let db_clone = Arc::clone(&self.db);
        let activity = Arc::clone(&self.activity);

        tokio::spawn(async move {
            println!("Starting timelapse background task...");
            let mut failures_in_a_row = 0;
            crate::diagnostics::info("capture", "Started").record();
            let mut tally = crate::diagnostics::Tally::new("capture");

            while running_clone.load(Ordering::SeqCst) {
                let started = std::time::Instant::now();
                let captured = Self::do_screenshot(&timelapse_root_path, &db_clone).await;
                tally.time("capture", started.elapsed());
                match captured {
                    Ok(Captured::Saved { day, number }) => {
                        failures_in_a_row = 0;
                        tally.count("saved", 1);
                        activity.frame_saved(&day, number);
                        tally.report_if_due();
                        sleep(Duration::from_secs(1)).await;
                    }
                    Ok(Captured::Black) => {
                        // The screen was off or locked: wait 10 seconds.
                        failures_in_a_row = 0;
                        tally.count("black", 1);
                        activity.black_frame_dropped();
                        tally.report_if_due();
                        sleep(Duration::from_secs(10)).await;
                    }
                    Err(error) => {
                        eprintln!("Screenshot error: {}", error);
                        failures_in_a_row += 1;
                        tally.count("failed", 1);
                        activity.capture_failed(error.to_string());

                        // Log the error
                        let entry = ErrorLogEntry {
                            timestamp: Utc::now(),
                            error_message: error.to_string(),
                        };

                        if let Ok(mut logs) = error_logs_clone.lock() {
                            logs.push(entry);
                            if logs.len() > 10000 {
                                logs.remove(0);
                            }
                        }

                        tally.report_if_due();
                        sleep(retry_delay(failures_in_a_row)).await;
                    }
                }
            }

            println!("Timelapse background task stopped.");
            crate::diagnostics::info("capture", "Stopped").record();
        });

        running
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn get_error_logs(&self) -> Vec<ErrorLogEntry> {
        self.error_logs
            .lock()
            .map(|logs| logs.clone())
            .unwrap_or_default()
    }

    pub fn clear_error_logs(&self) {
        if let Ok(mut logs) = self.error_logs.lock() {
            logs.clear();
        }
    }

    pub fn get_screenshot_metadata(
        &self,
        frame_number: u32,
        day: Option<&str>,
    ) -> Result<Option<(String, String)>, Error> {
        if let Ok(db_guard) = self.db.lock() {
            Ok(db_guard.get_screenshot_by_frame(frame_number, day)?)
        } else {
            Err(Error::DatabaseError(rusqlite::Error::InvalidQuery))
        }
    }

    /// Create today's folder if needed, and return its name and path.
    fn create_day_dir_if_needed(timelapse_root_path: &PathBuf) -> Result<(String, PathBuf), Error> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let day_dir = timelapse_root_path.join(&today);
        std::fs::create_dir_all(&day_dir)?;
        Ok((today, day_dir))
    }

    async fn do_screenshot(
        timelapse_root_path: &PathBuf,
        db: &Arc<Mutex<ScreenshotDatabase>>,
    ) -> Result<Captured, Error> {
        let (day, day_dir) = Self::create_day_dir_if_needed(timelapse_root_path)?;
        let filename = next_filename(&day_dir)?;
        let screenshot_path = String::from(
            day_dir
                .join(&filename)
                .to_str()
                .ok_or(Error::UnableToConvertScreenshotPathToString)?,
        );

        // Get the focused screen by finding which screen contains the active
        // window, and note that window for the screenshot's row.
        let (screen, window) = get_focused_screen().await?;

        // Capturing, resizing and encoding take tens of milliseconds, so they
        // run on the blocking pool rather than holding up a runtime worker.
        let path = screenshot_path.clone();
        let is_black = tokio::task::spawn_blocking(move || {
            with_capture(&screen, |capture| store_frame(capture, &path))
        })
            .await
            .map_err(|e| Error::UnableToWriteScreenshot {
                path: screenshot_path.clone(),
                reason: format!("Frame processing panicked: {}", e),
            })??;

        if is_black {
            println!("Screenshot is all black, skipping: {}", screenshot_path);
            Ok(Captured::Black)
        } else {
            // Extract frame number from filename (e.g., "00001.png" -> 1)
            let frame_number: u32 = filename
                .replace(".png", "")
                .parse()
                .unwrap_or(0);

            // Insert metadata into database with both UTC and local timestamps
            let created_at = Utc::now();
            let local_time = Local::now();
            if let Ok(db_guard) = db.lock() {
                db_guard.insert_capture(&day, frame_number, created_at, local_time, window.as_ref())?;
            }

            Ok(Captured::Saved { day, number: frame_number })
        }
    }
}

/// What one capture produced.
enum Captured {
    Saved { day: String, number: u32 },
    /// The frame was all black, so nothing was written.
    Black,
}

/// How long to wait after `failures_in_a_row` failed captures. A one-off
/// failure, such as the focused window closing mid-capture, is retried on the
/// next second; only `FAILURES_BEFORE_BACKOFF` in a row back off for a minute.
fn retry_delay(failures_in_a_row: u32) -> Duration {
    if failures_in_a_row >= FAILURES_BEFORE_BACKOFF {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(1)
    }
}

fn next_filename(day_dir: &PathBuf) -> Result<String, Error> {
    let entries = std::fs::read_dir(day_dir)?;
    let files = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_type()
                .map(|file_type| file_type.is_file())
                .unwrap_or(false)
        })
        .filter_map(|entry| entry.file_name().to_str().map(|s| s.to_string()));

    let max = files
        .filter_map(|filename| {
            filename
                .replace(".jpg", "")
                .replace(".png", "")
                .parse::<i32>()
                .ok()
        })
        .max()
        .unwrap_or(0);

    Ok(format!("{:05}.png", max + 1))
}

/// A captured screen as raw pixels, four bytes each, rows `bytes_per_row`
/// apart (which can be more than `width * 4`). On macOS the pixels are
/// borrowed from Core Graphics' own copy of the screen.
struct Capture<'a> {
    width: u32,
    height: u32,
    bytes_per_row: usize,
    pixels: Cow<'a, [u8]>,
    /// Whether each pixel is blue, green, red, alpha (Core Graphics' order)
    /// rather than red, green, blue, alpha.
    bgra: bool,
    /// Captured pixels per point of screen: 2 on a Retina display, 1 on a
    /// monitor at its native resolution.
    pixels_per_point: f64,
}

impl Capture<'_> {
    /// Decode a PNG, as the `screenshots` crate returns captures off macOS.
    #[cfg(any(not(target_os = "macos"), test))]
    fn from_png(data: &[u8], pixels_per_point: f64) -> Result<Capture<'static>, Error> {
        let image = image::load_from_memory_with_format(data, ImageFormat::Png)
            .map_err(|e| Error::UnableToCreateScreenshot {
                reason: format!("Failed to read image: {}", e),
            })?
            .into_rgba8();
        let (width, height) = image.dimensions();
        Ok(Capture {
            width,
            height,
            bytes_per_row: width as usize * 4,
            pixels: Cow::Owned(image.into_raw()),
            bgra: false,
            pixels_per_point,
        })
    }
}

/// Capture `screen` and hand the capture to `use_capture`.
///
/// On macOS this asks Core Graphics directly and lends out its pixels as they
/// are. The `screenshots` crate's own `capture()` swaps every pixel to RGBA and
/// encodes a full-resolution PNG, which `fit_to_frame` would only decode again.
/// The capture can't outlive the Core Graphics image, which isn't `Send`,
/// hence the callback rather than a return value.
#[cfg(target_os = "macos")]
fn with_capture<R>(
    screen: &Screen,
    use_capture: impl FnOnce(&Capture) -> Result<R, Error>,
) -> Result<R, Error> {
    use core_graphics::base::{kCGBitmapByteOrder32Little, kCGImageAlphaNoneSkipFirst};
    use core_graphics::color_space::CGColorSpace;
    use core_graphics::context::CGContext;
    use core_graphics::display::{
        kCGNullWindowID, kCGWindowImageDefault, kCGWindowListOptionOnScreenOnly, CGDisplay,
    };
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};

    let image = CGDisplay::screenshot(
        CGDisplay::new(screen.display_info.id).bounds(),
        kCGWindowListOptionOnScreenOnly,
        kCGNullWindowID,
        kCGWindowImageDefault,
    )
    .ok_or_else(|| Error::UnableToCreateScreenshot {
        reason: format!("Screen {} could not be captured", screen.display_info.id),
    })?;
    let (width, height) = (image.width(), image.height());
    let pixels_per_point = width as f64 / screen.display_info.width.max(1) as f64;

    if is_bgra(&image) {
        let data = image.data();
        return use_capture(&Capture {
            width: width as u32,
            height: height as u32,
            bytes_per_row: image.bytes_per_row(),
            pixels: Cow::Borrowed(data.bytes()),
            bgra: true,
            pixels_per_point,
        });
    }

    // Any other layout (16-bit pixels from an HDR display, say) is drawn into
    // a BGRA buffer of our own, which costs about as much as the PNG round
    // trip did.
    let mut pixels = vec![0u8; width * height * 4];
    {
        let context = CGContext::create_bitmap_context(
            Some(pixels.as_mut_ptr().cast()),
            width,
            height,
            8,
            width * 4,
            &CGColorSpace::create_device_rgb(),
            kCGImageAlphaNoneSkipFirst | kCGBitmapByteOrder32Little,
        );
        let bounds = CGRect::new(&CGPoint::new(0.0, 0.0), &CGSize::new(width as f64, height as f64));
        context.draw_image(bounds, &image);
    }
    use_capture(&Capture {
        width: width as u32,
        height: height as u32,
        bytes_per_row: width * 4,
        pixels: Cow::Owned(pixels),
        bgra: true,
        pixels_per_point,
    })
}

/// Whether `image` is 8-bit BGRA (or BGRX) in memory, which is what Core
/// Graphics returns for an ordinary display.
#[cfg(target_os = "macos")]
fn is_bgra(image: &core_graphics::image::CGImage) -> bool {
    use core_graphics::base::{
        kCGBitmapByteOrder32Little, kCGImageAlphaFirst, kCGImageAlphaNoneSkipFirst,
        kCGImageAlphaPremultipliedFirst,
    };
    use foreign_types::ForeignType;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        // core-graphics 0.22 has no wrapper for it.
        fn CGImageGetBitmapInfo(image: core_graphics::sys::CGImageRef) -> u32;
    }
    const ALPHA_INFO_MASK: u32 = 0x1F;
    const BYTE_ORDER_MASK: u32 = 0x7000;

    // SAFETY: a plain property read on a valid image.
    let info = unsafe { CGImageGetBitmapInfo(image.as_ptr()) };
    image.bits_per_pixel() == 32
        && image.bits_per_component() == 8
        && info & BYTE_ORDER_MASK == kCGBitmapByteOrder32Little
        && [kCGImageAlphaPremultipliedFirst, kCGImageAlphaFirst, kCGImageAlphaNoneSkipFirst]
            .contains(&(info & ALPHA_INFO_MASK))
}

#[cfg(not(target_os = "macos"))]
fn with_capture<R>(
    screen: &Screen,
    use_capture: impl FnOnce(&Capture) -> Result<R, Error>,
) -> Result<R, Error> {
    let image = screen.capture().map_err(|err| Error::UnableToCreateScreenshot {
        reason: err.to_string(),
    })?;
    use_capture(&Capture::from_png(image.buffer(), screen.display_info.scale_factor as f64)?)
}

/// The display the last capture used, so a capture that can't find the active
/// window stays on the same screen. 0 until the first capture.
static LAST_SCREEN_ID: AtomicU32 = AtomicU32::new(0);

/// The screen to capture: the one holding the active window.
///
/// Not the one under the mouse pointer, which is what capture used first:
/// Cmd-Tab to a window on another screen moves the focus without moving the
/// pointer, and the capture should follow what is being worked on.
///
/// The active window only chooses between screens, so not finding one is not a
/// reason to skip a capture. It happens while one of our own modal dialogs is
/// open, and for a moment when the focused window closes; the screen then is
/// the one the last capture used, or the main one.
///
/// Also returns the app and window in front, which the screenshot's row
/// records. It comes from the same lookup, so recording it costs nothing more
/// per capture than a bundle id read once per app.
async fn get_focused_screen() -> Result<(Screen, Option<FrontWindow>), Error> {
    let screens = Screen::all().map_err(|err| Error::UnableToCreateScreenshot {
        reason: err.to_string(),
    })?;
    let active = get_active_window().ok();
    let window = active.as_ref().map(|window| {
        (
            window.position.x as i32,
            window.position.y as i32,
            window.position.width as i32,
            window.position.height as i32,
        )
    });
    let rects: Vec<(u32, (i32, i32, u32, u32))> = screens
        .iter()
        .map(|screen| {
            let info = &screen.display_info;
            (info.id, (info.x, info.y, info.width, info.height))
        })
        .collect();
    let last = LAST_SCREEN_ID.load(Ordering::Relaxed);
    let chosen = choose_screen(window, &rects, (last != 0).then_some(last));

    let screen = match chosen.and_then(|id| screens.into_iter().find(|s| s.display_info.id == id)) {
        Some(screen) => screen,
        None => Screen::from_point(0, 0).map_err(|err| Error::UnableToCreateScreenshot {
            reason: err.to_string(),
        })?,
    };
    LAST_SCREEN_ID.store(screen.display_info.id, Ordering::Relaxed);
    Ok((screen, active.map(front_window)))
}

fn front_window(window: active_win_pos_rs::ActiveWindow) -> FrontWindow {
    let app_path = window.process_path.to_string_lossy().into_owned();
    FrontWindow {
        bundle_id: bundle_id(&app_path),
        app_name: window.app_name,
        app_path,
        title: window.title,
    }
}

/// The bundle identifier of the app bundle at `app_path`, read from its
/// Info.plist the first time that app is in front and remembered after that.
#[cfg(target_os = "macos")]
fn bundle_id(app_path: &str) -> Option<String> {
    use objc2_foundation::{NSBundle, NSString};
    use std::collections::HashMap;
    use std::sync::LazyLock;

    static BUNDLE_IDS: LazyLock<Mutex<HashMap<String, Option<String>>>> =
        LazyLock::new(Default::default);

    let mut known = BUNDLE_IDS.lock().ok()?;
    known
        .entry(app_path.to_string())
        .or_insert_with(|| {
            NSBundle::bundleWithPath(&NSString::from_str(app_path))
                .and_then(|bundle| bundle.bundleIdentifier())
                .map(|id| id.to_string())
        })
        .clone()
}

#[cfg(not(target_os = "macos"))]
fn bundle_id(_app_path: &str) -> Option<String> {
    None
}

/// Which of `screens` (id, rect) to capture: the one under the centre of the
/// active `window`, else the `last` one if it is still connected. `None`
/// means the main screen.
fn choose_screen(
    window: Option<(i32, i32, i32, i32)>,
    screens: &[(u32, (i32, i32, u32, u32))],
    last: Option<u32>,
) -> Option<u32> {
    let under_window = window.and_then(|window| {
        screens.iter().find(|(_, rect)| window_overlaps_screen(window, *rect))
    });
    under_window
        .or_else(|| screens.iter().find(|(id, _)| Some(*id) == last))
        .map(|(id, _)| *id)
}

fn window_overlaps_screen(window: (i32, i32, i32, i32), screen: (i32, i32, u32, u32)) -> bool {
    let (wx, wy, ww, wh) = window;
    let (sx, sy, sw, sh) = screen;

    // Check if window center is within screen bounds
    let window_center_x = wx + ww / 2;
    let window_center_y = wy + wh / 2;

    window_center_x >= sx
        && window_center_x < sx + sw as i32
        && window_center_y >= sy
        && window_center_y < sy + sh as i32
}

/// Size of a frame from an ordinary screen. Screens of other shapes are
/// letterboxed into it.
const FRAME_WIDTH: u32 = 1800;
const FRAME_HEIGHT: u32 = 1124;

/// A screen that would get fewer stored pixels per point than this in an
/// ordinary frame (a big monitor: 2560×1440 points gets 0.7) is stored bigger.
/// On test pages of such a screen OCR found about a quarter of the words; on a
/// laptop, which gets 1.14, three quarters.
const MIN_PIXELS_PER_POINT: f64 = 1.0;

/// How many pixels per point a big screen's frame gets, or as many as the
/// capture has if fewer. At 1.4, with OCR reading such frames in tiles (see
/// `ocr_tiles`), a 2560×1440-point screen's pages read about four fifths of
/// their words.
const BIG_SCREEN_PIXELS_PER_POINT: f64 = 1.4;

/// The size of the frame `capture` is stored in: `FRAME_WIDTH`×`FRAME_HEIGHT`,
/// or for a big screen the screen's own shape at `BIG_SCREEN_PIXELS_PER_POINT`,
/// without black bars (the converter letterboxes every frame into its 1800×1124
/// video, whatever its shape).
fn frame_size(capture: &Capture) -> (u32, u32) {
    let (width, height) = (capture.width as f64, capture.height as f64);
    let fit = (FRAME_WIDTH as f64 / width).min(FRAME_HEIGHT as f64 / height);
    let pixels_per_point = capture.pixels_per_point;
    // An unknown density (zero, negative, NaN) gets the ordinary frame.
    if !(pixels_per_point > 0.0 && pixels_per_point.is_finite()) || fit * pixels_per_point >= MIN_PIXELS_PER_POINT {
        return (FRAME_WIDTH, FRAME_HEIGHT);
    }
    let scale = (BIG_SCREEN_PIXELS_PER_POINT / pixels_per_point).min(1.0);
    // Even, as the video encoder's 4:2:0 chroma wants.
    let even = |length: f64| (((length * scale / 2.0).round() as u32) * 2).max(2);
    (even(width), even(height))
}

/// Fit a capture into a frame and write it to `file_path` as a PNG, unless the
/// frame is all black. Returns whether it was black (and so not written).
///
/// The PNG is written to a hidden temporary file beside `file_path` and renamed
/// into place, so an interrupted write never leaves a truncated `NNNNN.png`
/// for `next_filename`, the viewer and the converter to trip over.
fn store_frame(capture: &Capture, file_path: &str) -> Result<bool, Error> {
    let frame = fit_to_frame(capture, file_path)?;
    if is_image_all_black(&frame) {
        return Ok(true);
    }

    let write_error = |reason: String| Error::UnableToWriteScreenshot {
        path: file_path.to_string(),
        reason,
    };
    let final_path = std::path::Path::new(file_path);
    let file_name = final_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(Error::UnableToConvertScreenshotPathToString)?;
    // `.00042.png.tmp`: a dotfile the viewer hides, and not a `.png` the
    // converter or `next_filename` would count.
    let temp_path = final_path.with_file_name(format!(".{}.tmp", file_name));

    let written = frame
        .save_with_format(&temp_path, ImageFormat::Png)
        .map_err(|e| write_error(format!("Failed to write image: {}", e)))
        .and_then(|()| {
            std::fs::rename(&temp_path, final_path)
                .map_err(|e| write_error(format!("Failed to move image into place: {}", e)))
        });
    if written.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    written.map(|()| false)
}

thread_local! {
    // A resizer keeps scratch buffers between calls; the blocking pool reuses
    // its threads, so each keeps one.
    static RESIZER: std::cell::RefCell<Resizer> = std::cell::RefCell::new(Resizer::new());
}

/// Scale a capture to fit its `frame_size` keeping its aspect ratio, and
/// centre it on a black canvas of exactly that size.
fn fit_to_frame(capture: &Capture, file_path: &str) -> Result<RgbImage, Error> {
    let resize_error = |reason: String| Error::UnableToResizeScreenshot {
        path: file_path.to_string(),
        reason,
    };

    let (orig_width, orig_height) = (capture.width, capture.height);
    if orig_width == 0 || orig_height == 0 {
        return Err(resize_error("Captured image is empty".to_string()));
    }
    let row_len = orig_width as usize * 4;
    let bytes_per_row = capture.bytes_per_row;
    if bytes_per_row < row_len || bytes_per_row % 4 != 0 {
        return Err(resize_error(format!("Rows of {} bytes can't hold {} pixels", bytes_per_row, orig_width)));
    }
    // Core Graphics pads each row (to 3040 pixels for a 3024-pixel screen), so
    // the source is the whole padded buffer and the resize is told to read
    // only the image's own columns, rather than copying the rows together
    // first. A buffer whose last row stops short of the padding is padded out.
    let full_len = bytes_per_row * orig_height as usize;
    let padded: Cow<[u8]> = if capture.pixels.len() >= full_len {
        Cow::Borrowed(&capture.pixels[..full_len])
    } else if capture.pixels.len() >= full_len - (bytes_per_row - row_len) {
        let mut pixels = capture.pixels.to_vec();
        pixels.resize(full_len, 0);
        Cow::Owned(pixels)
    } else {
        return Err(resize_error("Captured image is shorter than its size".to_string()));
    };
    // The channel order doesn't matter to the resizer, so BGRA is resized as
    // it is and swapped once the frame is small. The alpha is opaque, so it is
    // resized as a plain fourth channel and dropped.
    let source = ImageRef::new((bytes_per_row / 4) as u32, orig_height, &padded, PixelType::U8x4)
        .map_err(|e| resize_error(format!("Failed to read image: {}", e)))?;

    let (frame_width, frame_height) = frame_size(capture);
    let scale = (frame_width as f64 / orig_width as f64).min(frame_height as f64 / orig_height as f64);
    let new_width = ((orig_width as f64 * scale) as u32).clamp(1, frame_width);
    let new_height = ((orig_height as f64 * scale) as u32).clamp(1, frame_height);
    let x_offset = (frame_width - new_width) / 2;
    let y_offset = (frame_height - new_height) / 2;

    // Resize straight into the centre of an opaque black canvas. Box filter, as
    // the ImageMagick version used: each output pixel is the average of the
    // source pixels it covers, which keeps text legible. It is also what keeps
    // the row padding out: `crop` bounds where output pixels are taken from,
    // but a wider filter (Lanczos3, say) would still reach into the padding
    // for the rightmost column.
    let mut canvas = image::RgbaImage::from_pixel(frame_width, frame_height, image::Rgba([0, 0, 0, 255]));
    let mut target = CroppedImageMut::new(&mut canvas, x_offset, y_offset, new_width, new_height)
        .map_err(|e| resize_error(format!("Failed to place image: {}", e)))?;
    let options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(FilterType::Box))
        .crop(0.0, 0.0, orig_width as f64, orig_height as f64)
        .use_alpha(false);
    RESIZER
        .with(|resizer| resizer.borrow_mut().resize(&source, &mut target, &options))
        .map_err(|e| resize_error(format!("Failed to resize image: {}", e)))?;

    if capture.bgra {
        for pixel in canvas.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }
    Ok(image::DynamicImage::ImageRgba8(canvas).into_rgb8())
}

/// Whether the frame is (almost) entirely black, which is what a capture of a
/// locked or sleeping screen looks like. Samples every 10th pixel in each
/// direction and compares the mean luminance, from 0.0 to 1.0, against 0.01.
fn is_image_all_black(image: &RgbImage) -> bool {
    let sample_size = 10;
    let mut total_brightness = 0.0;
    let mut pixel_count = 0u64;

    for y in (0..image.height()).step_by(sample_size) {
        for x in (0..image.width()).step_by(sample_size) {
            let [red, green, blue] = image.get_pixel(x, y).0;
            let brightness = 0.299 * red as f64 + 0.587 * green as f64 + 0.114 * blue as f64;
            total_brightness += brightness / 255.0;
            pixel_count += 1;
        }
    }

    pixel_count == 0 || total_brightness / (pixel_count as f64) < 0.01
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_photographer_new() {
        let temp_dir = TempDir::new().unwrap();
        let photographer = Photographer::new_in(temp_dir.path().to_path_buf());
        assert!(photographer.is_ok());

        let photographer = photographer.unwrap();
        assert!(!photographer.running.load(Ordering::SeqCst));
        assert_eq!(photographer.get_error_logs().len(), 0);
        // The database is created eagerly under the given root.
        assert!(temp_dir.path().join("screenshots.db").exists());
    }

    // `start` calls `tokio::spawn`, which panics outside a runtime, so this
    // needs a tokio test. What keeps the spawned capture loop from ever taking
    // a screenshot is that `stop()` clears the `running` flag *before* this
    // test reaches any yield point, so the loop's `while running` check fails
    // the first time it is polled. Adding an `.await` between `start()` and
    // `stop()` would let it capture the real screen — don't.
    #[tokio::test]
    async fn test_photographer_start_stop() {
        let temp_dir = TempDir::new().unwrap();
        let photographer = Photographer::new_in(temp_dir.path().to_path_buf()).unwrap();

        // Initially should not be running
        assert!(!photographer.running.load(Ordering::SeqCst));

        // Start the photographer
        let running_handle = photographer.start();
        assert!(running_handle.load(Ordering::SeqCst));

        // Stop the photographer
        photographer.stop();
        assert!(!photographer.running.load(Ordering::SeqCst));
    }

    #[test]
    fn test_photographer_error_logs() {
        let temp_dir = TempDir::new().unwrap();
        let photographer = Photographer::new_in(temp_dir.path().to_path_buf()).unwrap();

        // Initially should have no error logs
        assert_eq!(photographer.get_error_logs().len(), 0);

        // Add an error log manually (simulating what would happen during operation)
        {
            let mut logs = photographer.error_logs.lock().unwrap();
            logs.push(ErrorLogEntry {
                timestamp: Utc::now(),
                error_message: "Test error".to_string(),
            });
        }

        // Verify we can retrieve the error log
        let logs = photographer.get_error_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].error_message, "Test error");

        // Clear error logs
        photographer.clear_error_logs();
        assert_eq!(photographer.get_error_logs().len(), 0);
    }

    #[test]
    fn test_photographer_error_logs_limit() {
        let temp_dir = TempDir::new().unwrap();
        let photographer = Photographer::new_in(temp_dir.path().to_path_buf()).unwrap();

        // Add more than 10000 error logs
        {
            let mut logs = photographer.error_logs.lock().unwrap();
            for i in 0..10002 {
                logs.push(ErrorLogEntry {
                    timestamp: Utc::now(),
                    error_message: format!("Error {}", i),
                });

                // Simulate the limiting behavior from the actual code
                if logs.len() > 10000 {
                    logs.remove(0);
                }
            }
        }

        // Should be limited to 10000
        let logs = photographer.get_error_logs();
        assert_eq!(logs.len(), 10000);
        // First error should be "Error 2" (0 and 1 should have been removed)
        assert_eq!(logs[0].error_message, "Error 2");
    }

    #[test]
    fn test_create_day_dir_if_needed() {
        let temp_dir = TempDir::new().unwrap();
        let timelapse_root = temp_dir.path().to_path_buf();

        let result = Photographer::create_day_dir_if_needed(&timelapse_root);
        assert!(result.is_ok());

        let (day, day_dir) = result.unwrap();
        assert!(day_dir.exists());
        assert!(day_dir.is_dir());

        // Verify the directory name format (YYYY-MM-DD)
        let dir_name = day_dir.file_name().unwrap().to_str().unwrap();
        assert_eq!(dir_name, day);
        assert_eq!(dir_name.len(), 10); // YYYY-MM-DD is 10 characters
        assert_eq!(&dir_name[4..5], "-");
        assert_eq!(&dir_name[7..8], "-");
    }

    #[test]
    fn test_next_filename_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let day_dir = temp_dir.path().to_path_buf();

        let result = next_filename(&day_dir);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "00001.png");
    }

    #[test]
    fn test_next_filename_with_existing_files() {
        let temp_dir = TempDir::new().unwrap();
        let day_dir = temp_dir.path().to_path_buf();

        // Create some test files
        fs::write(day_dir.join("00001.png"), "test").unwrap();
        fs::write(day_dir.join("00002.png"), "test").unwrap();
        fs::write(day_dir.join("00003.jpg"), "test").unwrap();

        let result = next_filename(&day_dir);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "00004.png");
    }

    #[test]
    fn test_next_filename_with_gaps() {
        let temp_dir = TempDir::new().unwrap();
        let day_dir = temp_dir.path().to_path_buf();

        // Create files with gaps in numbering
        fs::write(day_dir.join("00001.png"), "test").unwrap();
        fs::write(day_dir.join("00005.png"), "test").unwrap();
        fs::write(day_dir.join("00010.png"), "test").unwrap();

        let result = next_filename(&day_dir);
        assert!(result.is_ok());
        // Should be max + 1 = 11
        assert_eq!(result.unwrap(), "00011.png");
    }

    #[test]
    fn test_next_filename_ignores_non_numeric() {
        let temp_dir = TempDir::new().unwrap();
        let day_dir = temp_dir.path().to_path_buf();

        // Create files with non-numeric names
        fs::write(day_dir.join("00001.png"), "test").unwrap();
        fs::write(day_dir.join("test.png"), "test").unwrap();
        fs::write(day_dir.join("image.jpg"), "test").unwrap();

        let result = next_filename(&day_dir);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "00002.png");
    }

    #[test]
    fn test_window_overlaps_screen_center_inside() {
        // Window centered at (100, 100) with size 50x50
        // Screen at (0, 0) with size 200x200
        let window = (75, 75, 50, 50);
        let screen = (0, 0, 200, 200);

        assert!(window_overlaps_screen(window, screen));
    }

    #[test]
    fn test_window_overlaps_screen_center_outside() {
        // Window centered at (300, 300) with size 50x50
        // Screen at (0, 0) with size 200x200
        let window = (275, 275, 50, 50);
        let screen = (0, 0, 200, 200);

        assert!(!window_overlaps_screen(window, screen));
    }

    #[test]
    fn test_window_overlaps_screen_edge_case() {
        // Window partially on screen, but center is inside
        let window = (175, 175, 50, 50);
        let screen = (0, 0, 200, 200);

        // Center is at (200, 200), which is exactly at the edge
        assert!(!window_overlaps_screen(window, screen));
    }

    #[test]
    fn test_window_overlaps_screen_multi_monitor() {
        // Simulate a second monitor at x=1920
        let window = (2000, 100, 100, 100);
        let screen1 = (0, 0, 1920, 1080);
        let screen2 = (1920, 0, 1920, 1080);

        assert!(!window_overlaps_screen(window, screen1));
        assert!(window_overlaps_screen(window, screen2));
    }

    #[test]
    fn captures_the_last_screen_when_there_is_no_active_window() {
        let screens = [(1, (0, 0, 1000, 800)), (2, (1000, 0, 1000, 800))];
        // The active window's centre picks the screen.
        assert_eq!(choose_screen(Some((1200, 100, 400, 300)), &screens, Some(1)), Some(2));
        // No active window (one of our dialogs is open): stay where we were.
        assert_eq!(choose_screen(None, &screens, Some(2)), Some(2));
        // A screen that has since been unplugged, or no capture yet: the main one.
        assert_eq!(choose_screen(None, &screens, Some(3)), None);
        assert_eq!(choose_screen(None, &screens, None), None);
    }

    #[test]
    fn backs_off_only_after_three_failures_in_a_row() {
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(2), Duration::from_secs(1));
        assert_eq!(retry_delay(3), Duration::from_secs(60));
        assert_eq!(retry_delay(10), Duration::from_secs(60));
    }

    #[test]
    fn test_error_display() {
        let error = Error::UnableToFindHomeDir;
        assert_eq!(error.to_string(), "Unable to find home dir");

        let error = Error::UnableToCreateScreenshot {
            reason: "test reason".to_string(),
        };
        assert_eq!(error.to_string(), "Unable to create screenshot because: test reason");

        let error = Error::UnableToResizeScreenshot {
            path: "/test/path".to_string(),
            reason: "resize failed".to_string(),
        };
        assert_eq!(error.to_string(), "Unable to resize screenshot /test/path because: resize failed");
    }

    #[test]
    fn test_error_log_entry_serialization() {
        let entry = ErrorLogEntry {
            timestamp: Utc::now(),
            error_message: "Test error message".to_string(),
        };

        // Test serialization
        let json = serde_json::to_string(&entry);
        assert!(json.is_ok());

        // Test deserialization
        let deserialized: Result<ErrorLogEntry, _> = serde_json::from_str(&json.unwrap());
        assert!(deserialized.is_ok());
        assert_eq!(deserialized.unwrap().error_message, "Test error message");
    }

    /// An RGBA capture of `image`, rows packed.
    fn capture_of(image: &RgbImage) -> Capture<'static> {
        let rgba = image::DynamicImage::ImageRgb8(image.clone()).into_rgba8();
        Capture {
            width: image.width(),
            height: image.height(),
            bytes_per_row: image.width() as usize * 4,
            pixels: Cow::Owned(rgba.into_raw()),
            bgra: false,
            pixels_per_point: 2.0,
        }
    }

    #[test]
    fn test_fit_to_frame_letterboxes_a_taller_screen() {
        // 1000×1000 scales to 1124×1124, centred with 338px black bars each side.
        let white = RgbImage::from_pixel(1000, 1000, image::Rgb([255, 255, 255]));
        let frame = fit_to_frame(&capture_of(&white), "test.png").unwrap();

        assert_eq!(frame.dimensions(), (FRAME_WIDTH, FRAME_HEIGHT));
        assert_eq!(frame.get_pixel(0, 562).0, [0, 0, 0]);
        assert_eq!(frame.get_pixel(337, 562).0, [0, 0, 0]);
        assert_eq!(frame.get_pixel(338, 562).0, [255, 255, 255]);
        assert_eq!(frame.get_pixel(900, 0).0, [255, 255, 255]);
        assert_eq!(frame.get_pixel(1461, 1123).0, [255, 255, 255]);
        assert_eq!(frame.get_pixel(1462, 562).0, [0, 0, 0]);
    }

    #[test]
    fn test_fit_to_frame_downscales_a_retina_capture() {
        let capture = RgbImage::from_pixel(3456, 2234, image::Rgb([10, 200, 30]));
        let frame = fit_to_frame(&capture_of(&capture), "test.png").unwrap();

        assert_eq!(frame.dimensions(), (FRAME_WIDTH, FRAME_HEIGHT));
        // 3456×2234 fits as 1738×1124, leaving 31px bars left and right.
        assert_eq!(frame.get_pixel(30, 500).0, [0, 0, 0]);
        assert_eq!(frame.get_pixel(31, 500).0, [10, 200, 30]);
        assert_eq!(frame.get_pixel(900, 500).0, [10, 200, 30]);
    }

    #[test]
    fn test_fit_to_frame_reads_padded_bgra_rows() {
        // Core Graphics' layout: blue, green, red, alpha, and rows that may
        // run past the image. Here each 900px row carries 16 bytes of padding,
        // filled with white so it would show if it leaked into the frame.
        let (width, height, bytes_per_row) = (900, 562, 900 * 4 + 16);
        let mut pixels = vec![255u8; bytes_per_row * height];
        for row in pixels.chunks_mut(bytes_per_row) {
            for pixel in row[..width * 4].chunks_mut(4) {
                pixel.copy_from_slice(&[30, 200, 10, 255]);
            }
        }
        let capture = Capture { width: width as u32, height: height as u32, bytes_per_row, pixels: Cow::Owned(pixels), bgra: true, pixels_per_point: 2.0 };

        let frame = fit_to_frame(&capture, "test.png").unwrap();

        // 900×562 scales by 2 to 1800×1124, filling the frame.
        assert_eq!(frame.get_pixel(0, 0).0, [10, 200, 30]);
        assert_eq!(frame.get_pixel(1799, 1123).0, [10, 200, 30]);
    }

    /// A BGRA capture the size of a 14" MacBook screen, its 3024-pixel rows
    /// padded to 3040 as Core Graphics pads them, the padding white.
    fn padded_retina_capture() -> (usize, Vec<u8>) {
        let (width, height, bytes_per_row) = (3024, 1964, 3040 * 4);
        let mut pixels = vec![255u8; bytes_per_row * height];
        for row in pixels.chunks_mut(bytes_per_row) {
            for pixel in row[..width * 4].chunks_mut(4) {
                pixel.copy_from_slice(&[30, 200, 10, 255]);
            }
        }
        (bytes_per_row, pixels)
    }

    #[test]
    fn test_fit_to_frame_keeps_row_padding_out_of_a_downscaled_frame() {
        let (bytes_per_row, pixels) = padded_retina_capture();
        let capture = Capture { width: 3024, height: 1964, bytes_per_row, pixels: Cow::Owned(pixels), bgra: true, pixels_per_point: 2.0 };

        let frame = fit_to_frame(&capture, "test.png").unwrap();

        // 3024×1964 fits as 1730×1124, from x = 35 to 1764.
        assert_eq!(frame.get_pixel(34, 562).0, [0, 0, 0]);
        assert_eq!(frame.get_pixel(35, 562).0, [10, 200, 30]);
        assert_eq!(frame.get_pixel(1764, 562).0, [10, 200, 30], "no padding in the last column");
        assert_eq!(frame.get_pixel(1765, 562).0, [0, 0, 0]);
    }

    #[test]
    fn test_fit_to_frame_accepts_a_last_row_without_padding() {
        let (bytes_per_row, mut pixels) = padded_retina_capture();
        pixels.truncate(pixels.len() - 16 * 4);
        let capture = Capture { width: 3024, height: 1964, bytes_per_row, pixels: Cow::Owned(pixels), bgra: true, pixels_per_point: 2.0 };

        let frame = fit_to_frame(&capture, "test.png").unwrap();

        assert_eq!(frame.get_pixel(1764, 1123).0, [10, 200, 30]);
    }

    #[test]
    fn test_fit_to_frame_rejects_rows_too_short_for_the_width() {
        for bytes_per_row in [399, 402] {
            let capture = Capture { width: 100, height: 100, bytes_per_row, pixels: Cow::Owned(vec![0; 402 * 100]), bgra: true, pixels_per_point: 2.0 };
            let result = fit_to_frame(&capture, "/test/path");
            assert!(matches!(result, Err(Error::UnableToResizeScreenshot { .. })), "{} bytes a row", bytes_per_row);
        }
    }

    #[test]
    fn test_fit_to_frame_rejects_a_short_buffer() {
        let capture = Capture { width: 100, height: 100, bytes_per_row: 400, pixels: Cow::Owned(vec![0; 399 * 100]), bgra: true, pixels_per_point: 2.0 };
        let result = fit_to_frame(&capture, "/test/path");
        assert!(matches!(result, Err(Error::UnableToResizeScreenshot { .. })));
    }

    /// Captures the main screen the way the Photographer does and fits it into
    /// a frame. Needs Screen Recording permission to see more than the
    /// wallpaper: `cargo test real_capture_fits -- --ignored --nocapture`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn test_real_capture_fits_a_frame() {
        let screen = Screen::from_point(0, 0).unwrap();
        let frame = with_capture(&screen, |capture| {
            println!(
                "{}×{}, {} bytes a row, bgra {}, borrowed {}",
                capture.width,
                capture.height,
                capture.bytes_per_row,
                capture.bgra,
                matches!(capture.pixels, Cow::Borrowed(_))
            );
            fit_to_frame(capture, "test.png")
        })
        .unwrap();
        assert_eq!(frame.dimensions(), (FRAME_WIDTH, FRAME_HEIGHT));
        // A locked or sleeping screen captures black.
        println!("all black: {}", is_image_all_black(&frame));
    }

    fn blank_capture(width: u32, height: u32, pixels_per_point: f64) -> Capture<'static> {
        Capture {
            width,
            height,
            bytes_per_row: width as usize * 4,
            pixels: Cow::Owned(vec![200; width as usize * height as usize * 4]),
            bgra: true,
            pixels_per_point,
        }
    }

    #[test]
    fn test_frame_size_grows_only_for_big_screens() {
        // A 14" MacBook (1512×982 points) keeps the ordinary frame: 1.14 px a point.
        assert_eq!(frame_size(&blank_capture(3024, 1964, 2.0)), (1800, 1124));
        // A 2560×1440-point screen at 2x would get 0.7 px a point; it gets 1.4,
        // in its own shape.
        assert_eq!(frame_size(&blank_capture(5120, 2880, 2.0)), (3584, 2016));
        // The same screen at 1x has only 1 px a point, so it is kept at that.
        assert_eq!(frame_size(&blank_capture(2560, 1440, 1.0)), (2560, 1440));
        // A 4K screen at "looks like 1920×1080" would get 0.94.
        assert_eq!(frame_size(&blank_capture(3840, 2160, 2.0)), (2688, 1512));
        // A 32:9 screen gets no black bars to pay for.
        assert_eq!(frame_size(&blank_capture(10240, 2880, 2.0)), (7168, 2016));
        // Unknown density: the ordinary frame.
        for unknown in [f64::NAN, 0.0, -1.0, f64::INFINITY] {
            assert_eq!(frame_size(&blank_capture(5120, 2880, unknown)), (1800, 1124));
        }
    }

    #[test]
    fn test_fit_to_frame_stores_a_big_screen_without_bars() {
        let frame = fit_to_frame(&blank_capture(2560, 1440, 1.0), "test.png").unwrap();

        assert_eq!(frame.dimensions(), (2560, 1440));
        assert_eq!(frame.get_pixel(0, 0).0, [200, 200, 200]);
        assert_eq!(frame.get_pixel(2559, 1439).0, [200, 200, 200]);
    }

    #[test]
    fn test_capture_from_png() {
        let image = RgbImage::from_pixel(3, 2, image::Rgb([1, 2, 3]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, ImageFormat::Png).unwrap();

        let capture = Capture::from_png(png.get_ref(), 2.0).unwrap();

        assert_eq!((capture.width, capture.height, capture.bytes_per_row), (3, 2, 12));
        assert_eq!(&capture.pixels[..4], &[1, 2, 3, 255]);
        assert!(!capture.bgra);
        assert!(Capture::from_png(b"not a png", 2.0).is_err());
    }

    #[test]
    fn test_is_image_all_black() {
        let black = RgbImage::new(FRAME_WIDTH, FRAME_HEIGHT);
        assert!(is_image_all_black(&black));

        // Near-black noise from a sleeping display still counts as black.
        let nearly_black = RgbImage::from_pixel(FRAME_WIDTH, FRAME_HEIGHT, image::Rgb([1, 1, 1]));
        assert!(is_image_all_black(&nearly_black));

        // A letterboxed dark-grey screen does not.
        let mut dim = RgbImage::new(FRAME_WIDTH, FRAME_HEIGHT);
        for (x, _, pixel) in dim.enumerate_pixels_mut() {
            if (300..1500).contains(&x) {
                *pixel = image::Rgb([20, 20, 20]);
            }
        }
        assert!(!is_image_all_black(&dim));
    }

    #[test]
    fn test_store_frame_writes_through_a_temporary_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("00001.png");
        let capture = RgbImage::from_pixel(3456, 2234, image::Rgb([200, 200, 200]));

        let is_black = store_frame(&capture_of(&capture), path.to_str().unwrap()).unwrap();

        assert!(!is_black);
        let written = image::open(&path).unwrap();
        assert_eq!((written.width(), written.height()), (FRAME_WIDTH, FRAME_HEIGHT));
        let names: Vec<String> = fs::read_dir(temp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["00001.png"], "no temporary file is left behind");
    }

    #[test]
    fn test_store_frame_drops_a_black_frame() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("00001.png");
        let capture = RgbImage::new(3456, 2234);

        assert!(store_frame(&capture_of(&capture), path.to_str().unwrap()).unwrap());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0, "nothing is written");
    }

    #[test]
    fn test_store_frame_cleans_up_after_a_failed_write() {
        let temp = TempDir::new().unwrap();
        // A directory where the PNG should go makes the rename fail.
        let path = temp.path().join("00001.png");
        fs::create_dir(&path).unwrap();
        let capture = RgbImage::from_pixel(100, 100, image::Rgb([200, 200, 200]));

        let result = store_frame(&capture_of(&capture), path.to_str().unwrap());

        assert!(matches!(result, Err(Error::UnableToWriteScreenshot { .. })));
        assert!(!temp.path().join(".00001.png.tmp").exists());
    }
}
