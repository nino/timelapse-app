#include "FolderListModel.hh"

#include <QDate>
#include <QDir>
#include <QRegularExpression>
#include <algorithm>

namespace timelapse {

FolderListModel::FolderListModel(QObject* parent)
   : QAbstractListModel(parent)
   , m_watcher(new QFileSystemWatcher(this)) {
   connect(m_watcher, &QFileSystemWatcher::directoryChanged,
           this, &FolderListModel::refresh);
}

auto FolderListModel::rowCount(QModelIndex const& parent) const -> int {
   if (parent.isValid()) {
      return 0;
   }
   return static_cast<int>(m_folders.size());
}

auto FolderListModel::data(QModelIndex const& index, int role) const -> QVariant {
   if (!index.isValid() || index.row() >= m_folders.size()) {
      return {};
   }

   QString const& folder = m_folders.at(index.row());

   switch (role) {
   case Qt::DisplayRole:
   case NameRole:
      return folder;
   case PathRole:
      return m_basePath + "/" + folder;
   case IsTodayRole:
      return folder == todayFolderName();
   default:
      return {};
   }
}

auto FolderListModel::roleNames() const -> QHash<int, QByteArray> {
   return {
      {NameRole, "name"},
      {PathRole, "path"},
      {IsTodayRole, "isToday"},
   };
}

auto FolderListModel::basePath() const -> QString {
   return m_basePath;
}

void FolderListModel::setBasePath(QString const& path) {
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
   loadFolders();
}

auto FolderListModel::count() const -> int {
   return static_cast<int>(m_folders.size());
}

auto FolderListModel::folderAt(int index) const -> QString {
   if (index < 0 || index >= m_folders.size()) {
      return {};
   }
   return m_folders.at(index);
}

auto FolderListModel::indexOf(QString const& folderName) const -> int {
   return m_folders.indexOf(folderName);
}

auto FolderListModel::todayFolderName() const -> QString {
   return QDate::currentDate().toString("yyyy-MM-dd");
}

auto FolderListModel::mostRecentFolder() const -> QString {
   if (m_folders.isEmpty()) {
      return {};
   }
   // Folders are sorted in ascending order, so last is most recent
   return m_folders.last();
}

void FolderListModel::refresh() {
   loadFolders();
}

void FolderListModel::loadFolders() {
   beginResetModel();
   m_folders.clear();

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

   // Match folders with date pattern YYYY-MM-DD
   static QRegularExpression datePattern("^\\d{4}-\\d{2}-\\d{2}$");

   QStringList entries = dir.entryList(QDir::Dirs | QDir::NoDotAndDotDot, QDir::Name);

   for (auto const& entry : entries) {
      if (datePattern.match(entry).hasMatch()) {
         m_folders.append(entry);
      }
   }

   // Sort in ascending order (oldest first, newest last)
   std::sort(m_folders.begin(), m_folders.end());

   endResetModel();
   emit countChanged();
}

}  // namespace timelapse
