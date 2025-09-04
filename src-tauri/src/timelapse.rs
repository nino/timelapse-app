use active_win_pos_rs::get_active_window;
use chrono::{DateTime, Utc};
use magick_rust::{magick_wand_genesis, MagickWand};
use screenshots::Screen;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    sync::Once,
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tokio::time::{sleep, Duration};

// Ensure MagickWand is initialized only once
static MAGICK_WAND_GENESIS: Once = Once::new();

fn init_magick_wand() {
    MAGICK_WAND_GENESIS.call_once(|| {
        magick_wand_genesis();
    });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorLogEntry {
    pub timestamp: DateTime<Utc>,
    pub error_message: String,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Unable to find home dir")]
    UnableToFindHomeDir,

    #[error("Unable to create screenshot")]
    UnableToCreateScreenshot,

    #[error("Unable to resize screenshot {path} because: {reason}")]
    UnableToResizeScreenshot { path: String, reason: String },

    #[error("Unable convert screenshot path to string")]
    UnableToConvertScreenshotPathToString,

    #[error("Unable to check if image is black: {reason}")]
    UnableToCheckIfImageIsBlack { reason: String },

    #[error("IO Error")]
    IoError(#[from] std::io::Error),
}

pub struct Photographer {
    timelapse_root_path: PathBuf,
    running: Arc<AtomicBool>,
    error_logs: Arc<Mutex<Vec<ErrorLogEntry>>>,
}

impl Photographer {
    pub fn new() -> Result<Photographer, Error> {
        // Initialize MagickWand
        init_magick_wand();

        Ok(Photographer {
            timelapse_root_path: dirs::home_dir()
                .ok_or(Error::UnableToFindHomeDir)?
                .join("Timelapse"),
            running: Arc::new(AtomicBool::new(false)),
            error_logs: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub fn start(&self) -> Arc<AtomicBool> {
        let running = Arc::clone(&self.running);
        running.store(true, Ordering::SeqCst);

        let timelapse_root_path = self.timelapse_root_path.clone();
        let running_clone = Arc::clone(&running);
        let error_logs_clone = Arc::clone(&self.error_logs);

        tokio::spawn(async move {
            println!("Starting timelapse background task...");

            while running_clone.load(Ordering::SeqCst) {
                match Self::do_screenshot(&timelapse_root_path).await {
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

    fn create_day_dir_if_needed(timelapse_root_path: &PathBuf) -> Result<PathBuf, Error> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let day_dir = timelapse_root_path.join(&today);
        std::fs::create_dir_all(&day_dir)?;
        Ok(day_dir)
    }

    async fn do_screenshot(timelapse_root_path: &PathBuf) -> Result<bool, Error> {
        let day_dir = Self::create_day_dir_if_needed(timelapse_root_path)?;
        let screenshot_path = String::from(
            day_dir
                .join(next_filename(&day_dir)?)
                .to_str()
                .ok_or(Error::UnableToConvertScreenshotPathToString)?,
        );

        capture_screenshot(&screenshot_path).await?;
        resize_screenshot(&screenshot_path).await?;

        // Check if the image is all black
        if is_image_all_black(&screenshot_path).await? {
            println!("Screenshot is all black, deleting: {}", screenshot_path);
            std::fs::remove_file(&screenshot_path)?;
            Ok(true) // Return true to indicate image was black and deleted
        } else {
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

async fn capture_screenshot(screenshot_path: &str) -> Result<(), Error> {
    // Get the focused screen by finding which screen contains the active window
    let focused_screen = get_focused_screen().await?;

    // Capture screenshot using system API
    let image = focused_screen
        .capture()
        .map_err(|_| Error::UnableToCreateScreenshot)?;

    // The Image struct contains PNG data in its buffer
    // We need to save it as a file
    let buffer = image.buffer();
    std::fs::write(screenshot_path, buffer).map_err(|_| Error::UnableToCreateScreenshot)?;

    println!("Screenshot saved to {}", screenshot_path);
    Ok(())
}

async fn get_focused_screen() -> Result<Screen, Error> {
    // Get the active window to determine which screen is focused
    let active_window = get_active_window().map_err(|_| Error::UnableToCreateScreenshot)?;

    // Get all available screens
    let screens = Screen::all().map_err(|_| Error::UnableToCreateScreenshot)?;

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
    Screen::from_point(0, 0).map_err(|_| Error::UnableToCreateScreenshot)
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

async fn resize_screenshot(file_path: &str) -> Result<(), Error> {
    let wand = MagickWand::new();

    // Read the image
    wand.read_image(file_path)
        .map_err(|e| Error::UnableToResizeScreenshot {
            path: file_path.to_string(),
            reason: format!("Failed to read image: {:?}", e),
        })?;

    // Get original dimensions
    let orig_width = wand.get_image_width() as f64;
    let orig_height = wand.get_image_height() as f64;

    // Target dimensions
    let target_width = 1800.0;
    let target_height = 1124.0;

    // Calculate scaling to fit within target dimensions while maintaining aspect ratio
    let scale_x = target_width / orig_width;
    let scale_y = target_height / orig_height;
    let scale = scale_x.min(scale_y);

    // Calculate new dimensions
    let new_width = (orig_width * scale) as usize;
    let new_height = (orig_height * scale) as usize;

    // Resize the image maintaining aspect ratio
    wand.resize_image(new_width, new_height, magick_rust::FilterType::Lanczos)
        .map_err(|e| Error::UnableToResizeScreenshot {
            path: file_path.to_string(),
            reason: format!("Failed to resize image: {:?}", e),
        })?;

    // Create a new black canvas of target size
    let canvas = MagickWand::new();
    canvas
        .new_image(
            target_width as usize,
            target_height as usize,
            &magick_rust::PixelWand::new(),
        )
        .map_err(|e| Error::UnableToResizeScreenshot {
            path: file_path.to_string(),
            reason: format!("Failed to create canvas: {:?}", e),
        })?;

    // Calculate position to center the resized image
    let x_offset = ((target_width - new_width as f64) / 2.0) as isize;
    let y_offset = ((target_height - new_height as f64) / 2.0) as isize;

    // Composite the resized image onto the black canvas
    canvas
        .compose_images(
            &wand,
            magick_rust::CompositeOperator::Over,
            true,
            x_offset,
            y_offset,
        )
        .map_err(|e| Error::UnableToResizeScreenshot {
            path: file_path.to_string(),
            reason: format!("Failed to composite image: {:?}", e),
        })?;

    // Write the final image back
    canvas
        .write_image(file_path)
        .map_err(|e| Error::UnableToResizeScreenshot {
            path: file_path.to_string(),
            reason: format!("Failed to write image: {:?}", e),
        })?;

    Ok(())
}

async fn is_image_all_black(file_path: &str) -> Result<bool, Error> {
    let wand = MagickWand::new();

    // Read the image
    wand.read_image(file_path)
        .map_err(|e| Error::UnableToCheckIfImageIsBlack {
            reason: format!("Failed to read image: {:?}", e),
        })?;

    let width = wand.get_image_width();
    let height = wand.get_image_height();

    // Sample pixels to check if image is all black
    // We'll check a grid of pixels across the image
    let sample_size = 10; // Check every 10th pixel
    let mut total_brightness = 0.0;
    let mut pixel_count = 0;

    for y in (0..height).step_by(sample_size) {
        for x in (0..width).step_by(sample_size) {
            match wand.get_image_pixel_color(x as isize, y as isize) {
                Some(pixel) => {
                    // Get RGB values and calculate brightness
                    let red = pixel.get_red();
                    let green = pixel.get_green();
                    let blue = pixel.get_blue();

                    // Calculate luminance (brightness)
                    let brightness = 0.299 * red + 0.587 * green + 0.114 * blue;
                    total_brightness += brightness;
                    pixel_count += 1;
                }
                None => {
                    // Skip pixels that can't be read
                    continue;
                }
            }
        }
    }

    if pixel_count == 0 {
        return Err(Error::UnableToCheckIfImageIsBlack {
            reason: "No pixels could be sampled".to_string(),
        });
    }

    let mean_brightness = total_brightness / pixel_count as f64;

    // Consider image "all black" if mean brightness is very close to 0
    // Using a small threshold to account for potential compression artifacts
    // PixelWand values are typically in the range 0.0 to 1.0
    Ok(mean_brightness < 0.01)
}
