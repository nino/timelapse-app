use chrono::{Local, Utc};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::core::database::Database;
use crate::core::error::{Error, ErrorCode, Result};
use crate::core::image_processor::ImageProcessor;
use crate::core::screen_capture::ScreenCapture;
use crate::utils::constants::{
   BLACK_SCREEN_WAIT, BLACK_THRESHOLD, ERROR_WAIT, SCREENSHOT_INTERVAL, TARGET_HEIGHT, TARGET_WIDTH,
};
use crate::utils::paths;

pub struct Photographer {
   screen_capture: ScreenCapture,
   image_processor: ImageProcessor,
   database: Option<Database>,
   running: Arc<AtomicBool>,
   current_day_dir: Option<PathBuf>,
   current_date: Option<String>,
}

impl Photographer {
   pub fn new() -> Result<Self> {
      // Ensure directories exist
      paths::ensure_dir(&paths::timelapse_dir());
      paths::ensure_dir(&paths::cache_dir());

      // Initialize database
      let db_path = paths::database_path();
      let database = Database::new(&db_path).ok();

      Ok(Self {
         screen_capture: ScreenCapture::new(),
         image_processor: ImageProcessor::new(),
         database,
         running: Arc::new(AtomicBool::new(false)),
         current_day_dir: None,
         current_date: None,
      })
   }

   pub fn is_running(&self) -> bool {
      self.running.load(Ordering::SeqCst)
   }

   pub fn start(&mut self) {
      if self.running.swap(true, Ordering::SeqCst) {
         return; // Already running
      }

      let running = self.running.clone();

      // Run capture loop in current thread for simplicity
      // In a real app, you'd want this in a separate thread
      while running.load(Ordering::SeqCst) {
         match self.capture_screenshot() {
            Ok(Some(_)) => {
               thread::sleep(SCREENSHOT_INTERVAL);
            }
            Ok(None) => {
               // Black screen
               thread::sleep(BLACK_SCREEN_WAIT);
            }
            Err(e) => {
               eprintln!("Capture error: {}", e);
               thread::sleep(ERROR_WAIT);
            }
         }
      }
   }

   pub fn stop(&mut self) {
      self.running.store(false, Ordering::SeqCst);
   }

   /// Capture a single screenshot. Returns Ok(Some(path)) on success,
   /// Ok(None) if skipped (black screen), or Err on failure.
   pub fn capture_screenshot(&mut self) -> Result<Option<PathBuf>> {
      // Check if date changed
      let today = paths::current_date_string();
      if self.current_date.as_ref() != Some(&today) {
         self.current_date = Some(today);
         self.current_day_dir = None;
      }

      // Ensure day directory
      let day_dir = self.ensure_day_directory()?;

      // Capture screenshot
      let image = self.screen_capture.capture_focused()?;

      // Process image
      let processed = self
         .image_processor
         .resize_with_letterbox(&image, TARGET_WIDTH, TARGET_HEIGHT)?;

      // Check for black screen
      if self.image_processor.is_image_black(&processed, BLACK_THRESHOLD) {
         return Ok(None);
      }

      // Get next frame number
      let frame_number = self.get_next_frame_number(&day_dir)?;

      // Build filename
      let filename = format!("{}.png", paths::format_frame_number(frame_number));
      let filepath = day_dir.join(&filename);

      // Save image
      self.image_processor.save_image(&processed, &filepath)?;

      // Record in database
      if let Some(ref db) = self.database {
         let now_utc = Utc::now();
         let now_local = Local::now();
         if let Err(e) = db.insert_screenshot(frame_number, now_utc, now_local) {
            eprintln!("Database error: {}", e);
            // Continue anyway - screenshot was saved
         }
      }

      Ok(Some(filepath))
   }

   /// Get metadata for a specific frame
   pub fn get_screenshot_metadata(
      &self,
      frame_number: u32,
   ) -> Result<Option<crate::core::database::ScreenshotMetadata>> {
      match &self.database {
         Some(db) => db.get_screenshot_by_frame(frame_number),
         None => Ok(None),
      }
   }

   fn ensure_day_directory(&mut self) -> Result<PathBuf> {
      if let Some(ref dir) = self.current_day_dir {
         return Ok(dir.clone());
      }

      let date = self.current_date.as_ref().unwrap();
      let day_dir = paths::day_folder(date);

      if !paths::ensure_dir(&day_dir) {
         return Err(Error::new(
            ErrorCode::DirectoryCreationFailed,
            format!("Failed to create day directory: {:?}", day_dir),
         ));
      }

      self.current_day_dir = Some(day_dir.clone());
      Ok(day_dir)
   }

   fn get_next_frame_number(&self, day_dir: &PathBuf) -> Result<u32> {
      if !day_dir.exists() {
         return Ok(1);
      }

      let entries = fs::read_dir(day_dir).map_err(|e| {
         Error::new(
            ErrorCode::FileReadError,
            format!("Failed to read directory: {}", e),
         )
      })?;

      let mut max_frame: u32 = 0;

      for entry in entries.filter_map(|e| e.ok()) {
         let path = entry.path();
         if path.extension().map(|e| e == "png").unwrap_or(false) {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
               if let Ok(num) = stem.parse::<u32>() {
                  max_frame = max_frame.max(num);
               }
            }
         }
      }

      Ok(max_frame + 1)
   }
}

impl Default for Photographer {
   fn default() -> Self {
      Self::new().expect("Failed to create Photographer")
   }
}
