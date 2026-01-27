#pragma once

#include <QImage>
#include <QObject>
#include <QRect>
#include <QString>
#include <expected>
#include <memory>
#include <vector>

namespace timelapse {

struct ScreenInfo {
   int32_t id;
   QRect geometry;
   QString name;
   bool isPrimary;
};

class ScreenCapture : public QObject {
   Q_OBJECT

public:
   explicit ScreenCapture(QObject* parent = nullptr);
   ~ScreenCapture() override = default;

   // Factory method - returns platform-specific implementation
   [[nodiscard]] static auto create(QObject* parent = nullptr)
      -> std::unique_ptr<ScreenCapture>;

   // Get all available screens
   [[nodiscard]] virtual auto screens() const -> std::vector<ScreenInfo> = 0;

   // Get the screen containing the active window
   [[nodiscard]] virtual auto focusedScreen() const
      -> std::expected<ScreenInfo, QString> = 0;

   // Capture a specific screen
   [[nodiscard]] virtual auto capture(ScreenInfo const& screen)
      -> std::expected<QImage, QString> = 0;

   // Convenience: capture the focused screen
   [[nodiscard]] auto captureFocused() -> std::expected<QImage, QString>;

signals:
   void captureError(QString const& error);
};

}  // namespace timelapse
