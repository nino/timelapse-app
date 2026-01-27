#pragma once

#include <chrono>
#include <cstdint>

namespace timelapse {

// Image dimensions for saved screenshots
inline constexpr int32_t kTargetWidth = 1800;
inline constexpr int32_t kTargetHeight = 1124;

// Black screen detection
inline constexpr double kBlackThreshold = 0.01;  // Mean brightness threshold
inline constexpr int32_t kSampleStep = 10;       // Pixel sampling step

// Timing intervals
inline constexpr auto kScreenshotInterval = std::chrono::seconds(1);
inline constexpr auto kBlackScreenWait = std::chrono::seconds(10);
inline constexpr auto kErrorWait = std::chrono::seconds(60);

// Video extraction
inline constexpr int32_t kVideoFps = 30;

// Cache management
inline constexpr int32_t kCacheEvictionDays = 15;

// Error logging
inline constexpr int32_t kMaxErrorLogs = 10000;

// File naming
inline constexpr int32_t kFrameNumberDigits = 5;

// Directory names
inline constexpr const char* kTimelapseDir = "Timelapse";
inline constexpr const char* kCacheDir = ".cache";
inline constexpr const char* kDatabaseName = "metadata.db";

}  // namespace timelapse
