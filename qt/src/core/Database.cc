#include "Database.hh"

#include <QSqlError>
#include <QSqlQuery>
#include <QUuid>

namespace timelapse {

Database::Database(QString const& dbPath, QObject* parent)
   : QObject(parent)
   , _connectionName(QUuid::createUuid().toString()) {
   this->_db = QSqlDatabase::addDatabase("QSQLITE", this->_connectionName);
   this->_db.setDatabaseName(dbPath);

   if (!this->_db.open()) {
      qWarning() << "Failed to open database:" << this->_db.lastError().text();
      return;
   }

   if (auto result = this->runMigrations(); !result) {
      qWarning() << "Failed to run migrations:" << result.error();
   }
}

Database::~Database() {
   if (this->_db.isOpen()) {
      this->_db.close();
   }
   QSqlDatabase::removeDatabase(this->_connectionName);
}

auto Database::isOpen() const -> bool {
   return this->_db.isOpen();
}

auto Database::runMigrations() -> std::expected<void, QString> {
   // Create migrations table first
   if (auto result = this->createMigrationsTable(); !result) {
      return result;
   }

   // Migration: create screenshots table
   if (!this->migrationApplied("001_create_screenshots")) {
      QSqlQuery query(this->_db);
      if (!query.exec(R"(
         CREATE TABLE IF NOT EXISTS screenshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            frame_number INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            local_time TEXT NOT NULL
         )
      )")) {
         return std::unexpected(query.lastError().text());
      }

      // Create index on frame_number
      if (!query.exec(R"(
         CREATE INDEX IF NOT EXISTS idx_screenshots_frame
         ON screenshots(frame_number)
      )")) {
         return std::unexpected(query.lastError().text());
      }

      if (auto result = this->recordMigration("001_create_screenshots"); !result) {
         return result;
      }
   }

   return {};
}

auto Database::createMigrationsTable() -> std::expected<void, QString> {
   QSqlQuery query(this->_db);
   if (!query.exec(R"(
      CREATE TABLE IF NOT EXISTS migrations (
         id INTEGER PRIMARY KEY AUTOINCREMENT,
         migration_name TEXT UNIQUE NOT NULL,
         applied_at TEXT NOT NULL
      )
   )")) {
      return std::unexpected(query.lastError().text());
   }
   return {};
}

auto Database::migrationApplied(QString const& name) -> bool {
   QSqlQuery query(this->_db);
   query.prepare("SELECT COUNT(*) FROM migrations WHERE migration_name = :name");
   query.bindValue(":name", name);

   if (!query.exec() || !query.next()) {
      return false;
   }

   return query.value(0).toInt() > 0;
}

auto Database::recordMigration(QString const& name) -> std::expected<void, QString> {
   QSqlQuery query(this->_db);
   query.prepare(R"(
      INSERT INTO migrations (migration_name, applied_at)
      VALUES (:name, :applied_at)
   )");
   query.bindValue(":name", name);
   query.bindValue(":applied_at", QDateTime::currentDateTimeUtc().toString(Qt::ISODate));

   if (!query.exec()) {
      return std::unexpected(query.lastError().text());
   }
   return {};
}

auto Database::insertScreenshot(uint32_t frameNumber,
                                QDateTime const& createdAt,
                                QDateTime const& localTime)
   -> std::expected<void, QString> {
   QSqlQuery query(this->_db);
   query.prepare(R"(
      INSERT INTO screenshots (frame_number, created_at, local_time)
      VALUES (:frame, :created, :local)
   )");

   query.bindValue(":frame", frameNumber);
   query.bindValue(":created", createdAt.toString(Qt::ISODate));
   query.bindValue(":local", localTime.toString(Qt::ISODate));

   if (!query.exec()) {
      return std::unexpected(query.lastError().text());
   }

   return {};
}

auto Database::getScreenshotByFrame(uint32_t frameNumber)
   -> std::expected<std::optional<ScreenshotMetadata>, QString> {
   QSqlQuery query(this->_db);
   query.prepare(R"(
      SELECT frame_number, created_at, local_time
      FROM screenshots
      WHERE frame_number = :frame
   )");
   query.bindValue(":frame", frameNumber);

   if (!query.exec()) {
      return std::unexpected(query.lastError().text());
   }

   if (!query.next()) {
      return std::nullopt;
   }

   return ScreenshotMetadata{
      .frameNumber = static_cast<uint32_t>(query.value(0).toUInt()),
      .createdAt = QDateTime::fromString(query.value(1).toString(), Qt::ISODate),
      .localTime = QDateTime::fromString(query.value(2).toString(), Qt::ISODate),
   };
}

auto Database::screenshotCount() -> std::expected<int64_t, QString> {
   QSqlQuery query(this->_db);
   if (!query.exec("SELECT COUNT(*) FROM screenshots")) {
      return std::unexpected(query.lastError().text());
   }

   if (!query.next()) {
      return 0;
   }

   return query.value(0).toLongLong();
}

}  // namespace timelapse
