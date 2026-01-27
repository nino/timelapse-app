#include "ScreenCapture.hh"

namespace timelapse {

ScreenCapture::ScreenCapture(QObject* parent)
   : QObject(parent) {}

auto ScreenCapture::captureFocused() -> std::expected<QImage, Error> {
   auto screen = this->focusedScreen();
   if (!screen) {
      return std::unexpected(screen.error());
   }

   return this->capture(*screen);
}

}  // namespace timelapse
