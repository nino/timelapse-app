#ifdef Q_OS_MACOS

#include "ScreenCapture.hh"

#include <QGuiApplication>
#include <QScreen>

#include <ApplicationServices/ApplicationServices.h>
#include <CoreGraphics/CoreGraphics.h>

namespace timelapse {

class ScreenCaptureMac : public ScreenCapture {
public:
   explicit ScreenCaptureMac(QObject* parent = nullptr)
      : ScreenCapture(parent) {}

   [[nodiscard]] auto screens() const -> std::vector<ScreenInfo> override {
      std::vector<ScreenInfo> result;
      auto* primaryScreen = QGuiApplication::primaryScreen();

      int32_t id = 0;
      for (auto* screen : QGuiApplication::screens()) {
         result.push_back(ScreenInfo{
            .id = id++,
            .geometry = screen->geometry(),
            .name = screen->name(),
            .isPrimary = (screen == primaryScreen),
         });
      }

      return result;
   }

   [[nodiscard]] auto focusedScreen() const
      -> std::expected<ScreenInfo, Error> override {
      // Get the active window bounds using CoreGraphics
      auto windowBounds = this->getActiveWindowBounds();
      if (!windowBounds) {
         // Fall back to primary screen
         auto allScreens = this->screens();
         for (auto const& screen : allScreens) {
            if (screen.isPrimary) {
               return screen;
            }
         }
         if (!allScreens.empty()) {
            return allScreens[0];
         }
         return std::unexpected(Error{ErrorCode::NoScreensFound, "No screens found"});
      }

      // Find which screen contains the window center
      QPoint center = windowBounds->center();
      for (auto const& screen : this->screens()) {
         if (screen.geometry.contains(center)) {
            return screen;
         }
      }

      // Fall back to primary
      for (auto const& screen : this->screens()) {
         if (screen.isPrimary) {
            return screen;
         }
      }

      return std::unexpected(Error{ErrorCode::ScreenNotFound, "Could not determine focused screen"});
   }

   [[nodiscard]] auto capture(ScreenInfo const& screen)
      -> std::expected<QImage, Error> override {
      // Use Qt's screen grab for simplicity and cross-platform consistency
      auto* qscreen = QGuiApplication::screens().value(screen.id);
      if (!qscreen) {
         return std::unexpected(Error{ErrorCode::ScreenNotFound, "Screen not found"});
      }

      QPixmap pixmap = qscreen->grabWindow(0);
      if (pixmap.isNull()) {
         return std::unexpected(Error{ErrorCode::CaptureFailed, "Failed to capture screen"});
      }

      return pixmap.toImage();
   }

private:
   [[nodiscard]] auto getActiveWindowBounds() const
      -> std::expected<QRect, Error> {
      // Get list of windows
      CFArrayRef windowList = CGWindowListCopyWindowInfo(
         kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
         kCGNullWindowID);

      if (!windowList) {
         return std::unexpected(Error{ErrorCode::CaptureFailed, "Failed to get window list"});
      }

      QRect result;
      bool found = false;

      CFIndex count = CFArrayGetCount(windowList);
      for (CFIndex i = 0; i < count && !found; i++) {
         CFDictionaryRef window =
            static_cast<CFDictionaryRef>(CFArrayGetValueAtIndex(windowList, i));

         // Check if this window is on screen level 0 (normal windows)
         CFNumberRef layerRef = static_cast<CFNumberRef>(
            CFDictionaryGetValue(window, kCGWindowLayer));
         if (layerRef) {
            int32_t layer = 0;
            CFNumberGetValue(layerRef, kCFNumberSInt32Type, &layer);
            if (layer != 0) {
               continue;  // Skip non-normal windows
            }
         }

         // Get window bounds
         CFDictionaryRef boundsRef = static_cast<CFDictionaryRef>(
            CFDictionaryGetValue(window, kCGWindowBounds));
         if (boundsRef) {
            CGRect bounds;
            if (CGRectMakeWithDictionaryRepresentation(boundsRef, &bounds)) {
               result = QRect(
                  static_cast<int>(bounds.origin.x),
                  static_cast<int>(bounds.origin.y),
                  static_cast<int>(bounds.size.width),
                  static_cast<int>(bounds.size.height));
               found = true;
            }
         }
      }

      CFRelease(windowList);

      if (!found) {
         return std::unexpected(Error{ErrorCode::CaptureFailed, "No active window found"});
      }

      return result;
   }
};

// Factory implementation for macOS
auto ScreenCapture::create(QObject* parent) -> std::unique_ptr<ScreenCapture> {
   return std::make_unique<ScreenCaptureMac>(parent);
}

}  // namespace timelapse

#endif  // Q_OS_MACOS
