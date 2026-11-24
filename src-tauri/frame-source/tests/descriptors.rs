//! Scrubbing must not leave file descriptors behind. Its own test binary,
//! so no other test opens files in this process while it counts them.

use std::fs;
use std::sync::{Arc, Barrier};

use frame_source::{FrameSource, Tools};
use tempfile::TempDir;

/// How many file descriptors this process has open.
fn open_descriptors() -> usize {
    fs::read_dir("/dev/fd").unwrap().count()
}

#[test]
fn frame_times_asked_for_at_once_leave_no_descriptors_behind() {
    let root = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let day = root.path().join("2026-10-04");
    fs::create_dir(&day).unwrap();
    for number in 1..=50 {
        fs::write(day.join(format!("{number:05}.png")), b"png").unwrap();
    }
    // Like the capture loop's connection, which stays open while the app
    // runs. While it does, SQLite keeps the descriptor of every other
    // connection that closes, so each one opened at once is one kept.
    let writer = rusqlite::Connection::open(root.path().join("screenshots.db")).unwrap();
    writer
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE screenshots (id INTEGER PRIMARY KEY, day TEXT, frame_number INTEGER,
                                       local_time TEXT);
             CREATE INDEX screenshots_day_frame ON screenshots (day, frame_number);",
        )
        .unwrap();
    for number in 1..=50 {
        writer
            .execute(
                "INSERT INTO screenshots (day, frame_number, local_time) VALUES ('2026-10-04', ?1, ?2)",
                rusqlite::params![number, format!("2026-10-04T09:00:{:02}", number % 60)],
            )
            .unwrap();
    }
    let source = Arc::new(
        FrameSource::new(
            root.path().to_path_buf(),
            cache.path().to_path_buf(),
            u64::MAX,
            Tools::new("ffmpeg"),
        )
        .unwrap(),
    );
    assert_eq!(source.day("2026-10-04").unwrap().frame_count, 50);
    let before = open_descriptors();

    // A fast scrub: a frame time asked for at every step, many in flight.
    let callers = 64;
    let start = Arc::new(Barrier::new(callers));
    let threads: Vec<_> = (0..callers)
        .map(|caller| {
            let (source, start) = (Arc::clone(&source), Arc::clone(&start));
            std::thread::spawn(move || {
                start.wait();
                let time = source.frame_time("2026-10-04", caller % 50).unwrap().unwrap();
                assert!(time.exact);
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }

    let kept = open_descriptors().saturating_sub(before);
    assert!(kept <= 4, "{kept} descriptors are still open after the scrub");
    drop(writer);
}
