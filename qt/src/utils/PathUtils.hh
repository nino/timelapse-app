#pragma once

#include <QString>

namespace timelapse::PathUtils {

// Get the user's home directory
[[nodiscard]] auto homeDir() -> QString;

// Get the timelapse base directory (~/Timelapse)
[[nodiscard]] auto timelapseDir() -> QString;

// Get the cache directory (~/Timelapse/.cache)
[[nodiscard]] auto cacheDir() -> QString;

// Get the database path (~/Timelapse/.cache/metadata.db)
[[nodiscard]] auto databasePath() -> QString;

// Get the day folder path (~/Timelapse/YYYY-MM-DD)
[[nodiscard]] auto dayFolder(QString const& date) -> QString;

// Get current date string in YYYY-MM-DD format
[[nodiscard]] auto currentDateString() -> QString;

// Format a frame number with leading zeros (e.g., 00001)
[[nodiscard]] auto formatFrameNumber(int32_t number) -> QString;

// Ensure a directory exists, creating it if necessary
[[nodiscard]] auto ensureDir(QString const& path) -> bool;

}  // namespace timelapse::PathUtils
