#include "ErrorLog.hh"

#include <QMutexLocker>

#include "utils/Constants.hh"

namespace timelapse {

ErrorLog::ErrorLog(QObject* parent)
   : QObject(parent) {}

void ErrorLog::append(QString const& message) {
   QMutexLocker locker(&m_mutex);

   m_entries.push_back(ErrorLogEntry{
      .timestamp = QDateTime::currentDateTime(),
      .message = message,
   });

   trimIfNeeded();

   locker.unlock();
   emit errorLogged(message);
}

auto ErrorLog::entries() const -> std::vector<ErrorLogEntry> {
   QMutexLocker locker(&m_mutex);
   return m_entries;
}

auto ErrorLog::recentEntries(int32_t count) const -> std::vector<ErrorLogEntry> {
   QMutexLocker locker(&m_mutex);

   if (count >= static_cast<int32_t>(m_entries.size())) {
      return m_entries;
   }

   return std::vector<ErrorLogEntry>(
      m_entries.end() - count,
      m_entries.end());
}

void ErrorLog::clear() {
   {
      QMutexLocker locker(&m_mutex);
      m_entries.clear();
   }
   emit cleared();
}

auto ErrorLog::count() const -> int32_t {
   QMutexLocker locker(&m_mutex);
   return static_cast<int32_t>(m_entries.size());
}

void ErrorLog::trimIfNeeded() {
   // Called with mutex already held
   if (m_entries.size() > static_cast<size_t>(kMaxErrorLogs)) {
      // Remove oldest entries
      auto excess = m_entries.size() - kMaxErrorLogs;
      m_entries.erase(m_entries.begin(), m_entries.begin() + excess);
   }
}

}  // namespace timelapse
