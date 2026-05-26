#pragma once

#include <QAbstractListModel>
#include <QFileSystemWatcher>
#include <QString>
#include <QStringList>

namespace timelapse {

class FolderListModel : public QAbstractListModel {
   Q_OBJECT
   Q_PROPERTY(QString basePath READ basePath WRITE setBasePath NOTIFY basePathChanged)
   Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
   enum Roles {
      NameRole = Qt::UserRole + 1,
      PathRole,
      IsTodayRole
   };

   explicit FolderListModel(QObject* parent = nullptr);
   ~FolderListModel() override = default;

   [[nodiscard]] auto rowCount(QModelIndex const& parent = {}) const -> int override;
   [[nodiscard]] auto data(QModelIndex const& index, int role) const -> QVariant override;
   [[nodiscard]] auto roleNames() const -> QHash<int, QByteArray> override;

   [[nodiscard]] auto basePath() const -> QString;
   void setBasePath(QString const& path);

   [[nodiscard]] auto count() const -> int;

   // Get folder name at index
   [[nodiscard]] auto folderAt(int index) const -> QString;

   // Find index of a folder by name
   [[nodiscard]] auto indexOf(QString const& folderName) const -> int;

   // Get today's folder name (YYYY-MM-DD)
   [[nodiscard]] auto todayFolderName() const -> QString;

   // Get the most recent folder (last in sorted list)
   [[nodiscard]] auto mostRecentFolder() const -> QString;

public slots:
   void refresh();

signals:
   void basePathChanged();
   void countChanged();

private:
   void loadFolders();

   QString _basePath;
   QStringList _folders;
   QFileSystemWatcher* _watcher{nullptr};
};

}  // namespace timelapse
