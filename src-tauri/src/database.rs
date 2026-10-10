//! `screenshots.db`, next to the screenshots in the library root.
//!
//! Every migration runs in the one `IMMEDIATE` transaction of
//! `run_migrations` (check applied, run, record), so a crash never leaves one
//! half applied. The older ones exist for libraries written before them:
//! `split_timestamps` split the legacy `creation_date` column into
//! `created_at` (UTC) and `local_time`, and `compact_ocr_frames` replaced the
//! `lines_json` column (every line's text again, with full-precision floats)
//! by packed `boxes`, dropped rows repeating the previous row's text, and
//! vacuums once.
//!
//! OCR rows are keyed by (day, frame number), because frame numbers restart
//! in every day folder. A day read from video has no frame numbers, so there
//! `ocr_progress.by_position` is set and `frame_number`/`last_frame` are
//! 1-based positions in the day's videos; `ocr_done_through` ignores those
//! rows so the converter never mistakes one for the other.

use rusqlite::{Connection, OptionalExtension, Result, Transaction, TransactionBehavior};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use chrono::{DateTime, Utc, Local};
use crate::menu_app::menu_bar_app;

/// How `ocr_fts` splits text into words. Has to stay what the
/// `add_day_and_ocr` migration created the table with, so that
/// `lines_matching` finds the same words a search does.
const OCR_TOKENIZER: &str = "unicode61 remove_diacritics 2";

pub struct ScreenshotDatabase {
    conn: Connection,
}

