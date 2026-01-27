use std::time::Duration;

/// Target width for processed screenshots
pub const TARGET_WIDTH: u32 = 1800;

/// Target height for processed screenshots
pub const TARGET_HEIGHT: u32 = 1124;

/// Threshold for black screen detection (0.0 - 1.0)
pub const BLACK_THRESHOLD: f64 = 0.01;

/// Interval between screenshots
pub const SCREENSHOT_INTERVAL: Duration = Duration::from_secs(1);

/// Wait time after error
pub const ERROR_WAIT: Duration = Duration::from_secs(5);

/// Wait time when screen is black
pub const BLACK_SCREEN_WAIT: Duration = Duration::from_secs(30);

/// Number of digits in frame number
pub const FRAME_NUMBER_DIGITS: usize = 5;

/// Sampling step for brightness calculation
pub const SAMPLE_STEP: u32 = 10;

/// Name of the timelapse directory
pub const TIMELAPSE_DIR: &str = "timelapse";

/// Name of the cache subdirectory
pub const CACHE_DIR: &str = ".cache";

/// Name of the database file
pub const DATABASE_NAME: &str = "timelapse.db";

/// Default FPS for video frame extraction
pub const DEFAULT_EXTRACTION_FPS: u32 = 30;
