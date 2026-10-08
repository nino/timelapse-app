//! What the app looked like when it quit, so the next launch can put it back:
//! where each window was and how big, which windows were open, and which day
//! and frame the viewer showed.
//!
//! It lives in `state.json` in a per-profile folder of the app's config
//! directory, so a dev build never moves the release build's windows or opens
//! a day only one library has. Changes are kept in memory and written by a
//! background thread once they have stopped for a second (dragging a window
//! reports every pixel it moves), and straight away when the app quits. A
//! missing or unreadable file means a first launch.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long changes must stop before they are written.
const WRITE_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppState {
    /// Keyed by window label.
    pub windows: BTreeMap<String, WindowState>,
    pub viewer: ViewerPosition,
}

/// A window's outer top-left corner and inner size, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Whether it was open when the app quit. Closing a window clears this;
    /// quitting leaves it as it is.
    #[serde(default)]
    pub open: bool,
}

/// What the viewer showed. Mirrors `ViewerPosition` in `src/viewerPosition.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewerPosition {
    /// `None` while the viewer follows the newest day's newest frame, so the
    /// next launch opens whatever day is newest by then.
    pub day: Option<String>,
    /// Frame index in `day`; `None` for its newest frame.
    pub index: Option<u64>,
}

/// A rectangle in logical pixels: a window, or a screen's usable area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn overlap(&self, other: &Rect) -> (f64, f64) {
        let width = (self.x + self.width).min(other.x + other.width) - self.x.max(other.x);
        let height = (self.y + self.height).min(other.y + other.height) - self.y.max(other.y);
        (width.max(0.0), height.max(0.0))
    }
}

/// How much of a window's top edge must be on a screen for it to be left
/// where it was: enough title bar to grab and drag it.
const GRAB_WIDTH: f64 = 80.0;
const GRAB_HEIGHT: f64 = 20.0;

/// Where to put a window saved at `saved`, given the usable area of each
/// screen (the primary one first). A window whose title bar can still be
/// grabbed stays put; one that would be out of reach (its screen was
/// unplugged, or the arrangement changed) moves onto the screen it overlaps
/// most, else the primary one, shrunk to fit if it has to.
pub fn fit(saved: Rect, screens: &[Rect]) -> Rect {
    let title_bar = Rect { height: GRAB_HEIGHT, ..saved };
    let reachable = screens.iter().any(|screen| {
        let (width, height) = title_bar.overlap(screen);
        width >= GRAB_WIDTH.min(saved.width) && height >= GRAB_HEIGHT
    });
    if reachable || screens.is_empty() {
        return saved;
    }
    let area = |screen: &Rect| {
        let (width, height) = saved.overlap(screen);
        width * height
    };
    let screen = screens
        .iter()
        .filter(|screen| area(screen) > 0.0)
        .max_by(|a, b| area(a).total_cmp(&area(b)))
        .unwrap_or(&screens[0]);
    let width = saved.width.min(screen.width);
    let height = saved.height.min(screen.height);
    Rect {
        x: saved.x.clamp(screen.x, screen.x + screen.width - width),
        y: saved.y.clamp(screen.y, screen.y + screen.height - height),
        width,
        height,
    }
}

/// The state in memory, and the file it is saved to.
pub struct AppStateStore {
    path: Option<PathBuf>,
    current: Arc<Mutex<AppState>>,
    /// Wakes the writer thread. `None` without a file to write.
    changed: Option<Mutex<Sender<()>>>,
    /// Held while writing, so the writer thread and a save on quit never write
    /// the temp file at the same time.
    writing: Arc<Mutex<()>>,
}

impl AppStateStore {
    /// Loads `state.json` from `dir` and starts the thread that saves it.
    /// With no `dir`, changes are kept for this run only.
    pub fn load(dir: Option<PathBuf>) -> Self {
        let path = dir.map(|dir| dir.join("state.json"));
        let current = Arc::new(Mutex::new(path.as_deref().map(read).unwrap_or_default()));
        let writing = Arc::new(Mutex::new(()));
        let changed = path.clone().map(|path| {
            let (tx, rx) = mpsc::channel::<()>();
            let current = Arc::clone(&current);
            let writing = Arc::clone(&writing);
            std::thread::spawn(move || {
                while rx.recv().is_ok() {
                    // Wait until the changes stop.
                    while rx.recv_timeout(WRITE_DELAY).is_ok() {}
                    let state = current.lock().unwrap().clone();
                    let _guard = writing.lock().unwrap();
                    if let Err(e) = write(&path, &state) {
                        eprintln!("Could not save the window state: {}", e);
                    }
                }
            });
            Mutex::new(tx)
        });
        AppStateStore {
            path,
            current,
            changed,
            writing,
        }
    }

    pub fn get(&self) -> AppState {
        self.current.lock().unwrap().clone()
    }

    pub fn window(&self, label: &str) -> Option<WindowState> {
        self.current.lock().unwrap().windows.get(label).copied()
    }

    /// Applies `change`, and saves the result a moment later if it changed
    /// anything.
    pub fn update(&self, change: impl FnOnce(&mut AppState)) {
        let changed = {
            let mut current = self.current.lock().unwrap();
            let before = current.clone();
            change(&mut current);
            *current != before
        };
        if changed {
            if let Some(tx) = &self.changed {
                let _ = tx.lock().unwrap().send(());
            }
        }
    }

    /// Records where a window is and how big, and that it is open.
    pub fn set_window(&self, label: &str, rect: Rect) {
        self.update(|state| {
            state.windows.insert(
                label.to_owned(),
                WindowState {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                    open: true,
                },
            );
        });
    }

