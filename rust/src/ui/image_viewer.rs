use objc2::rc::Retained;
use objc2_app_kit::{NSImage, NSImageScaling, NSImageView};
use objc2_foundation::{MainThreadMarker, NSString};
use std::path::Path;

pub struct ImageViewer {
   view: Retained<NSImageView>,
}

impl ImageViewer {
   pub fn new(mtm: MainThreadMarker) -> Self {
      let view = unsafe {
         let view = NSImageView::new(mtm);
         view.setImageScaling(NSImageScaling::NSImageScaleProportionallyUpOrDown);
         view
      };

      Self { view }
   }

   pub fn view(&self) -> &NSImageView {
      &self.view
   }

   pub fn set_image_path(&self, path: &Path) {
      if let Some(path_str) = path.to_str() {
         unsafe {
            let ns_path = NSString::from_str(path_str);
            if let Some(image) = NSImage::initWithContentsOfFile(NSImage::alloc(), &ns_path) {
               self.view.setImage(Some(&image));
            }
         }
      }
   }

   pub fn clear(&self) {
      unsafe {
         self.view.setImage(None);
      }
   }
}
