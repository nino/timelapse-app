#include "MainWindow.hh"

#include <QDateTime>
#include <QHBoxLayout>
#include <QKeyEvent>
#include <QVBoxLayout>

#include "utils/PathUtils.hh"

namespace timelapse {

MainWindow::MainWindow(QWidget* parent)
   : QMainWindow(parent)
   , _photographer(std::make_unique<Photographer>(this))
   , _videoExtractor(std::make_unique<VideoExtractor>(this))
   , _cacheManager(std::make_unique<CacheManager>(this))
   , _folderModel(std::make_unique<FolderListModel>(this))
   , _fileModel(std::make_unique<FileListModel>(this))
   , _videoModel(std::make_unique<VideoListModel>(this))
   , _videoFrameModel(std::make_unique<FileListModel>(this)) {
   setWindowTitle("Timelapse Viewer");
   resize(1200, 800);

   // Setup models
   this->_folderModel->setBasePath(PathUtils::timelapseDir());
   this->_videoModel->setBasePath(PathUtils::timelapseDir());
   this->_videoFrameModel->setFileFilter("*.jpg");

   this->setupUi();
   this->setupConnections();

   // Select initial content
   this->selectInitialFolder();

   // Start screenshot capture
   this->_photographer->start();
}

MainWindow::~MainWindow() {
   this->_photographer->stop();
}

void MainWindow::setupUi() {
   this->_centralWidget = new QWidget(this);
   setCentralWidget(this->_centralWidget);

   auto* mainLayout = new QVBoxLayout(this->_centralWidget);
   mainLayout->setContentsMargins(0, 0, 0, 0);
   mainLayout->setSpacing(0);

   // Header
   auto* header = new QWidget();
   header->setStyleSheet("background-color: #f3f4f6; border-bottom: 1px solid #d1d5db;");
   auto* headerLayout = new QHBoxLayout(header);
   headerLayout->setContentsMargins(12, 8, 12, 8);

   // Title
   auto* titleLabel = new QLabel("Timelapse Viewer");
   titleLabel->setStyleSheet("font-size: 18px; font-weight: 600;");
   headerLayout->addWidget(titleLabel);

   headerLayout->addSpacing(16);

   // Mode toggle
   auto* modeContainer = new QWidget();
   modeContainer->setStyleSheet("background-color: #e5e7eb; border-radius: 8px; padding: 4px;");
   auto* modeLayout = new QHBoxLayout(modeContainer);
   modeLayout->setContentsMargins(4, 4, 4, 4);
   modeLayout->setSpacing(0);

   this->_imagesButton = new QPushButton("Images");
   this->_videosButton = new QPushButton("Videos");

   QString buttonStyle = R"(
      QPushButton {
         background: transparent;
         border: none;
         padding: 6px 12px;
         border-radius: 6px;
         font-weight: 500;
         color: #4b5563;
      }
      QPushButton:checked {
         background: white;
         color: #111827;
      }
      QPushButton:hover:!checked {
         color: #111827;
      }
   )";
   this->_imagesButton->setStyleSheet(buttonStyle);
   this->_videosButton->setStyleSheet(buttonStyle);
   this->_imagesButton->setCheckable(true);
   this->_videosButton->setCheckable(true);
   this->_imagesButton->setChecked(true);

   this->_modeButtonGroup = new QButtonGroup(this);
   this->_modeButtonGroup->addButton(this->_imagesButton, 0);
   this->_modeButtonGroup->addButton(this->_videosButton, 1);
   this->_modeButtonGroup->setExclusive(true);

   modeLayout->addWidget(this->_imagesButton);
   modeLayout->addWidget(this->_videosButton);
   headerLayout->addWidget(modeContainer);

   headerLayout->addSpacing(16);

