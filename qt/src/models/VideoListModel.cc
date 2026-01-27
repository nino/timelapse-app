#include "VideoListModel.hh"

#include <QDir>
#include <QFileInfo>

namespace timelapse {

VideoListModel::VideoListModel(QObject* parent)
   : QAbstractListModel(parent)
   , m_watcher(new QFileSystemWatcher(this)) {
   connect(m_watcher, &QFileSystemWatcher::directoryChanged,
           this, &VideoListModel::refresh);
}

auto VideoListModel::rowCount(QModelIndex const& parent) const -> int {
   if (parent.isValid()) {
      return 0;
   }
   return static_cast<int>(m_videos.size());
}

auto VideoListModel::data(QModelIndex const& index, int role) const -> QVariant {
   if (!index.isValid() || index.row() >= m_videos.size()) {
      return {};
   }

   QString const& video = m_videos.at(index.row());

   switch (role) {
   case Qt::DisplayRole:
   case NameRole:
      return video;
   case PathRole:
      return m_basePath + "/" + video;
   case SizeRole: {
      QFileInfo info(m_basePath + "/" + video);
      return info.size();
   }
   default:
      return {};
   }
}

auto VideoListModel::roleNames() const -> QHash<int, QByteArray> {
   return {
      {NameRole, "name"},
      {PathRole, "path"},
      {SizeRole, "size"},
   };
}

auto VideoListModel::basePath() const -> QString {
   return m_basePath;
}

void VideoListModel::setBasePath(QString const& path) {
   if (m_basePath == path) {
      return;
   }

   // Remove old path from watcher
   if (!m_basePath.isEmpty()) {
      m_watcher->removePath(m_basePath);
   }

   m_basePath = path;

   // Add new path to watcher
   if (!m_basePath.isEmpty() && QDir(m_basePath).exists()) {
      m_watcher->addPath(m_basePath);
   }

   emit basePathChanged();
   loadVideos();
}

auto VideoListModel::count() const -> int {
   return static_cast<int>(m_videos.size());
}

auto VideoListModel::videoAt(int index) const -> QString {
   if (index < 0 || index >= m_videos.size()) {
      return {};
   }
   return m_videos.at(index);
}

auto VideoListModel::pathAt(int index) const -> QString {
   if (index < 0 || index >= m_videos.size()) {
      return {};
   }
   return m_basePath + "/" + m_videos.at(index);
}

auto VideoListModel::mostRecentVideo() const -> QString {
   if (m_videos.isEmpty()) {
      return {};
   }
   // Videos are sorted by name, last is most recent
   return m_videos.last();
}

void VideoListModel::refresh() {
   loadVideos();
}

void VideoListModel::loadVideos() {
   beginResetModel();
   m_videos.clear();

   if (m_basePath.isEmpty()) {
      endResetModel();
      emit countChanged();
      return;
   }

   QDir dir(m_basePath);
   if (!dir.exists()) {
      endResetModel();
      emit countChanged();
      return;
   }

   // Filter for video files
   QStringList filters;
   filters << "*.mov" << "*.mp4" << "*.avi" << "*.mkv";

   m_videos = dir.entryList(filters, QDir::Files, QDir::Name);

   endResetModel();
   emit countChanged();
}

}  // namespace timelapse
