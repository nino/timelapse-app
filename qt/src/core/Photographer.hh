#pragma once

#include <QObject>
#include <QTimer>
#include <atomic>
#include <memory>

#include "Database.hh"
#include "ErrorLog.hh"
#include "ImageProcessor.hh"
#include "ScreenCapture.hh"

namespace timelapse {

class Photographer : public QObject {
   Q_OBJECT
   Q_PROPERTY(bool running READ isRunning NOTIFY runningChanged)

public:
   explicit Photographer(QObject* parent = nullptr);
   ~Photographer() override;

   [[nodiscard]] auto isRunning() const -> bool;
   [[nodiscard]] auto errorLog() const -> ErrorLog*;
   [[nodiscard]] auto database() const -> Database*;

   // Get metadata for a specific frame (delegates to database)
   [[nodiscard]] auto getScreenshotMetadata(uint32_t frameNumber)
      -> std::expected<std::optional<ScreenshotMetadata>, QString>;

public slots:
   void start();
   void stop();

signals:
   void runningChanged(bool running);
   void screenshotCaptured(QString const& path, uint32_t frameNumber);
   void screenshotSkipped(QString const& reason);
   void errorOccurred(QString const& error);

private slots:
   void captureScreenshot();

private:
   auto ensureDayDirectory() -> std::expected<QString, QString>;
   auto getNextFrameNumber(QString const& dayDir) -> std::expected<uint32_t, QString>;
   void scheduleNextCapture(std::chrono::milliseconds delay);
   void logError(QString const& error);

   std::unique_ptr<ScreenCapture> m_screenCapture;
   std::unique_ptr<ImageProcessor> m_imageProcessor;
   std::unique_ptr<Database> m_database;
   std::unique_ptr<ErrorLog> m_errorLog;

   QTimer* m_captureTimer{nullptr};
   QString m_currentDayDir;
   QString m_currentDate;
   std::atomic<bool> m_running{false};
};

}  // namespace timelapse
