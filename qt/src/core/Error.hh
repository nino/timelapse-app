#pragma once

#include <QString>

namespace timelapse {

enum class ErrorCode {
   Unknown,

   // Screen capture errors
   NoScreensFound,
   ScreenNotFound,
   CaptureFailed,

   // Video extraction errors
   ExtractionInProgress,
   FfmpegNotFound,
   CacheCreationFailed,
   FfmpegStartFailed,
   FfmpegFailed,
   FfmpegExitError,

   // File/directory errors
   DirectoryCreationFailed,
   FileNotFound,
   FileReadError,
   FileWriteError,

   // Database errors
   DatabaseError,
   DatabaseConnectionFailed,
   DatabaseQueryFailed,
};

class Error {
public:
   Error() = default;

   Error(ErrorCode code, QString message = {})
      : _code(code)
      , _message(std::move(message)) {}

   [[nodiscard]] auto code() const -> ErrorCode { return this->_code; }
   [[nodiscard]] auto message() const -> QString const& { return this->_message; }

   // Convenience: check if this is a specific error
   [[nodiscard]] auto is(ErrorCode code) const -> bool { return this->_code == code; }

   // For use in conditions (true if there's an error)
   explicit operator bool() const { return this->_code != ErrorCode::Unknown; }

   // Human-readable error code name
   [[nodiscard]] auto codeName() const -> QString {
      switch (this->_code) {
      case ErrorCode::Unknown:
         return "Unknown";
      case ErrorCode::NoScreensFound:
         return "NoScreensFound";
      case ErrorCode::ScreenNotFound:
         return "ScreenNotFound";
      case ErrorCode::CaptureFailed:
         return "CaptureFailed";
      case ErrorCode::ExtractionInProgress:
         return "ExtractionInProgress";
      case ErrorCode::FfmpegNotFound:
         return "FfmpegNotFound";
      case ErrorCode::CacheCreationFailed:
         return "CacheCreationFailed";
      case ErrorCode::FfmpegStartFailed:
         return "FfmpegStartFailed";
      case ErrorCode::FfmpegFailed:
         return "FfmpegFailed";
      case ErrorCode::FfmpegExitError:
         return "FfmpegExitError";
      case ErrorCode::DirectoryCreationFailed:
         return "DirectoryCreationFailed";
      case ErrorCode::FileNotFound:
         return "FileNotFound";
      case ErrorCode::FileReadError:
         return "FileReadError";
      case ErrorCode::FileWriteError:
         return "FileWriteError";
      case ErrorCode::DatabaseError:
         return "DatabaseError";
      case ErrorCode::DatabaseConnectionFailed:
         return "DatabaseConnectionFailed";
      case ErrorCode::DatabaseQueryFailed:
         return "DatabaseQueryFailed";
      }
      return "Unknown";
   }

   // Full description: "[CodeName] message"
   [[nodiscard]] auto toString() const -> QString {
      if (this->_message.isEmpty()) {
         return this->codeName();
      }
      return QString("[%1] %2").arg(this->codeName(), this->_message);
   }

private:
   ErrorCode _code{ErrorCode::Unknown};
   QString _message;
};

}  // namespace timelapse
