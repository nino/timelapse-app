use rusqlite::{Connection, OptionalExtension, Result, Transaction, TransactionBehavior};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use chrono::{DateTime, Utc, Local};

/// How `ocr_fts` splits text into words. Has to stay what the
/// `add_day_and_ocr` migration created the table with, so that
/// `lines_matching` finds the same words a search does.
const OCR_TOKENIZER: &str = "unicode61 remove_diacritics 2";

pub struct ScreenshotDatabase {
    conn: Connection,
}

impl ScreenshotDatabase {
    /// Open an existing database for reading only: no schema set-up and no
    /// write lock, so searching never holds up the capture loop or OCR.
    /// `None` if the database does not exist yet.
    pub fn open_read_only(db_path: PathBuf) -> Result<Option<Self>> {
        if !db_path.is_file() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // Readers wait while a writer commits; a commit is quick.
        conn.busy_timeout(Duration::from_secs(5))?;
        Ok(Some(Self { conn }))
    }

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

        // Migration 2: record which day folder each screenshot belongs to, and
        // add the OCR tables. Frame numbers restart at 1 in every day folder,
        // so a frame number alone does not identify a screenshot.
        if !Self::migration_applied(&tx, "add_day_and_ocr")? {
            Self::add_day_and_ocr(&tx)?;
            tx.execute(
                "INSERT INTO migrations (migration_name, applied_at) VALUES (?1, ?2)",
                rusqlite::params!["add_day_and_ocr", Utc::now().to_rfc3339()],
            )?;
        }

        // Migration 3: which screenshot each frame of an hourly video was
        // made from, so a frame keeps its capture time once its PNG is gone.
        if !Self::migration_applied(&tx, "add_video_frames")? {
            tx.execute_batch(
                "-- Written by the video converter. frame_index counts from 0
                 -- within the video; local_time is the screenshot's row in
                 -- `screenshots` when it has one, else the PNG's mtime.
                 CREATE TABLE video_frames (
                     video TEXT NOT NULL,
                     frame_index INTEGER NOT NULL,
                     day TEXT NOT NULL,
                     frame_number INTEGER NOT NULL,
                     local_time TEXT NOT NULL,
                     PRIMARY KEY (video, frame_index)
                 ) WITHOUT ROWID;",
            )?;
            tx.execute(
                "INSERT INTO migrations (migration_name, applied_at) VALUES (?1, ?2)",
                rusqlite::params!["add_video_frames", Utc::now().to_rfc3339()],
            )?;
        }

        tx.commit()?;

