#include "VideoExtractor.hh"

#include <QCryptographicHash>
#include <QDir>
#include <QFileInfo>
#include <QStandardPaths>

#include "utils/Constants.hh"
#include "utils/PathUtils.hh"

namespace timelapse {

VideoExtractor::VideoExtractor(QObject* parent)
   : QObject(parent) {}

VideoExtractor::~VideoExtractor() {
   this->cancelExtraction();
}

auto VideoExtractor::extractFrames(QString const& videoPath, int32_t fps)
   -> std::expected<QString, Error> {
   if (this->_extracting) {
      return std::unexpected(Error{ErrorCode::ExtractionInProgress,
                                   "Extraction already in progress"});
   }

   // Find ffmpeg
   auto ffmpegPath = this->findFfmpeg();
   if (!ffmpegPath) {
      return std::unexpected(ffmpegPath.error());
   }

   // Get video filename for cache folder
   QFileInfo videoInfo(videoPath);
   QString videoFilename = videoInfo.fileName();

   // Check if already cached
   if (this->hasCachedFrames(videoFilename)) {
      return this->generateCacheFolderName(videoFilename);
   }

   // Create cache folder
   QString cacheFolderName = this->generateCacheFolderName(videoFilename);
   QString cacheFolderPath = PathUtils::cacheDir() + "/" + cacheFolderName;

   if (!PathUtils::ensureDir(cacheFolderPath)) {
      return std::unexpected(Error{ErrorCode::CacheCreationFailed,
                                   "Failed to create cache folder: " + cacheFolderPath});
   }

   // Build ffmpeg command
   QString outputPattern = cacheFolderPath + "/%05d.jpg";
   QStringList args;
   args << "-i" << videoPath
        << "-vf" << QString("fps=%1").arg(fps)
        << "-q:v" << "2"  // High quality JPEG
        << outputPattern;

   // Start ffmpeg process
   this->_ffmpegProcess = new QProcess(this);
   this->_extracting = true;

   emit extractionStarted(videoPath);

   this->_ffmpegProcess->start(*ffmpegPath, args);

   if (!this->_ffmpegProcess->waitForStarted(5000)) {
      this->_extracting = false;
      QString error = this->_ffmpegProcess->errorString();
      delete this->_ffmpegProcess;
      this->_ffmpegProcess = nullptr;
      return std::unexpected(Error{ErrorCode::FfmpegStartFailed,
                                   "Failed to start ffmpeg: " + error});
   }

   // Wait for completion (blocking for simplicity)
   // In a real app, you'd use signals for async handling
   if (!this->_ffmpegProcess->waitForFinished(-1)) {  // No timeout
      this->_extracting = false;
      QString error = this->_ffmpegProcess->errorString();
      delete this->_ffmpegProcess;
      this->_ffmpegProcess = nullptr;
      return std::unexpected(Error{ErrorCode::FfmpegFailed, "ffmpeg failed: " + error});
   }

   int exitCode = this->_ffmpegProcess->exitCode();
   this->_extracting = false;
   delete this->_ffmpegProcess;
   this->_ffmpegProcess = nullptr;

   if (exitCode != 0) {
      return std::unexpected(Error{ErrorCode::FfmpegExitError,
                                   "ffmpeg exited with code: " + QString::number(exitCode)});
   }

   emit extractionFinished(cacheFolderName);
   return cacheFolderName;
}

auto VideoExtractor::hasCachedFrames(QString const& videoFilename) const -> bool {
   QString cacheFolderPath = this->getCacheFolderPath(videoFilename);
   QDir cacheDir(cacheFolderPath);

   if (!cacheDir.exists()) {
      return false;
   }

   // Check if there are any jpg files
   QStringList filters;
   filters << "*.jpg";
   return !cacheDir.entryList(filters, QDir::Files).isEmpty();
}

auto VideoExtractor::getCacheFolderPath(QString const& videoFilename) const -> QString {
   return PathUtils::cacheDir() + "/" + this->generateCacheFolderName(videoFilename);
}

auto VideoExtractor::isExtracting() const -> bool {
   return this->_extracting;
}

void VideoExtractor::cancelExtraction() {
   if (this->_ffmpegProcess && this->_ffmpegProcess->state() != QProcess::NotRunning) {
      this->_ffmpegProcess->kill();
      this->_ffmpegProcess->waitForFinished(3000);
   }
   this->_extracting = false;
}

auto VideoExtractor::findFfmpeg() const -> std::expected<QString, Error> {
   // Check common locations
   QStringList searchPaths;

#ifdef Q_OS_MACOS
   searchPaths << "/opt/homebrew/bin/ffmpeg"
               << "/usr/local/bin/ffmpeg";
#endif

#ifdef Q_OS_LINUX
   searchPaths << "/usr/bin/ffmpeg"
               << "/usr/local/bin/ffmpeg";
#endif

#ifdef Q_OS_WIN
   searchPaths << "C:/ffmpeg/bin/ffmpeg.exe"
               << "ffmpeg.exe";  // Rely on PATH
#endif

   // Also search in PATH
   QString pathFfmpeg = QStandardPaths::findExecutable("ffmpeg");
   if (!pathFfmpeg.isEmpty()) {
      return pathFfmpeg;
   }

   for (auto const& path : searchPaths) {
      if (QFileInfo::exists(path)) {
         return path;
      }
   }

   return std::unexpected(Error{ErrorCode::FfmpegNotFound,
                                "ffmpeg not found. Please install ffmpeg."});
}

auto VideoExtractor::generateCacheFolderName(QString const& videoFilename) const
   -> QString {
   // Create a hash-based folder name to avoid special characters
   QByteArray hash = QCryptographicHash::hash(
      videoFilename.toUtf8(),
      QCryptographicHash::Md5);

   // Use first 8 chars of hex hash + sanitized filename
   QString safeName = videoFilename;
   safeName.replace(QRegularExpression("[^a-zA-Z0-9._-]"), "_");

   return hash.toHex().left(8) + "_" + safeName;
}

}  // namespace timelapse
