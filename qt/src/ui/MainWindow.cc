#include "MainWindow.hh"

#include <QDateTime>
#include <QHBoxLayout>
#include <QKeyEvent>
#include <QVBoxLayout>

#include "utils/PathUtils.hh"

namespace timelapse {

MainWindow::MainWindow(QWidget* parent)
   : QMainWindow(parent)
   , m_photographer(std::make_unique<Photographer>(this))
   , m_videoExtractor(std::make_unique<VideoExtractor>(this))
   , m_cacheManager(std::make_unique<CacheManager>(this))
   , m_folderModel(std::make_unique<FolderListModel>(this))
   , m_fileModel(std::make_unique<FileListModel>(this))
   , m_videoModel(std::make_unique<VideoListModel>(this))
   , m_videoFrameModel(std::make_unique<FileListModel>(this)) {
   setWindowTitle("Timelapse Viewer");
   resize(1200, 800);

   // Setup models
   m_folderModel->setBasePath(PathUtils::timelapseDir());
   m_videoModel->setBasePath(PathUtils::timelapseDir());
   m_videoFrameModel->setFileFilter("*.jpg");

   setupUi();
   setupConnections();

   // Select initial content
   selectInitialFolder();

   // Start screenshot capture
   m_photographer->start();
}

MainWindow::~MainWindow() {
   m_photographer->stop();
}

void MainWindow::setupUi() {
   m_centralWidget = new QWidget(this);
   setCentralWidget(m_centralWidget);

   auto* mainLayout = new QVBoxLayout(m_centralWidget);
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

   m_imagesButton = new QPushButton("Images");
   m_videosButton = new QPushButton("Videos");

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
   m_imagesButton->setStyleSheet(buttonStyle);
   m_videosButton->setStyleSheet(buttonStyle);
   m_imagesButton->setCheckable(true);
   m_videosButton->setCheckable(true);
   m_imagesButton->setChecked(true);

   m_modeButtonGroup = new QButtonGroup(this);
   m_modeButtonGroup->addButton(m_imagesButton, 0);
   m_modeButtonGroup->addButton(m_videosButton, 1);
   m_modeButtonGroup->setExclusive(true);

   modeLayout->addWidget(m_imagesButton);
   modeLayout->addWidget(m_videosButton);
   headerLayout->addWidget(modeContainer);

   headerLayout->addSpacing(16);

   // Folder selector (images mode)
   m_folderCombo = new QComboBox();
   m_folderCombo->setMinimumWidth(150);
   m_folderCombo->setStyleSheet(R"(
      QComboBox {
         background: #374151;
         color: white;
         border: 1px solid #4b5563;
         border-radius: 4px;
         padding: 4px 8px;
      }
   )");
   headerLayout->addWidget(m_folderCombo);

   // Video selector (videos mode)
   m_videoCombo = new QComboBox();
   m_videoCombo->setMinimumWidth(200);
   m_videoCombo->setStyleSheet(m_folderCombo->styleSheet());
   m_videoCombo->setVisible(false);
   headerLayout->addWidget(m_videoCombo);

   headerLayout->addSpacing(16);

   // Frame count label
   m_frameCountLabel = new QLabel();
   m_frameCountLabel->setStyleSheet("color: #4b5563; font-size: 13px;");
   headerLayout->addWidget(m_frameCountLabel);

   headerLayout->addSpacing(16);

   // Timestamp label
   m_timestampLabel = new QLabel();
   m_timestampLabel->setStyleSheet("color: #4b5563; font-size: 13px; font-family: monospace;");
   headerLayout->addWidget(m_timestampLabel);

   headerLayout->addStretch();

   // Status label
   m_statusLabel = new QLabel();
   m_statusLabel->setStyleSheet("color: #6b7280; font-size: 12px;");
   headerLayout->addWidget(m_statusLabel);

   mainLayout->addWidget(header);

   // Image viewer
   m_imageViewer = new ImageViewer();
   mainLayout->addWidget(m_imageViewer, 1);

   // Bottom controls
   auto* controls = new QWidget();
   controls->setStyleSheet("background-color: #f3f4f6;");
   auto* controlsLayout = new QHBoxLayout(controls);
   controlsLayout->setContentsMargins(16, 12, 16, 12);

   // Scrubber
   m_scrubber = new FrameScrubber();
   controlsLayout->addWidget(m_scrubber, 1);

   controlsLayout->addSpacing(16);

   // Refresh button
   m_refreshButton = new QPushButton("↻ Refresh");
   m_refreshButton->setStyleSheet(R"(
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
   controlsLayout->addWidget(m_refreshButton);

   mainLayout->addWidget(controls);

   // Populate folder combo
   for (int i = m_folderModel->count() - 1; i >= 0; --i) {
      QString folder = m_folderModel->folderAt(i);
      QString displayText = folder;
      if (folder == m_folderModel->todayFolderName()) {
         displayText += " (Today)";
      }
      m_folderCombo->addItem(displayText, folder);
   }

   // Populate video combo
   for (int i = 0; i < m_videoModel->count(); ++i) {
      m_videoCombo->addItem(m_videoModel->videoAt(i));
   }
}

void MainWindow::setupConnections() {
   // Mode toggle
   connect(m_modeButtonGroup, &QButtonGroup::idClicked, this, [this](int id) {
      onViewModeChanged(id == 0 ? ViewMode::Images : ViewMode::Videos);
   });

   // Folder selection
   connect(m_folderCombo, QOverload<int>::of(&QComboBox::currentIndexChanged),
           this, &MainWindow::onFolderSelected);

   // Video selection
   connect(m_videoCombo, QOverload<int>::of(&QComboBox::currentIndexChanged),
           this, &MainWindow::onVideoSelected);

   // Scrubber
   connect(m_scrubber, &FrameScrubber::valueChanged,
           this, &MainWindow::onFrameChanged);

   // Refresh button
   connect(m_refreshButton, &QPushButton::clicked,
           this, &MainWindow::onRefreshClicked);

   // File model changes
   connect(m_fileModel.get(), &FileListModel::countChanged, this, [this]() {
      if (m_viewMode == ViewMode::Images) {
         m_scrubber->setRange(0, std::max(0, m_fileModel->count() - 1));
         m_scrubber->setValue(std::max(0, m_fileModel->count() - 1));
         updateFrameCountDisplay();
      }
   });

   // Video frame model changes
   connect(m_videoFrameModel.get(), &FileListModel::countChanged, this, [this]() {
      if (m_viewMode == ViewMode::Videos) {
         m_scrubber->setRange(0, std::max(0, m_videoFrameModel->count() - 1));
         m_scrubber->setValue(0);
         updateFrameCountDisplay();
      }
   });

   // Photographer signals
   connect(m_photographer.get(), &Photographer::screenshotCaptured,
           this, [this](QString const& path, uint32_t frameNumber) {
      Q_UNUSED(path)
      Q_UNUSED(frameNumber)
      m_statusLabel->setText("Screenshot captured");
   });

   connect(m_photographer.get(), &Photographer::errorOccurred,
           this, [this](QString const& error) {
      m_statusLabel->setText("Error: " + error);
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
      navigateFrames(-step);
      event->accept();
      break;
   case Qt::Key_Right:
      navigateFrames(step);
      event->accept();
      break;
   default:
      QMainWindow::keyPressEvent(event);
   }
}

void MainWindow::onViewModeChanged(ViewMode mode) {
   if (m_viewMode == mode) {
      return;
   }

   m_viewMode = mode;

   // Update UI visibility
   m_folderCombo->setVisible(mode == ViewMode::Images);
   m_videoCombo->setVisible(mode == ViewMode::Videos);

   // Update scrubber
   if (mode == ViewMode::Images) {
      m_scrubber->setRange(0, std::max(0, m_fileModel->count() - 1));
      m_scrubber->setValue(std::max(0, m_fileModel->count() - 1));
   } else {
      selectInitialVideo();
   }

   updateFrameCountDisplay();
   loadCurrentFrame();
}

void MainWindow::onFolderSelected(int index) {
   if (index < 0) {
      return;
   }

   QString folder = m_folderCombo->itemData(index).toString();
   QString folderPath = PathUtils::dayFolder(folder);
   m_fileModel->setFolderPath(folderPath);
}

void MainWindow::onVideoSelected(int index) {
   if (index < 0 || m_isExtractingFrames) {
      return;
   }

   QString videoPath = m_videoModel->pathAt(index);
   QString videoFilename = m_videoModel->videoAt(index);

   m_isExtractingFrames = true;
   m_imageViewer->setPlaceholderText("Extracting frames from video...");
   m_imageViewer->clear();

   // Extract frames (or use cache)
   auto result = m_videoExtractor->extractFrames(videoPath);
   m_isExtractingFrames = false;

   if (!result) {
      m_imageViewer->setPlaceholderText("Failed to extract frames: " + result.error());
      return;
   }

   m_currentVideoCacheFolder = *result;
   QString cachePath = PathUtils::cacheDir() + "/" + m_currentVideoCacheFolder;
   m_videoFrameModel->setFolderPath(cachePath);
}

void MainWindow::onFrameChanged(int frame) {
   m_currentFrame = frame;
   loadCurrentFrame();
   updateTimestampDisplay();
   updateFrameCountDisplay();
}

void MainWindow::onRefreshClicked() {
   if (m_viewMode == ViewMode::Images) {
      m_folderModel->refresh();
      m_fileModel->refresh();
   } else {
      m_videoModel->refresh();
      m_videoFrameModel->refresh();
   }
}

void MainWindow::loadCurrentFrame() {
   QString path;

   if (m_viewMode == ViewMode::Images) {
      if (m_fileModel->count() == 0) {
         m_imageViewer->setPlaceholderText("No screenshots available");
         m_imageViewer->clear();
         return;
      }
      path = m_fileModel->pathAt(m_currentFrame);
   } else {
      if (m_videoFrameModel->count() == 0) {
         if (!m_isExtractingFrames) {
            m_imageViewer->setPlaceholderText("No video frames available");
         }
         m_imageViewer->clear();
         return;
      }
      path = m_videoFrameModel->pathAt(m_currentFrame);
   }

   m_imageViewer->setImagePath(path);
}

void MainWindow::updateTimestampDisplay() {
   if (m_viewMode != ViewMode::Images || m_fileModel->count() == 0) {
      m_timestampLabel->clear();
      return;
   }

   int32_t frameNumber = m_fileModel->frameNumberAt(m_currentFrame);
   auto metadata = m_photographer->getScreenshotMetadata(frameNumber);

   QString timeStr;
   bool isEstimate = true;

   if (metadata && *metadata) {
      QDateTime localTime = (*metadata)->localTime;
      timeStr = localTime.toString("HH:mm");
      isEstimate = false;
   } else {
      // Estimate based on frame number (1 frame per second)
      int totalMinutes = m_currentFrame / 60;
      int hours = totalMinutes / 60;
      int minutes = totalMinutes % 60;
      timeStr = QString("%1:%2")
                   .arg(hours, 2, 10, QChar('0'))
                   .arg(minutes, 2, 10, QChar('0'));
   }

   m_timestampLabel->setText(isEstimate ? "~" + timeStr : timeStr);
}

void MainWindow::updateFrameCountDisplay() {
   int count = m_viewMode == ViewMode::Images
                  ? m_fileModel->count()
                  : m_videoFrameModel->count();

   if (count == 0) {
      m_frameCountLabel->clear();
      return;
   }

   m_frameCountLabel->setText(QString("Frame %1 / %2")
                                 .arg(m_currentFrame + 1)
                                 .arg(count));
}

void MainWindow::navigateFrames(int delta) {
   int maxFrame = m_viewMode == ViewMode::Images
                     ? m_fileModel->count() - 1
                     : m_videoFrameModel->count() - 1;

   int newFrame = std::clamp(m_currentFrame + delta, 0, std::max(0, maxFrame));
   m_scrubber->setValue(newFrame);
}

void MainWindow::selectInitialFolder() {
   if (m_folderModel->count() == 0) {
      return;
   }

   // Try to select today's folder
   QString today = m_folderModel->todayFolderName();
   int todayIndex = m_folderModel->indexOf(today);

   if (todayIndex >= 0) {
      // Find in combo (reversed order)
      for (int i = 0; i < m_folderCombo->count(); ++i) {
         if (m_folderCombo->itemData(i).toString() == today) {
            m_folderCombo->setCurrentIndex(i);
            return;
         }
      }
   }

   // Otherwise select most recent (first in combo since it's reversed)
   if (m_folderCombo->count() > 0) {
      m_folderCombo->setCurrentIndex(0);
   }
}

void MainWindow::selectInitialVideo() {
   if (m_videoModel->count() == 0) {
      return;
   }

   // Select most recent video (last in list)
   m_videoCombo->setCurrentIndex(m_videoModel->count() - 1);
}

}  // namespace timelapse
