use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, msg_send_id, ClassType};
use objc2_app_kit::{NSScreen, NSWindow};
use objc2_core_graphics::{
   CGDirectDisplayID, CGDisplayBounds, CGMainDisplayID, CGRect, CGWindowListCopyWindowInfo,
   CGWindowListOption,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

use crate::core::error::{Error, ErrorCode, Result};

#[derive(Debug, Clone)]
pub struct ScreenInfo {
   pub id: u32,
   pub x: f64,
   pub y: f64,
   pub width: f64,
   pub height: f64,
   pub name: String,
   pub is_primary: bool,
}

impl ScreenInfo {
   pub fn contains_point(&self, x: f64, y: f64) -> bool {
      x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
   }
}

pub struct ScreenCapture;

impl ScreenCapture {
   pub fn new() -> Self {
      Self
   }

   /// Get list of all available screens
   pub fn screens(&self) -> Vec<ScreenInfo> {
      let mut result = Vec::new();

      unsafe {
         let screens = NSScreen::screens();
         let main_screen = NSScreen::mainScreen();
         let main_frame = main_screen.as_ref().map(|s| s.frame());

         for (id, screen) in screens.iter().enumerate() {
            let frame = screen.frame();
            let name = screen
               .localizedName()
               .to_string();

            let is_primary = main_frame
               .map(|mf| {
                  (frame.origin.x - mf.origin.x).abs() < 1.0
                     && (frame.origin.y - mf.origin.y).abs() < 1.0
               })
               .unwrap_or(false);

            result.push(ScreenInfo {
               id: id as u32,
               x: frame.origin.x,
               y: frame.origin.y,
               width: frame.size.width,
               height: frame.size.height,
               name,
               is_primary,
            });
         }
      }

      result
   }

   /// Get the screen containing the focused window
   pub fn focused_screen(&self) -> Result<ScreenInfo> {
      let screens = self.screens();
      if screens.is_empty() {
         return Err(Error::new(ErrorCode::NoScreensFound, "No screens found"));
      }

      // Try to find the screen with the focused window
      unsafe {
         // Get info about all windows
         let window_list = CGWindowListCopyWindowInfo(
            CGWindowListOption::kCGWindowListOptionOnScreenOnly
               | CGWindowListOption::kCGWindowListExcludeDesktopElements,
            0,
         );

         if let Some(windows) = window_list {
            let windows: Retained<NSArray<NSDictionary<NSString, AnyObject>>> =
               Retained::cast(windows);

            // Find the frontmost window (first in list is typically frontmost)
            for window_info in windows.iter() {
               // Get window bounds
               let bounds_key = NSString::from_str("kCGWindowBounds");
               if let Some(bounds_dict) = window_info.objectForKey(&bounds_key) {
                  let bounds_dict: &NSDictionary<NSString, NSNumber> =
                     unsafe { &*(bounds_dict as *const _ as *const _) };

                  let x_key = NSString::from_str("X");
                  let y_key = NSString::from_str("Y");
                  let w_key = NSString::from_str("Width");
                  let h_key = NSString::from_str("Height");

                  if let (Some(x), Some(y), Some(w), Some(h)) = (
                     bounds_dict.objectForKey(&x_key),
                     bounds_dict.objectForKey(&y_key),
                     bounds_dict.objectForKey(&w_key),
                     bounds_dict.objectForKey(&h_key),
                  ) {
                     let x: f64 = x.as_f64();
                     let y: f64 = y.as_f64();
                     let w: f64 = w.as_f64();
                     let h: f64 = h.as_f64();

                     // Calculate center point
                     let center_x = x + w / 2.0;
                     let center_y = y + h / 2.0;

                     // Find which screen contains this point
                     for screen in &screens {
                        if screen.contains_point(center_x, center_y) {
                           return Ok(screen.clone());
                        }
                     }
                  }
               }
               break; // Only check the first (frontmost) window
            }
         }
      }

      // Fall back to primary screen
      self.primary_screen()
   }

   /// Get the primary screen
   pub fn primary_screen(&self) -> Result<ScreenInfo> {
      let screens = self.screens();

      // First try to find the one marked as primary
      for screen in &screens {
         if screen.is_primary {
            return Ok(screen.clone());
         }
      }

      // Otherwise return the first one
      screens
         .into_iter()
         .next()
         .ok_or_else(|| Error::new(ErrorCode::NoScreensFound, "No screens found"))
   }

   /// Capture a screenshot of the specified screen
   pub fn capture(&self, screen: &ScreenInfo) -> Result<image::RgbaImage> {
      unsafe {
         let screens = NSScreen::screens();
         let ns_screen = screens
            .get(screen.id as usize)
            .ok_or_else(|| Error::new(ErrorCode::ScreenNotFound, "Screen not found"))?;

         // Get the display ID for this screen
         let device_desc = ns_screen.deviceDescription();
         let screen_number_key = NSString::from_str("NSScreenNumber");

         let display_id: CGDirectDisplayID = device_desc
            .objectForKey(&screen_number_key)
            .map(|n| {
               let n: &NSNumber = &*(n as *const _ as *const _);
               n.as_u32()
            })
            .unwrap_or_else(CGMainDisplayID);

         // Capture the display
         let cg_image = objc2_core_graphics::CGDisplayCreateImage(display_id);

         if cg_image.is_null() {
            return Err(Error::new(ErrorCode::CaptureFailed, "Failed to capture screen"));
         }

         // Get image dimensions
         let width = objc2_core_graphics::CGImageGetWidth(cg_image) as u32;
         let height = objc2_core_graphics::CGImageGetHeight(cg_image) as u32;

         // Create a bitmap context to extract pixel data
         let color_space = objc2_core_graphics::CGColorSpaceCreateDeviceRGB();
         let bytes_per_row = width * 4;
         let mut pixel_data: Vec<u8> = vec![0; (bytes_per_row * height) as usize];

         let context = objc2_core_graphics::CGBitmapContextCreate(
            pixel_data.as_mut_ptr() as *mut _,
            width as usize,
            height as usize,
            8,
            bytes_per_row as usize,
            color_space,
            objc2_core_graphics::kCGImageAlphaPremultipliedLast
               | objc2_core_graphics::kCGBitmapByteOrder32Big,
         );

         if context.is_null() {
            objc2_core_graphics::CGImageRelease(cg_image);
            objc2_core_graphics::CGColorSpaceRelease(color_space);
            return Err(Error::new(
               ErrorCode::CaptureFailed,
               "Failed to create bitmap context",
            ));
         }

         // Draw the image into the context
         let rect = CGRect {
            origin: objc2_core_graphics::CGPoint { x: 0.0, y: 0.0 },
            size: objc2_core_graphics::CGSize {
               width: width as f64,
               height: height as f64,
            },
         };
         objc2_core_graphics::CGContextDrawImage(context, rect, cg_image);

         // Clean up
         objc2_core_graphics::CGContextRelease(context);
         objc2_core_graphics::CGImageRelease(cg_image);
         objc2_core_graphics::CGColorSpaceRelease(color_space);

         // Create image from pixel data
         image::RgbaImage::from_raw(width, height, pixel_data).ok_or_else(|| {
            Error::new(
               ErrorCode::CaptureFailed,
               "Failed to create image from pixel data",
            )
         })
      }
   }

   /// Capture a screenshot of the focused screen
   pub fn capture_focused(&self) -> Result<image::RgbaImage> {
      let screen = self.focused_screen()?;
      self.capture(&screen)
   }
}

impl Default for ScreenCapture {
   fn default() -> Self {
      Self::new()
   }
}
