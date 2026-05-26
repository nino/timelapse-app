use chrono::{DateTime, Local, Utc};
use rusqlite::{params, Connection};
use std::path::Path;

use crate::core::error::{Error, ErrorCode, Result};

#[derive(Debug, Clone)]
pub struct ScreenshotMetadata {
   pub frame_number: u32,
   pub created_at: DateTime<Utc>,
   pub local_time: DateTime<Local>,
}

pub struct Database {
   conn: Connection,
}

impl Database {
   pub fn new(db_path: &Path) -> Result<Self> {
      let conn = Connection::open(db_path).map_err(|e| {
         Error::new(
            ErrorCode::DatabaseConnectionFailed,
            format!("Failed to open database: {}", e),
         )
      })?;

      let db = Self { conn };
      db.run_migrations()?;

      Ok(db)
   }

   fn run_migrations(&self) -> Result<()> {
      self.create_migrations_table()?;

      // Migration: create screenshots table
      if !self.migration_applied("001_create_screenshots")? {
         self.conn
            .execute(
               "CREATE TABLE IF NOT EXISTS screenshots (
                  id INTEGER PRIMARY KEY AUTOINCREMENT,
                  frame_number INTEGER NOT NULL,
                  created_at TEXT NOT NULL,
                  local_time TEXT NOT NULL
               )",
               [],
            )
            .map_err(|e| {
               Error::new(
                  ErrorCode::DatabaseQueryFailed,
                  format!("Failed to create screenshots table: {}", e),
               )
            })?;

         self.conn
            .execute(
               "CREATE INDEX IF NOT EXISTS idx_screenshots_frame ON screenshots(frame_number)",
               [],
            )
            .map_err(|e| {
               Error::new(
                  ErrorCode::DatabaseQueryFailed,
                  format!("Failed to create index: {}", e),
               )
            })?;

         self.record_migration("001_create_screenshots")?;
      }

      Ok(())
   }

   fn create_migrations_table(&self) -> Result<()> {
      self.conn
         .execute(
            "CREATE TABLE IF NOT EXISTS migrations (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               migration_name TEXT UNIQUE NOT NULL,
               applied_at TEXT NOT NULL
            )",
            [],
         )
         .map_err(|e| {
            Error::new(
               ErrorCode::DatabaseQueryFailed,
               format!("Failed to create migrations table: {}", e),
            )
         })?;

      Ok(())
   }

   fn migration_applied(&self, name: &str) -> Result<bool> {
      let count: i64 = self
         .conn
         .query_row(
            "SELECT COUNT(*) FROM migrations WHERE migration_name = ?1",
            [name],
            |row| row.get(0),
         )
         .unwrap_or(0);

      Ok(count > 0)
   }

   fn record_migration(&self, name: &str) -> Result<()> {
      let now = Utc::now().to_rfc3339();
      self.conn
         .execute(
            "INSERT INTO migrations (migration_name, applied_at) VALUES (?1, ?2)",
            params![name, now],
         )
         .map_err(|e| {
            Error::new(
               ErrorCode::DatabaseQueryFailed,
               format!("Failed to record migration: {}", e),
            )
         })?;

      Ok(())
   }

   /// Insert a new screenshot record
   pub fn insert_screenshot(
      &self,
      frame_number: u32,
      created_at: DateTime<Utc>,
      local_time: DateTime<Local>,
   ) -> Result<()> {
      self.conn
         .execute(
            "INSERT INTO screenshots (frame_number, created_at, local_time) VALUES (?1, ?2, ?3)",
            params![
               frame_number,
               created_at.to_rfc3339(),
               local_time.to_rfc3339()
            ],
         )
         .map_err(|e| {
            Error::new(
               ErrorCode::DatabaseQueryFailed,
               format!("Failed to insert screenshot: {}", e),
            )
         })?;

      Ok(())
   }

   /// Get metadata for a specific frame
   pub fn get_screenshot_by_frame(&self, frame_number: u32) -> Result<Option<ScreenshotMetadata>> {
      let result = self.conn.query_row(
         "SELECT frame_number, created_at, local_time FROM screenshots WHERE frame_number = ?1",
         [frame_number],
         |row| {
            let frame: u32 = row.get(0)?;
            let created_str: String = row.get(1)?;
            let local_str: String = row.get(2)?;

            Ok((frame, created_str, local_str))
         },
      );

      match result {
         Ok((frame, created_str, local_str)) => {
            let created_at = DateTime::parse_from_rfc3339(&created_str)
               .map(|dt| dt.with_timezone(&Utc))
               .map_err(|e| {
                  Error::new(
                     ErrorCode::DatabaseQueryFailed,
                     format!("Failed to parse created_at: {}", e),
                  )
               })?;

            let local_time = DateTime::parse_from_rfc3339(&local_str)
               .map(|dt| dt.with_timezone(&Local))
               .map_err(|e| {
                  Error::new(
                     ErrorCode::DatabaseQueryFailed,
                     format!("Failed to parse local_time: {}", e),
                  )
               })?;

            Ok(Some(ScreenshotMetadata {
               frame_number: frame,
               created_at,
               local_time,
            }))
         }
         Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
         Err(e) => Err(Error::new(
            ErrorCode::DatabaseQueryFailed,
            format!("Failed to query screenshot: {}", e),
         )),
      }
   }

   /// Get total screenshot count
   pub fn screenshot_count(&self) -> Result<i64> {
      self.conn
         .query_row("SELECT COUNT(*) FROM screenshots", [], |row| row.get(0))
         .map_err(|e| {
            Error::new(
               ErrorCode::DatabaseQueryFailed,
               format!("Failed to count screenshots: {}", e),
            )
         })
   }
}
