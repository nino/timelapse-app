use active_win_pos_rs::get_active_window;
use chrono::{DateTime, Utc, Local};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{ImageFormat, RgbImage};
use screenshots::Screen;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tokio::time::{sleep, Duration};
use crate::database::ScreenshotDatabase;

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
        })
    }

    pub fn start(&self) -> Arc<AtomicBool> {
        let running = Arc::clone(&self.running);
        running.store(true, Ordering::SeqCst);

        let timelapse_root_path = self.timelapse_root_path.clone();
        let running_clone = Arc::clone(&running);
        let error_logs_clone = Arc::clone(&self.error_logs);
        let db_clone = Arc::clone(&self.db);

        tokio::spawn(async move {
            println!("Starting timelapse background task...");

            while running_clone.load(Ordering::SeqCst) {
                match Self::do_screenshot(&timelapse_root_path, &db_clone).await {
                    Ok(is_black) => {
                        if is_black {
                            // Image was all black and deleted, wait 10 seconds
                            sleep(Duration::from_secs(10)).await;
                        } else {
                            // Normal screenshot, wait 1 second
                            sleep(Duration::from_secs(1)).await;
                        }
                    }
                    Err(error) => {
                        eprintln!("Screenshot error: {}", error);

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

                        sleep(Duration::from_secs(60)).await;
                    }
                }
            }

            println!("Timelapse background task stopped.");
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

    pub fn get_screenshot_metadata(&self, frame_number: u32) -> Result<Option<(String, String)>, Error> {
        if let Ok(db_guard) = self.db.lock() {
            Ok(db_guard.get_screenshot_by_frame(frame_number)?)
        } else {
            Err(Error::DatabaseError(rusqlite::Error::InvalidQuery))
        }
    }

    fn create_day_dir_if_needed(timelapse_root_path: &PathBuf) -> Result<PathBuf, Error> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let day_dir = timelapse_root_path.join(&today);
        std::fs::create_dir_all(&day_dir)?;
        Ok(day_dir)
    }

    async fn do_screenshot(
        timelapse_root_path: &PathBuf,
        db: &Arc<Mutex<ScreenshotDatabase>>,
    ) -> Result<bool, Error> {
        let day_dir = Self::create_day_dir_if_needed(timelapse_root_path)?;
        let filename = next_filename(&day_dir)?;
        let screenshot_path = String::from(
            day_dir
                .join(&filename)
                .to_str()
                .ok_or(Error::UnableToConvertScreenshotPathToString)?,
        );

        let image_data = capture_screenshot().await?;
        let frame = fit_to_frame(&image_data, &screenshot_path)?;

        // A black frame is dropped before it is written
        if is_image_all_black(&frame) {
            println!("Screenshot is all black, skipping: {}", screenshot_path);
            Ok(true) // Return true to indicate image was black and dropped
        } else {
            frame
                .save_with_format(&screenshot_path, ImageFormat::Png)
                .map_err(|e| Error::UnableToResizeScreenshot {
                    path: screenshot_path.clone(),
                    reason: format!("Failed to write image: {}", e),
                })?;

            // Extract frame number from filename (e.g., "00001.png" -> 1)
            let frame_number: u32 = filename
                .replace(".png", "")
                .parse()
                .unwrap_or(0);

            // Insert metadata into database with both UTC and local timestamps
            let created_at = Utc::now();
            let local_time = Local::now();
            if let Ok(db_guard) = db.lock() {
                db_guard.insert_screenshot(frame_number, created_at, local_time)?;
            }

            Ok(false) // Return false for normal screenshots
        }
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

async fn capture_screenshot() -> Result<Vec<u8>, Error> {
    // Get the focused screen by finding which screen contains the active window
    let focused_screen = get_focused_screen().await?;

    // Capture screenshot using system API
    let image = focused_screen
        .capture()
        .map_err(|err| Error::UnableToCreateScreenshot {
            reason: err.to_string(),
        })?;

    // The Image struct contains PNG data in its buffer
    // We need to save it as a file
    let buffer = image.buffer();
    // std::fs::write(screenshot_path, buffer).map_err(|_| Error::UnableToCreateScreenshot)?;

    // println!("Screenshot saved to {}", screenshot_path);
    Ok(buffer.clone())
}

async fn get_focused_screen() -> Result<Screen, Error> {
    // Get the active window to determine which screen is focused
    let active_window = get_active_window().map_err(|_| Error::UnableToCreateScreenshot {
        reason: "Can't get active window".to_owned(),
    })?;

    // Get all available screens
    let screens = Screen::all().map_err(|err| Error::UnableToCreateScreenshot {
        reason: err.to_string(),
    })?;

    // Find the screen that contains the active window
    for screen in screens {
        let screen_rect = (
            screen.display_info.x,
            screen.display_info.y,
            screen.display_info.width,
            screen.display_info.height,
        );
        // Convert window position from f64 to i32
        let window_rect = (
            active_window.position.x as i32,
            active_window.position.y as i32,
            active_window.position.width as i32,
            active_window.position.height as i32,
        );

        // Check if the window is primarily on this screen
        if window_overlaps_screen(window_rect, screen_rect) {
            return Ok(screen);
        }
    }

    // Fallback to primary screen if no overlap found
    Screen::from_point(0, 0).map_err(|err| Error::UnableToCreateScreenshot {
        reason: err.to_string(),
    })
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

/// Size of every stored frame. Screens of other shapes are letterboxed into it.
const FRAME_WIDTH: u32 = 1800;
const FRAME_HEIGHT: u32 = 1124;

/// Decode a captured PNG, scale it to fit `FRAME_WIDTH`×`FRAME_HEIGHT` keeping
/// its aspect ratio, and centre it on a black canvas of exactly that size.
fn fit_to_frame(data: &[u8], file_path: &str) -> Result<RgbImage, Error> {
    let resize_error = |reason: String| Error::UnableToResizeScreenshot {
        path: file_path.to_string(),
        reason,
    };

    let source = image::load_from_memory_with_format(data, ImageFormat::Png)
        .map_err(|e| resize_error(format!("Failed to read image: {}", e)))?
        .into_rgb8();
    let (orig_width, orig_height) = source.dimensions();
    if orig_width == 0 || orig_height == 0 {
        return Err(resize_error("Captured image is empty".to_string()));
    }

    let scale = (FRAME_WIDTH as f64 / orig_width as f64).min(FRAME_HEIGHT as f64 / orig_height as f64);
    let new_width = ((orig_width as f64 * scale) as u32).clamp(1, FRAME_WIDTH);
    let new_height = ((orig_height as f64 * scale) as u32).clamp(1, FRAME_HEIGHT);

    // Box filter, as the ImageMagick version used: each output pixel is the
    // average of the source pixels it covers, which keeps text legible.
    let mut resized = RgbImage::new(new_width, new_height);
    Resizer::new()
        .resize(
            &source,
            &mut resized,
            &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Box)),
        )
        .map_err(|e| resize_error(format!("Failed to resize image: {}", e)))?;

    let mut canvas = RgbImage::new(FRAME_WIDTH, FRAME_HEIGHT);
    let x_offset = (FRAME_WIDTH - new_width) / 2;
    let y_offset = (FRAME_HEIGHT - new_height) / 2;
    image::imageops::replace(&mut canvas, &resized, x_offset as i64, y_offset as i64);
    Ok(canvas)
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

        let day_dir = result.unwrap();
        assert!(day_dir.exists());
        assert!(day_dir.is_dir());

        // Verify the directory name format (YYYY-MM-DD)
        let dir_name = day_dir.file_name().unwrap().to_str().unwrap();
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

    fn png_of(image: &RgbImage) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn test_fit_to_frame_letterboxes_a_taller_screen() {
        // 1000×1000 scales to 1124×1124, centred with 338px black bars each side.
        let white = RgbImage::from_pixel(1000, 1000, image::Rgb([255, 255, 255]));
        let frame = fit_to_frame(&png_of(&white), "test.png").unwrap();

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
        let frame = fit_to_frame(&png_of(&capture), "test.png").unwrap();

        assert_eq!(frame.dimensions(), (FRAME_WIDTH, FRAME_HEIGHT));
        // 3456×2234 fits as 1738×1124, leaving 31px bars left and right.
        assert_eq!(frame.get_pixel(30, 500).0, [0, 0, 0]);
        assert_eq!(frame.get_pixel(31, 500).0, [10, 200, 30]);
        assert_eq!(frame.get_pixel(900, 500).0, [10, 200, 30]);
    }

    #[test]
    fn test_fit_to_frame_rejects_garbage() {
        let result = fit_to_frame(b"not a png", "/test/path");
        assert!(matches!(result, Err(Error::UnableToResizeScreenshot { .. })));
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
}
