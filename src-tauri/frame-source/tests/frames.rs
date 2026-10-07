//! End-to-end tests against real files and the real ffmpeg. Each generated
//! video frame is a flat gray whose level encodes its frame number, so a test
//! can check it got the right frame and not just *a* frame.

use std::fs;
use std::path::Path;
use std::process::Command;

use frame_source::{FrameSource, Source, Tools, CHUNK_FRAMES};
use tempfile::TempDir;

const LEVEL_STEP: usize = 4;

fn make_video(path: &Path, frames: usize, level_offset: usize) {
    let filter = format!(
        "color=c=black:s=32x32:r=15,format=gray,geq=lum='mod(N*{LEVEL_STEP}+{level_offset},256)'"
    );
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", &filter])
        .args(["-frames:v", &frames.to_string()])
        // Keyframes every 2 s, like a real encode, so seeking has to decode
        // forward from a keyframe to land on the right frame.
        .args([
            "-c:v", "libx264", "-g", "30", "-pix_fmt", "yuv420p", "-qp", "0",
        ])
        .arg(path)
        .status()
        .expect("ffmpeg must be installed to run these tests");
    assert!(status.success());
}

/// Mean gray level of an image, via ffmpeg so the tests need no image crate.
fn gray_level(bytes: &[u8], dir: &Path) -> usize {
    let file = dir.join("probe.jpg");
    fs::write(&file, bytes).unwrap();
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&file)
        .args([
            "-vf",
            "scale=1:1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "gray",
            "-",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    usize::from(output.stdout[0])
}

fn assert_frame(
    source: &FrameSource,
    date: &str,
    index: usize,
    expected_level: usize,
    scratch: &Path,
) {
    let frame = source.frame(date, index).unwrap();
    assert_eq!(frame.mime, "image/jpeg");
    let level = gray_level(&frame.bytes, scratch);
    assert!(
        level.abs_diff(expected_level) <= 2,
        "frame {index} of {date}: expected gray {expected_level}, got {level}"
    );
}

struct Library {
    root: TempDir,
    cache: TempDir,
    scratch: TempDir,
}

impl Library {
    fn new() -> Self {
        Self {
            root: TempDir::new().unwrap(),
            cache: TempDir::new().unwrap(),
            scratch: TempDir::new().unwrap(),
        }
    }

    fn source(&self, cap: u64) -> FrameSource {
        // Whatever ffmpeg is on PATH; the app passes its bundled one.
        FrameSource::new(
            self.root.path().to_path_buf(),
            self.cache.path().to_path_buf(),
            cap,
            Tools::new("ffmpeg"),
        )
        .unwrap()
    }
}

#[test]
fn serves_exact_frames_across_chunks_and_videos() {
    let lib = Library::new();
    // Two sessions on one day: 200 frames, then 40 more starting at gray 100.
    make_video(&lib.root.path().join("2024-12-20--09-00-00.mov"), 200, 0);
    make_video(&lib.root.path().join("2024-12-20--17-05-22.mov"), 40, 100);
    let source = lib.source(u64::MAX);

    let day = source.day("2024-12-20").unwrap();
    assert_eq!(day.frame_count, 240);
    assert_eq!(day.source, Source::Video);

    let scratch = lib.scratch.path();
    for index in [
        0,
        1,
        37,
        CHUNK_FRAMES - 1,
        CHUNK_FRAMES,
        CHUNK_FRAMES + 1,
        199,
    ] {
        assert_frame(
            &source,
            "2024-12-20",
            index,
            index * LEVEL_STEP % 256,
            scratch,
        );
    }
    // Frame 200 of the day is frame 0 of the second video.
    assert_frame(&source, "2024-12-20", 200, 100, scratch);
    assert_frame(
        &source,
        "2024-12-20",
        239,
        (100 + 39 * LEVEL_STEP) % 256,
        scratch,
    );

    assert!(source.frame("2024-12-20", 240).is_err());
}

/// A screenshots.db holding `times` (local ISO times) as frames 1, 2, ….
fn write_db(root: &Path, times: &[&str]) {
    let conn = rusqlite::Connection::open(root.join("screenshots.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE screenshots (id INTEGER PRIMARY KEY, frame_number INTEGER, local_time TEXT)",
    )
    .unwrap();
    for (n, time) in times.iter().enumerate() {
        conn.execute(
            "INSERT INTO screenshots (frame_number, local_time) VALUES (?1, ?2)",
            rusqlite::params![n + 1, time],
        )
        .unwrap();
    }
}

#[test]
fn times_legacy_videos_from_the_database_not_the_file_name() {
    let lib = Library::new();
    // Named after when the old script ran, not when the frames were taken.
    make_video(&lib.root.path().join("2025-12-01--09-00-00.mov"), 2, 0);
    make_video(&lib.root.path().join("2025-12-01--09-30-00.mov"), 1, 0);
    make_video(&lib.root.path().join("2025-12-02--09-00-00.mov"), 2, 0);
    write_db(
        lib.root.path(),
        &[
            "2025-12-01T19:54:11+00:00",
            "2025-12-01T19:54:12+00:00",
            "2025-12-01T20:01:00+00:00",
            // 2025-12-02 has one row for two frames: no way to line them up.
            "2025-12-02T10:00:00+00:00",
        ],
    );
    let source = lib.source(u64::MAX);

    let time = source.frame_time("2025-12-01", 2).unwrap().unwrap();
    assert_eq!(
        (time.local_time.as_str(), time.exact),
        ("2025-12-01T20:01:00+00:00", true)
    );
    assert_eq!(source.frame_time("2025-12-02", 0).unwrap(), None);
    // Before the database existed there is nothing to go on.
    make_video(&lib.root.path().join("2024-12-20--09-00-00.mov"), 2, 0);
    assert_eq!(source.frame_time("2024-12-20", 0).unwrap(), None);
}

#[test]
fn skips_broken_and_duplicate_videos() {
    let lib = Library::new();
    make_video(&lib.root.path().join("2024-12-29--11-17-04.mov"), 20, 0);
    fs::copy(
        lib.root.path().join("2024-12-29--11-17-04.mov"),
        lib.root.path().join("2024-12-29--14-17-25.mov"),
    )
    .unwrap();
    fs::write(
        lib.root.path().join("2024-12-29--16-44-03.mov"),
        b"36 bytes of not a video....",
    )
    .unwrap();
    let source = lib.source(u64::MAX);

    assert_eq!(source.day("2024-12-29").unwrap().frame_count, 20);
}

#[test]
fn prefers_screenshots_and_tracks_new_ones() {
    let lib = Library::new();
    let day_dir = lib.root.path().join("2026-10-04");
    fs::create_dir(&day_dir).unwrap();
    fs::write(day_dir.join("00001.png"), b"one").unwrap();
    fs::write(day_dir.join("00002.png"), b"two").unwrap();
    // A converted copy of the same day must not be counted twice.
    make_video(&day_dir.join("2026-10-04--09-00-00.mov"), 2, 0);
    let source = lib.source(u64::MAX);

    let day = source.day("2026-10-04").unwrap();
    assert_eq!((day.frame_count, day.source), (2, Source::Screenshots));
    let frame = source.frame("2026-10-04", 1).unwrap();
    assert_eq!(
        (frame.bytes.as_slice(), frame.mime),
        (&b"two"[..], "image/png")
    );

    // The capture loop keeps writing; the next question sees the new frame.
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(day_dir.join("00003.png"), b"three").unwrap();
    assert_eq!(source.day("2026-10-04").unwrap().frame_count, 3);
    assert_eq!(source.frame("2026-10-04", 2).unwrap().bytes, b"three");

    assert!(source.frame_time("2026-10-04", 0).unwrap().unwrap().exact);
}

/// Write a screenshot whose mtime (its capture time) is `time` local.
fn write_shot(dir: &Path, name: &str, body: &[u8], time: &str) {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    let local = chrono::NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M:%S")
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(local.into())
        .unwrap();
}

#[test]
fn stitches_converted_hours_and_remaining_screenshots_in_time_order() {
    let lib = Library::new();
    let root = lib.root.path();
    let day_dir = root.join("2026-10-04");
    fs::create_dir(&day_dir).unwrap();
    // 09:00 was converted in two parts and its screenshots deleted.
    make_video(&root.join("2026-10-04--09-40-00--hourly-2.mov"), 2, 100);
    make_video(&root.join("2026-10-04--09-00-05--hourly.mov"), 3, 0);
    // 10:00 was converted but its screenshots are still here: they win.
    make_video(&root.join("2026-10-04--10-00-00--hourly.mov"), 2, 50);
    write_shot(&day_dir, "07201.png", b"ten", "2026-10-04 10:00:00");
    write_shot(&day_dir, "07202.png", b"ten-oh-one", "2026-10-04 10:00:01");
    // 11:00 is converted and gone again.
    make_video(&root.join("2026-10-04--11-00-00--hourly.mov"), 1, 200);
    let source = lib.source(u64::MAX);

    let day = source.day("2026-10-04").unwrap();
    assert_eq!((day.frame_count, day.source), (8, Source::Mixed));
    let scratch = lib.scratch.path();
    assert_frame(&source, "2026-10-04", 0, 0, scratch);
    assert_frame(&source, "2026-10-04", 2, 2 * LEVEL_STEP, scratch);
    assert_frame(&source, "2026-10-04", 3, 100, scratch);
    assert_eq!(source.frame("2026-10-04", 6).unwrap().bytes, b"ten-oh-one");
    assert_frame(&source, "2026-10-04", 7, 200, scratch);
    assert!(source.frame("2026-10-04", 8).is_err());

    let time = |index| source.frame_time("2026-10-04", index).unwrap().unwrap();
    assert_eq!(
        (time(1).local_time.as_str(), time(1).exact),
        ("2026-10-04T09:00:06", false)
    );
    assert_eq!(
        (time(6).local_time.as_str(), time(6).exact),
        ("2026-10-04T10:00:01", true)
    );
}

/// Record `times` as the converter's `video_frames` rows for `video`.
fn record_video(root: &Path, video: &str, times: &[&str]) {
    let conn = rusqlite::Connection::open(root.join("screenshots.db")).unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS video_frames (video TEXT, frame_index INTEGER, day TEXT,
                                                  frame_number INTEGER, local_time TEXT)",
        [],
    )
    .unwrap();
    for (index, time) in times.iter().enumerate() {
        conn.execute(
            "INSERT INTO video_frames VALUES (?1, ?2, substr(?1, 1, 10), ?2 + 1, ?3)",
            rusqlite::params![video, index, time],
        )
        .unwrap();
    }
}

#[test]
fn times_hourly_videos_from_the_frames_the_converter_recorded() {
    let lib = Library::new();
    let root = lib.root.path();
    make_video(&root.join("2026-10-04--09-00-05--hourly.mov"), 3, 0);
    // The screen was locked from 09:00:07 to 09:40, so one second per frame
    // would put the last frame at 09:00:07.
    write_db(root, &[]);
    record_video(
        root,
        "2026-10-04--09-00-05--hourly.mov",
        &[
            "2026-10-04T09:00:05.3+02:00",
            "2026-10-04T09:00:06.7+02:00",
            "2026-10-04T09:40:00.1+02:00",
        ],
    );
    let source = lib.source(u64::MAX);

    let time = |index| source.frame_time("2026-10-04", index).unwrap().unwrap();
    assert_eq!(
        (time(0).local_time.as_str(), time(0).exact),
        ("2026-10-04T09:00:05.3+02:00", true)
    );
    assert_eq!(
        (time(2).local_time.as_str(), time(2).exact),
        ("2026-10-04T09:40:00.1+02:00", true)
    );
}

#[test]
fn ignores_a_record_that_does_not_match_the_video() {
    let lib = Library::new();
    let root = lib.root.path();
    make_video(&root.join("2026-10-04--09-00-05--hourly.mov"), 3, 0);
    write_db(root, &[]);
    record_video(
        root,
        "2026-10-04--09-00-05--hourly.mov",
        &["2026-10-04T09:00:05.3+02:00", "2026-10-04T09:40:00.1+02:00"],
    );
    let source = lib.source(u64::MAX);

    let time = source.frame_time("2026-10-04", 1).unwrap().unwrap();
    assert_eq!((time.local_time.as_str(), time.exact), ("2026-10-04T09:00:06", false));
}

#[test]
fn lines_up_unrecorded_hourly_videos_with_the_database() {
    let lib = Library::new();
    let root = lib.root.path();
    // Converted before frames were recorded. Named after its first frame's
    // file time; each row is written a moment after its file.
    make_video(&root.join("2026-10-04--09-00-05--hourly.mov"), 3, 0);
    write_db(
        root,
        &[
            "2026-10-04T08:59:59.9+02:00", // the hour before
            "2026-10-04T09:00:03.6+02:00", // deleted before the conversion
            "2026-10-04T09:00:05.9+02:00",
            "2026-10-04T09:00:07.3+02:00",
            "2026-10-04T09:40:00.1+02:00", // after the screen was locked
            "2026-10-04T10:00:00.5+02:00", // the next hour
        ],
    );
    let source = lib.source(u64::MAX);

    let time = |index| {
        let time = source.frame_time("2026-10-04", index).unwrap().unwrap();
        assert!(!time.exact);
        time.local_time
    };
    assert_eq!(
        [time(0), time(1), time(2)],
        [
            "2026-10-04T09:00:05.9+02:00",
            "2026-10-04T09:00:07.3+02:00",
            "2026-10-04T09:40:00.1+02:00"
        ]
    );
}

#[test]
fn serves_a_fully_converted_day_from_its_hourly_videos() {
    let lib = Library::new();
    make_video(
        &lib.root.path().join("2026-10-01--09-10-00--hourly.mov"),
        2,
        0,
    );
    // An old whole-day render of the same day is ignored once hourly videos exist.
    make_video(&lib.root.path().join("2026-10-01--23-00-00.mov"), 5, 0);
    let source = lib.source(u64::MAX);

    assert_eq!(source.days().unwrap(), vec!["2026-10-01"]);
    let day = source.day("2026-10-01").unwrap();
    assert_eq!((day.frame_count, day.source), (2, Source::Video));
}

#[test]
fn lists_days_and_rejects_paths_that_are_not_days() {
    let lib = Library::new();
    fs::create_dir(lib.root.path().join("2026-10-04")).unwrap();
    fs::create_dir(lib.root.path().join(".cache")).unwrap();
    make_video(&lib.root.path().join("2024-12-20--09-00-00.mov"), 2, 0);
    let source = lib.source(u64::MAX);

    assert_eq!(source.days().unwrap(), vec!["2024-12-20", "2026-10-04"]);
    assert!(source.day("../etc").is_err());
    assert!(source.frame(".cache", 0).is_err());
}

#[test]
fn stays_under_its_cache_cap() {
    let lib = Library::new();
    make_video(
        &lib.root.path().join("2024-12-20--09-00-00.mov"),
        CHUNK_FRAMES * 3,
        0,
    );
    let source = lib.source(1);

    // Each chunk alone is over the 1-byte cap, so only the latest survives,
    // and going back to an evicted chunk decodes it again.
    for index in [0, CHUNK_FRAMES, 2 * CHUNK_FRAMES, 5] {
        assert_frame(
            &source,
            "2024-12-20",
            index,
            index * LEVEL_STEP % 256,
            lib.scratch.path(),
        );
    }
    let chunk_dirs = fs::read_dir(lib.cache.path())
        .unwrap()
        .flat_map(|video| fs::read_dir(video.unwrap().path()).unwrap())
        .count();
    assert_eq!(chunk_dirs, 1);
}
