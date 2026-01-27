#pragma once

#include <QObject>
#include <QProcess>
#include <QString>
#include <expected>

namespace timelapse {

class VideoExtractor : public QObject {
   Q_OBJECT

public:
   explicit VideoExtractor(QObject* parent = nullptr);
   ~VideoExtractor() override;

   // Extract frames from video at specified FPS
   // Returns the cache folder name on success
   [[nodiscard]] auto extractFrames(QString const& videoPath,
                                    int32_t fps = 30)
      -> std::expected<QString, QString>;

   // Check if frames already exist in cache for a video
   [[nodiscard]] auto hasCachedFrames(QString const& videoFilename) const -> bool;

   // Get the cache folder path for a video
   [[nodiscard]] auto getCacheFolderPath(QString const& videoFilename) const -> QString;

   // Check if extraction is in progress
   [[nodiscard]] auto isExtracting() const -> bool;

   // Cancel ongoing extraction
   void cancelExtraction();

signals:
   void extractionStarted(QString const& videoPath);
   void extractionProgress(int percent);
   void extractionFinished(QString const& cacheFolderName);
   void extractionFailed(QString const& error);

private:
   [[nodiscard]] auto findFfmpeg() const -> std::expected<QString, QString>;
   [[nodiscard]] auto generateCacheFolderName(QString const& videoFilename) const
      -> QString;

   QProcess* m_ffmpegProcess{nullptr};
   bool m_extracting{false};
};

}  // namespace timelapse
