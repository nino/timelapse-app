#include "Photographer.hh"

#include <QDir>
#include <QRegularExpression>
#include <QtConcurrent>

#include "utils/Constants.hh"
#include "utils/PathUtils.hh"

namespace timelapse {

Photographer::Photographer(QObject* parent)
   : QObject(parent)
   , _screenCapture(ScreenCapture::create(this))
   , _imageProcessor(std::make_unique<ImageProcessor>(this))
   , _errorLog(std::make_unique<ErrorLog>(this)) {
   // Ensure directories exist
   PathUtils::ensureDir(PathUtils::timelapseDir());
   PathUtils::ensureDir(PathUtils::cacheDir());

   // Initialize database
   this->_database = std::make_unique<Database>(PathUtils::databasePath(), this);

   // Setup timer
   this->_captureTimer = new QTimer(this);
   this->_captureTimer->setSingleShot(true);
   connect(this->_captureTimer, &QTimer::timeout, this, &Photographer::captureScreenshot);
}

Photographer::~Photographer() {
   this->stop();
}

auto Photographer::isRunning() const -> bool {
   return this->_running.load();
}

auto Photographer::errorLog() const -> ErrorLog* {
   return this->_errorLog.get();
}

auto Photographer::database() const -> Database* {
   return this->_database.get();
}

auto Photographer::getScreenshotMetadata(uint32_t frameNumber)
   -> std::expected<std::optional<ScreenshotMetadata>, QString> {
   return this->_database->getScreenshotByFrame(frameNumber);
}

void Photographer::start() {
   if (this->_running.exchange(true)) {
      return;  // Already running
   }

   emit runningChanged(true);
   this->captureScreenshot();
}

void Photographer::stop() {
   if (!this->_running.exchange(false)) {
      return;  // Already stopped
   }

   this->_captureTimer->stop();
   emit runningChanged(false);
}

void Photographer::captureScreenshot() {
   if (!this->_running.load()) {
      return;
   }

   // Check if date changed (new day)
   QString today = PathUtils::currentDateString();
   if (today != this->_currentDate) {
      this->_currentDate = today;
      this->_currentDayDir.clear();  // Force recreation
   }

   // Ensure day directory exists
   auto dayDir = this->ensureDayDirectory();
   if (!dayDir) {
      this->logError(dayDir.error());
      this->scheduleNextCapture(kErrorWait);
      return;
   }

   // Capture screenshot
   auto image = this->_screenCapture->captureFocused();
   if (!image) {
      this->logError("Capture failed: " + image.error());
      this->scheduleNextCapture(kErrorWait);
      return;
   }

   // Process image (resize with letterbox)
   auto processed = this->_imageProcessor->resizeWithLetterbox(
      *image, kTargetWidth, kTargetHeight);
   if (!processed) {
      this->logError("Processing failed: " + processed.error());
      this->scheduleNextCapture(kErrorWait);
      return;
   }

   // Check for black screen
   if (this->_imageProcessor->isImageBlack(*processed, kBlackThreshold)) {
      emit screenshotSkipped("Screen is black");
      this->scheduleNextCapture(kBlackScreenWait);
      return;
   }

   // Get next frame number
   auto frameNumber = this->getNextFrameNumber(*dayDir);
   if (!frameNumber) {
      this->logError("Failed to get frame number: " + frameNumber.error());
      this->scheduleNextCapture(kErrorWait);
      return;
   }

   // Build filename
   QString filename = PathUtils::formatFrameNumber(*frameNumber) + ".png";
   QString filepath = *dayDir + "/" + filename;

   // Save image
   auto saveResult = this->_imageProcessor->saveImage(*processed, filepath);
   if (!saveResult) {
      this->logError("Save failed: " + saveResult.error());
      this->scheduleNextCapture(kErrorWait);
      return;
   }

   // Record in database
   QDateTime now = QDateTime::currentDateTimeUtc();
   QDateTime local = QDateTime::currentDateTime();
   auto dbResult = this->_database->insertScreenshot(*frameNumber, now, local);
   if (!dbResult) {
      this->logError("Database insert failed: " + dbResult.error());
      // Continue anyway - screenshot was saved
   }

   emit screenshotCaptured(filepath, *frameNumber);
   this->scheduleNextCapture(kScreenshotInterval);
}

auto Photographer::ensureDayDirectory() -> std::expected<QString, QString> {
   if (!this->_currentDayDir.isEmpty()) {
      return this->_currentDayDir;
   }

   QString dayDir = PathUtils::dayFolder(this->_currentDate);
   if (!PathUtils::ensureDir(dayDir)) {
      return std::unexpected("Failed to create day directory: " + dayDir);
   }

   this->_currentDayDir = dayDir;
   return this->_currentDayDir;
}

auto Photographer::getNextFrameNumber(QString const& dayDir)
   -> std::expected<uint32_t, QString> {
   QDir dir(dayDir);
   if (!dir.exists()) {
      return 1;  // First frame
   }

   // Find highest existing frame number
   QStringList filters;
   filters << "*.png";
   QStringList files = dir.entryList(filters, QDir::Files, QDir::Name);

   if (files.isEmpty()) {
      return 1;  // First frame
   }

   // Parse the last filename to get the highest number
   static QRegularExpression re("^(\\d+)\\.png$");
   uint32_t maxFrame = 0;

   for (auto const& file : files) {
      auto match = re.match(file);
      if (match.hasMatch()) {
         uint32_t num = match.captured(1).toUInt();
         maxFrame = std::max(maxFrame, num);
      }
   }

   return maxFrame + 1;
}

void Photographer::scheduleNextCapture(std::chrono::milliseconds delay) {
   if (this->_running.load()) {
      this->_captureTimer->start(delay.count());
   }
}

void Photographer::logError(QString const& error) {
   this->_errorLog->append(error);
   emit errorOccurred(error);
}

}  // namespace timelapse
