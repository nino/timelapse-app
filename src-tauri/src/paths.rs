use std::path::{Path, PathBuf};

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

/// The `ffmpeg` to run: the copy bundled with the app when there is one, else
/// whatever `ffmpeg` is on `PATH`.
pub fn ffmpeg() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
    sidecar_or_path(exe_dir.as_deref(), "ffmpeg")
}

/// Tauri installs `externalBin` sidecars next to the app's executable
/// (`Contents/MacOS/` in the bundle, `target/<profile>/` under `tauri dev`), so
/// that is where a bundled tool is. Falls back to the bare name, resolved
/// through `PATH`, which is how tests and Linux builds find a system ffmpeg.
fn sidecar_or_path(exe_dir: Option<&Path>, name: &str) -> PathBuf {
    exe_dir
        .map(|dir| dir.join(name))
        .filter(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
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
    fn prefers_a_sidecar_next_to_the_executable() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(sidecar_or_path(Some(dir.path()), "ffmpeg"), PathBuf::from("ffmpeg"));
        assert_eq!(sidecar_or_path(None, "ffmpeg"), PathBuf::from("ffmpeg"));

        std::fs::write(dir.path().join("ffmpeg"), b"").unwrap();
        assert_eq!(sidecar_or_path(Some(dir.path()), "ffmpeg"), dir.path().join("ffmpeg"));
    }
}
