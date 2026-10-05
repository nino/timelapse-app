use rusqlite::{Connection, Result, TransactionBehavior};
use std::path::PathBuf;
use std::time::Duration;
use chrono::{DateTime, Utc, Local};

pub struct ScreenshotDatabase {
    conn: Connection,
}

impl ScreenshotDatabase {
    /// Create a new database connection and initialize the schema
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let mut conn = Connection::open(db_path)?;

        // A migration holds the write lock for as long as it takes to rewrite the
        // screenshots table, which on a large library is well past rusqlite's 5s
        // default. Another process opening the database meanwhile should wait it
        // out rather than fail with SQLITE_BUSY.
        conn.busy_timeout(Duration::from_secs(30))?;

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
        Self::run_migrations(&mut conn)?;

        Ok(Self { conn })
    }

    /// Run all pending migrations
    fn run_migrations(conn: &mut Connection) -> Result<()> {
        // Migration 1: Split creation_date into created_at and local_time.
        //
        // The applied-check, the table rewrite and the bookkeeping insert all run
        // inside one transaction, so the migration either lands completely or not
        // at all - a crash halfway through leaves the old schema untouched instead
        // of a half-rewritten table.
        //
        // IMMEDIATE takes the write lock up front, before the applied-check reads
        // anything. Without it two processes opening the same database can both
        // see the migration as pending and both try to record it, and the loser
        // dies on `UNIQUE constraint failed: migrations.migration_name`.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        if !Self::migration_applied(&tx, "split_timestamps")? {
            // Check if the old schema exists (has creation_date column but not created_at)
            let has_old_schema: bool = tx.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('screenshots') WHERE name = 'creation_date'",
                [],
                |row| {
                    let count: i32 = row.get(0)?;
                    Ok(count > 0)
                },
            )?;

            let has_new_schema: bool = tx.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('screenshots') WHERE name = 'created_at'",
                [],
                |row| {
                    let count: i32 = row.get(0)?;
                    Ok(count > 0)
                },
            )?;

            if has_old_schema && !has_new_schema {
                println!("Migrating database: splitting creation_date into created_at and local_time");

                // Rename the old table
                tx.execute("ALTER TABLE screenshots RENAME TO screenshots_old", [])?;

                // Create new table with updated schema
                tx.execute(
                    "CREATE TABLE screenshots (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        frame_number INTEGER NOT NULL,
                        created_at TEXT NOT NULL,
                        local_time TEXT NOT NULL
                    )",
                    [],
                )?;

                // Copy data from old table (use creation_date for both columns)
                tx.execute(
                    "INSERT INTO screenshots (id, frame_number, created_at, local_time)
                     SELECT id, frame_number, creation_date, creation_date FROM screenshots_old",
                    [],
                )?;

                // Drop old table
                tx.execute("DROP TABLE screenshots_old", [])?;

                println!("Database migration completed successfully");
            }

            // Record migration as applied
            tx.execute(
                "INSERT INTO migrations (migration_name, applied_at) VALUES (?1, ?2)",
                rusqlite::params!["split_timestamps", Utc::now().to_rfc3339()],
            )?;
        }

        tx.commit()?;

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
    use std::path::Path;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::TempDir;

    /// Create a database using the pre-migration schema and seed one row per
    /// entry in `creation_dates` (frame numbers are 1-based indices).
    ///
    /// `creation_date` is deliberately left nullable so a test can seed a row
    /// that the migration cannot copy into the new NOT NULL `created_at` column.
    fn create_old_schema_db(db_path: &Path, creation_dates: &[Option<&str>]) {
        let conn = Connection::open(db_path).unwrap();

        conn.execute(
            "CREATE TABLE screenshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                frame_number INTEGER NOT NULL,
                creation_date TEXT
            )",
            [],
        )
        .unwrap();

        for (index, creation_date) in creation_dates.iter().enumerate() {
            conn.execute(
                "INSERT INTO screenshots (frame_number, creation_date) VALUES (?1, ?2)",
                rusqlite::params![index as i64 + 1, creation_date],
            )
            .unwrap();
        }
    }

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
        create_old_schema_db(
            &db_path,
            &[Some("2024-01-01T12:00:00Z"), Some("2024-01-01T12:00:01Z")],
        );

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

    #[test]
    fn test_failed_migration_rolls_back() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // The NULL creation_date cannot be copied into the new NOT NULL
        // created_at column, so the migration fails at its third step - after the
        // rename and the CREATE TABLE have already run.
        create_old_schema_db(
            &db_path,
            &[Some("2024-01-01T12:00:00Z"), None, Some("2024-01-01T12:00:02Z")],
        );

        assert!(ScreenshotDatabase::new(db_path.clone()).is_err());

        // Everything the migration did must have been rolled back: the original
        // table is still there, under its original name, with all of its rows.
        let conn = Connection::open(&db_path).unwrap();

        let has_old_column: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('screenshots') WHERE name = 'creation_date'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_old_column, 1, "screenshots table should still hold the old schema");

        let row_count: i32 = conn
            .query_row("SELECT COUNT(*) FROM screenshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row_count, 3, "no rows should have been lost");

        let leftover_tables: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='screenshots_old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(leftover_tables, 0, "half-migrated screenshots_old should not survive");

        let migration_count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM migrations WHERE migration_name = 'split_timestamps'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migration_count, 0, "a failed migration must not be recorded as applied");
    }

    #[test]
    fn test_concurrent_open_migrates_exactly_once() {
        // Several connections opening the same un-migrated database used to race
        // through the applied-check together; the losers then died on
        // `UNIQUE constraint failed: migrations.migration_name`. Repeated because
        // the original flakiness only showed up in a minority of runs.
        const THREADS: usize = 8;

        for _ in 0..10 {
            let temp_dir = TempDir::new().unwrap();
            let db_path = temp_dir.path().join("test.db");
            create_old_schema_db(
                &db_path,
                &[Some("2024-01-01T12:00:00Z"), Some("2024-01-01T12:00:01Z")],
            );

            let barrier = Arc::new(Barrier::new(THREADS));
            let handles: Vec<_> = (0..THREADS)
                .map(|_| {
                    let barrier = Arc::clone(&barrier);
                    let db_path = db_path.clone();
                    thread::spawn(move || {
                        barrier.wait();
                        ScreenshotDatabase::new(db_path).map(|_| ())
                    })
                })
                .collect();

            for handle in handles {
                handle
                    .join()
                    .unwrap()
                    .expect("concurrent open should not fail");
            }

            let conn = Connection::open(&db_path).unwrap();

            let migration_count: i32 = conn
                .query_row(
                    "SELECT COUNT(*) FROM migrations WHERE migration_name = 'split_timestamps'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(migration_count, 1, "migration should be recorded exactly once");

            let row_count: i32 = conn
                .query_row("SELECT COUNT(*) FROM screenshots", [], |row| row.get(0))
                .unwrap();
            assert_eq!(row_count, 2, "rows should have been migrated exactly once");

            let created_at: String = conn
                .query_row(
                    "SELECT created_at FROM screenshots WHERE frame_number = 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(created_at, "2024-01-01T12:00:00Z");
        }
    }
}
