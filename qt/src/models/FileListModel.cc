#include "FileListModel.hh"

#include <QDir>
#include <QRegularExpression>

namespace timelapse {

FileListModel::FileListModel(QObject* parent)
   : QAbstractListModel(parent)
   , _watcher(new QFileSystemWatcher(this)) {
   connect(this->_watcher, &QFileSystemWatcher::directoryChanged,
           this, &FileListModel::refresh);
}

auto FileListModel::rowCount(QModelIndex const& parent) const -> int {
   if (parent.isValid()) {
      return 0;
   }
   return static_cast<int>(this->_files.size());
}

auto FileListModel::data(QModelIndex const& index, int role) const -> QVariant {
   if (!index.isValid() || index.row() >= this->_files.size()) {
      return {};
   }

   QString const& file = this->_files.at(index.row());

   switch (role) {
   case Qt::DisplayRole:
   case NameRole:
      return file;
   case PathRole:
      return this->_folderPath + "/" + file;
   case FrameNumberRole:
      return this->extractFrameNumber(file);
   default:
      return {};
   }
}

auto FileListModel::roleNames() const -> QHash<int, QByteArray> {
   return {
      {NameRole, "name"},
      {PathRole, "path"},
      {FrameNumberRole, "frameNumber"},
   };
}

auto FileListModel::folderPath() const -> QString {
   return this->_folderPath;
}

void FileListModel::setFolderPath(QString const& path) {
   if (this->_folderPath == path) {
      return;
   }

   // Remove old path from watcher
   if (!this->_folderPath.isEmpty()) {
      this->_watcher->removePath(this->_folderPath);
   }

   this->_folderPath = path;

   // Add new path to watcher
   if (!this->_folderPath.isEmpty() && QDir(this->_folderPath).exists()) {
      this->_watcher->addPath(this->_folderPath);
   }

   emit folderPathChanged();
   this->loadFiles();
}

auto FileListModel::fileFilter() const -> QString {
   return this->_fileFilter;
}

void FileListModel::setFileFilter(QString const& filter) {
   if (this->_fileFilter == filter) {
      return;
   }

   this->_fileFilter = filter;
   emit fileFilterChanged();
   this->loadFiles();
}

auto FileListModel::count() const -> int {
   return static_cast<int>(this->_files.size());
}

auto FileListModel::fileAt(int index) const -> QString {
   if (index < 0 || index >= this->_files.size()) {
      return {};
   }
   return this->_files.at(index);
}

auto FileListModel::pathAt(int index) const -> QString {
   if (index < 0 || index >= this->_files.size()) {
      return {};
   }
   return this->_folderPath + "/" + this->_files.at(index);
}

auto FileListModel::frameNumberAt(int index) const -> int32_t {
   if (index < 0 || index >= this->_files.size()) {
      return 0;
   }
   return this->extractFrameNumber(this->_files.at(index));
}

void FileListModel::refresh() {
   this->loadFiles();
}

void FileListModel::loadFiles() {
   beginResetModel();
   this->_files.clear();

   if (this->_folderPath.isEmpty()) {
      endResetModel();
      emit countChanged();
      return;
   }

   QDir dir(this->_folderPath);
   if (!dir.exists()) {
      endResetModel();
      emit countChanged();
      return;
   }

   QStringList filters;
   filters << this->_fileFilter;

   this->_files = dir.entryList(filters, QDir::Files, QDir::Name);

   endResetModel();
   emit countChanged();
}

auto FileListModel::extractFrameNumber(QString const& filename) const -> int32_t {
   // Extract numeric part from filename like "00001.png" or "00001.jpg"
   static QRegularExpression re("^(\\d+)\\.");
   auto match = re.match(filename);

   if (match.hasMatch()) {
      return match.captured(1).toInt();
   }

   return 0;
}

}  // namespace timelapse
