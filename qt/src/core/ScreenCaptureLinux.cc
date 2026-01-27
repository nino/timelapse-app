#if defined(Q_OS_LINUX) || (defined(Q_OS_UNIX) && !defined(Q_OS_MACOS))

#include "ScreenCapture.hh"

#include <QGuiApplication>
#include <QScreen>

#include <X11/Xlib.h>

namespace timelapse {

class ScreenCaptureLinux : public ScreenCapture {
public:
   explicit ScreenCaptureLinux(QObject* parent = nullptr)
      : ScreenCapture(parent)
      , _display(XOpenDisplay(nullptr)) {}

   ~ScreenCaptureLinux() override {
      if (this->_display) {
         XCloseDisplay(this->_display);
      }
   }

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
      -> std::expected<ScreenInfo, QString> override {
      if (!this->_display) {
         // Fall back to primary screen
         return this->getPrimaryScreen();
      }

      // Get the focused window
      Window focusedWindow;
      int revertTo;
      XGetInputFocus(this->_display, &focusedWindow, &revertTo);

      if (focusedWindow == None || focusedWindow == PointerRoot) {
         return this->getPrimaryScreen();
      }

      // Get window geometry
      Window root;
      int x, y;
      unsigned int width, height, border, depth;
      if (!XGetGeometry(this->_display, focusedWindow, &root,
                        &x, &y, &width, &height, &border, &depth)) {
         return this->getPrimaryScreen();
      }

      // Translate coordinates to root window
      Window child;
      int rootX, rootY;
      XTranslateCoordinates(this->_display, focusedWindow, root,
                           0, 0, &rootX, &rootY, &child);

      QPoint center(rootX + width / 2, rootY + height / 2);

      // Find which screen contains this point
      for (auto const& screen : this->screens()) {
         if (screen.geometry.contains(center)) {
            return screen;
         }
      }

      return this->getPrimaryScreen();
   }

   [[nodiscard]] auto capture(ScreenInfo const& screen)
      -> std::expected<QImage, QString> override {
      auto* qscreen = QGuiApplication::screens().value(screen.id);
      if (!qscreen) {
         return std::unexpected("Screen not found");
      }

      QPixmap pixmap = qscreen->grabWindow(0);
      if (pixmap.isNull()) {
         return std::unexpected("Failed to capture screen");
      }

      return pixmap.toImage();
   }

private:
   [[nodiscard]] auto getPrimaryScreen() const
      -> std::expected<ScreenInfo, QString> {
      auto allScreens = this->screens();
      for (auto const& screen : allScreens) {
         if (screen.isPrimary) {
            return screen;
         }
      }
      if (!allScreens.empty()) {
         return allScreens[0];
      }
      return std::unexpected("No screens found");
   }

   Display* _display;
};

// Factory implementation for Linux
auto ScreenCapture::create(QObject* parent) -> std::unique_ptr<ScreenCapture> {
   return std::make_unique<ScreenCaptureLinux>(parent);
}

}  // namespace timelapse

#endif  // Q_OS_LINUX
