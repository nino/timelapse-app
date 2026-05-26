#include "ErrorLog.hh"

#include <QMutexLocker>

#include "utils/Constants.hh"

namespace timelapse {

ErrorLog::ErrorLog(QObject* parent)
   : QObject(parent) {}

void ErrorLog::append(QString const& message) {
   QMutexLocker locker(&this->_mutex);

   this->_entries.push_back(ErrorLogEntry{
      .timestamp = QDateTime::currentDateTime(),
      .message = message,
   });

   this->trimIfNeeded();

   locker.unlock();
   emit errorLogged(message);
}

auto ErrorLog::entries() const -> std::vector<ErrorLogEntry> {
   QMutexLocker locker(&this->_mutex);
   return this->_entries;
}

auto ErrorLog::recentEntries(int32_t count) const -> std::vector<ErrorLogEntry> {
   QMutexLocker locker(&this->_mutex);

   if (count >= static_cast<int32_t>(this->_entries.size())) {
      return this->_entries;
   }

   return std::vector<ErrorLogEntry>(
      this->_entries.end() - count,
      this->_entries.end());
}

void ErrorLog::clear() {
   {
      QMutexLocker locker(&this->_mutex);
      this->_entries.clear();
   }
   emit cleared();
}

auto ErrorLog::count() const -> int32_t {
   QMutexLocker locker(&this->_mutex);
   return static_cast<int32_t>(this->_entries.size());
}

void ErrorLog::trimIfNeeded() {
   // Called with mutex already held
   if (this->_entries.size() > static_cast<size_t>(kMaxErrorLogs)) {
      // Remove oldest entries
      auto excess = this->_entries.size() - kMaxErrorLogs;
      this->_entries.erase(this->_entries.begin(), this->_entries.begin() + excess);
   }
}

}  // namespace timelapse