   // Folder selector (images mode)
   this->_folderCombo = new QComboBox();
   this->_folderCombo->setMinimumWidth(150);
   this->_folderCombo->setStyleSheet(R"(
      QComboBox {
         background: #374151;
         color: white;
         border: 1px solid #4b5563;
         border-radius: 4px;
         padding: 4px 8px;
      }
   )");
   headerLayout->addWidget(this->_folderCombo);

   // Video selector (videos mode)
   this->_videoCombo = new QComboBox();
   this->_videoCombo->setMinimumWidth(200);
   this->_videoCombo->setStyleSheet(this->_folderCombo->styleSheet());
   this->_videoCombo->setVisible(false);
   headerLayout->addWidget(this->_videoCombo);

   headerLayout->addSpacing(16);

   // Frame count label
   this->_frameCountLabel = new QLabel();
   this->_frameCountLabel->setStyleSheet("color: #4b5563; font-size: 13px;");
   headerLayout->addWidget(this->_frameCountLabel);

   headerLayout->addSpacing(16);

   // Timestamp label
   this->_timestampLabel = new QLabel();
   this->_timestampLabel->setStyleSheet("color: #4b5563; font-size: 13px; font-family: monospace;");
   headerLayout->addWidget(this->_timestampLabel);

   headerLayout->addStretch();

   // Status label
   this->_statusLabel = new QLabel();
   this->_statusLabel->setStyleSheet("color: #6b7280; font-size: 12px;");
   headerLayout->addWidget(this->_statusLabel);

   mainLayout->addWidget(header);

   // Image viewer
   this->_imageViewer = new ImageViewer();
   mainLayout->addWidget(this->_imageViewer, 1);

   // Bottom controls
   auto* controls = new QWidget();
   controls->setStyleSheet("background-color: #f3f4f6;");
   auto* controlsLayout = new QHBoxLayout(controls);
   controlsLayout->setContentsMargins(16, 12, 16, 12);

   // Scrubber
   this->_scrubber = new FrameScrubber();
   controlsLayout->addWidget(this->_scrubber, 1);

   controlsLayout->addSpacing(16);

