#include "FileListModel.hh"

#include <QDir>
#include <QRegularExpression>

namespace timelapse {

FileListModel::FileListModel(QObject* parent)
   : QAbstractListModel(parent)
   , m_watcher(new QFileSystemWatcher(this)) {
   connect(m_watcher, &QFileSystemWatcher::directoryChanged,
           this, &FileListModel::refresh);
}

auto FileListModel::rowCount(QModelIndex const& parent) const -> int {
   if (parent.isValid()) {
      return 0;
   }
   return static_cast<int>(m_files.size());
}

auto FileListModel::data(QModelIndex const& index, int role) const -> QVariant {
   if (!index.isValid() || index.row() >= m_files.size()) {
      return {};
   }

   QString const& file = m_files.at(index.row());

   switch (role) {
   case Qt::DisplayRole:
   case NameRole:
      return file;
   case PathRole:
      return m_folderPath + "/" + file;
   case FrameNumberRole:
      return extractFrameNumber(file);
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
   return m_folderPath;
}

void FileListModel::setFolderPath(QString const& path) {
   if (m_folderPath == path) {
      return;
   }

   // Remove old path from watcher
   if (!m_folderPath.isEmpty()) {
      m_watcher->removePath(m_folderPath);
   }

   m_folderPath = path;

   // Add new path to watcher
   if (!m_folderPath.isEmpty() && QDir(m_folderPath).exists()) {
      m_watcher->addPath(m_folderPath);
   }

   emit folderPathChanged();
   loadFiles();
}

auto FileListModel::fileFilter() const -> QString {
   return m_fileFilter;
}

void FileListModel::setFileFilter(QString const& filter) {
   if (m_fileFilter == filter) {
      return;
   }

   m_fileFilter = filter;
   emit fileFilterChanged();
   loadFiles();
}

auto FileListModel::count() const -> int {
   return static_cast<int>(m_files.size());
}

auto FileListModel::fileAt(int index) const -> QString {
   if (index < 0 || index >= m_files.size()) {
      return {};
   }
   return m_files.at(index);
}

auto FileListModel::pathAt(int index) const -> QString {
   if (index < 0 || index >= m_files.size()) {
      return {};
   }
   return m_folderPath + "/" + m_files.at(index);
}

auto FileListModel::frameNumberAt(int index) const -> int32_t {
   if (index < 0 || index >= m_files.size()) {
      return 0;
   }
   return extractFrameNumber(m_files.at(index));
}

void FileListModel::refresh() {
   loadFiles();
}

void FileListModel::loadFiles() {
   beginResetModel();
   m_files.clear();

   if (m_folderPath.isEmpty()) {
      endResetModel();
      emit countChanged();
      return;
   }

   QDir dir(m_folderPath);
   if (!dir.exists()) {
      endResetModel();
      emit countChanged();
      return;
   }

   QStringList filters;
   filters << m_fileFilter;

   m_files = dir.entryList(filters, QDir::Files, QDir::Name);

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
