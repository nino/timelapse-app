use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{
   NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType,
   NSButton, NSComboBox, NSControl, NSImage, NSImageScaling, NSImageView, NSLayoutConstraint,
   NSPopUpButton, NSSlider, NSStackView, NSUserInterfaceLayoutOrientation, NSView, NSWindow,
   NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
   ns_string, CGFloat, MainThreadMarker, NSArray, NSNotification, NSObject, NSObjectProtocol,
   NSPoint, NSRect, NSSize, NSString,
};
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::core::{Photographer, VideoExtractor};
use crate::utils::paths;

// Application state
struct AppState {
   current_folder: Option<String>,
   current_frame: usize,
   files: Vec<PathBuf>,
   folders: Vec<String>,
   view_mode: ViewMode,
}

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
   Images,
   Videos,
}

impl Default for AppState {
   fn default() -> Self {
      Self {
         current_folder: None,
         current_frame: 0,
         files: Vec::new(),
         folders: Vec::new(),
         view_mode: ViewMode::Images,
      }
   }
}

declare_class!(
   pub struct AppDelegate;

   unsafe impl ClassType for AppDelegate {
      type Super = NSObject;
      type Mutability = mutability::MainThreadOnly;
      const NAME: &'static str = "AppDelegate";
   }

   impl DeclaredClass for AppDelegate {
      type Ivars = AppDelegateIvars;
   }

   unsafe impl NSObjectProtocol for AppDelegate {}

   unsafe impl NSApplicationDelegate for AppDelegate {
      #[method(applicationDidFinishLaunching:)]
      fn did_finish_launching(&self, _notification: &NSNotification) {
         self.setup_window();
      }

      #[method(applicationShouldTerminateAfterLastWindowClosed:)]
      fn should_terminate_after_last_window_closed(&self, _sender: &NSApplication) -> bool {
         true
      }
   }
);

struct AppDelegateIvars {
   window: RefCell<Option<Retained<NSWindow>>>,
   image_view: RefCell<Option<Retained<NSImageView>>>,
   slider: RefCell<Option<Retained<NSSlider>>>,
   folder_popup: RefCell<Option<Retained<NSPopUpButton>>>,
   frame_label: RefCell<Option<Retained<objc2_app_kit::NSTextField>>>,
   state: RefCell<AppState>,
}

