use image::{imageops, GenericImageView, ImageBuffer, Rgba, RgbaImage};
use std::path::Path;

use crate::core::error::{Error, ErrorCode, Result};
use crate::utils::constants::SAMPLE_STEP;

pub struct ImageProcessor;

impl ImageProcessor {
   pub fn new() -> Self {
      Self
   }

   /// Resize an image with letterboxing to maintain aspect ratio
   pub fn resize_with_letterbox(
      &self,
      source: &RgbaImage,
      target_width: u32,
      target_height: u32,
   ) -> Result<RgbaImage> {
      if source.width() == 0 || source.height() == 0 {
         return Err(Error::new(ErrorCode::ImageProcessingFailed, "Source image is empty"));
      }

      // Create black background
      let mut result: RgbaImage = ImageBuffer::from_pixel(
         target_width,
         target_height,
         Rgba([0, 0, 0, 255]),
      );

      // Calculate scaling
      let source_ratio = source.width() as f64 / source.height() as f64;
      let target_ratio = target_width as f64 / target_height as f64;

      let (scaled_width, scaled_height) = if source_ratio > target_ratio {
         // Source is wider - fit to width
         let w = target_width;
         let h = (target_width as f64 / source_ratio) as u32;
         (w, h)
      } else {
         // Source is taller - fit to height
         let h = target_height;
         let w = (target_height as f64 * source_ratio) as u32;
         (w, h)
      };

      // Scale the source image
      let scaled = imageops::resize(
         source,
         scaled_width,
         scaled_height,
         imageops::FilterType::Lanczos3,
      );

      // Calculate position to center
      let x = (target_width - scaled_width) / 2;
      let y = (target_height - scaled_height) / 2;

      // Copy scaled image onto result
      imageops::overlay(&mut result, &scaled, x as i64, y as i64);

      Ok(result)
   }

   /// Check if an image is mostly black
   pub fn is_image_black(&self, image: &RgbaImage, threshold: f64) -> bool {
      let brightness = self.calculate_mean_brightness(image);
      brightness < threshold
   }

   /// Calculate the mean brightness of an image by sampling pixels
   fn calculate_mean_brightness(&self, image: &RgbaImage) -> f64 {
      if image.width() == 0 || image.height() == 0 {
         return 0.0;
      }

      let mut total_brightness = 0.0;
      let mut sample_count = 0u64;

      for y in (0..image.height()).step_by(SAMPLE_STEP as usize) {
         for x in (0..image.width()).step_by(SAMPLE_STEP as usize) {
            let pixel = image.get_pixel(x, y);

            // Calculate luminance using standard weights
            let luminance = 0.299 * pixel[0] as f64
               + 0.587 * pixel[1] as f64
               + 0.114 * pixel[2] as f64;

            total_brightness += luminance / 255.0;
            sample_count += 1;
         }
      }

      if sample_count == 0 {
         return 0.0;
      }

      total_brightness / sample_count as f64
   }

   /// Save an image to a file as PNG
   pub fn save_image(&self, image: &RgbaImage, path: &Path) -> Result<()> {
      image.save(path).map_err(|e| {
         Error::new(
            ErrorCode::FileWriteError,
            format!("Failed to save image: {}", e),
         )
      })
   }
}

impl Default for ImageProcessor {
   fn default() -> Self {
      Self::new()
   }
}
