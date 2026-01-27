use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{NSImage, NSImageScaling, NSImageView, NSView};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::path::Path;

declare_class!(
   pub struct ImageViewer;

   unsafe impl ClassType for ImageViewer {
      type Super = NSImageView;
      type Mutability = mutability::MainThreadOnly;
      const NAME: &'static str = "ImageViewer";
   }

   impl DeclaredClass for ImageViewer {
      type Ivars = ();
   }

   unsafe impl ImageViewer {
      #[method_id(initWithFrame:)]
      fn init_with_frame(this: objc2::rc::Allocated<Self>, frame: objc2_foundation::NSRect) -> Option<Retained<Self>> {
         let this: Option<Retained<Self>> = unsafe { msg_send_id![super(this), initWithFrame: frame] };
         if let Some(ref this) = this {
            unsafe {
               this.setImageScaling(NSImageScaling::NSImageScaleProportionallyUpOrDown);
               this.setWantsLayer(true);
               if let Some(layer) = this.layer() {
                  // Set background color to black
                  let black = objc2_quartz_core::CGColorCreateGenericRGB(0.0, 0.0, 0.0, 1.0);
                  layer.setBackgroundColor(black);
               }
            }
         }
         this
      }
   }
);

impl ImageViewer {
   pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
      let frame = objc2_foundation::NSRect::new(
         objc2_foundation::NSPoint::new(0.0, 0.0),
         objc2_foundation::NSSize::new(800.0, 600.0),
      );
      unsafe { msg_send_id![Self::alloc(), initWithFrame: frame] }
   }

   pub fn set_image_path(&self, path: &Path) {
      if let Some(path_str) = path.to_str() {
         unsafe {
            let ns_path = NSString::from_str(path_str);
            if let Some(image) = NSImage::initWithContentsOfFile(NSImage::alloc(), &ns_path) {
               self.setImage(Some(&image));
            }
         }
      }
   }

   pub fn clear(&self) {
      unsafe {
         self.setImage(None);
      }
   }
}