        Ok(())
    }

    fn add_day_and_ocr(tx: &Transaction) -> Result<()> {
        // Existing rows get the date part of their local timestamp, which is
        // the name of the folder they were written to (the day folder is
        // named from the local date too).
        tx.execute_batch(
            "ALTER TABLE screenshots ADD COLUMN day TEXT;
             UPDATE screenshots SET day = substr(local_time, 1, 10);
             CREATE INDEX screenshots_day_frame ON screenshots (day, frame_number);

             -- One row per frame that was actually run through OCR. Frames
             -- skipped because the screen had not changed get no row; a search
             -- hit on frame N stands for every frame up to the next row.
             CREATE TABLE ocr_frames (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 day TEXT NOT NULL,
                 frame_number INTEGER NOT NULL,
                 text TEXT NOT NULL,
                 lines_json TEXT NOT NULL,
                 processed_at TEXT NOT NULL,
                 UNIQUE (day, frame_number)
             );

             -- Per day folder: every frame up to and including last_frame has
             -- been handled (OCR'd or skipped as unchanged). The video
             -- converter reads this before deleting an hour's PNGs.
             CREATE TABLE ocr_progress (
                 day TEXT PRIMARY KEY,
                 last_frame INTEGER NOT NULL
             );

             -- remove_diacritics 2 makes a search for 'chapter' match the
             -- stray accents Vision sometimes adds, as in 'Chápter'.
             CREATE VIRTUAL TABLE ocr_fts USING fts5(
                 text,
                 content = 'ocr_frames',
                 content_rowid = 'id',
                 tokenize = 'unicode61 remove_diacritics 2'
             );

             CREATE TRIGGER ocr_frames_ai AFTER INSERT ON ocr_frames BEGIN
                 INSERT INTO ocr_fts (rowid, text) VALUES (new.id, new.text);
             END;
             CREATE TRIGGER ocr_frames_ad AFTER DELETE ON ocr_frames BEGIN
                 INSERT INTO ocr_fts (ocr_fts, rowid, text) VALUES ('delete', old.id, old.text);
             END;
             CREATE TRIGGER ocr_frames_au AFTER UPDATE ON ocr_frames BEGIN
                 INSERT INTO ocr_fts (ocr_fts, rowid, text) VALUES ('delete', old.id, old.text);
                 INSERT INTO ocr_fts (rowid, text) VALUES (new.id, new.text);
             END;",
        )
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

    /// Insert a new screenshot record. `day` is the name of the day folder the
    /// PNG was written to (`YYYY-MM-DD`).
    pub fn insert_screenshot(
        &self,
        day: &str,
        frame_number: u32,
        created_at: DateTime<Utc>,
        local_time: DateTime<Local>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO screenshots (day, frame_number, created_at, local_time) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                day,
                frame_number,
                created_at.to_rfc3339(),
                local_time.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Get screenshot metadata by frame number. Frame numbers restart in every
    /// day folder, so pass `day` to get the right one; without it this returns
    /// the most recent screenshot with that number.
    pub fn get_screenshot_by_frame(
        &self,
        frame_number: u32,
        day: Option<&str>,
    ) -> Result<Option<(String, String)>> {
        self.conn
            .query_row(
                "SELECT created_at, local_time FROM screenshots
                 WHERE frame_number = ?1 AND (?2 IS NULL OR day = ?2)
                 ORDER BY id DESC LIMIT 1",
                rusqlite::params![frame_number, day],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
    }

    /// Record that OCR has handled frame `frame_number` of `day`. `result` is
    /// the recognized text and the per-line JSON, or `None` when the frame was
    /// skipped because the screen had not changed. Either way the day's
    /// progress mark moves up to this frame.
    pub fn record_ocr_frame(
        &self,
        day: &str,
        frame_number: u32,
        result: Option<(&str, &str)>,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        if let Some((text, lines_json)) = result {
            tx.execute(
                "INSERT INTO ocr_frames (day, frame_number, text, lines_json, processed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (day, frame_number) DO UPDATE SET
                     text = excluded.text,
                     lines_json = excluded.lines_json,
                     processed_at = excluded.processed_at",
                rusqlite::params![day, frame_number, text, lines_json, Utc::now().to_rfc3339()],
            )?;
        }

        tx.execute(
            "INSERT INTO ocr_progress (day, last_frame) VALUES (?1, ?2)
             ON CONFLICT (day) DO UPDATE SET last_frame = max(last_frame, excluded.last_frame)",
            rusqlite::params![day, frame_number],
        )?;

        tx.commit()
    }

    /// The highest frame of `day` that OCR has handled, with every frame below
    /// it handled too. `None` if OCR has not started on that day.
    pub fn ocr_done_through(&self, day: &str) -> Result<Option<u32>> {
        self.conn
            .query_row(
                "SELECT last_frame FROM ocr_progress WHERE day = ?1",
                [day],
                |row| row.get(0),
            )
            .optional()
    }

    /// Record the screenshots `video` was encoded from, in video order, as
    /// (frame number, mtime) in day folder `day`. Replaces whatever was
    /// recorded for `video` before. Each frame's time is its `screenshots`
    /// row's when there is one, so it does not change when the PNG gives way
    /// to the video.
    pub fn record_video_frames(
        &self,
        video: &str,
        day: &str,
        frames: &[(u32, DateTime<Local>)],
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM video_frames WHERE video = ?1", [video])?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO video_frames (video, frame_index, day, frame_number, local_time)
                 VALUES (?1, ?2, ?3, ?4, COALESCE(
                     (SELECT local_time FROM screenshots
                      WHERE day = ?3 AND frame_number = ?4
                      ORDER BY id DESC LIMIT 1),
                     ?5))",
            )?;
            for (index, (frame_number, modified)) in frames.iter().enumerate() {
                insert.execute(rusqlite::params![
                    video,
                    index,
                    day,
                    frame_number,
                    modified.to_rfc3339()
                ])?;
            }
        }
        tx.commit()
    }

    /// The frame numbers recorded for `video`, in video order.
    pub fn video_frame_numbers(&self, video: &str) -> Result<Vec<u32>> {
        let mut statement = self
            .conn
            .prepare("SELECT frame_number FROM video_frames WHERE video = ?1 ORDER BY frame_index")?;
        let numbers = statement.query_map([video], |row| row.get(0))?.collect();
        numbers
    }

    /// Whether `record_video_frames` has run for `video`.
    pub fn has_video_frames(&self, video: &str) -> Result<bool> {
        self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM video_frames WHERE video = ?1)",
            [video],
            |row| row.get(0),
        )
    }

    /// Full-text search over OCR'd frames, newest first. Every word in `query`
    /// has to appear; the last one also matches as a prefix, so results show
    /// up while a word is still being typed.
    pub fn search_ocr(&self, query: &str, limit: u32) -> Result<Vec<OcrHit>> {
        let Some(fts_query) = fts_query(query) else {
            return Ok(Vec::new());
        };

        let mut statement = self.conn.prepare(
            "SELECT ocr_frames.day, ocr_frames.frame_number,
                    snippet(ocr_fts, 0, '[', ']', '…', 12)
             FROM ocr_fts JOIN ocr_frames ON ocr_frames.id = ocr_fts.rowid
             WHERE ocr_fts MATCH ?1
             ORDER BY ocr_frames.day DESC, ocr_frames.frame_number DESC
             LIMIT ?2",
        )?;

        let hits = statement
            .query_map(rusqlite::params![fts_query, limit], |row| {
                Ok(OcrHit {
                    day: row.get(0)?,
                    frame_number: row.get(1)?,
                    snippet: row.get(2)?,
                })
            })?
            .collect();
        hits
    }

    /// Every OCR'd frame of `day` whose text matches `query` (as `search_ocr`
    /// matches it), in frame order, with how far its text stays on screen.
    pub fn search_ocr_in_day(&self, day: &str, query: &str) -> Result<Vec<OcrDayMatch>> {
        let Some(fts_query) = fts_query(query) else {
            return Ok(Vec::new());
        };

        // A row stands for every frame up to the day's next row, because OCR
        // only writes a row when the screen changed. The day's last row runs
        // to the end of what OCR has handled.
        let mut statement = self.conn.prepare(
            "WITH day_rows AS (
                 SELECT id, frame_number,
                        LEAD(frame_number) OVER (ORDER BY frame_number) AS next_frame
                 FROM ocr_frames WHERE day = ?1
             )
             SELECT day_rows.frame_number, day_rows.next_frame, ocr_progress.last_frame
             FROM day_rows
             JOIN ocr_fts ON ocr_fts.rowid = day_rows.id
             LEFT JOIN ocr_progress ON ocr_progress.day = ?1
             WHERE ocr_fts MATCH ?2
             ORDER BY day_rows.frame_number",
        )?;

        let matches = statement
            .query_map(rusqlite::params![day, fts_query], |row| {
                Ok(OcrDayMatch {
                    frame_number: row.get(0)?,
                    next_frame: row.get(1)?,
                    done_through: row.get(2)?,
                })
            })?
            .collect();
        matches
    }

    /// On each day, how many times text matching `query` came onto the screen:
    /// a run of matching OCR rows with no other row between them counts once,
    /// the way the find bar merges them. Newest day first.
    pub fn count_ocr_matches(&self, query: &str) -> Result<Vec<(String, u32)>> {
        let Some(fts_query) = fts_query(query) else {
            return Ok(Vec::new());
        };

        let mut statement = self.conn.prepare(
            "WITH hits AS (
                 SELECT rowid AS id FROM ocr_fts WHERE ocr_fts MATCH ?1
             ),
             runs AS (
                 SELECT day, hit,
                        LAG(hit, 1, 0) OVER (PARTITION BY day ORDER BY frame_number) AS after_hit
                 FROM (SELECT ocr_frames.day, ocr_frames.frame_number, ocr_frames.id IN hits AS hit
                       FROM ocr_frames)
             )
             SELECT day, COUNT(*) FROM runs
             WHERE hit AND NOT after_hit
             GROUP BY day
             ORDER BY day DESC",
        )?;

        let counts = statement
            .query_map([fts_query], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect();
        counts
    }

    /// The recognized lines of frame `frame_number` of `day`, as `ocr::OcrLine`s
    /// in JSON, or `None` if OCR has no row for it.
    pub fn ocr_lines(&self, day: &str, frame_number: u32) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT lines_json FROM ocr_frames WHERE day = ?1 AND frame_number = ?2",
                rusqlite::params![day, frame_number],
                |row| row.get(0),
            )
            .optional()
    }

    /// Changes whenever OCR records a frame or moves a day's progress on, so
    /// a caller can tell cheaply whether searching again could find anything
    /// new. Screenshots being captured don't change it.
    pub fn ocr_version(&self) -> Result<String> {
        self.conn.query_row(
            "SELECT (SELECT COALESCE(MAX(id), 0) FROM ocr_frames) || ':' ||
                    (SELECT COUNT(*) FROM ocr_frames) || ':' ||
                    (SELECT COALESCE(SUM(last_frame), 0) FROM ocr_progress)",
            [],
            |row| row.get(0),
        )
    }
}

