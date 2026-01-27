#include "FolderListModel.hh"

#include <QDate>
#include <QDir>
#include <QRegularExpression>
#include <algorithm>

namespace timelapse {

FolderListModel::FolderListModel(QObject* parent)
   : QAbstractListModel(parent)
   , _watcher(new QFileSystemWatcher(this)) {
   connect(this->_watcher, &QFileSystemWatcher::directoryChanged,
           this, &FolderListModel::refresh);
}

auto FolderListModel::rowCount(QModelIndex const& parent) const -> int {
   if (parent.isValid()) {
      return 0;
   }
   return static_cast<int>(this->_folders.size());
}

auto FolderListModel::data(QModelIndex const& index, int role) const -> QVariant {
   if (!index.isValid() || index.row() >= this->_folders.size()) {
      return {};
   }

   QString const& folder = this->_folders.at(index.row());

   switch (role) {
   case Qt::DisplayRole:
   case NameRole:
      return folder;
   case PathRole:
      return this->_basePath + "/" + folder;
   case IsTodayRole:
      return folder == this->todayFolderName();
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
   return this->_basePath;
}

void FolderListModel::setBasePath(QString const& path) {
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
   this->loadFolders();
}

auto FolderListModel::count() const -> int {
   return static_cast<int>(this->_folders.size());
}

auto FolderListModel::folderAt(int index) const -> QString {
   if (index < 0 || index >= this->_folders.size()) {
      return {};
   }
   return this->_folders.at(index);
}

auto FolderListModel::indexOf(QString const& folderName) const -> int {
   return this->_folders.indexOf(folderName);
}

auto FolderListModel::todayFolderName() const -> QString {
   return QDate::currentDate().toString("yyyy-MM-dd");
}

auto FolderListModel::mostRecentFolder() const -> QString {
   if (this->_folders.isEmpty()) {
      return {};
   }
   // Folders are sorted in ascending order, so last is most recent
   return this->_folders.last();
}

void FolderListModel::refresh() {
   this->loadFolders();
}

void FolderListModel::loadFolders() {
   beginResetModel();
   this->_folders.clear();

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

   // Match folders with date pattern YYYY-MM-DD
   static QRegularExpression datePattern("^\\d{4}-\\d{2}-\\d{2}$");

   QStringList entries = dir.entryList(QDir::Dirs | QDir::NoDotAndDotDot, QDir::Name);

   for (auto const& entry : entries) {
      if (datePattern.match(entry).hasMatch()) {
         this->_folders.append(entry);
      }
   }

   // Sort in ascending order (oldest first, newest last)
   std::sort(this->_folders.begin(), this->_folders.end());

   endResetModel();
   emit countChanged();
}

}  // namespace timelapse
