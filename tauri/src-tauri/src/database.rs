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

        // Create migrations table if it doesn't exist
        conn.execute(
            "CREATE TABLE IF NOT EXISTS migrations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                migration_name TEXT UNIQUE NOT NULL,
                applied_at TEXT NOT NULL
            )",
            [],
        )?;

        // Create the screenshots table with initial schema (for new databases)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS screenshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                frame_number INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                local_time TEXT NOT NULL
            )",
            [],
        )?;

        // Run migrations
        Self::run_migrations(&conn)?;

        Ok(Self { conn })
    }

    /// Run all pending migrations
    fn run_migrations(conn: &Connection) -> Result<()> {
        // Migration 1: Split creation_date into created_at and local_time
        if !Self::migration_applied(conn, "split_timestamps")? {
            // Check if the old schema exists (has creation_date column but not created_at)
            let has_old_schema: bool = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('screenshots') WHERE name = 'creation_date'")?
                .query_row([], |row| {
                    let count: i32 = row.get(0)?;
                    Ok(count > 0)
                })?;

            let has_new_schema: bool = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('screenshots') WHERE name = 'created_at'")?
                .query_row([], |row| {
                    let count: i32 = row.get(0)?;
                    Ok(count > 0)
                })?;

            if has_old_schema && !has_new_schema {
                println!("Migrating database: splitting creation_date into created_at and local_time");

                // Rename the old table
                conn.execute("ALTER TABLE screenshots RENAME TO screenshots_old", [])?;

                // Create new table with updated schema
                conn.execute(
                    "CREATE TABLE screenshots (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        frame_number INTEGER NOT NULL,
                        created_at TEXT NOT NULL,
                        local_time TEXT NOT NULL
                    )",
                    [],
                )?;

                // Copy data from old table (use creation_date for both columns)
                conn.execute(
                    "INSERT INTO screenshots (id, frame_number, created_at, local_time)
                     SELECT id, frame_number, creation_date, creation_date FROM screenshots_old",
                    [],
                )?;

                // Drop old table
                conn.execute("DROP TABLE screenshots_old", [])?;

                println!("Database migration completed successfully");
            }

            // Record migration as applied
            conn.execute(
                "INSERT INTO migrations (migration_name, applied_at) VALUES (?1, ?2)",
                rusqlite::params!["split_timestamps", Utc::now().to_rfc3339()],
            )?;
        }

        Ok(())
    }

    /// Check if a migration has been applied
    fn migration_applied(conn: &Connection, name: &str) -> Result<bool> {
        let count: i32 = conn.query_row(
            "SELECT COUNT(*) FROM migrations WHERE migration_name = ?1",
            [name],
            |row| row.get(0),
        )?;
        Ok(count > 0)
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

    /// Get screenshot metadata by frame number
    pub fn get_screenshot_by_frame(&self, frame_number: u32) -> Result<Option<(String, String)>> {
        let result = self.conn.query_row(
            "SELECT created_at, local_time FROM screenshots WHERE frame_number = ?1",
            [frame_number],
            |row| {
                let created_at: String = row.get(0)?;
                let local_time: String = row.get(1)?;
                Ok((created_at, local_time))
            },
        );

        match result {
            Ok(data) => Ok(Some(data)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
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

    #[test]
    fn test_migration_from_old_schema() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // Create a database with the old schema
        {
            let conn = Connection::open(&db_path).unwrap();

            // Create old schema with creation_date column
            conn.execute(
                "CREATE TABLE screenshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    frame_number INTEGER NOT NULL,
                    creation_date TEXT NOT NULL
                )",
                [],
            ).unwrap();

            // Insert test data with old schema
            conn.execute(
                "INSERT INTO screenshots (frame_number, creation_date) VALUES (?1, ?2)",
                rusqlite::params![1, "2024-01-01T12:00:00Z"],
            ).unwrap();

            conn.execute(
                "INSERT INTO screenshots (frame_number, creation_date) VALUES (?1, ?2)",
                rusqlite::params![2, "2024-01-01T12:00:01Z"],
            ).unwrap();
        }

        // Open database with new code (should trigger migration)
        let db = ScreenshotDatabase::new(db_path).unwrap();

        // Verify data was migrated
        let count: i32 = db.conn
            .query_row("SELECT COUNT(*) FROM screenshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);

        // Verify new schema columns exist
        let frame_1_created_at: String = db.conn
            .query_row(
                "SELECT created_at FROM screenshots WHERE frame_number = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(frame_1_created_at, "2024-01-01T12:00:00Z");

        let frame_1_local_time: String = db.conn
            .query_row(
                "SELECT local_time FROM screenshots WHERE frame_number = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(frame_1_local_time, "2024-01-01T12:00:00Z");

        // Verify migration was recorded
        let migration_count: i32 = db.conn
            .query_row(
                "SELECT COUNT(*) FROM migrations WHERE migration_name = 'split_timestamps'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migration_count, 1);
    }

    #[test]
    fn test_migrations_table_created() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = ScreenshotDatabase::new(db_path).unwrap();

        // Verify migrations table exists
        let table_exists: i32 = db.conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_exists, 1);
    }
}
