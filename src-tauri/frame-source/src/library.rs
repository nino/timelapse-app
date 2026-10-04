//! Finding what a day is made of: which screenshots and videos exist for it.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime};

/// A video named `YYYY-MM-DD--HH-MM-SS.<mov|mp4>`, where the time is that of
/// its first frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFile {
    pub path: PathBuf,
    pub start: NaiveDateTime,
}

/// `2024-12-20` for a day folder name, `None` for anything else (including
/// dotfiles such as `.cache`).
pub fn parse_day(name: &str) -> Option<NaiveDate> {
    if name.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(name, "%Y-%m-%d").ok()
}

/// The start time encoded in a video file name, if it follows the
/// `YYYY-MM-DD--HH-MM-SS.<ext>` convention.
pub fn parse_video_name(name: &str) -> Option<NaiveDateTime> {
    let (stem, ext) = name.rsplit_once('.')?;
    if !matches!(ext.to_ascii_lowercase().as_str(), "mov" | "mp4") || stem.len() != 20 {
        return None;
    }
    NaiveDateTime::parse_from_str(stem, "%Y-%m-%d--%H-%M-%S").ok()
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
        } else if let Some(start) = parse_video_name(name) {
            days.insert(start.date());
        }
    }
    Ok(days.into_iter().collect())
}

/// Screenshot file names in a day folder, in capture order. A missing folder is
/// an empty day, not an error.
pub fn list_screenshots(day_dir: &Path) -> std::io::Result<Vec<String>> {
    let entries = match fs::read_dir(day_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut shots: Vec<(u32, String)> = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else { continue };
        if let Some(number) = parse_screenshot_name(&name) {
            shots.push((number, name));
        }
    }
    shots.sort();
    Ok(shots.into_iter().map(|(_, name)| name).collect())
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
            let Ok(name) = entry.file_name().into_string() else { continue };
            let Some(start) = parse_video_name(&name) else { continue };
            if start.date() == day && entry.file_type()?.is_file() {
                videos.push(VideoFile { path: entry.path(), start });
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
        assert_eq!(parse_day("2024-12-20"), NaiveDate::from_ymd_opt(2024, 12, 20));
        assert_eq!(parse_day(".cache"), None);
        assert_eq!(parse_day("2024-12-20--12-48-38.mov"), None);

        assert_eq!(
            parse_video_name("2024-12-20--12-48-38.mov"),
            NaiveDate::from_ymd_opt(2024, 12, 20).unwrap().and_hms_opt(12, 48, 38)
        );
        assert!(parse_video_name("2025-10-20--09-03-51.mp4").is_some());
        assert_eq!(parse_video_name("2024-12-20--12-48-38.mov.partial"), None);
        assert_eq!(parse_video_name("holiday.mov"), None);

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
        fs::write(dir.path().join("screenshots.db"), b"").unwrap();

        let days = list_days(dir.path()).unwrap();
        assert_eq!(
            days,
            vec![
                NaiveDate::from_ymd_opt(2024, 12, 20).unwrap(),
                NaiveDate::from_ymd_opt(2026, 10, 4).unwrap(),
            ]
        );
    }

    #[test]
    fn sorts_screenshots_numerically_and_ignores_other_files() {
        let dir = TempDir::new().unwrap();
        for name in ["00010.png", "00002.png", "100000.png", ".DS_Store", "notes.txt"] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        assert_eq!(
            list_screenshots(dir.path()).unwrap(),
            vec!["00002.png", "00010.png", "100000.png"]
        );
        assert!(list_screenshots(&dir.path().join("missing")).unwrap().is_empty());
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
            vec!["2024-12-29--11-17-04.mov", "2024-12-29--14-17-25.mov", "2024-12-29--16-00-00.mov"]
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
