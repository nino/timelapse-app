use std::fs;
use std::path::PathBuf;

use crate::utils::constants::{CACHE_DIR, DATABASE_NAME, FRAME_NUMBER_DIGITS, TIMELAPSE_DIR};

/// Get the user's home directory
pub fn home_dir() -> PathBuf {
   dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Get the timelapse directory path
pub fn timelapse_dir() -> PathBuf {
   home_dir().join(TIMELAPSE_DIR)
}

/// Get the cache directory path
pub fn cache_dir() -> PathBuf {
   timelapse_dir().join(CACHE_DIR)
}

/// Get the database file path
pub fn database_path() -> PathBuf {
   cache_dir().join(DATABASE_NAME)
}

/// Get the day folder path for a given date string
pub fn day_folder(date: &str) -> PathBuf {
   timelapse_dir().join(date)
}

/// Get the current date as a string (YYYY-MM-DD)
pub fn current_date_string() -> String {
   chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Format a frame number with leading zeros
pub fn format_frame_number(number: u32) -> String {
   format!("{:0width$}", number, width = FRAME_NUMBER_DIGITS)
}

/// Ensure a directory exists, creating it if necessary
pub fn ensure_dir(path: &PathBuf) -> bool {
   if path.exists() {
      return true;
   }
   fs::create_dir_all(path).is_ok()
}
