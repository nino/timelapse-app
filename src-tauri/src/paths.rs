use std::path::PathBuf;

/// Name of the directory under `$HOME` holding screenshots, rendered videos,
/// the extracted-frame cache and the SQLite database.
///
/// Debug builds — which is what `bun run tauri dev` produces — use a separate
/// library so development never reads from or writes into the real one.
///
/// Two things must stay in step with this constant:
/// - `src/timelapseRoot.ts`, which the frontend uses to build the same paths.
/// - `capabilities/default.json`, whose `fs:scope` allow-list has to name both
///   directories, since the scope is baked in at build time and cannot branch
///   on the profile.
pub const TIMELAPSE_DIR_NAME: &str = if cfg!(debug_assertions) {
    "Timelapse_dev"
} else {
    "Timelapse"
};

/// Absolute path to the timelapse library, or `None` when the home directory
/// cannot be resolved.
pub fn timelapse_root() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(TIMELAPSE_DIR_NAME))
}

/// Homebrew's `bin` directories (Apple Silicon first, then Intel).
const HOMEBREW_BINS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// `path` with Homebrew's `bin` directories appended where missing.
///
/// An app opened from Finder inherits launchd's `PATH`
/// (`/usr/bin:/bin:/usr/sbin:/sbin`), not the shell's, so the bundled app
/// cannot find the Homebrew `ffmpeg` that frame extraction and the converter
/// shell out to. `bun run tauri dev` starts from a shell and never noticed.
pub fn path_with_homebrew(path: &str) -> String {
    let mut dirs: Vec<&str> = path.split(':').filter(|d| !d.is_empty()).collect();
    for bin in HOMEBREW_BINS {
        if !dirs.contains(&bin) {
            dirs.push(bin);
        }
    }
    dirs.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_name_tracks_build_profile() {
        if cfg!(debug_assertions) {
            assert_eq!(TIMELAPSE_DIR_NAME, "Timelapse_dev");
        } else {
            assert_eq!(TIMELAPSE_DIR_NAME, "Timelapse");
        }

        assert!(timelapse_root().unwrap().ends_with(TIMELAPSE_DIR_NAME));
    }

    #[test]
    fn path_gains_homebrew_bins_once() {
        assert_eq!(
            path_with_homebrew("/usr/bin:/bin:/usr/sbin:/sbin"),
            "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin"
        );
        assert_eq!(
            path_with_homebrew("/opt/homebrew/bin:/usr/bin"),
            "/opt/homebrew/bin:/usr/bin:/usr/local/bin"
        );
        assert_eq!(path_with_homebrew(""), "/opt/homebrew/bin:/usr/local/bin");
    }
}
