#pragma once

#include <QAbstractListModel>
#include <QFileSystemWatcher>
#include <QString>
#include <QStringList>

namespace timelapse {

class FileListModel : public QAbstractListModel {
   Q_OBJECT
   Q_PROPERTY(QString folderPath READ folderPath WRITE setFolderPath NOTIFY folderPathChanged)
   Q_PROPERTY(QString fileFilter READ fileFilter WRITE setFileFilter NOTIFY fileFilterChanged)
   Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
   enum Roles {
      NameRole = Qt::UserRole + 1,
      PathRole,
      FrameNumberRole
   };

   explicit FileListModel(QObject* parent = nullptr);
   ~FileListModel() override = default;

   [[nodiscard]] auto rowCount(QModelIndex const& parent = {}) const -> int override;
   [[nodiscard]] auto data(QModelIndex const& index, int role) const -> QVariant override;
   [[nodiscard]] auto roleNames() const -> QHash<int, QByteArray> override;

   [[nodiscard]] auto folderPath() const -> QString;
   void setFolderPath(QString const& path);

   [[nodiscard]] auto fileFilter() const -> QString;
   void setFileFilter(QString const& filter);

   [[nodiscard]] auto count() const -> int;

   // Get file name at index
   [[nodiscard]] auto fileAt(int index) const -> QString;

   // Get full path at index
   [[nodiscard]] auto pathAt(int index) const -> QString;

   // Get frame number from filename (e.g., "00001.png" -> 1)
   [[nodiscard]] auto frameNumberAt(int index) const -> int32_t;

public slots:
   void refresh();

signals:
   void folderPathChanged();
   void fileFilterChanged();
   void countChanged();

private:
   void loadFiles();
   [[nodiscard]] auto extractFrameNumber(QString const& filename) const -> int32_t;

   QString m_folderPath;
   QString m_fileFilter{"*.png"};
   QStringList m_files;
   QFileSystemWatcher* m_watcher{nullptr};
};

}  // namespace timelapse
