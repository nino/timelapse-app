#pragma once

#include <QDateTime>
#include <QObject>
#include <QSqlDatabase>
#include <QString>
#include <expected>
#include <optional>

#include "Error.hh"

namespace timelapse {

struct ScreenshotMetadata {
   uint32_t frameNumber;
   QDateTime createdAt;  // UTC
   QDateTime localTime;  // Local timezone
};

class Database : public QObject {
   Q_OBJECT

public:
   explicit Database(QString const& dbPath, QObject* parent = nullptr);
   ~Database() override;

   // Non-copyable
   Database(Database const&) = delete;
   auto operator=(Database const&) -> Database& = delete;

   [[nodiscard]] auto isOpen() const -> bool;

   // Insert a new screenshot record
   auto insertScreenshot(uint32_t frameNumber,
                         QDateTime const& createdAt,
                         QDateTime const& localTime) -> std::expected<void, Error>;

   // Get metadata for a specific frame
   [[nodiscard]] auto getScreenshotByFrame(uint32_t frameNumber)
      -> std::expected<std::optional<ScreenshotMetadata>, Error>;

   // Get total screenshot count
   [[nodiscard]] auto screenshotCount() -> std::expected<int64_t, Error>;

private:
   auto runMigrations() -> std::expected<void, Error>;
   auto createMigrationsTable() -> std::expected<void, Error>;
   [[nodiscard]] auto migrationApplied(QString const& name) -> bool;
   auto recordMigration(QString const& name) -> std::expected<void, Error>;

   QSqlDatabase _db;
   QString _connectionName;
};

}  // namespace timelapse
