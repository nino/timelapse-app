use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::core::error::{Error, ErrorCode, Result};
use crate::utils::constants::DEFAULT_EXTRACTION_FPS;
use crate::utils::paths;

pub struct VideoExtractor {
   extracting: bool,
}

impl VideoExtractor {
   pub fn new() -> Self {
      Self { extracting: false }
   }

   /// Extract frames from a video file
   pub fn extract_frames(&mut self, video_path: &Path, fps: Option<u32>) -> Result<String> {
      if self.extracting {
         return Err(Error::new(
            ErrorCode::ExtractionInProgress,
            "Extraction already in progress",
         ));
      }

      let fps = fps.unwrap_or(DEFAULT_EXTRACTION_FPS);

      // Find ffmpeg
      let ffmpeg_path = self.find_ffmpeg()?;

      // Get video filename for cache folder
      let video_filename = video_path
         .file_name()
         .and_then(|n| n.to_str())
         .unwrap_or("unknown");

      // Check if already cached
      if self.has_cached_frames(video_filename) {
         return Ok(self.generate_cache_folder_name(video_filename));
      }

      // Create cache folder
      let cache_folder_name = self.generate_cache_folder_name(video_filename);
      let cache_folder_path = paths::cache_dir().join(&cache_folder_name);

      if !paths::ensure_dir(&cache_folder_path) {
         return Err(Error::new(
            ErrorCode::CacheCreationFailed,
            format!("Failed to create cache folder: {:?}", cache_folder_path),
         ));
      }

      // Build output pattern
      let output_pattern = cache_folder_path.join("%05d.jpg");

      self.extracting = true;

      // Run ffmpeg
      let result = Command::new(&ffmpeg_path)
         .args([
            "-i",
            video_path.to_str().unwrap_or(""),
            "-vf",
            &format!("fps={}", fps),
            "-q:v",
            "2",
            output_pattern.to_str().unwrap_or(""),
         ])
         .output();

      self.extracting = false;

      match result {
         Ok(output) => {
            if output.status.success() {
               Ok(cache_folder_name)
            } else {
               let stderr = String::from_utf8_lossy(&output.stderr);
               Err(Error::new(
                  ErrorCode::FfmpegExitError,
                  format!(
                     "ffmpeg exited with code {:?}: {}",
                     output.status.code(),
                     stderr
                  ),
               ))
            }
         }
         Err(e) => Err(Error::new(
            ErrorCode::FfmpegStartFailed,
            format!("Failed to run ffmpeg: {}", e),
         )),
      }
   }

   /// Check if frames are already cached for a video
   pub fn has_cached_frames(&self, video_filename: &str) -> bool {
      let cache_path = self.get_cache_folder_path(video_filename);
      if !cache_path.exists() {
         return false;
      }

      // Check for any jpg files
      fs::read_dir(&cache_path)
         .map(|entries| {
            entries.filter_map(|e| e.ok()).any(|e| {
               e.path()
                  .extension()
                  .map(|ext| ext == "jpg")
                  .unwrap_or(false)
            })
         })
         .unwrap_or(false)
   }

   /// Get the cache folder path for a video
   pub fn get_cache_folder_path(&self, video_filename: &str) -> PathBuf {
      paths::cache_dir().join(self.generate_cache_folder_name(video_filename))
   }

   /// Check if extraction is in progress
   pub fn is_extracting(&self) -> bool {
      self.extracting
   }

   /// Find ffmpeg executable
   fn find_ffmpeg(&self) -> Result<PathBuf> {
      // Check common locations
      let search_paths = [
         "/opt/homebrew/bin/ffmpeg",
         "/usr/local/bin/ffmpeg",
         "/usr/bin/ffmpeg",
      ];

      for path in search_paths {
         let p = PathBuf::from(path);
         if p.exists() {
            return Ok(p);
         }
      }

      // Try to find in PATH
      if let Ok(output) = Command::new("which").arg("ffmpeg").output() {
         if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
               return Ok(PathBuf::from(path));
            }
         }
      }

      Err(Error::new(
         ErrorCode::FfmpegNotFound,
         "ffmpeg not found. Please install ffmpeg.",
      ))
   }

   /// Generate a cache folder name from the video filename
   fn generate_cache_folder_name(&self, video_filename: &str) -> String {
      use std::collections::hash_map::DefaultHasher;
      use std::hash::{Hash, Hasher};

      let mut hasher = DefaultHasher::new();
      video_filename.hash(&mut hasher);
      let hash = hasher.finish();

      // Sanitize filename
      let safe_name: String = video_filename
         .chars()
         .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '_' || c == '-' {
               c
            } else {
               '_'
            }
         })
         .collect();

      format!("{:08x}_{}", hash as u32, safe_name)
   }
}

impl Default for VideoExtractor {
   fn default() -> Self {
      Self::new()
   }
}
