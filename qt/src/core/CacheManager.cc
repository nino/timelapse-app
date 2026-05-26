#include "CacheManager.hh"

#include <QDateTime>
#include <QDir>
#include <QDirIterator>
#include <QFileInfo>

#include "utils/PathUtils.hh"

namespace timelapse {

CacheManager::CacheManager(QObject* parent)
   : QObject(parent) {}

auto CacheManager::evictOldEntries(std::chrono::days maxAge)
   -> std::expected<int32_t, QString> {
   QString cache = cachePath();
   QDir cacheDir(cache);

   if (!cacheDir.exists()) {
      return 0;  // Nothing to evict
   }

   QDateTime cutoff = QDateTime::currentDateTime().addDays(-maxAge.count());
   int32_t removedCount = 0;

   // Iterate through cache subdirectories
   QStringList entries = cacheDir.entryList(QDir::Dirs | QDir::NoDotAndDotDot);

   for (auto const& entry : entries) {
      QString entryPath = cache + "/" + entry;
      QFileInfo info(entryPath);

      // Check modification time
      if (info.lastModified() < cutoff) {
         if (removeDirectory(entryPath)) {
            removedCount++;
         }
      }
   }

   emit evictionComplete(removedCount);
   return removedCount;
}

auto CacheManager::cacheSize() const -> std::expected<int64_t, QString> {
   QString cache = cachePath();
   QDir cacheDir(cache);

   if (!cacheDir.exists()) {
      return 0;
   }

   int64_t totalSize = 0;

   QDirIterator it(cache, QDir::Files, QDirIterator::Subdirectories);
   while (it.hasNext()) {
      it.next();
      totalSize += it.fileInfo().size();
   }

   return totalSize;
}

auto CacheManager::cachedVideoCount() const -> int32_t {
   QString cache = cachePath();
   QDir cacheDir(cache);

   if (!cacheDir.exists()) {
      return 0;
   }

   return cacheDir.entryList(QDir::Dirs | QDir::NoDotAndDotDot).count();
}

auto CacheManager::clearCache() -> std::expected<void, QString> {
   QString cache = cachePath();
   QDir cacheDir(cache);

   if (!cacheDir.exists()) {
      return {};  // Nothing to clear
   }

   // Remove all subdirectories but keep the .cache folder itself
   QStringList entries = cacheDir.entryList(QDir::Dirs | QDir::NoDotAndDotDot);

   for (auto const& entry : entries) {
      QString entryPath = cache + "/" + entry;
      if (!removeDirectory(entryPath)) {
         return std::unexpected("Failed to remove: " + entryPath);
      }
   }

   // Also remove any files directly in cache (like metadata.db)
   // But preserve the database file
   QStringList files = cacheDir.entryList(QDir::Files);
   for (auto const& file : files) {
      if (file != "metadata.db") {
         cacheDir.remove(file);
      }
   }

   emit cacheCleared();
   return {};
}

auto CacheManager::cachePath() const -> QString {
   return PathUtils::cacheDir();
}

auto CacheManager::removeDirectory(QString const& path) -> bool {
   QDir dir(path);
   return dir.removeRecursively();
}

}  // namespace timelapse
