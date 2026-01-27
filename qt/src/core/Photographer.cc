#include "Photographer.hh"

#include <QDir>
#include <QRegularExpression>
#include <QtConcurrent>

#include "utils/Constants.hh"
#include "utils/PathUtils.hh"

namespace timelapse {

Photographer::Photographer(QObject* parent)
   : QObject(parent)
   , m_screenCapture(ScreenCapture::create(this))
   , m_imageProcessor(std::make_unique<ImageProcessor>(this))
   , m_errorLog(std::make_unique<ErrorLog>(this)) {
   // Ensure directories exist
   PathUtils::ensureDir(PathUtils::timelapseDir());
   PathUtils::ensureDir(PathUtils::cacheDir());

   // Initialize database
   m_database = std::make_unique<Database>(PathUtils::databasePath(), this);

   // Setup timer
   m_captureTimer = new QTimer(this);
   m_captureTimer->setSingleShot(true);
   connect(m_captureTimer, &QTimer::timeout, this, &Photographer::captureScreenshot);
}

Photographer::~Photographer() {
   stop();
}

auto Photographer::isRunning() const -> bool {
   return m_running.load();
}

auto Photographer::errorLog() const -> ErrorLog* {
   return m_errorLog.get();
}

auto Photographer::database() const -> Database* {
   return m_database.get();
}

auto Photographer::getScreenshotMetadata(uint32_t frameNumber)
   -> std::expected<std::optional<ScreenshotMetadata>, QString> {
   return m_database->getScreenshotByFrame(frameNumber);
}

void Photographer::start() {
   if (m_running.exchange(true)) {
      return;  // Already running
   }

   emit runningChanged(true);
   captureScreenshot();
}

void Photographer::stop() {
   if (!m_running.exchange(false)) {
      return;  // Already stopped
   }

   m_captureTimer->stop();
   emit runningChanged(false);
}

void Photographer::captureScreenshot() {
   if (!m_running.load()) {
      return;
   }

   // Check if date changed (new day)
   QString today = PathUtils::currentDateString();
   if (today != m_currentDate) {
      m_currentDate = today;
      m_currentDayDir.clear();  // Force recreation
   }

   // Ensure day directory exists
   auto dayDir = ensureDayDirectory();
   if (!dayDir) {
      logError(dayDir.error());
      scheduleNextCapture(kErrorWait);
      return;
   }

   // Capture screenshot
   auto image = m_screenCapture->captureFocused();
   if (!image) {
      logError("Capture failed: " + image.error());
      scheduleNextCapture(kErrorWait);
      return;
   }

   // Process image (resize with letterbox)
   auto processed = m_imageProcessor->resizeWithLetterbox(
      *image, kTargetWidth, kTargetHeight);
   if (!processed) {
      logError("Processing failed: " + processed.error());
      scheduleNextCapture(kErrorWait);
      return;
   }

   // Check for black screen
   if (m_imageProcessor->isImageBlack(*processed, kBlackThreshold)) {
      emit screenshotSkipped("Screen is black");
      scheduleNextCapture(kBlackScreenWait);
      return;
   }

   // Get next frame number
   auto frameNumber = getNextFrameNumber(*dayDir);
   if (!frameNumber) {
      logError("Failed to get frame number: " + frameNumber.error());
      scheduleNextCapture(kErrorWait);
      return;
   }

   // Build filename
   QString filename = PathUtils::formatFrameNumber(*frameNumber) + ".png";
   QString filepath = *dayDir + "/" + filename;

   // Save image
   auto saveResult = m_imageProcessor->saveImage(*processed, filepath);
   if (!saveResult) {
      logError("Save failed: " + saveResult.error());
      scheduleNextCapture(kErrorWait);
      return;
   }

   // Record in database
   QDateTime now = QDateTime::currentDateTimeUtc();
   QDateTime local = QDateTime::currentDateTime();
   auto dbResult = m_database->insertScreenshot(*frameNumber, now, local);
   if (!dbResult) {
      logError("Database insert failed: " + dbResult.error());
      // Continue anyway - screenshot was saved
   }

   emit screenshotCaptured(filepath, *frameNumber);
   scheduleNextCapture(kScreenshotInterval);
}

auto Photographer::ensureDayDirectory() -> std::expected<QString, QString> {
   if (!m_currentDayDir.isEmpty()) {
      return m_currentDayDir;
   }

   QString dayDir = PathUtils::dayFolder(m_currentDate);
   if (!PathUtils::ensureDir(dayDir)) {
      return std::unexpected("Failed to create day directory: " + dayDir);
   }

   m_currentDayDir = dayDir;
   return m_currentDayDir;
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
   if (m_running.load()) {
      m_captureTimer->start(delay.count());
   }
}

void Photographer::logError(QString const& error) {
   m_errorLog->append(error);
   emit errorOccurred(error);
}

}  // namespace timelapse
