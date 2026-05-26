use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
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

   // Image processing errors
   ImageProcessingFailed,
}

#[derive(Debug, Error)]
#[error("[{code:?}] {message}")]
pub struct Error {
   pub code: ErrorCode,
   pub message: String,
}

impl Error {
   pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
      Self {
         code,
         message: message.into(),
      }
   }

   pub fn unknown(message: impl Into<String>) -> Self {
      Self::new(ErrorCode::Unknown, message)
   }

   pub fn is(&self, code: ErrorCode) -> bool {
      self.code == code
   }
}

pub type Result<T> = std::result::Result<T, Error>;