/// Which of `lines` hold at least one word of `query`, matched the way the
/// OCR index matches it (same tokenizer, so case and accents are ignored, and
/// the last word as a prefix). Indices into `lines`, ascending.
pub fn lines_matching(lines: &[&str], query: &str) -> Result<Vec<usize>> {
    let words = fts_words(query);
    let Some((last, rest)) = words.split_last() else {
        return Ok(Vec::new());
    };
    let mut any_word = rest.to_vec();
    any_word.push(format!("{}*", last));

    let conn = Connection::open_in_memory()?;
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE lines USING fts5(text, tokenize = '{OCR_TOKENIZER}')"
    ))?;
    for (i, line) in lines.iter().enumerate() {
        conn.execute(
            "INSERT INTO lines (rowid, text) VALUES (?1, ?2)",
            rusqlite::params![i as i64, line],
        )?;
    }
    let mut statement =
        conn.prepare("SELECT rowid FROM lines WHERE lines MATCH ?1 ORDER BY rowid")?;
    let found = statement
        .query_map([any_word.join(" OR ")], |row| row.get::<_, i64>(0))?
        .map(|row| row.map(|i| i as usize))
        .collect();
    found
}

/// A frame of one day whose OCR text matched a search.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrDayMatch {
    pub frame_number: u32,
    /// The day's next OCR'd frame, where this frame's text stops standing in
    /// for the frames after it. `None` for the day's last OCR'd frame.
    pub next_frame: Option<u32>,
    /// The highest frame OCR has handled on the day (see `ocr_done_through`).
    pub done_through: Option<u32>,
}

