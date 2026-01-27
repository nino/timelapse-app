#include "ScreenCapture.hh"

namespace timelapse {

ScreenCapture::ScreenCapture(QObject* parent)
   : QObject(parent) {}

auto ScreenCapture::captureFocused() -> std::expected<QImage, QString> {
   auto screen = focusedScreen();
   if (!screen) {
      return std::unexpected(screen.error());
   }

   return capture(*screen);
}

}  // namespace timelapse
