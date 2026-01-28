use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::ClassType;
use objc2_app_kit::NSScreen;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, MainThreadMarker};

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

      // Need to be on main thread for NSScreen
      let Some(mtm) = MainThreadMarker::new() else {
         return result;
      };

      unsafe {
         let screens = NSScreen::screens(mtm);
         let main_screen = NSScreen::mainScreen(mtm);
         let main_frame = main_screen.as_ref().map(|s| s.frame());

         for (id, screen) in screens.iter().enumerate() {
            let frame = screen.frame();
            let name = screen.localizedName().to_string();

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

      // For simplicity, just return primary screen
      // Full implementation would use CGWindowListCopyWindowInfo
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

   /// Capture a screenshot of the specified screen using CGDisplayCreateImage
   pub fn capture(&self, screen: &ScreenInfo) -> Result<image::RgbaImage> {
      // Use Core Graphics to capture
      // Link against CoreGraphics framework
      #[link(name = "CoreGraphics", kind = "framework")]
      extern "C" {
         fn CGMainDisplayID() -> u32;
         fn CGDisplayCreateImage(display: u32) -> *mut std::ffi::c_void;
         fn CGImageGetWidth(image: *mut std::ffi::c_void) -> usize;
         fn CGImageGetHeight(image: *mut std::ffi::c_void) -> usize;
         fn CGImageRelease(image: *mut std::ffi::c_void);
         fn CGColorSpaceCreateDeviceRGB() -> *mut std::ffi::c_void;
         fn CGColorSpaceRelease(space: *mut std::ffi::c_void);
         fn CGBitmapContextCreate(
            data: *mut u8,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: *mut std::ffi::c_void,
            bitmap_info: u32,
         ) -> *mut std::ffi::c_void;
         fn CGContextDrawImage(
            context: *mut std::ffi::c_void,
            rect: CGRect,
            image: *mut std::ffi::c_void,
         );
         fn CGContextRelease(context: *mut std::ffi::c_void);
      }

      #[repr(C)]
      #[derive(Copy, Clone)]
      struct CGPoint {
         x: f64,
         y: f64,
      }

      #[repr(C)]
      #[derive(Copy, Clone)]
      struct CGSize {
         width: f64,
         height: f64,
      }

      #[repr(C)]
      #[derive(Copy, Clone)]
      struct CGRect {
         origin: CGPoint,
         size: CGSize,
      }

      const K_CG_IMAGE_ALPHA_PREMULTIPLIED_LAST: u32 = 1;
      const K_CG_BITMAP_BYTE_ORDER_32_BIG: u32 = 1 << 12;

      unsafe {
         // Get display ID - for simplicity use main display
         // A full implementation would map screen.id to display ID
         let display_id = CGMainDisplayID();

         let cg_image = CGDisplayCreateImage(display_id);
         if cg_image.is_null() {
            return Err(Error::new(ErrorCode::CaptureFailed, "Failed to capture screen"));
         }

         let width = CGImageGetWidth(cg_image);
         let height = CGImageGetHeight(cg_image);

         let color_space = CGColorSpaceCreateDeviceRGB();
         let bytes_per_row = width * 4;
         let mut pixel_data: Vec<u8> = vec![0; bytes_per_row * height];

         let context = CGBitmapContextCreate(
            pixel_data.as_mut_ptr(),
            width,
            height,
            8,
            bytes_per_row,
            color_space,
            K_CG_IMAGE_ALPHA_PREMULTIPLIED_LAST | K_CG_BITMAP_BYTE_ORDER_32_BIG,
         );

         if context.is_null() {
            CGImageRelease(cg_image);
            CGColorSpaceRelease(color_space);
            return Err(Error::new(
               ErrorCode::CaptureFailed,
               "Failed to create bitmap context",
            ));
         }

         let rect = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
               width: width as f64,
               height: height as f64,
            },
         };
         CGContextDrawImage(context, rect, cg_image);

         CGContextRelease(context);
         CGImageRelease(cg_image);
         CGColorSpaceRelease(color_space);

         image::RgbaImage::from_raw(width as u32, height as u32, pixel_data).ok_or_else(|| {
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