   // Refresh button
   this->_refreshButton = new QPushButton("↻ Refresh");
   this->_refreshButton->setStyleSheet(R"(
      QPushButton {
         background: qlineargradient(y1:0, y2:1, stop:0 #fdf4ff, stop:1 #fffbeb);
         border: 2px solid #eab308;
         border-radius: 12px;
         padding: 6px 12px;
         font-weight: 500;
         color: #92400e;
      }
      QPushButton:hover {
         background: qlineargradient(y1:0, y2:1, stop:0 #fae8ff, stop:1 #fef3c7);
      }
      QPushButton:disabled {
         background: #e5e7eb;
         border-color: #9ca3af;
         color: #9ca3af;
      }
   )");
   controlsLayout->addWidget(this->_refreshButton);

   mainLayout->addWidget(controls);

   // Populate folder combo
   for (int i = this->_folderModel->count() - 1; i >= 0; --i) {
      QString folder = this->_folderModel->folderAt(i);
      QString displayText = folder;
      if (folder == this->_folderModel->todayFolderName()) {
         displayText += " (Today)";
      }
      this->_folderCombo->addItem(displayText, folder);
   }

   // Populate video combo
   for (int i = 0; i < this->_videoModel->count(); ++i) {
      this->_videoCombo->addItem(this->_videoModel->videoAt(i));
   }
}

void MainWindow::setupConnections() {
   // Mode toggle
   connect(this->_modeButtonGroup, &QButtonGroup::idClicked, this, [this](int id) {
      this->onViewModeChanged(id == 0 ? ViewMode::Images : ViewMode::Videos);
   });

   // Folder selection
   connect(this->_folderCombo, QOverload<int>::of(&QComboBox::currentIndexChanged),
           this, &MainWindow::onFolderSelected);

   // Video selection
   connect(this->_videoCombo, QOverload<int>::of(&QComboBox::currentIndexChanged),
           this, &MainWindow::onVideoSelected);

   // Scrubber
   connect(this->_scrubber, &FrameScrubber::valueChanged,
           this, &MainWindow::onFrameChanged);

   // Refresh button
   connect(this->_refreshButton, &QPushButton::clicked,
           this, &MainWindow::onRefreshClicked);

   // File model changes
   connect(this->_fileModel.get(), &FileListModel::countChanged, this, [this]() {
      if (this->_viewMode == ViewMode::Images) {
         this->_scrubber->setRange(0, std::max(0, this->_fileModel->count() - 1));
         this->_scrubber->setValue(std::max(0, this->_fileModel->count() - 1));
         this->updateFrameCountDisplay();
      }
   });

   // Video frame model changes
   connect(this->_videoFrameModel.get(), &FileListModel::countChanged, this, [this]() {
      if (this->_viewMode == ViewMode::Videos) {
         this->_scrubber->setRange(0, std::max(0, this->_videoFrameModel->count() - 1));
         this->_scrubber->setValue(0);
         this->updateFrameCountDisplay();
      }
   });

   // Photographer signals
   connect(this->_photographer.get(), &Photographer::screenshotCaptured,
           this, [this](QString const& path, uint32_t frameNumber) {
      Q_UNUSED(path)
      Q_UNUSED(frameNumber)
      this->_statusLabel->setText("Screenshot captured");
   });

   connect(this->_photographer.get(), &Photographer::errorOccurred,
           this, [this](QString const& error) {
      this->_statusLabel->setText("Error: " + error);
   });
}

void MainWindow::keyPressEvent(QKeyEvent* event) {
   int step = 1;
   if (event->modifiers() & Qt::ShiftModifier) {
      step = 10;
   }
   if (event->modifiers() & Qt::AltModifier) {
      step = 100;
   }

   switch (event->key()) {
   case Qt::Key_Left:
      this->navigateFrames(-step);
      event->accept();
      break;
   case Qt::Key_Right:
      this->navigateFrames(step);
      event->accept();
      break;
   default:
      QMainWindow::keyPressEvent(event);
   }
}

void MainWindow::onViewModeChanged(ViewMode mode) {
   if (this->_viewMode == mode) {
      return;
   }

   this->_viewMode = mode;

   // Update UI visibility
   this->_folderCombo->setVisible(mode == ViewMode::Images);
   this->_videoCombo->setVisible(mode == ViewMode::Videos);

   // Update scrubber
   if (mode == ViewMode::Images) {
      this->_scrubber->setRange(0, std::max(0, this->_fileModel->count() - 1));
      this->_scrubber->setValue(std::max(0, this->_fileModel->count() - 1));
   } else {
      this->selectInitialVideo();
   }

   this->updateFrameCountDisplay();
   this->loadCurrentFrame();
}

void MainWindow::onFolderSelected(int index) {
   if (index < 0) {
      return;
   }

   QString folder = this->_folderCombo->itemData(index).toString();
   QString folderPath = PathUtils::dayFolder(folder);
   this->_fileModel->setFolderPath(folderPath);
}

void MainWindow::onVideoSelected(int index) {
   if (index < 0 || this->_isExtractingFrames) {
      return;
   }

   QString videoPath = this->_videoModel->pathAt(index);
   QString videoFilename = this->_videoModel->videoAt(index);

   this->_isExtractingFrames = true;
   this->_imageViewer->setPlaceholderText("Extracting frames from video...");
   this->_imageViewer->clear();

   // Extract frames (or use cache)
   auto result = this->_videoExtractor->extractFrames(videoPath);
   this->_isExtractingFrames = false;

   if (!result) {
      this->_imageViewer->setPlaceholderText("Failed to extract frames: " + result.error());
      return;
   }

   this->_currentVideoCacheFolder = *result;
   QString cachePath = PathUtils::cacheDir() + "/" + this->_currentVideoCacheFolder;
   this->_videoFrameModel->setFolderPath(cachePath);
}

void MainWindow::onFrameChanged(int frame) {
   this->_currentFrame = frame;
   this->loadCurrentFrame();
   this->updateTimestampDisplay();
   this->updateFrameCountDisplay();
}

void MainWindow::onRefreshClicked() {
   if (this->_viewMode == ViewMode::Images) {
      this->_folderModel->refresh();
      this->_fileModel->refresh();
   } else {
      this->_videoModel->refresh();
      this->_videoFrameModel->refresh();
   }
}

void MainWindow::loadCurrentFrame() {
   QString path;

   if (this->_viewMode == ViewMode::Images) {
      if (this->_fileModel->count() == 0) {
         this->_imageViewer->setPlaceholderText("No screenshots available");
         this->_imageViewer->clear();
         return;
      }
      path = this->_fileModel->pathAt(this->_currentFrame);
   } else {
      if (this->_videoFrameModel->count() == 0) {
         if (!this->_isExtractingFrames) {
            this->_imageViewer->setPlaceholderText("No video frames available");
         }
         this->_imageViewer->clear();
         return;
      }
      path = this->_videoFrameModel->pathAt(this->_currentFrame);
   }

   this->_imageViewer->setImagePath(path);
}

void MainWindow::updateTimestampDisplay() {
   if (this->_viewMode != ViewMode::Images || this->_fileModel->count() == 0) {
      this->_timestampLabel->clear();
      return;
   }

   int32_t frameNumber = this->_fileModel->frameNumberAt(this->_currentFrame);
   auto metadata = this->_photographer->getScreenshotMetadata(frameNumber);

   QString timeStr;
   bool isEstimate = true;

   if (metadata && *metadata) {
      QDateTime localTime = (*metadata)->localTime;
      timeStr = localTime.toString("HH:mm");
      isEstimate = false;
   } else {
      // Estimate based on frame number (1 frame per second)
      int totalMinutes = this->_currentFrame / 60;
      int hours = totalMinutes / 60;
      int minutes = totalMinutes % 60;
      timeStr = QString("%1:%2")
                   .arg(hours, 2, 10, QChar('0'))
                   .arg(minutes, 2, 10, QChar('0'));
   }

   this->_timestampLabel->setText(isEstimate ? "~" + timeStr : timeStr);
}

void MainWindow::updateFrameCountDisplay() {
   int count = this->_viewMode == ViewMode::Images
                  ? this->_fileModel->count()
                  : this->_videoFrameModel->count();

   if (count == 0) {
      this->_frameCountLabel->clear();
      return;
   }

   this->_frameCountLabel->setText(QString("Frame %1 / %2")
                                 .arg(this->_currentFrame + 1)
                                 .arg(count));
}

void MainWindow::navigateFrames(int delta) {
   int maxFrame = this->_viewMode == ViewMode::Images
                     ? this->_fileModel->count() - 1
                     : this->_videoFrameModel->count() - 1;

   int newFrame = std::clamp(this->_currentFrame + delta, 0, std::max(0, maxFrame));
   this->_scrubber->setValue(newFrame);
}

void MainWindow::selectInitialFolder() {
   if (this->_folderModel->count() == 0) {
      return;
   }

   // Try to select today's folder
   QString today = this->_folderModel->todayFolderName();
   int todayIndex = this->_folderModel->indexOf(today);

   if (todayIndex >= 0) {
      // Find in combo (reversed order)
      for (int i = 0; i < this->_folderCombo->count(); ++i) {
         if (this->_folderCombo->itemData(i).toString() == today) {
            this->_folderCombo->setCurrentIndex(i);
            return;
         }
      }
   }

   // Otherwise select most recent (first in combo since it's reversed)
   if (this->_folderCombo->count() > 0) {
      this->_folderCombo->setCurrentIndex(0);
   }
}

void MainWindow::selectInitialVideo() {
   if (this->_videoModel->count() == 0) {
      return;
   }

   // Select most recent video (last in list)
   this->_videoCombo->setCurrentIndex(this->_videoModel->count() - 1);
}

}  // namespace timelapse
