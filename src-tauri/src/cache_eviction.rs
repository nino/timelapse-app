//! Removes old folders from `<library>/.cache` at launch. Only the
//! converter's staging folders live there now; the rest are left from older
//! versions, which kept decoded frames in the library.

use std::path::Path;

/// Delete every directory directly under `<root>/.cache` whose mtime is more
/// than 15 days old, and report how many went.
///
/// This is the only `remove_dir_all` in the app, so it takes the library root
/// as an argument: that is what lets the tests exercise it against a `TempDir`
/// instead of the real library.
pub(crate) fn evict_old_cache_impl(root: &Path) -> Result<String, String> {
    let cache_dir = root.join(".cache");

    if !cache_dir.exists() {
        return Ok("Cache directory does not exist".to_string());
    }

    let now = std::time::SystemTime::now();
    let fifteen_days = std::time::Duration::from_secs(15 * 24 * 60 * 60);

    let entries = std::fs::read_dir(&cache_dir)
        .map_err(|e| format!("Failed to read cache directory: {}", e))?;

    let mut removed_count = 0;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        // Get the modification time of the directory
        let metadata = std::fs::metadata(&path)
            .map_err(|e| format!("Failed to get metadata for {:?}: {}", path, e))?;

        let modified = metadata.modified()
            .map_err(|e| format!("Failed to get modified time for {:?}: {}", path, e))?;

        // Check if older than 15 days
        if let Ok(age) = now.duration_since(modified) {
            if age > fifteen_days {
                println!("Removing old cache folder: {:?} (age: {} days)", path, age.as_secs() / 86400);
                std::fs::remove_dir_all(&path)
                    .map_err(|e| format!("Failed to remove directory {:?}: {}", path, e))?;
                removed_count += 1;
            }
        }
    }

    Ok(format!("Removed {} old cache folders", removed_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File, FileTimes};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    /// A temp library root with an empty `.cache` inside it. The `TempDir` is
    /// returned so the caller keeps it alive for the duration of the test.
    fn temp_library() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join(".cache");
        fs::create_dir_all(&cache_dir).unwrap();
        (temp_dir, cache_dir)
    }

    /// A cache folder holding one file, aged to `mtime_age`.
    ///
    /// Order matters: writing the file bumps the directory's mtime, so the
    /// backdating has to come last.
    fn cache_folder(cache_dir: &Path, name: &str, mtime_age: Duration) -> PathBuf {
        let path = cache_dir.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("frame000001.jpg"), b"not really a jpeg").unwrap();
        set_age(&path, mtime_age);
        path
    }

    /// Backdate `path`'s mtime by `age`. Eviction compares mtime against the
    /// clock, so stamping it explicitly is what keeps these tests off the
    /// wall-clock — no sleeping, and no dependence on when they run.
    fn set_age(path: &Path, age: Duration) {
        let when = SystemTime::now() - age;
        File::open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(when))
            .unwrap();
    }

    #[test]
    fn test_evict_old_cache_removes_stale_folder() {
        let (temp_dir, cache_dir) = temp_library();
        let stale = cache_folder(&cache_dir, "2020-01-01", 30 * DAY);

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Removed 1 old cache folders");
        assert!(!stale.exists(), "a 30-day-old cache folder should be gone");
    }

    #[test]
    fn test_evict_old_cache_keeps_recent_folder() {
        let (temp_dir, cache_dir) = temp_library();
        let recent = cache_folder(&cache_dir, "2026-08-01", 14 * DAY);

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Removed 0 old cache folders");
        assert!(recent.exists(), "a 14-day-old cache folder should survive");
        assert!(recent.join("frame000001.jpg").exists());
    }

    // The cutoff is `age > 15 days`. Both folders here sit an hour away from it
    // so the outcome cannot hinge on how long the test itself takes to run.
    #[test]
    fn test_evict_old_cache_cutoff_is_fifteen_days() {
        let (temp_dir, cache_dir) = temp_library();
        let just_inside = cache_folder(&cache_dir, "inside", 15 * DAY - Duration::from_secs(3600));
        let just_outside = cache_folder(&cache_dir, "outside", 15 * DAY + Duration::from_secs(3600));

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Removed 1 old cache folders");
        assert!(just_inside.exists(), "14d23h is inside the window");
        assert!(!just_outside.exists(), "15d1h is outside the window");
    }

    #[test]
    fn test_evict_old_cache_skips_plain_files() {
        let (temp_dir, cache_dir) = temp_library();
        let loose_file = cache_dir.join("stale-note.txt");
        fs::write(&loose_file, b"old, but not a directory").unwrap();
        set_age(&loose_file, 30 * DAY);

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Removed 0 old cache folders");
        assert!(loose_file.exists(), "eviction only ever removes directories");
    }

    #[test]
    fn test_evict_old_cache_counts_only_what_it_removed() {
        let (temp_dir, cache_dir) = temp_library();
        let stale_a = cache_folder(&cache_dir, "stale-a", 20 * DAY);
        let stale_b = cache_folder(&cache_dir, "stale-b", 90 * DAY);
        let fresh = cache_folder(&cache_dir, "fresh", Duration::from_secs(60));
        let loose_file = cache_dir.join("stale-note.txt");
        fs::write(&loose_file, b"old, but not a directory").unwrap();
        set_age(&loose_file, 30 * DAY);

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Removed 2 old cache folders");
        assert!(!stale_a.exists());
        assert!(!stale_b.exists());
        assert!(fresh.exists());
        assert!(loose_file.exists());
    }

    #[test]
    fn test_evict_old_cache_without_cache_dir() {
        // A library that has never had frames extracted from it.
        let temp_dir = TempDir::new().unwrap();

        let result = evict_old_cache_impl(temp_dir.path());

        assert_eq!(result.unwrap(), "Cache directory does not exist");
    }

    #[test]
    fn test_evict_old_cache_leaves_the_rest_of_the_library_alone() {
        let (temp_dir, cache_dir) = temp_library();
        cache_folder(&cache_dir, "stale", 30 * DAY);

        // A day of screenshots, as old as the cache folder next to it.
        let day_dir = temp_dir.path().join("2020-01-01");
        fs::create_dir_all(&day_dir).unwrap();
        fs::write(day_dir.join("00001.png"), b"screenshot").unwrap();
        set_age(&day_dir, 30 * DAY);

        evict_old_cache_impl(temp_dir.path()).unwrap();

        assert!(
            day_dir.join("00001.png").exists(),
            "eviction must never reach outside .cache"
        );
    }
}