/// One search result: the frame whose OCR text matched, and the matching
/// passage with the hits wrapped in `[` `]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OcrHit {
    pub day: String,
    pub frame_number: u32,
    pub snippet: String,
}

/// Turn free text into an FTS5 query: each word becomes a quoted string, so
/// characters like `-`, `:` or `*` are matched literally instead of being
/// read as query syntax, and the last word is a prefix match.
fn fts_query(query: &str) -> Option<String> {
    let words = fts_words(query);
    let (last, rest) = words.split_last()?;
    let mut parts = rest.to_vec();
    parts.push(format!("{}*", last));
    Some(parts.join(" "))
}

/// Each word of `query` as a quoted FTS5 string.
fn fts_words(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
        .collect()
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
        let result = db.insert_screenshot("2024-01-01", frame_number, created_at, local_time);
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
            let result = db.insert_screenshot("2024-01-01", i, Utc::now(), Local::now());
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

    fn day_of(db: &ScreenshotDatabase, frame_number: u32) -> Option<String> {
        db.conn
            .query_row(
                "SELECT day FROM screenshots WHERE frame_number = ?1",
                [frame_number],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn test_migration_fills_day_from_local_time() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        create_old_schema_db(
            &db_path,
            &[Some("2024-01-01T23:59:59+01:00"), Some("2024-01-02T00:00:01+01:00")],
        );

        let db = ScreenshotDatabase::new(db_path).unwrap();

        assert_eq!(day_of(&db, 1).as_deref(), Some("2024-01-01"));
        assert_eq!(day_of(&db, 2).as_deref(), Some("2024-01-02"));
    }

    #[test]
    fn test_insert_screenshot_records_day() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();

        db.insert_screenshot("2024-03-04", 7, Utc::now(), Local::now()).unwrap();

        assert_eq!(day_of(&db, 7).as_deref(), Some("2024-03-04"));
    }

    #[test]
    fn test_get_screenshot_by_frame_tells_days_apart() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();
        let monday = "2024-01-01T09:00:00+00:00".parse::<DateTime<Utc>>().unwrap();
        let tuesday = "2024-01-02T09:00:00+00:00".parse::<DateTime<Utc>>().unwrap();

        db.insert_screenshot("2024-01-01", 1, monday, monday.with_timezone(&Local)).unwrap();
        db.insert_screenshot("2024-01-02", 1, tuesday, tuesday.with_timezone(&Local)).unwrap();

        let (created_at, _) = db.get_screenshot_by_frame(1, Some("2024-01-01")).unwrap().unwrap();
        assert_eq!(created_at, monday.to_rfc3339());

        let (created_at, _) = db.get_screenshot_by_frame(1, Some("2024-01-02")).unwrap().unwrap();
        assert_eq!(created_at, tuesday.to_rfc3339());

        // Without a day, the most recently inserted one wins.
        let (created_at, _) = db.get_screenshot_by_frame(1, None).unwrap().unwrap();
        assert_eq!(created_at, tuesday.to_rfc3339());

        assert_eq!(db.get_screenshot_by_frame(1, Some("2024-01-03")).unwrap(), None);
    }

    #[test]
    fn test_ocr_progress_only_moves_forward() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();

        assert_eq!(db.ocr_done_through("2024-01-01").unwrap(), None);

        db.record_ocr_frame("2024-01-01", 1, Some(("hello", "[]"))).unwrap();
        db.record_ocr_frame("2024-01-01", 2, None).unwrap();
        assert_eq!(db.ocr_done_through("2024-01-01").unwrap(), Some(2));

        db.record_ocr_frame("2024-01-01", 1, Some(("hello again", "[]"))).unwrap();
        assert_eq!(db.ocr_done_through("2024-01-01").unwrap(), Some(2));

        // Skipped frames get no row of their own.
        let rows: i32 = db
            .conn
            .query_row("SELECT COUNT(*) FROM ocr_frames", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);

        assert_eq!(db.ocr_done_through("2024-01-02").unwrap(), None);
    }

    fn hit(day: &str, frame_number: u32) -> (String, u32) {
        (day.to_string(), frame_number)
    }

    fn search(db: &ScreenshotDatabase, query: &str) -> Vec<(String, u32)> {
        db.search_ocr(query, 10)
            .unwrap()
            .into_iter()
            .map(|hit| (hit.day, hit.frame_number))
            .collect()
    }

    #[test]
    fn test_record_video_frames_takes_times_from_screenshots() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();
        let shot: DateTime<Local> = "2024-01-02T09:00:00.5+00:00".parse().unwrap();
        let file: DateTime<Local> = "2024-01-02T09:00:01+00:00".parse().unwrap();
        let other_day: DateTime<Local> = "2024-01-01T17:00:00+00:00".parse().unwrap();
        db.insert_screenshot("2024-01-02", 1, shot.into(), shot).unwrap();
        // Frame 2 of another day must not lend its time.
        db.insert_screenshot("2024-01-01", 2, other_day.into(), other_day).unwrap();
        let video = "2024-01-02--09-00-00--hourly.mov";

        assert!(!db.has_video_frames(video).unwrap());
        db.record_video_frames(video, "2024-01-02", &[(1, file), (2, file), (3, file)]).unwrap();
        db.record_video_frames(video, "2024-01-02", &[(1, file), (2, file)]).unwrap();

        assert!(db.has_video_frames(video).unwrap());
        let mut statement = db
            .conn
            .prepare("SELECT frame_index, frame_number, local_time FROM video_frames ORDER BY frame_index")
            .unwrap();
        let rows: Vec<(u32, u32, String)> = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![(0, 1, shot.to_rfc3339()), (1, 2, file.to_rfc3339())],
            "a second record replaces the first"
        );
    }

    #[test]
    fn test_search_ocr() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();

        db.record_ocr_frame("2024-01-01", 3, Some(("ffmpeg -i input.mov\nChápter 1", "[]")))
            .unwrap();
        db.record_ocr_frame("2024-01-02", 9, Some(("Blender: modelling the chair", "[]")))
            .unwrap();
        db.record_ocr_frame("2024-01-02", 12, Some(("chapter list for the chair video", "[]")))
            .unwrap();

        // Newest first.
        assert_eq!(search(&db, "chair"), vec![hit("2024-01-02", 12), hit("2024-01-02", 9)]);
        // Every word has to match.
        assert_eq!(search(&db, "chair blender"), vec![hit("2024-01-02", 9)]);
        // Accents Vision added by mistake do not get in the way.
        assert_eq!(search(&db, "chapter"), vec![hit("2024-01-02", 12), hit("2024-01-01", 3)]);
        // The last word matches as a prefix.
        assert_eq!(search(&db, "mod"), vec![hit("2024-01-02", 9)]);
        // Query syntax characters are taken literally rather than failing.
        assert_eq!(search(&db, "-i input.mov"), vec![hit("2024-01-01", 3)]);
        assert_eq!(search(&db, "\"unbalanced"), Vec::<(String, u32)>::new());
        assert_eq!(search(&db, "   "), Vec::<(String, u32)>::new());

        let hits = db.search_ocr("blender", 10).unwrap();
        assert_eq!(hits[0].snippet, "[Blender]: modelling the chair");
    }

    #[test]
    fn test_search_ocr_follows_updates() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();

        db.record_ocr_frame("2024-01-01", 1, Some(("old words", "[]"))).unwrap();
        db.record_ocr_frame("2024-01-01", 1, Some(("new words", "[]"))).unwrap();

        assert_eq!(search(&db, "old"), Vec::<(String, u32)>::new());
        assert_eq!(search(&db, "new"), vec![hit("2024-01-01", 1)]);
    }

    #[test]
    fn test_search_ocr_in_day() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();

        db.record_ocr_frame("2024-01-01", 2, Some(("cargo build", "[1]"))).unwrap();
        db.record_ocr_frame("2024-01-01", 3, None).unwrap();
        db.record_ocr_frame("2024-01-01", 5, Some(("bun test", "[2]"))).unwrap();
        db.record_ocr_frame("2024-01-01", 8, Some(("cargo test", "[3]"))).unwrap();
        db.record_ocr_frame("2024-01-01", 9, None).unwrap();
        db.record_ocr_frame("2024-01-02", 1, Some(("cargo run", "[4]"))).unwrap();

        let found: Vec<_> = db
            .search_ocr_in_day("2024-01-01", "cargo")
            .unwrap()
            .into_iter()
            .map(|m| (m.frame_number, m.next_frame, m.done_through))
            .collect();
        assert_eq!(
            found,
            vec![
                // Its run ends at the next row, even though that row didn't match.
                (2, Some(5), Some(9)),
                (8, None, Some(9)),
            ]
        );
        assert_eq!(db.ocr_lines("2024-01-01", 5).unwrap().as_deref(), Some("[2]"));
        assert_eq!(db.ocr_lines("2024-01-01", 3).unwrap(), None);
        assert!(db.search_ocr_in_day("2024-01-01", "  ").unwrap().is_empty());
        assert!(db.search_ocr_in_day("2024-01-03", "cargo").unwrap().is_empty());
    }

    #[test]
    fn test_count_ocr_matches() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();

        // Two runs on the 1st: frames 2-5, then 9.
        db.record_ocr_frame("2024-01-01", 2, Some(("cargo build", "[]"))).unwrap();
        db.record_ocr_frame("2024-01-01", 5, Some(("cargo test", "[]"))).unwrap();
        db.record_ocr_frame("2024-01-01", 7, Some(("bun test", "[]"))).unwrap();
        db.record_ocr_frame("2024-01-01", 9, Some(("cargo run", "[]"))).unwrap();
        db.record_ocr_frame("2024-01-02", 1, Some(("cargo run", "[]"))).unwrap();
        db.record_ocr_frame("2024-01-03", 1, Some(("bun run", "[]"))).unwrap();

        assert_eq!(
            db.count_ocr_matches("cargo").unwrap(),
            vec![("2024-01-02".to_string(), 1), ("2024-01-01".to_string(), 2)]
        );
        assert!(db.count_ocr_matches("").unwrap().is_empty());
    }

    #[test]
    fn test_lines_matching() {
        let lines = ["$ Cargo build", "Café au lait", "latest results", "cargo-test passed"];
        // Case and accents don't matter, any word will do, and the last word
        // is a prefix but the others are whole words.
        assert_eq!(lines_matching(&lines, "cargo").unwrap(), vec![0, 3]);
        assert_eq!(lines_matching(&lines, "cafe").unwrap(), vec![1]);
        assert_eq!(lines_matching(&lines, "test lai").unwrap(), vec![1, 3]);
        assert_eq!(lines_matching(&lines, "\"cargo $").unwrap(), vec![0, 3]);
        assert!(lines_matching(&lines, " ").unwrap().is_empty());
    }

    #[test]
    fn test_read_only_and_version() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.db");
        assert!(ScreenshotDatabase::open_read_only(path.clone()).unwrap().is_none());

        let db = ScreenshotDatabase::new(path.clone()).unwrap();
        let reader = ScreenshotDatabase::open_read_only(path).unwrap().unwrap();
        let before = reader.ocr_version().unwrap();
        db.record_ocr_frame("2024-01-01", 1, None).unwrap();
        let skipped = reader.ocr_version().unwrap();
        assert_ne!(before, skipped);
        db.record_ocr_frame("2024-01-01", 2, Some(("cargo", "[]"))).unwrap();
        assert_ne!(skipped, reader.ocr_version().unwrap());
        assert_eq!(reader.count_ocr_matches("cargo").unwrap().len(), 1);
    }

    #[test]
    fn test_fts_query() {
        assert_eq!(fts_query("hello"), Some("\"hello\"*".to_string()));
        assert_eq!(fts_query(" a  b "), Some("\"a\" \"b\"*".to_string()));
        assert_eq!(fts_query("say \"hi\""), Some("\"say\" \"\"\"hi\"\"\"*".to_string()));
        assert_eq!(fts_query(""), None);
    }

    #[test]
    fn test_migration_from_split_timestamps_schema() {
        // A library written by the previous release: split_timestamps already
        // applied, and screenshots without a day column.
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE migrations (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     migration_name TEXT UNIQUE NOT NULL,
                     applied_at TEXT NOT NULL
                 );
                 INSERT INTO migrations (migration_name, applied_at)
                     VALUES ('split_timestamps', '2024-01-01T00:00:00Z');
                 CREATE TABLE screenshots (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     frame_number INTEGER NOT NULL,
                     created_at TEXT NOT NULL,
                     local_time TEXT NOT NULL
                 );
                 INSERT INTO screenshots (frame_number, created_at, local_time)
                     VALUES (5, '2024-06-30T22:30:00+00:00', '2024-07-01T00:30:00+02:00');",
            )
            .unwrap();
        }

        let db = ScreenshotDatabase::new(db_path.clone()).unwrap();
        assert_eq!(day_of(&db, 5).as_deref(), Some("2024-07-01"));
        drop(db);

        // Opening again does not try to migrate a second time.
        let db = ScreenshotDatabase::new(db_path).unwrap();
        let applied: i32 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM migrations WHERE migration_name = 'add_day_and_ocr'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1);
    }
}
