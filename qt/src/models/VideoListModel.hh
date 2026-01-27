#pragma once

#include <QAbstractListModel>
#include <QFileSystemWatcher>
#include <QString>
#include <QStringList>

namespace timelapse {

class VideoListModel : public QAbstractListModel {
   Q_OBJECT
   Q_PROPERTY(QString basePath READ basePath WRITE setBasePath NOTIFY basePathChanged)
   Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
   enum Roles {
      NameRole = Qt::UserRole + 1,
      PathRole,
      SizeRole
   };

   explicit VideoListModel(QObject* parent = nullptr);
   ~VideoListModel() override = default;

   [[nodiscard]] auto rowCount(QModelIndex const& parent = {}) const -> int override;
   [[nodiscard]] auto data(QModelIndex const& index, int role) const -> QVariant override;
   [[nodiscard]] auto roleNames() const -> QHash<int, QByteArray> override;

   [[nodiscard]] auto basePath() const -> QString;
   void setBasePath(QString const& path);

   [[nodiscard]] auto count() const -> int;

   // Get video filename at index
   [[nodiscard]] auto videoAt(int index) const -> QString;

   // Get full path at index
   [[nodiscard]] auto pathAt(int index) const -> QString;

   // Get the most recent video (last in list)
   [[nodiscard]] auto mostRecentVideo() const -> QString;

public slots:
   void refresh();

signals:
   void basePathChanged();
   void countChanged();

private:
   void loadVideos();

   QString m_basePath;
   QStringList m_videos;
   QFileSystemWatcher* m_watcher{nullptr};
};

}  // namespace timelapse
