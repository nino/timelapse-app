use rusqlite::{Connection, Result};
use std::path::PathBuf;
use chrono::{DateTime, Utc, Local};

pub struct ScreenshotDatabase {
    conn: Connection,
}

impl ScreenshotDatabase {
    /// Create a new database connection and initialize the schema
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        // Create the screenshots table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS screenshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                frame_number INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                local_time TEXT NOT NULL
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    /// Insert a new screenshot record
    pub fn insert_screenshot(
        &self,
        frame_number: u32,
        created_at: DateTime<Utc>,
        local_time: DateTime<Local>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO screenshots (frame_number, created_at, local_time) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                frame_number,
                created_at.to_rfc3339(),
                local_time.to_rfc3339()
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_database_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = ScreenshotDatabase::new(db_path.clone());
        assert!(db.is_ok());

        // Verify the database file was created
        assert!(db_path.exists());
    }

    #[test]
    fn test_insert_screenshot() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = ScreenshotDatabase::new(db_path).unwrap();

        // Insert a screenshot record
        let frame_number = 1;
        let created_at = Utc::now();
        let local_time = Local::now();
        let result = db.insert_screenshot(frame_number, created_at, local_time);
        assert!(result.is_ok());

        // Verify the record was inserted
        let count: i32 = db.conn
            .query_row("SELECT COUNT(*) FROM screenshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_insert_multiple_screenshots() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = ScreenshotDatabase::new(db_path).unwrap();

        // Insert multiple screenshot records
        for i in 1..=5 {
            let result = db.insert_screenshot(i, Utc::now(), Local::now());
            assert!(result.is_ok());
        }

        // Verify all records were inserted
        let count: i32 = db.conn
            .query_row("SELECT COUNT(*) FROM screenshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 5);
    }
}
