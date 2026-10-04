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
        .args(["-c:v", "libx264", "-g", "30", "-pix_fmt", "yuv420p", "-qp", "0"])
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
        .args(["-vf", "scale=1:1", "-f", "rawvideo", "-pix_fmt", "gray", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    usize::from(output.stdout[0])
}

fn assert_frame(source: &FrameSource, date: &str, index: usize, expected_level: usize, scratch: &Path) {
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
        FrameSource::with_tools(
            self.root.path().to_path_buf(),
            self.cache.path().to_path_buf(),
            cap,
            Tools::locate(),
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
    for index in [0, 1, 37, CHUNK_FRAMES - 1, CHUNK_FRAMES, CHUNK_FRAMES + 1, 199] {
        assert_frame(&source, "2024-12-20", index, index * LEVEL_STEP % 256, scratch);
    }
    // Frame 200 of the day is frame 0 of the second video.
    assert_frame(&source, "2024-12-20", 200, 100, scratch);
    assert_frame(&source, "2024-12-20", 239, (100 + 39 * LEVEL_STEP) % 256, scratch);

    assert!(source.frame("2024-12-20", 240).is_err());
}

#[test]
fn estimates_video_times_from_the_file_name() {
    let lib = Library::new();
    make_video(&lib.root.path().join("2024-12-20--09-00-00.mov"), 10, 0);
    let source = lib.source(u64::MAX);

    let time = source.frame_time("2024-12-20", 5).unwrap().unwrap();
    assert_eq!(time.local_time, "2024-12-20T09:00:05");
    assert!(!time.exact);
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
    fs::write(lib.root.path().join("2024-12-29--16-44-03.mov"), b"36 bytes of not a video....").unwrap();
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
    assert_eq!((frame.bytes.as_slice(), frame.mime), (&b"two"[..], "image/png"));

    // The capture loop keeps writing; the next question sees the new frame.
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(day_dir.join("00003.png"), b"three").unwrap();
    assert_eq!(source.day("2026-10-04").unwrap().frame_count, 3);
    assert_eq!(source.frame("2026-10-04", 2).unwrap().bytes, b"three");

    assert!(source.frame_time("2026-10-04", 0).unwrap().unwrap().exact);
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
    make_video(&lib.root.path().join("2024-12-20--09-00-00.mov"), CHUNK_FRAMES * 3, 0);
    let source = lib.source(1);

    // Each chunk alone is over the 1-byte cap, so only the latest survives,
    // and going back to an evicted chunk decodes it again.
    for index in [0, CHUNK_FRAMES, 2 * CHUNK_FRAMES, 5] {
        assert_frame(&source, "2024-12-20", index, index * LEVEL_STEP % 256, lib.scratch.path());
    }
    let chunk_dirs = fs::read_dir(lib.cache.path())
        .unwrap()
        .flat_map(|video| fs::read_dir(video.unwrap().path()).unwrap())
        .count();
    assert_eq!(chunk_dirs, 1);
}
