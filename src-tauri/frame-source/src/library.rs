//! Finding what a day is made of: which screenshots and videos exist for it.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, Timelike};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoKind {
    /// `YYYY-MM-DD--HH-MM-SS.mov` from the old `all-timelapses-to-video`
    /// script: a whole day (or what was left of it), named after when the
    /// script ran. Its screenshots are gone.
    Legacy,
    /// `YYYY-MM-DD--HH-MM-SS--hourly[-N].mov` from the app's converter: one
    /// clock hour of screenshots (part N of it, if the hour was split), named
    /// after its first frame. The screenshots may or may not still exist.
    Hourly { part: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFile {
    pub path: PathBuf,
    pub start: NaiveDateTime,
    pub kind: VideoKind,
}

impl VideoFile {
    /// The clock hour an hourly video covers: its day plus the hour of its
    /// first frame, which is how `converter.rs` buckets screenshots.
    pub fn hour(&self) -> NaiveDateTime {
        truncate_to_hour(self.start)
    }
}

/// A screenshot in a day folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    pub name: String,
    pub number: u32,
    /// Local modification time, which is when it was captured.
    pub modified: NaiveDateTime,
}

impl Shot {
    /// The clock hour this screenshot belongs to, the same way `converter.rs`
    /// decides which hourly video it goes into.
    pub fn hour(&self) -> NaiveDateTime {
        truncate_to_hour(self.modified)
    }
}

fn truncate_to_hour(time: NaiveDateTime) -> NaiveDateTime {
    time.date().and_hms_opt(time.hour(), 0, 0).unwrap_or(time)
}

/// `2024-12-20` for a day folder name, `None` for anything else (including
/// dotfiles such as `.cache`).
pub fn parse_day(name: &str) -> Option<NaiveDate> {
    if name.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(name, "%Y-%m-%d").ok()
}

/// The start time and kind encoded in a video file name:
/// `YYYY-MM-DD--HH-MM-SS.<mov|mp4>` (legacy) or
/// `YYYY-MM-DD--HH-MM-SS--hourly[-N].mov` (converter).
pub fn parse_video_name(name: &str) -> Option<(NaiveDateTime, VideoKind)> {
    let (stem, ext) = name.rsplit_once('.')?;
    if !matches!(ext.to_ascii_lowercase().as_str(), "mov" | "mp4") || !stem.is_char_boundary(20) {
        return None;
    }
    let (time, tag) = stem.split_at(20);
    let start = NaiveDateTime::parse_from_str(time, "%Y-%m-%d--%H-%M-%S").ok()?;
    let kind = match tag {
        "" => VideoKind::Legacy,
        "--hourly" => VideoKind::Hourly { part: 1 },
        _ => {
            let part: u32 = tag.strip_prefix("--hourly-")?.parse().ok()?;
            if part < 2 {
                return None;
            }
            VideoKind::Hourly { part }
        }
    };
    Some((start, kind))
}

/// The frame number of a screenshot named `NNNNN.png`.
pub fn parse_screenshot_name(name: &str) -> Option<u32> {
    let stem = name.strip_suffix(".png")?;
    if stem.is_empty() || !stem.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    stem.parse().ok()
}

/// Every day that has a folder or a video, oldest first.
pub fn list_days(root: &Path) -> std::io::Result<Vec<NaiveDate>> {
    let mut days = BTreeSet::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if let Some(day) = parse_day(name) {
                days.insert(day);
            }
        } else if let Some((start, _)) = parse_video_name(name) {
            days.insert(start.date());
        }
    }
    Ok(days.into_iter().collect())
}

/// Screenshots in a day folder, in frame-number order. A missing folder is an
/// empty day, not an error.
pub fn list_screenshots(day_dir: &Path) -> std::io::Result<Vec<Shot>> {
    let entries = match fs::read_dir(day_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut shots = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(number) = parse_screenshot_name(&name) else {
            continue;
        };
        // The converter may delete it between the listing and the stat.
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        let modified = DateTime::<Local>::from(modified).naive_local();
        shots.push(Shot {
            name,
            number,
            modified,
        });
    }
    shots.sort_by_key(|shot| shot.number);
    Ok(shots)
}

/// Videos for `day`, from the library root and from the day folder, in start
/// order. Duplicates are not removed here; see `fingerprint`.
pub fn list_videos(root: &Path, day: NaiveDate) -> std::io::Result<Vec<VideoFile>> {
    let mut videos = Vec::new();
    let day_dir = root.join(day.format("%Y-%m-%d").to_string());
    for dir in [root, day_dir.as_path()] {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Some((start, kind)) = parse_video_name(&name) else {
                continue;
            };
            if start.date() == day && entry.file_type()?.is_file() {
                videos.push(VideoFile {
                    path: entry.path(),
                    start,
                    kind,
                });
            }
        }
    }
    videos.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.path.cmp(&b.path)));
    Ok(videos)
}

