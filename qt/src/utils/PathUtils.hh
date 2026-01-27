#pragma once

#include <QString>

namespace timelapse {

class PathUtils {
public:
   // Get the user's home directory
   [[nodiscard]] static auto homeDir() -> QString;

   // Get the timelapse base directory (~/Timelapse)
   [[nodiscard]] static auto timelapseDir() -> QString;

   // Get the cache directory (~/Timelapse/.cache)
   [[nodiscard]] static auto cacheDir() -> QString;

   // Get the database path (~/Timelapse/.cache/metadata.db)
   [[nodiscard]] static auto databasePath() -> QString;

   // Get the day folder path (~/Timelapse/YYYY-MM-DD)
   [[nodiscard]] static auto dayFolder(QString const& date) -> QString;

   // Get current date string in YYYY-MM-DD format
   [[nodiscard]] static auto currentDateString() -> QString;

   // Format a frame number with leading zeros (e.g., 00001)
   [[nodiscard]] static auto formatFrameNumber(int32_t number) -> QString;

   // Ensure a directory exists, creating it if necessary
   [[nodiscard]] static auto ensureDir(QString const& path) -> bool;
};

}  // namespace timelapse
