#pragma once

#include <QButtonGroup>
#include <QComboBox>
#include <QLabel>
#include <QMainWindow>
#include <QPushButton>
#include <memory>

#include "core/CacheManager.hh"
#include "core/Photographer.hh"
#include "core/VideoExtractor.hh"
#include "models/FileListModel.hh"
#include "models/FolderListModel.hh"
#include "models/VideoListModel.hh"
#include "FrameScrubber.hh"
#include "ImageViewer.hh"

namespace timelapse {

enum class ViewMode {
   Images,
   Videos
};

class MainWindow : public QMainWindow {
   Q_OBJECT

public:
   explicit MainWindow(QWidget* parent = nullptr);
   ~MainWindow() override;

protected:
   void keyPressEvent(QKeyEvent* event) override;

private slots:
   void onViewModeChanged(ViewMode mode);
   void onFolderSelected(int index);
   void onVideoSelected(int index);
   void onFrameChanged(int frame);
   void onRefreshClicked();
   void loadCurrentFrame();
   void updateTimestampDisplay();
   void updateFrameCountDisplay();

private:
   void setupUi();
   void setupConnections();
   void navigateFrames(int delta);
   void selectInitialFolder();
   void selectInitialVideo();

   // Core components
   std::unique_ptr<Photographer> _photographer;
   std::unique_ptr<VideoExtractor> _videoExtractor;
   std::unique_ptr<CacheManager> _cacheManager;

   // Models
   std::unique_ptr<FolderListModel> _folderModel;
   std::unique_ptr<FileListModel> _fileModel;
   std::unique_ptr<VideoListModel> _videoModel;
   std::unique_ptr<FileListModel> _videoFrameModel;

   // UI widgets
   QWidget* _centralWidget{nullptr};
   QPushButton* _imagesButton{nullptr};
   QPushButton* _videosButton{nullptr};
   QButtonGroup* _modeButtonGroup{nullptr};
   QComboBox* _folderCombo{nullptr};
   QComboBox* _videoCombo{nullptr};
   QLabel* _frameCountLabel{nullptr};
   QLabel* _timestampLabel{nullptr};
   QLabel* _statusLabel{nullptr};
   ImageViewer* _imageViewer{nullptr};
   FrameScrubber* _scrubber{nullptr};
   QPushButton* _refreshButton{nullptr};

   // State
   ViewMode _viewMode{ViewMode::Images};
   int _currentFrame{0};
   QString _currentVideoCacheFolder;
   bool _isExtractingFrames{false};
};

}  // namespace timelapse
