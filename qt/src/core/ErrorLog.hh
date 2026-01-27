#pragma once

#include <QDateTime>
#include <QMutex>
#include <QObject>
#include <QString>
#include <vector>

namespace timelapse {

struct ErrorLogEntry {
   QDateTime timestamp;
   QString message;
};

class ErrorLog : public QObject {
   Q_OBJECT

public:
   explicit ErrorLog(QObject* parent = nullptr);
   ~ErrorLog() override = default;

   // Non-copyable
   ErrorLog(ErrorLog const&) = delete;
   auto operator=(ErrorLog const&) -> ErrorLog& = delete;

   // Add an error to the log
   void append(QString const& message);

   // Get all error entries
   [[nodiscard]] auto entries() const -> std::vector<ErrorLogEntry>;

   // Get the most recent N entries
   [[nodiscard]] auto recentEntries(int32_t count) const -> std::vector<ErrorLogEntry>;

   // Clear all entries
   void clear();

   // Get the number of entries
   [[nodiscard]] auto count() const -> int32_t;

signals:
   void errorLogged(QString const& message);
   void cleared();

private:
   void trimIfNeeded();

   mutable QMutex m_mutex;
   std::vector<ErrorLogEntry> m_entries;
};

}  // namespace timelapse