/// The app and window that were in front when a screenshot was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontWindow {
    /// The app's name, as the window server reports it (`Blender`).
    pub app_name: String,
    /// The app's bundle identifier (`org.blenderfoundation.blender`), when it
    /// has one.
    pub bundle_id: Option<String>,
    /// The app bundle on macOS (`/Applications/Blender.app`), the executable
    /// elsewhere.
    pub app_path: String,
    /// The window's title. Empty when the app doesn't give its window one.
    pub title: String,
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

        // Run migrations. A migration that shrank the OCR data leaves the
        // freed pages inside the file until it is vacuumed.
        if Self::run_migrations(&mut conn)? {
            // Not fatal: the space is reused by later writes either way.
            if let Err(e) = conn.execute_batch("VACUUM") {
                eprintln!("Failed to vacuum the database: {}", e);
            }
        }

        Ok(Self { conn })
    }

    /// Run all pending migrations. Returns whether the file should be
    /// vacuumed, which can't happen inside the migrations' transaction.
    fn run_migrations(conn: &mut Connection) -> Result<bool> {
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

        // Migration 4: store each OCR row's line boxes as packed integers
        // instead of JSON that repeated every line's text, and drop rows
        // whose text is the same as the row before them.
        let mut vacuum = false;
        if !Self::migration_applied(&tx, "compact_ocr_frames")? {
            Self::compact_ocr_frames(&tx)?;
            tx.execute(
                "INSERT INTO migrations (migration_name, applied_at) VALUES (?1, ?2)",
                rusqlite::params!["compact_ocr_frames", Utc::now().to_rfc3339()],
            )?;
            vacuum = true;
        }

        // Migration 5: let OCR progress count positions in a day's videos, for
        // days that only exist as video and so have no frame numbers.
        if !Self::migration_applied(&tx, "ocr_video_positions")? {
            tx.execute(
                "ALTER TABLE ocr_progress ADD COLUMN by_position INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            tx.execute(
                "INSERT INTO migrations (migration_name, applied_at) VALUES (?1, ?2)",
                rusqlite::params!["ocr_video_positions", Utc::now().to_rfc3339()],
            )?;
        }

        // Migration 6: from 2026-10-08 16:01, Vision failed on every frame
        // in a long-running app (it had lost the Neural Engine), and OCR
        // marked each frame it failed on as done. Start every day read from
        // video over, and re-read the screenshots still on disk from that
        // day on. Rows already found stay; reading a frame again replaces
        // its row.
        if !Self::migration_applied(&tx, "reread_after_vision_failure")? {
            tx.execute_batch(
                "DELETE FROM ocr_progress WHERE by_position;
                 UPDATE ocr_progress SET last_frame = 0
                     WHERE NOT by_position AND day >= '2026-10-08';",
            )?;
            tx.execute(
                "INSERT INTO migrations (migration_name, applied_at) VALUES (?1, ?2)",
                rusqlite::params!["reread_after_vision_failure", Utc::now().to_rfc3339()],
            )?;
        }

        // Migration 7: which app and window were in front for each
        // screenshot, so a day can later be cut down to the time spent in one
        // app. A window is stored once and screenshots point at it, so a
        // long title isn't repeated every second.
        if !Self::migration_applied(&tx, "add_windows")? {
            tx.execute_batch(
                "CREATE TABLE windows (
                     id INTEGER PRIMARY KEY,
                     app_name TEXT NOT NULL,
                     bundle_id TEXT,
                     app_path TEXT NOT NULL,
                     title TEXT NOT NULL,
                     UNIQUE (app_path, app_name, title)
                 );
                 CREATE INDEX windows_bundle_id ON windows (bundle_id);
                 -- NULL: taken before this was recorded, or with no window
                 -- in front.
                 ALTER TABLE screenshots ADD COLUMN window_id INTEGER REFERENCES windows (id);",
            )?;
            tx.execute(
                "INSERT INTO migrations (migration_name, applied_at) VALUES (?1, ?2)",
                rusqlite::params!["add_windows", Utc::now().to_rfc3339()],
            )?;
        }

        // Migration 8: the front app as the menu bar names it, read from
        // each OCR row's text, for screenshots taken before `window_id` was
        // recorded and for days that only exist as video.
        if !Self::migration_applied(&tx, "add_menu_app")? {
            tx.execute("ALTER TABLE ocr_frames ADD COLUMN menu_app TEXT", [])?;
            Self::fill_menu_apps(&tx)?;
            tx.execute(
                "INSERT INTO migrations (migration_name, applied_at) VALUES (?1, ?2)",
                rusqlite::params!["add_menu_app", Utc::now().to_rfc3339()],
            )?;
        }

        tx.commit()?;

        Ok(vacuum)
    }

    /// Set every OCR row's `menu_app` from its text.
    fn fill_menu_apps(tx: &Transaction) -> Result<()> {
        let mut select = tx.prepare("SELECT id, text, boxes FROM ocr_frames")?;
        let mut update = tx.prepare("UPDATE ocr_frames SET menu_app = ?1 WHERE id = ?2")?;
        let mut rows = select.query([])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let text: String = row.get(1)?;
            let boxes: Vec<u8> = row.get(2)?;
            if let Some(app) = menu_bar_app(&text, &unpack_boxes(&boxes)) {
                update.execute(rusqlite::params![app, id])?;
            }
        }
        Ok(())
    }

    fn compact_ocr_frames(tx: &Transaction) -> Result<()> {
        // The update trigger re-indexed a row on any update; only a change of
        // text needs that, and filling in `boxes` below is not one.
        tx.execute_batch(
            "DROP TRIGGER ocr_frames_au;
             CREATE TRIGGER ocr_frames_au AFTER UPDATE OF text ON ocr_frames BEGIN
                 INSERT INTO ocr_fts (ocr_fts, rowid, text) VALUES ('delete', old.id, old.text);
                 INSERT INTO ocr_fts (rowid, text) VALUES (new.id, new.text);
             END;
             -- See `pack_boxes`. Line N of `text` has box N.
             ALTER TABLE ocr_frames ADD COLUMN boxes BLOB NOT NULL DEFAULT x'';",
        )?;

        #[derive(serde::Deserialize)]
        struct JsonLine {
            x: f64,
            y: f64,
            width: f64,
            height: f64,
        }
        {
            let mut select = tx.prepare("SELECT id, lines_json FROM ocr_frames")?;
            let mut update = tx.prepare("UPDATE ocr_frames SET boxes = ?1 WHERE id = ?2")?;
            let mut rows = select.query([])?;
            while let Some(row) = rows.next()? {
                let id: i64 = row.get(0)?;
                let json: String = row.get(1)?;
                // A row whose JSON can't be read keeps no boxes, so it gets
                // no highlights, which is all the JSON was for.
                let boxes: Vec<LineBox> = serde_json::from_str::<Vec<JsonLine>>(&json)
                    .map(|lines| {
                        lines.iter().map(|l| [l.x, l.y, l.width, l.height]).collect()
                    })
                    .unwrap_or_default();
                update.execute(rusqlite::params![pack_boxes(&boxes), id])?;
            }
        }

        // A row stands for every frame up to the next one, so a row with the
        // same text as the one before it adds nothing a search can find.
        tx.execute_batch(
            "DELETE FROM ocr_frames WHERE id IN (
                 SELECT id FROM (
                     SELECT id, text,
                            LAG(text) OVER (PARTITION BY day ORDER BY frame_number) AS previous
                     FROM ocr_frames
                 ) WHERE text = previous
             );
             ALTER TABLE ocr_frames DROP COLUMN lines_json;",
        )
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
    #[cfg(test)]
    pub fn insert_screenshot(
        &self,
        day: &str,
        frame_number: u32,
        created_at: DateTime<Utc>,
        local_time: DateTime<Local>,
    ) -> Result<()> {
        self.insert_capture(day, frame_number, created_at, local_time, None)
    }

    /// Insert a new screenshot record, with the window that was in front
    /// when it was taken.
    pub fn insert_capture(
        &self,
        day: &str,
        frame_number: u32,
        created_at: DateTime<Utc>,
        local_time: DateTime<Local>,
        window: Option<&FrontWindow>,
    ) -> Result<()> {
        let window_id = window.map(|window| self.window_id(window)).transpose()?;
        self.conn
            .prepare_cached(
                "INSERT INTO screenshots (day, frame_number, created_at, local_time, window_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(rusqlite::params![
                day,
                frame_number,
                created_at.to_rfc3339(),
                local_time.to_rfc3339(),
                window_id,
            ])?;
        Ok(())
    }

    /// The `windows` row for `window`, added if it is new. The same window
    /// stays in front for most captures, so this is nearly always one
    /// indexed lookup.
    fn window_id(&self, window: &FrontWindow) -> Result<i64> {
        let key = rusqlite::params![window.app_path, window.app_name, window.title];
        let found = self
            .conn
            .prepare_cached(
                "SELECT id FROM windows WHERE app_path = ?1 AND app_name = ?2 AND title = ?3",
            )?
            .query_row(key, |row| row.get(0))
            .optional()?;
        if let Some(id) = found {
            return Ok(id);
        }
        self.conn
            .prepare_cached(
                "INSERT INTO windows (app_path, app_name, title, bundle_id) VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(rusqlite::params![
                window.app_path,
                window.app_name,
                window.title,
                window.bundle_id
            ])?;
        Ok(self.conn.last_insert_rowid())
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
    /// the recognized text, one line per box, and the boxes, or `None` when
    /// the frame was skipped because the screen had not changed. Either way
    /// the day's progress mark moves up to this frame.
    pub fn record_ocr_frame(
        &self,
        day: &str,
        frame_number: u32,
        result: Option<(&str, &[LineBox])>,
    ) -> Result<()> {
        self.record(day, frame_number, result, false)
    }

    /// `record_ocr_frame` for a day that only exists as video, where frames
    /// have no numbers: `position` is the frame's 1-based position in the
    /// day's videos played back to back (one more than its frame-source
    /// index), and the day's progress counts positions from then on.
    pub fn record_video_ocr_frame(
        &self,
        day: &str,
        position: u32,
        result: Option<(&str, &[LineBox])>,
    ) -> Result<()> {
        self.record(day, position, result, true)
    }

    fn record(
        &self,
        day: &str,
        frame_number: u32,
        result: Option<(&str, &[LineBox])>,
        by_position: bool,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        if let Some((text, boxes)) = result {
            tx.execute(
                "INSERT INTO ocr_frames (day, frame_number, text, boxes, processed_at, menu_app)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (day, frame_number) DO UPDATE SET
                     text = excluded.text,
                     boxes = excluded.boxes,
                     processed_at = excluded.processed_at,
                     menu_app = excluded.menu_app",
                rusqlite::params![
                    day,
                    frame_number,
                    text,
                    pack_boxes(boxes),
                    Utc::now().to_rfc3339(),
                    menu_bar_app(text, boxes)
                ],
            )?;
        }

        tx.execute(
            "INSERT INTO ocr_progress (day, last_frame, by_position) VALUES (?1, ?2, ?3)
             ON CONFLICT (day) DO UPDATE SET last_frame = max(last_frame, excluded.last_frame)",
            rusqlite::params![day, frame_number, by_position],
        )?;

        tx.commit()
    }

    /// The highest frame of `day` that OCR has handled, with every frame below
    /// it handled too. `None` if OCR has not started on that day's
    /// screenshots, including when it is reading the day from video, so the
    /// video converter never takes a video position for a frame number.
    pub fn ocr_done_through(&self, day: &str) -> Result<Option<u32>> {
        self.conn
            .query_row(
                "SELECT last_frame FROM ocr_progress WHERE day = ?1 AND NOT by_position",
                [day],
                |row| row.get(0),
            )
            .optional()
    }

    /// How far OCR has got through `day`, and whether it counts frame numbers
    /// or video positions there.
    pub fn ocr_progress(&self, day: &str) -> Result<Option<OcrProgress>> {
        self.conn
            .query_row(
                "SELECT last_frame, by_position FROM ocr_progress WHERE day = ?1",
                [day],
                |row| {
                    let last: u32 = row.get(0)?;
                    Ok(if row.get(1)? {
                        OcrProgress::VideoPosition(last)
                    } else {
                        OcrProgress::FrameNumber(last)
                    })
                },
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
                    snippet(ocr_fts, 0, '[', ']', '…', 12),
                    coalesce(ocr_progress.by_position, 0)
             FROM ocr_fts JOIN ocr_frames ON ocr_frames.id = ocr_fts.rowid
             LEFT JOIN ocr_progress ON ocr_progress.day = ocr_frames.day
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
                    from_video: row.get(3)?,
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

    /// The recognized text of frame `frame_number` of `day` and the box of
    /// each of its lines, or `None` if OCR has no row for it.
    pub fn ocr_lines(&self, day: &str, frame_number: u32) -> Result<Option<(String, Vec<LineBox>)>> {
        self.conn
            .query_row(
                "SELECT text, boxes FROM ocr_frames WHERE day = ?1 AND frame_number = ?2",
                rusqlite::params![day, frame_number],
                |row| Ok((row.get(0)?, unpack_boxes(&row.get::<_, Vec<u8>>(1)?))),
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

/// One OCR line's box as `[x, y, width, height]`, normalized to the frame's
/// size, with the origin at the bottom-left corner as Vision reports it.
pub type LineBox = [f64; 4];

/// Each coordinate as a little-endian u16 over 0..=1, 8 bytes a line: a
/// 65535th of the 1800-pixel frame width is far below a pixel, and a frame of
/// 100 lines takes under 1 KB where the JSON it replaced took about 15.
pub fn pack_boxes(boxes: &[LineBox]) -> Vec<u8> {
    boxes
        .iter()
        .flatten()
        .flat_map(|v| ((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes())
        .collect()
}

pub fn unpack_boxes(bytes: &[u8]) -> Vec<LineBox> {
    bytes
        .chunks_exact(8)
        .map(|line| {
            let mut b = [0.0; 4];
            for (v, pair) in b.iter_mut().zip(line.chunks_exact(2)) {
                *v = f64::from(u16::from_le_bytes([pair[0], pair[1]])) / 65535.0;
            }
            b
        })
        .collect()
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
    /// The screenshot's `NNNNN`, or its 1-based position in the day's videos
    /// when `from_video` is set.
    pub frame_number: u32,
    pub snippet: String,
    /// The day was read from video because its screenshots were already gone.
    pub from_video: bool,
}

/// A day's OCR progress mark; see `record_ocr_frame` and
/// `record_video_ocr_frame`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrProgress {
    FrameNumber(u32),
    VideoPosition(u32),
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

    impl ScreenshotDatabase {
    /// The window that was in front for frame `frame_number` of `day`, if it
    /// was recorded.
    fn front_window(&self, day: &str, frame_number: u32) -> Result<Option<FrontWindow>> {
        self.conn
            .query_row(
                "SELECT w.app_name, w.bundle_id, w.app_path, w.title
                 FROM screenshots s JOIN windows w ON w.id = s.window_id
                 WHERE s.day = ?1 AND s.frame_number = ?2
                 ORDER BY s.id DESC LIMIT 1",
                rusqlite::params![day, frame_number],
                |row| {
                    Ok(FrontWindow {
                        app_name: row.get(0)?,
                        bundle_id: row.get(1)?,
                        app_path: row.get(2)?,
                        title: row.get(3)?,
                    })
                },
            )
            .optional()
    }
    }

    fn window(app_name: &str, title: &str) -> FrontWindow {
        FrontWindow {
            app_name: app_name.to_string(),
            bundle_id: Some(format!("org.example.{}", app_name.to_lowercase())),
            app_path: format!("/Applications/{app_name}.app"),
            title: title.to_string(),
        }
    }

    #[test]
    fn records_the_window_in_front_once_per_window() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();
        let blender = window("Blender", "scene.blend");
        let editor = window("Zed", "main.rs");

        db.insert_capture("2026-10-10", 1, Utc::now(), Local::now(), Some(&blender)).unwrap();
        db.insert_capture("2026-10-10", 2, Utc::now(), Local::now(), Some(&blender)).unwrap();
        db.insert_capture("2026-10-10", 3, Utc::now(), Local::now(), Some(&editor)).unwrap();
        db.insert_capture("2026-10-10", 4, Utc::now(), Local::now(), None).unwrap();

        assert_eq!(db.front_window("2026-10-10", 1).unwrap(), Some(blender.clone()));
        assert_eq!(db.front_window("2026-10-10", 2).unwrap(), Some(blender));
        assert_eq!(db.front_window("2026-10-10", 3).unwrap(), Some(editor));
        assert_eq!(db.front_window("2026-10-10", 4).unwrap(), None);
        let windows: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM windows", [], |row| row.get(0))
            .unwrap();
        assert_eq!(windows, 2);
    }

    #[test]
    fn a_window_without_a_bundle_id_is_recorded() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();
        let tool = FrontWindow { bundle_id: None, ..window("tool", "") };

        db.insert_capture("2026-10-10", 1, Utc::now(), Local::now(), Some(&tool)).unwrap();

        assert_eq!(db.front_window("2026-10-10", 1).unwrap(), Some(tool));
    }

    fn menu_app(db: &ScreenshotDatabase, day: &str, frame_number: u32) -> Option<String> {
        db.conn
            .query_row(
                "SELECT menu_app FROM ocr_frames WHERE day = ?1 AND frame_number = ?2",
                rusqlite::params![day, frame_number],
                |row| row.get(0),
            )
            .unwrap()
    }

    const MENU_BAR: &str = "Blender\nWindow";
    const MENU_BAR_BOXES: [LineBox; 2] =
        [[0.0523, 0.9766, 0.0349, 0.0143], [0.0974, 0.9767, 0.0334, 0.0139]];

    #[test]
    fn ocr_rows_record_the_app_the_menu_bar_names() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();

        db.record_ocr_frame("2026-10-10", 1, Some((MENU_BAR, &MENU_BAR_BOXES))).unwrap();
        db.record_video_ocr_frame("2026-10-09", 1, Some((MENU_BAR, &MENU_BAR_BOXES))).unwrap();
        db.record_ocr_frame("2026-10-10", 2, Some(("22:39", &[[0.38, 0.75, 0.22, 0.1]])))
            .unwrap();

        assert_eq!(menu_app(&db, "2026-10-10", 1).as_deref(), Some("Blender"));
        assert_eq!(menu_app(&db, "2026-10-09", 1).as_deref(), Some("Blender"));
        assert_eq!(menu_app(&db, "2026-10-10", 2), None);
    }

    #[test]
    fn the_migration_reads_the_app_from_existing_ocr_rows() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        {
            let db = ScreenshotDatabase::new(db_path.clone()).unwrap();
            db.record_ocr_frame("2026-10-10", 1, Some((MENU_BAR, &MENU_BAR_BOXES))).unwrap();
            // As the row was before the migration.
            db.conn
                .execute_batch(
                    "UPDATE ocr_frames SET menu_app = NULL;
                     DELETE FROM migrations WHERE migration_name = 'add_menu_app';
                     ALTER TABLE ocr_frames DROP COLUMN menu_app;",
                )
                .unwrap();
        }

        let db = ScreenshotDatabase::new(db_path).unwrap();

        assert_eq!(menu_app(&db, "2026-10-10", 1).as_deref(), Some("Blender"));
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

        db.record_ocr_frame("2024-01-01", 1, Some(("hello", &[]))).unwrap();
        db.record_ocr_frame("2024-01-01", 2, None).unwrap();
        assert_eq!(db.ocr_done_through("2024-01-01").unwrap(), Some(2));

        db.record_ocr_frame("2024-01-01", 1, Some(("hello again", &[]))).unwrap();
        assert_eq!(db.ocr_done_through("2024-01-01").unwrap(), Some(2));

        // Skipped frames get no row of their own.
        let rows: i32 = db
            .conn
            .query_row("SELECT COUNT(*) FROM ocr_frames", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);

        assert_eq!(db.ocr_done_through("2024-01-02").unwrap(), None);
    }

    #[test]
    fn test_video_positions_are_not_frame_numbers() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();

        db.record_ocr_frame("2024-01-01", 7, None).unwrap();
        db.record_video_ocr_frame("2024-01-02", 1, Some(("old video text", &[])))
            .unwrap();
        db.record_video_ocr_frame("2024-01-02", 40, None).unwrap();

        assert_eq!(
            db.ocr_progress("2024-01-01").unwrap(),
            Some(OcrProgress::FrameNumber(7))
        );
        assert_eq!(
            db.ocr_progress("2024-01-02").unwrap(),
            Some(OcrProgress::VideoPosition(40))
        );
        assert_eq!(db.ocr_progress("2024-01-03").unwrap(), None);
        // The converter's check never counts video positions.
        assert_eq!(db.ocr_done_through("2024-01-02").unwrap(), None);

        let hits = db.search_ocr("video", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].frame_number, hits[0].from_video), (1, true));
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

        db.record_ocr_frame("2024-01-01", 3, Some(("ffmpeg -i input.mov\nChápter 1", &[])))
            .unwrap();
        db.record_ocr_frame("2024-01-02", 9, Some(("Blender: modelling the chair", &[])))
            .unwrap();
        db.record_ocr_frame("2024-01-02", 12, Some(("chapter list for the chair video", &[])))
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

        db.record_ocr_frame("2024-01-01", 1, Some(("old words", &[]))).unwrap();
        db.record_ocr_frame("2024-01-01", 1, Some(("new words", &[]))).unwrap();

        assert_eq!(search(&db, "old"), Vec::<(String, u32)>::new());
        assert_eq!(search(&db, "new"), vec![hit("2024-01-01", 1)]);
    }

    #[test]
    fn test_search_ocr_in_day() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();

        db.record_ocr_frame("2024-01-01", 2, Some(("cargo build", &[]))).unwrap();
        db.record_ocr_frame("2024-01-01", 3, None).unwrap();
        db.record_ocr_frame("2024-01-01", 5, Some(("bun test", &[[0.0, 1.0, 1.0, 0.0]]))).unwrap();
        db.record_ocr_frame("2024-01-01", 8, Some(("cargo test", &[]))).unwrap();
        db.record_ocr_frame("2024-01-01", 9, None).unwrap();
        db.record_ocr_frame("2024-01-02", 1, Some(("cargo run", &[]))).unwrap();

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
        assert_eq!(
            db.ocr_lines("2024-01-01", 5).unwrap(),
            Some(("bun test".to_string(), vec![[0.0, 1.0, 1.0, 0.0]]))
        );
        assert_eq!(db.ocr_lines("2024-01-01", 3).unwrap(), None);
        assert!(db.search_ocr_in_day("2024-01-01", "  ").unwrap().is_empty());
        assert!(db.search_ocr_in_day("2024-01-03", "cargo").unwrap().is_empty());
    }

    #[test]
    fn test_count_ocr_matches() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("test.db")).unwrap();

        // Two runs on the 1st: frames 2-5, then 9.
        db.record_ocr_frame("2024-01-01", 2, Some(("cargo build", &[]))).unwrap();
        db.record_ocr_frame("2024-01-01", 5, Some(("cargo test", &[]))).unwrap();
        db.record_ocr_frame("2024-01-01", 7, Some(("bun test", &[]))).unwrap();
        db.record_ocr_frame("2024-01-01", 9, Some(("cargo run", &[]))).unwrap();
        db.record_ocr_frame("2024-01-02", 1, Some(("cargo run", &[]))).unwrap();
        db.record_ocr_frame("2024-01-03", 1, Some(("bun run", &[]))).unwrap();

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
        db.record_ocr_frame("2024-01-01", 2, Some(("cargo", &[]))).unwrap();
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
    fn test_boxes_pack_to_a_65535th() {
        let boxes = [[0.0, 1.0, 0.125, 0.3333], [-0.5, 2.0, 0.5, 0.999_99]];
        let packed = pack_boxes(&boxes);
        assert_eq!(packed.len(), 16);
        let unpacked = unpack_boxes(&packed);
        let clamped = [[0.0, 1.0, 0.125, 0.3333], [0.0, 1.0, 0.5, 0.999_99]];
        for (got, want) in unpacked.iter().flatten().zip(clamped.iter().flatten()) {
            assert!((got - want).abs() <= 0.5 / 65535.0, "{got} vs {want}");
        }
        assert!(unpack_boxes(&[]).is_empty());
    }

    #[test]
    fn test_reread_after_vision_failure_migration() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        {
            let db = ScreenshotDatabase::new(db_path.clone()).unwrap();
            db.record_ocr_frame("2026-10-07", 900, None).unwrap();
            db.record_ocr_frame("2026-10-08", 7528, Some(("kept", &[[0.0, 0.0, 1.0, 1.0]]))).unwrap();
            db.record_video_ocr_frame("2026-02-07", 2476, None).unwrap();
            db.conn
                .execute("DELETE FROM migrations WHERE migration_name = 'reread_after_vision_failure'", [])
                .unwrap();
        }

        let db = ScreenshotDatabase::new(db_path.clone()).unwrap();

        assert_eq!(db.ocr_progress("2026-10-07").unwrap(), Some(OcrProgress::FrameNumber(900)));
        assert_eq!(db.ocr_progress("2026-10-08").unwrap(), Some(OcrProgress::FrameNumber(0)));
        assert_eq!(db.ocr_progress("2026-02-07").unwrap(), None);
        assert!(db.ocr_lines("2026-10-08", 7528).unwrap().is_some());
    }

    #[test]
    fn test_compact_ocr_frames_migration() {
        // A library from before the boxes were packed: lines_json repeats
        // every line, and OCR wrote a row each time the pixels changed.
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        {
            let mut conn = Connection::open(&db_path).unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute_batch(
                "CREATE TABLE migrations (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     migration_name TEXT UNIQUE NOT NULL,
                     applied_at TEXT NOT NULL
                 );
                 INSERT INTO migrations (migration_name, applied_at) VALUES
                     ('split_timestamps', '2024-01-01T00:00:00Z'),
                     ('add_day_and_ocr', '2024-01-01T00:00:00Z');
                 CREATE TABLE screenshots (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     frame_number INTEGER NOT NULL,
                     created_at TEXT NOT NULL,
                     local_time TEXT NOT NULL
                 );",
            )
            .unwrap();
            ScreenshotDatabase::add_day_and_ocr(&tx).unwrap();
            let line = |text: &str, y: f64| {
                format!(
                    r#"{{"text":"{text}","confidence":0.5,"x":0.25,"y":{y},"width":0.5,"height":0.125}}"#
                )
            };
            let rows = [
                ("2024-01-01", 1, "cargo build\nok", format!("[{},{}]", line("cargo build", 0.5), line("ok", 0.0))),
                ("2024-01-01", 4, "cargo build\nok", "[]".to_string()),
                ("2024-01-01", 7, "cargo test", format!("[{}]", line("cargo test", 0.5))),
                ("2024-01-01", 9, "cargo build\nok", "[]".to_string()),
                ("2024-01-02", 1, "cargo test", "not json".to_string()),
            ];
            for (day, frame, text, json) in rows {
                tx.execute(
                    "INSERT INTO ocr_frames (day, frame_number, text, lines_json, processed_at)
                     VALUES (?1, ?2, ?3, ?4, '2024-01-01T00:00:00Z')",
                    rusqlite::params![day, frame, text, json],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }

        let db = ScreenshotDatabase::new(db_path).unwrap();

        // Frame 4 repeated frame 1; frame 9 came back after something else,
        // and the next day starts afresh.
        let frames: Vec<(String, u32)> = db
            .conn
            .prepare("SELECT day, frame_number FROM ocr_frames ORDER BY day, frame_number")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_>>()
            .unwrap();
        let frames: Vec<(&str, u32)> = frames.iter().map(|(d, f)| (d.as_str(), *f)).collect();
        assert_eq!(
            frames,
            vec![("2024-01-01", 1), ("2024-01-01", 7), ("2024-01-01", 9), ("2024-01-02", 1)]
        );

        assert_eq!(
            db.ocr_lines("2024-01-01", 1).unwrap(),
            Some((
                "cargo build\nok".to_string(),
                unpack_boxes(&pack_boxes(&[[0.25, 0.5, 0.5, 0.125], [0.25, 0.0, 0.5, 0.125]]))
            ))
        );
        assert_eq!(db.ocr_lines("2024-01-02", 1).unwrap(), Some(("cargo test".to_string(), vec![])));

        let has_json: i32 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('ocr_frames') WHERE name = 'lines_json'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_json, 0);

        // The index lost the deleted row and still finds the others.
        let found: Vec<u32> = db
            .search_ocr_in_day("2024-01-01", "build")
            .unwrap()
            .into_iter()
            .map(|m| m.frame_number)
            .collect();
        assert_eq!(found, vec![1, 9]);
        let in_index: i32 = db
            .conn
            .query_row("SELECT COUNT(*) FROM ocr_fts WHERE ocr_fts MATCH 'cargo'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(in_index, 4);
        db.conn
            .execute_batch("INSERT INTO ocr_fts (ocr_fts, rank) VALUES ('integrity-check', 1)")
            .unwrap();
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
