use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
};
use thiserror::Error;
use tokio::time::{sleep, Duration};

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
}

impl Photographer {
    pub fn new() -> Result<Photographer, Error> {
        Ok(Photographer {
            timelapse_root_path: dirs::home_dir()
                .ok_or(Error::UnableToFindHomeDir)?
                .join("Timelapse"),
            running: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn start(&self) -> Arc<AtomicBool> {
        let running = Arc::clone(&self.running);
        running.store(true, Ordering::SeqCst);

        let timelapse_root_path = self.timelapse_root_path.clone();
        let running_clone = Arc::clone(&running);

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
                    Err(reason) => {
                        eprintln!("Screenshot error: {}", reason);
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
        .filter_map(|filename| filename.replace(".jpg", "").parse::<i32>().ok())
        .max()
        .unwrap_or(0);

    Ok(format!("{:05}.jpg", max + 1))
}

async fn capture_screenshot(screenshot_path: &str) -> Result<(), Error> {
    let output = tokio::process::Command::new("screencapture")
        .args(&["-x", "-t", "jpg", "-m", screenshot_path])
        .output()
        .await?;

    if output.status.success() {
        println!("Screenshot saved to {}", screenshot_path);
        Ok(())
    } else {
        println!("Unable to save screenshot");
        Err(Error::UnableToCreateScreenshot)
    }
}

async fn resize_screenshot(file_path: &str) -> Result<(), Error> {
    let output = tokio::process::Command::new("convert")
        .args(&["-scale", "1800x1124!", file_path, file_path])
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Error::UnableToResizeScreenshot {
            path: file_path.to_string(),
            reason: stderr.to_string(),
        })
    }
}

async fn is_image_all_black(file_path: &str) -> Result<bool, Error> {
    // Use ImageMagick's identify command to get mean pixel value
    // For a completely black image, the mean should be 0
    let output = tokio::process::Command::new("identify")
        .args(&["-format", "%[mean]", file_path])
        .output()
        .await?;

    if output.status.success() {
        let mean_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let mean_value: f64 = mean_str
            .parse()
            .map_err(|e| Error::UnableToCheckIfImageIsBlack {
                reason: format!("Failed to parse mean value '{}': {}", mean_str, e),
            })?;

        // Consider image "all black" if mean pixel value is very close to 0
        // Using a small threshold to account for potential compression artifacts
        Ok(mean_value < 1.0)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Error::UnableToCheckIfImageIsBlack {
            reason: stderr.to_string(),
        })
    }
}
