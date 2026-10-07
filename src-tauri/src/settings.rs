//! The app's preferences, edited in the Settings window.
//!
//! They live in `settings.json` in the app's config directory. Every change is
//! written straight away, the way a macOS settings window applies a change
//! without a Save button. A missing or unreadable file means the defaults.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Install new releases in the background (see `updater.rs`). "Check for
    /// Updates…" works either way.
    pub update_automatically: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            update_automatically: true,
        }
    }
}

/// The settings in memory, and the file they are saved to.
pub struct SettingsStore {
    path: Option<PathBuf>,
    current: Mutex<Settings>,
}

impl SettingsStore {
    /// Loads `settings.json` from `dir`. With no `dir` (no config directory
    /// could be found), changes still apply but are not saved.
    pub fn load(dir: Option<PathBuf>) -> Self {
        let path = dir.map(|dir| dir.join("settings.json"));
        let current = path.as_deref().map(read).unwrap_or_default();
        SettingsStore {
            path,
            current: Mutex::new(current),
        }
    }

    pub fn get(&self) -> Settings {
        *self.current.lock().unwrap()
    }

    /// Applies `change` and saves the result. The new settings stay in effect
    /// even if saving fails.
    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
        let mut current = self.current.lock().unwrap();
        change(&mut current);
        if let Some(path) = &self.path {
            write(path, &current)?;
        }
        Ok(*current)
    }
}

fn read(path: &Path) -> Settings {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            eprintln!("Ignoring unreadable {:?}: {}", path, e);
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

/// Writes beside the target and renames into place, so a crash never leaves a
/// half-written file that would reset every setting.
fn write(path: &Path, settings: &Settings) -> Result<(), String> {
    let dir = path.parent().ok_or("settings path has no parent")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn defaults_to_updating_automatically() {
        let dir = TempDir::new().unwrap();
        let store = SettingsStore::load(Some(dir.path().to_path_buf()));
        assert!(store.get().update_automatically);
    }

    #[test]
    fn a_change_is_saved_and_survives_a_reload() {
        let dir = TempDir::new().unwrap();
        let store = SettingsStore::load(Some(dir.path().join("config")));
        let saved = store.update(|s| s.update_automatically = false).unwrap();
        assert!(!saved.update_automatically);
        assert!(!store.get().update_automatically);

        let reloaded = SettingsStore::load(Some(dir.path().join("config")));
        assert!(!reloaded.get().update_automatically);
        assert!(!dir.path().join("config/settings.json.tmp").exists());
    }

    #[test]
    fn an_unreadable_file_means_the_defaults() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("settings.json"), "not json").unwrap();
        let store = SettingsStore::load(Some(dir.path().to_path_buf()));
        assert_eq!(store.get(), Settings::default());
    }

    #[test]
    fn unknown_and_missing_keys_are_tolerated() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("settings.json"), r#"{"somethingElse": 1}"#).unwrap();
        let store = SettingsStore::load(Some(dir.path().to_path_buf()));
        assert_eq!(store.get(), Settings::default());
    }

    #[test]
    fn without_a_directory_changes_apply_but_are_not_saved() {
        let store = SettingsStore::load(None);
        store.update(|s| s.update_automatically = false).unwrap();
        assert!(!store.get().update_automatically);
    }
}