    /// Records whether a window is open, keeping where it was.
    pub fn set_open(&self, label: &str, open: bool) {
        self.update(|state| {
            if let Some(window) = state.windows.get_mut(label) {
                window.open = open;
            }
        });
    }

    /// Writes the state now. Called when the app quits, which doesn't wait
    /// for the writer thread.
    pub fn save_now(&self) {
        let Some(path) = &self.path else { return };
        let state = self.get();
        let _guard = self.writing.lock().unwrap();
        if let Err(e) = write(path, &state) {
            eprintln!("Could not save the window state: {}", e);
        }
    }
}

fn read(path: &Path) -> AppState {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            eprintln!("Ignoring unreadable {:?}: {}", path, e);
            AppState::default()
        }),
        Err(_) => AppState::default(),
    }
}

/// Writes beside the target and renames into place, so a crash never leaves a
/// half-written file.
fn write(path: &Path, state: &AppState) -> Result<(), String> {
    let dir = path.parent().ok_or("state path has no parent")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect { x, y, width, height }
    }

    const LAPTOP: Rect = Rect { x: 0.0, y: 25.0, width: 1512.0, height: 920.0 };
    const EXTERNAL: Rect = Rect { x: 1512.0, y: -400.0, width: 2560.0, height: 1415.0 };

    #[test]
    fn a_window_on_screen_stays_put() {
        let saved = rect(100.0, 100.0, 800.0, 600.0);
        assert_eq!(fit(saved, &[LAPTOP]), saved);
    }

    #[test]
    fn a_window_partly_off_screen_stays_put_while_its_title_bar_can_be_grabbed() {
        let saved = rect(1400.0, 800.0, 800.0, 600.0);
        assert_eq!(fit(saved, &[LAPTOP]), saved);
        // Straddling two screens counts too.
        let straddling = rect(1200.0, 100.0, 800.0, 600.0);
        assert_eq!(fit(straddling, &[LAPTOP, EXTERNAL]), straddling);
    }

    #[test]
    fn a_window_on_an_unplugged_screen_moves_to_the_primary_one() {
        let saved = rect(2000.0, -300.0, 800.0, 600.0);
        assert_eq!(fit(saved, &[LAPTOP, EXTERNAL]), saved);
        assert_eq!(fit(saved, &[LAPTOP]), rect(712.0, 25.0, 800.0, 600.0));
    }

    #[test]
    fn a_window_too_big_for_its_screen_shrinks_to_fit() {
        let saved = rect(3000.0, -300.0, 2400.0, 1300.0);
        assert_eq!(fit(saved, &[LAPTOP]), rect(0.0, 25.0, 1512.0, 920.0));
    }

    #[test]
    fn a_window_whose_title_bar_is_under_the_menu_bar_moves_down() {
        // Mostly on the laptop's screen, but its top is above the usable area.
        let saved = rect(100.0, -50.0, 800.0, 600.0);
        assert_eq!(fit(saved, &[LAPTOP]), rect(100.0, 25.0, 800.0, 600.0));
    }

    #[test]
    fn without_screens_nothing_moves() {
        let saved = rect(-5000.0, -5000.0, 800.0, 600.0);
        assert_eq!(fit(saved, &[]), saved);
    }

    #[test]
    fn state_survives_a_reload() {
        let dir = TempDir::new().unwrap();
        let store = AppStateStore::load(Some(dir.path().join("config")));
        store.set_window("main", rect(10.0, 20.0, 800.0, 600.0));
        store.set_window("activity", rect(30.0, 40.0, 460.0, 700.0));
        store.set_open("activity", false);
        store.update(|state| {
            state.viewer = ViewerPosition {
                day: Some("2026-10-07".into()),
                index: Some(1234),
            }
        });
        store.save_now();

        let reloaded = AppStateStore::load(Some(dir.path().join("config"))).get();
        assert_eq!(reloaded, store.get());
        assert!(reloaded.windows["main"].open);
        assert!(!reloaded.windows["activity"].open);
        assert_eq!(reloaded.viewer.index, Some(1234));
        assert!(!dir.path().join("config/state.json.tmp").exists());
    }

    #[test]
    fn changes_are_written_once_they_stop() {
        let dir = TempDir::new().unwrap();
        let store = AppStateStore::load(Some(dir.path().to_path_buf()));
        for x in 0..20 {
            store.set_window("main", rect(x as f64, 0.0, 800.0, 600.0));
        }
        let file = dir.path().join("state.json");
        assert!(!file.exists(), "nothing is written while changes keep coming");
        std::thread::sleep(WRITE_DELAY * 3);
        let saved = AppStateStore::load(Some(dir.path().to_path_buf())).get();
        assert_eq!(saved.windows["main"].x, 19.0);
    }

    #[test]
    fn an_unreadable_file_means_a_first_launch() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("state.json"), "not json").unwrap();
        let store = AppStateStore::load(Some(dir.path().to_path_buf()));
        assert_eq!(store.get(), AppState::default());
    }

    #[test]
    fn closing_a_window_keeps_where_it_was() {
        let store = AppStateStore::load(None);
        store.set_window("settings", rect(10.0, 20.0, 440.0, 160.0));
        store.set_open("settings", false);
        let window = store.window("settings").unwrap();
        assert!(!window.open);
        assert_eq!((window.x, window.y), (10.0, 20.0));
        // Closing a window never saved is a no-op.
        store.set_open("activity", false);
        assert!(store.window("activity").is_none());
    }
}
