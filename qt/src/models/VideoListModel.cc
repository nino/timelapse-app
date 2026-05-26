#include "VideoListModel.hh"

#include <QDir>
#include <QFileInfo>

namespace timelapse {

VideoListModel::VideoListModel(QObject* parent)
   : QAbstractListModel(parent)
   , _watcher(new QFileSystemWatcher(this)) {
   connect(this->_watcher, &QFileSystemWatcher::directoryChanged,
           this, &VideoListModel::refresh);
}

auto VideoListModel::rowCount(QModelIndex const& parent) const -> int {
   if (parent.isValid()) {
      return 0;
   }
   return static_cast<int>(this->_videos.size());
}

auto VideoListModel::data(QModelIndex const& index, int role) const -> QVariant {
   if (!index.isValid() || index.row() >= this->_videos.size()) {
      return {};
   }

   QString const& video = this->_videos.at(index.row());

   switch (role) {
   case Qt::DisplayRole:
   case NameRole:
      return video;
   case PathRole:
      return this->_basePath + "/" + video;
   case SizeRole: {
      QFileInfo info(this->_basePath + "/" + video);
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
   return this->_basePath;
}

void VideoListModel::setBasePath(QString const& path) {
   if (this->_basePath == path) {
      return;
   }

   // Remove old path from watcher
   if (!this->_basePath.isEmpty()) {
      this->_watcher->removePath(this->_basePath);
   }

   this->_basePath = path;

   // Add new path to watcher
   if (!this->_basePath.isEmpty() && QDir(this->_basePath).exists()) {
      this->_watcher->addPath(this->_basePath);
   }

   emit basePathChanged();
   this->loadVideos();
}

auto VideoListModel::count() const -> int {
   return static_cast<int>(this->_videos.size());
}

auto VideoListModel::videoAt(int index) const -> QString {
   if (index < 0 || index >= this->_videos.size()) {
      return {};
   }
   return this->_videos.at(index);
}

auto VideoListModel::pathAt(int index) const -> QString {
   if (index < 0 || index >= this->_videos.size()) {
      return {};
   }
   return this->_basePath + "/" + this->_videos.at(index);
}

auto VideoListModel::mostRecentVideo() const -> QString {
   if (this->_videos.isEmpty()) {
      return {};
   }
   // Videos are sorted by name, last is most recent
   return this->_videos.last();
}

void VideoListModel::refresh() {
   this->loadVideos();
}

void VideoListModel::loadVideos() {
   beginResetModel();
   this->_videos.clear();

   if (this->_basePath.isEmpty()) {
      endResetModel();
      emit countChanged();
      return;
   }

   QDir dir(this->_basePath);
   if (!dir.exists()) {
      endResetModel();
      emit countChanged();
      return;
   }

   // Filter for video files
   QStringList filters;
   filters << "*.mov" << "*.mp4" << "*.avi" << "*.mkv";

   this->_videos = dir.entryList(filters, QDir::Files, QDir::Name);

   endResetModel();
   emit countChanged();
}

}  // namespace timelapse
