#include "PathUtils.hh"

#include <QDate>
#include <QDir>
#include <QStandardPaths>

#include "Constants.hh"

namespace timelapse {

auto PathUtils::homeDir() -> QString {
   return QStandardPaths::writableLocation(QStandardPaths::HomeLocation);
}

auto PathUtils::timelapseDir() -> QString {
   return homeDir() + "/" + kTimelapseDir;
}

auto PathUtils::cacheDir() -> QString {
   return timelapseDir() + "/" + kCacheDir;
}

auto PathUtils::databasePath() -> QString {
   return cacheDir() + "/" + kDatabaseName;
}

auto PathUtils::dayFolder(QString const& date) -> QString {
   return timelapseDir() + "/" + date;
}

auto PathUtils::currentDateString() -> QString {
   return QDate::currentDate().toString("yyyy-MM-dd");
}

auto PathUtils::formatFrameNumber(int32_t number) -> QString {
   return QString("%1").arg(number, kFrameNumberDigits, 10, QChar('0'));
}

auto PathUtils::ensureDir(QString const& path) -> bool {
   QDir dir(path);
   if (dir.exists()) {
      return true;
   }
   return dir.mkpath(".");
}

}  // namespace timelapse