/// Size plus a hash of the first MiB: enough to spot the byte-identical
/// copies in the legacy library (the same render saved under two start
/// times) without reading whole files.
pub fn fingerprint(path: &Path) -> std::io::Result<(u64, u64)> {
    let len = fs::metadata(path)?.len();
    let mut head = Vec::with_capacity(1 << 20);
    fs::File::open(path)?.take(1 << 20).read_to_end(&mut head)?;
    // FNV-1a; this only has to separate files of equal size on one disk.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in head {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Ok((len, hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parses_names() {
        assert_eq!(
            parse_day("2024-12-20"),
            NaiveDate::from_ymd_opt(2024, 12, 20)
        );
        assert_eq!(parse_day(".cache"), None);
        assert_eq!(parse_day("2024-12-20--12-48-38.mov"), None);

        let at = |h, m, s| {
            NaiveDate::from_ymd_opt(2024, 12, 20)
                .unwrap()
                .and_hms_opt(h, m, s)
                .unwrap()
        };
        assert_eq!(
            parse_video_name("2024-12-20--12-48-38.mov"),
            Some((at(12, 48, 38), VideoKind::Legacy))
        );
        assert!(parse_video_name("2025-10-20--09-03-51.mp4").is_some());
        assert_eq!(
            parse_video_name("2024-12-20--09-10-00--hourly.mov"),
            Some((at(9, 10, 0), VideoKind::Hourly { part: 1 }))
        );
        assert_eq!(
            parse_video_name("2024-12-20--09-40-00--hourly-2.mov"),
            Some((at(9, 40, 0), VideoKind::Hourly { part: 2 }))
        );
        assert_eq!(parse_video_name("2024-12-20--09-40-00--hourly-1.mov"), None);
        assert_eq!(parse_video_name("2024-12-20--09-40-00--daily.mov"), None);
        assert_eq!(parse_video_name("2024-12-20--12-48-38.mov.partial"), None);
        assert_eq!(parse_video_name("holiday.mov"), None);
        assert_eq!(parse_video_name("é.mov"), None);

        assert_eq!(parse_screenshot_name("00042.png"), Some(42));
        assert_eq!(parse_screenshot_name("thumb.png"), None);
        assert_eq!(parse_screenshot_name(".png"), None);
    }

    #[test]
    fn lists_days_from_folders_and_videos() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("2026-10-04")).unwrap();
        fs::create_dir(dir.path().join(".cache")).unwrap();
        fs::write(dir.path().join("2024-12-20--12-48-38.mov"), b"a").unwrap();
        fs::write(dir.path().join("2024-12-20--17-05-22.mov"), b"b").unwrap();
        // A day whose screenshots were all converted and deleted.
        fs::write(dir.path().join("2026-10-01--09-10-00--hourly.mov"), b"c").unwrap();
        fs::write(dir.path().join("screenshots.db"), b"").unwrap();

        let days = list_days(dir.path()).unwrap();
        assert_eq!(
            days,
            vec![
                NaiveDate::from_ymd_opt(2024, 12, 20).unwrap(),
                NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
            ]
        );
    }

    #[test]
    fn sorts_screenshots_numerically_and_ignores_other_files() {
        let dir = TempDir::new().unwrap();
        for name in [
            "00010.png",
            "00002.png",
            "100000.png",
            ".DS_Store",
            "notes.txt",
        ] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        let names: Vec<_> = list_screenshots(dir.path())
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["00002.png", "00010.png", "100000.png"]);
        assert!(list_screenshots(&dir.path().join("missing"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn orders_videos_by_start_from_root_and_day_folder() {
        let dir = TempDir::new().unwrap();
        let day = NaiveDate::from_ymd_opt(2024, 12, 29).unwrap();
        fs::write(dir.path().join("2024-12-29--14-17-25.mov"), b"b").unwrap();
        fs::write(dir.path().join("2024-12-29--11-17-04.mov"), b"a").unwrap();
        fs::write(dir.path().join("2024-12-28--23-59-59.mov"), b"other day").unwrap();
        fs::create_dir(dir.path().join("2024-12-29")).unwrap();
        fs::write(dir.path().join("2024-12-29/2024-12-29--16-00-00.mov"), b"c").unwrap();

        let videos = list_videos(dir.path(), day).unwrap();
        let names: Vec<_> = videos
            .iter()
            .map(|v| v.path.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            names,
            vec![
                "2024-12-29--11-17-04.mov",
                "2024-12-29--14-17-25.mov",
                "2024-12-29--16-00-00.mov"
            ]
        );
    }

    #[test]
    fn fingerprints_match_only_for_identical_files() {
        let dir = TempDir::new().unwrap();
        for (name, body) in [("a", "same"), ("b", "same"), ("c", "diff")] {
            fs::write(dir.path().join(name), body).unwrap();
        }
        let print = |name: &str| fingerprint(&dir.path().join(name)).unwrap();
        assert_eq!(print("a"), print("b"));
        assert_ne!(print("a"), print("c"));
    }
}
