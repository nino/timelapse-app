#include "PathUtils.hh"

#include <QDate>
#include <QDir>
#include <QStandardPaths>

#include "Constants.hh"

namespace timelapse::PathUtils {

auto homeDir() -> QString {
   return QStandardPaths::writableLocation(QStandardPaths::HomeLocation);
}

auto timelapseDir() -> QString {
   return homeDir() + "/" + kTimelapseDir;
}

auto cacheDir() -> QString {
   return timelapseDir() + "/" + kCacheDir;
}

auto databasePath() -> QString {
   return cacheDir() + "/" + kDatabaseName;
}

auto dayFolder(QString const& date) -> QString {
   return timelapseDir() + "/" + date;
}

auto currentDateString() -> QString {
   return QDate::currentDate().toString("yyyy-MM-dd");
}

auto formatFrameNumber(int32_t number) -> QString {
   return QString("%1").arg(number, kFrameNumberDigits, 10, QChar('0'));
}

auto ensureDir(QString const& path) -> bool {
   QDir dir(path);
   if (dir.exists()) {
      return true;
   }
   return dir.mkpath(".");
}

}  // namespace timelapse::PathUtils
