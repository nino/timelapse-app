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
}
