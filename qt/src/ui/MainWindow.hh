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
   std::unique_ptr<Photographer> m_photographer;
   std::unique_ptr<VideoExtractor> m_videoExtractor;
   std::unique_ptr<CacheManager> m_cacheManager;

   // Models
   std::unique_ptr<FolderListModel> m_folderModel;
   std::unique_ptr<FileListModel> m_fileModel;
   std::unique_ptr<VideoListModel> m_videoModel;
   std::unique_ptr<FileListModel> m_videoFrameModel;

   // UI widgets
   QWidget* m_centralWidget{nullptr};
   QPushButton* m_imagesButton{nullptr};
   QPushButton* m_videosButton{nullptr};
   QButtonGroup* m_modeButtonGroup{nullptr};
   QComboBox* m_folderCombo{nullptr};
   QComboBox* m_videoCombo{nullptr};
   QLabel* m_frameCountLabel{nullptr};
   QLabel* m_timestampLabel{nullptr};
   QLabel* m_statusLabel{nullptr};
   ImageViewer* m_imageViewer{nullptr};
   FrameScrubber* m_scrubber{nullptr};
   QPushButton* m_refreshButton{nullptr};

   // State
   ViewMode m_viewMode{ViewMode::Images};
   int m_currentFrame{0};
   QString m_currentVideoCacheFolder;
   bool m_isExtractingFrames{false};
};

}  // namespace timelapse