impl AppDelegate {
   pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
      let this = mtm.alloc::<Self>();
      let this = this.set_ivars(AppDelegateIvars {
         window: RefCell::new(None),
         image_view: RefCell::new(None),
         slider: RefCell::new(None),
         folder_popup: RefCell::new(None),
         frame_label: RefCell::new(None),
         state: RefCell::new(AppState::default()),
      });
      unsafe { msg_send_id![super(this), init] }
   }

   fn setup_window(&self) {
      let mtm = MainThreadMarker::new().unwrap();

      // Create window
      let frame = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(1200.0, 800.0));
      let style = NSWindowStyleMask::Titled
         | NSWindowStyleMask::Closable
         | NSWindowStyleMask::Miniaturizable
         | NSWindowStyleMask::Resizable;

      let window = unsafe {
         NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            frame,
            style,
            NSBackingStoreType::NSBackingStoreBuffered,
            false,
         )
      };

      unsafe {
         window.setTitle(ns_string!("Timelapse Viewer"));
         window.center();

         // Create main content view
         let content_view = window.contentView().unwrap();

         // Create image view
         let image_view = NSImageView::new(mtm);
         image_view.setImageScaling(NSImageScaling::NSImageScaleProportionallyUpOrDown);
         image_view.setTranslatesAutoresizingMaskIntoConstraints(false);

         // Create slider
         let slider = NSSlider::new(mtm);
         slider.setMinValue(0.0);
         slider.setMaxValue(100.0);
         slider.setTranslatesAutoresizingMaskIntoConstraints(false);

         // Create folder popup
         let folder_popup = NSPopUpButton::new(mtm);
         folder_popup.setTranslatesAutoresizingMaskIntoConstraints(false);

         // Create frame label
         let frame_label = objc2_app_kit::NSTextField::labelWithString(ns_string!("Frame 0 / 0"));
         frame_label.setTranslatesAutoresizingMaskIntoConstraints(false);

         // Create mode buttons
         let images_button = NSButton::buttonWithTitle_target_action(
            ns_string!("Images"),
            None,
            None,
         );
         images_button.setTranslatesAutoresizingMaskIntoConstraints(false);

         let videos_button = NSButton::buttonWithTitle_target_action(
            ns_string!("Videos"),
            None,
            None,
         );
         videos_button.setTranslatesAutoresizingMaskIntoConstraints(false);

         // Create refresh button
         let refresh_button = NSButton::buttonWithTitle_target_action(
            ns_string!("Refresh"),
            None,
            None,
         );
         refresh_button.setTranslatesAutoresizingMaskIntoConstraints(false);

         // Create toolbar stack view
         let toolbar = NSStackView::new(mtm);
         toolbar.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
         toolbar.setSpacing(10.0);
         toolbar.setTranslatesAutoresizingMaskIntoConstraints(false);
         toolbar.addArrangedSubview(&images_button);
         toolbar.addArrangedSubview(&videos_button);
         toolbar.addArrangedSubview(&folder_popup);
         toolbar.addArrangedSubview(&frame_label);
         toolbar.addArrangedSubview(&refresh_button);

         // Create bottom controls stack
         let bottom_controls = NSStackView::new(mtm);
         bottom_controls.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
         bottom_controls.setSpacing(10.0);
         bottom_controls.setTranslatesAutoresizingMaskIntoConstraints(false);
         bottom_controls.addArrangedSubview(&slider);

         // Add views to content view
         content_view.addSubview(&toolbar);
         content_view.addSubview(&image_view);
         content_view.addSubview(&bottom_controls);

         // Setup constraints
         let views: &[&NSView] = &[
            &toolbar,
            &*image_view,
            &bottom_controls,
         ];

         // Toolbar at top
         NSLayoutConstraint::activateConstraints(&NSArray::from_vec(vec![
            toolbar.topAnchor().constraintEqualToAnchor_constant(
               &content_view.topAnchor(),
               10.0,
            ),
            toolbar.leadingAnchor().constraintEqualToAnchor_constant(
               &content_view.leadingAnchor(),
               10.0,
            ),
            toolbar.trailingAnchor().constraintEqualToAnchor_constant(
               &content_view.trailingAnchor(),
               -10.0,
            ),
         ]));

         // Image view in center
         NSLayoutConstraint::activateConstraints(&NSArray::from_vec(vec![
            image_view.topAnchor().constraintEqualToAnchor_constant(
               &toolbar.bottomAnchor(),
               10.0,
            ),
            image_view.leadingAnchor().constraintEqualToAnchor_constant(
               &content_view.leadingAnchor(),
               10.0,
            ),
            image_view.trailingAnchor().constraintEqualToAnchor_constant(
               &content_view.trailingAnchor(),
               -10.0,
            ),
            image_view.bottomAnchor().constraintEqualToAnchor_constant(
               &bottom_controls.topAnchor(),
               -10.0,
            ),
         ]));

         // Bottom controls at bottom
         NSLayoutConstraint::activateConstraints(&NSArray::from_vec(vec![
            bottom_controls.leadingAnchor().constraintEqualToAnchor_constant(
               &content_view.leadingAnchor(),
               10.0,
            ),
            bottom_controls.trailingAnchor().constraintEqualToAnchor_constant(
               &content_view.trailingAnchor(),
               -10.0,
            ),
            bottom_controls.bottomAnchor().constraintEqualToAnchor_constant(
               &content_view.bottomAnchor(),
               -10.0,
            ),
            bottom_controls.heightAnchor().constraintEqualToConstant(30.0),
         ]));

         // Store references
         *self.ivars().window.borrow_mut() = Some(window.clone());
         *self.ivars().image_view.borrow_mut() = Some(image_view);
         *self.ivars().slider.borrow_mut() = Some(slider);
         *self.ivars().folder_popup.borrow_mut() = Some(folder_popup);
         *self.ivars().frame_label.borrow_mut() = Some(frame_label);

         // Load initial data
         self.load_folders();
         self.load_files();
         self.update_display();

         // Show window
         window.makeKeyAndOrderFront(None);
      }
   }

   fn load_folders(&self) {
      let timelapse_dir = paths::timelapse_dir();
      let mut folders = Vec::new();

      if let Ok(entries) = fs::read_dir(&timelapse_dir) {
         for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
               if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                  if !name.starts_with('.') {
                     folders.push(name.to_string());
                  }
               }
            }
         }
      }

      folders.sort();
      folders.reverse(); // Most recent first

      // Update popup menu
      if let Some(ref popup) = *self.ivars().folder_popup.borrow() {
         unsafe {
            popup.removeAllItems();
            for folder in &folders {
               let title = if folder == &paths::current_date_string() {
                  format!("{} (Today)", folder)
               } else {
                  folder.clone()
               };
               popup.addItemWithTitle(&NSString::from_str(&title));
            }
         }
      }

      let mut state = self.ivars().state.borrow_mut();
      state.folders = folders.clone();
      if !folders.is_empty() && state.current_folder.is_none() {
         state.current_folder = Some(folders[0].clone());
      }
   }

   fn load_files(&self) {
      let state = self.ivars().state.borrow();
      let folder = match &state.current_folder {
         Some(f) => f.clone(),
         None => return,
      };
      drop(state);

      let folder_path = paths::day_folder(&folder);
      let mut files = Vec::new();

      if let Ok(entries) = fs::read_dir(&folder_path) {
         for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map(|e| e == "png").unwrap_or(false) {
               files.push(path);
            }
         }
      }

      files.sort();

      let file_count = files.len();

      let mut state = self.ivars().state.borrow_mut();
      state.files = files;
      if file_count > 0 {
         state.current_frame = file_count - 1; // Start at last frame
      } else {
         state.current_frame = 0;
      }
      drop(state);

      // Update slider
      if let Some(ref slider) = *self.ivars().slider.borrow() {
         unsafe {
            slider.setMaxValue((file_count.saturating_sub(1)) as f64);
            slider.setDoubleValue((file_count.saturating_sub(1)) as f64);
         }
      }
   }

   fn update_display(&self) {
      let state = self.ivars().state.borrow();

      // Update image
      if let Some(path) = state.files.get(state.current_frame) {
         if let Some(ref image_view) = *self.ivars().image_view.borrow() {
            if let Some(path_str) = path.to_str() {
               unsafe {
                  let ns_path = NSString::from_str(path_str);
                  if let Some(image) =
                     NSImage::initWithContentsOfFile(NSImage::alloc(), &ns_path)
                  {
                     image_view.setImage(Some(&image));
                  }
               }
            }
         }
      }

      // Update frame label
      if let Some(ref label) = *self.ivars().frame_label.borrow() {
         let text = format!("Frame {} / {}", state.current_frame + 1, state.files.len());
         unsafe {
            label.setStringValue(&NSString::from_str(&text));
         }
      }
   }
}

pub struct App;

impl App {
   pub fn run() {
      let mtm = MainThreadMarker::new().expect("Must run on main thread");

      let app = NSApplication::sharedApplication(mtm);
      unsafe {
         app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
      }

      let delegate = AppDelegate::new(mtm);
      unsafe {
         app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
         app.run();
      }
   }
}
