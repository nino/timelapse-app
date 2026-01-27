#pragma once

#include <QObject>
#include <QString>
#include <chrono>
#include <expected>

namespace timelapse {

class CacheManager : public QObject {
   Q_OBJECT

public:
   explicit CacheManager(QObject* parent = nullptr);
   ~CacheManager() override = default;

   // Evict cache entries older than maxAge
   // Returns the number of entries removed
   [[nodiscard]] auto evictOldEntries(std::chrono::days maxAge = std::chrono::days{15})
      -> std::expected<int32_t, QString>;

   // Get total cache size in bytes
   [[nodiscard]] auto cacheSize() const -> std::expected<int64_t, QString>;

   // Get number of cached video folders
   [[nodiscard]] auto cachedVideoCount() const -> int32_t;

   // Clear entire cache
   [[nodiscard]] auto clearCache() -> std::expected<void, QString>;

   // Get the cache directory path
   [[nodiscard]] auto cachePath() const -> QString;

signals:
   void evictionComplete(int32_t removedCount);
   void cacheCleared();

private:
   [[nodiscard]] auto removeDirectory(QString const& path) -> bool;
};

}  // namespace timelapse
