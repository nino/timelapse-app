#ifdef Q_OS_WIN

#include "ScreenCapture.hh"

#include <QGuiApplication>
#include <QScreen>

#include <Windows.h>

namespace timelapse {

class ScreenCaptureWindows : public ScreenCapture {
public:
   explicit ScreenCaptureWindows(QObject* parent = nullptr)
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
      // Get the foreground window
      HWND foregroundWindow = GetForegroundWindow();
      if (!foregroundWindow) {
         return this->getPrimaryScreen();
      }

      // Get window rect
      RECT rect;
      if (!GetWindowRect(foregroundWindow, &rect)) {
         return this->getPrimaryScreen();
      }

      // Calculate center point
      QPoint center(
         (rect.left + rect.right) / 2,
         (rect.top + rect.bottom) / 2);

      // Find which screen contains this point
      for (auto const& screen : this->screens()) {
         if (screen.geometry.contains(center)) {
            return screen;
         }
      }

      return this->getPrimaryScreen();
   }

   [[nodiscard]] auto capture(ScreenInfo const& screen)
      -> std::expected<QImage, Error> override {
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
   [[nodiscard]] auto getPrimaryScreen() const
      -> std::expected<ScreenInfo, Error> {
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
};

// Factory implementation for Windows
auto ScreenCapture::create(QObject* parent) -> std::unique_ptr<ScreenCapture> {
   return std::make_unique<ScreenCaptureWindows>(parent);
}

}  // namespace timelapse

#endif  // Q_OS_WIN
