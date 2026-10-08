//! Keeps release builds on the newest GitHub release.
//!
//! The endpoint in `tauri.conf.json` is the `latest.json` the release workflow
//! uploads to the rolling `latest` release, and the plugin only installs an
//! update whose signature checks out against the public key next to it.
//!
//! Two ways in:
//! - In the background, while "Update automatically" is on in Settings: a check
//!   at launch and then hourly. An installed update replaces the bundle on disk
//!   but not the running process, and this app is rarely quit, so it relaunches
//!   itself, but only while none of its windows is focused, so nobody is
//!   scrubbing through a day when it goes.
//!   It never installs an update that needs an admin password (the app is in
//!   a folder this account can't write to, such as /Applications for a
//!   non-admin user), because the plugin would ask for one with no warning,
//!   and again every hour if the dialog were cancelled.
//! - "Check for Updates…" in the app menu, whatever the setting: it says what
//!   it found in a dialog and relaunches straight away if asked to. Here the
//!   admin password dialog is expected, since someone asked for the update.

use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::{Mutex, Notify};

use crate::diagnostics;
use crate::settings::SettingsStore;

/// How often to look for a new release. Every push to `main` is one.
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// How often to look for a moment to relaunch once an update is installed.
const RELAUNCH_POLL: Duration = Duration::from_secs(30);

/// Shared by the background loop and "Check for Updates…".
#[derive(Default)]
pub struct Updater {
    /// Held while checking or installing, so the two never install at once.
    /// Holds the version already installed on disk, waiting for a relaunch.
    installed: Mutex<Option<String>>,
    /// Wakes the background loop early, when "Update automatically" is turned on.
    wake: Notify,
}

pub type UpdaterState = Arc<Updater>;

impl Updater {
    /// Tells the background loop the setting changed, so turning it on checks
    /// now rather than at the next hourly tick.
    pub fn setting_changed(&self) {
        self.wake.notify_one();
    }
}

/// Debug builds never update: they would replace a `tauri dev` binary with
/// the release build, which uses the real library rather than `~/Timelapse_dev`.
const ENABLED: bool = !cfg!(debug_assertions);

/// The background loop, for the life of the app.
pub fn spawn(app: AppHandle) {
    if !ENABLED {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let updater = Arc::clone(app.state::<UpdaterState>().inner());
        loop {
            if app.state::<SettingsStore>().get().update_automatically && can_install_silently() {
                match install_newer(&app, &updater).await {
                    Ok(Some(_)) => break,
                    Ok(None) => {}
                    Err(e) => eprintln!("Update check failed: {}", e),
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(CHECK_INTERVAL) => {}
                _ = updater.wake.notified() => {}
            }
        }
        while window_is_focused(&app) {
            tokio::time::sleep(RELAUNCH_POLL).await;
        }
        crate::before_quit(&app);
        app.restart();
    });
}

/// Installs a newer release if there is one, and returns its version. An
/// update already installed and waiting for a relaunch counts as found.
async fn install_newer(app: &AppHandle, updater: &Updater) -> Result<Option<String>, String> {
    let mut installed = updater.installed.lock().await;
    if installed.is_some() {
        return Ok(installed.clone());
    }
    let Some(update) = check(app).await? else {
        return Ok(None);
    };
    install(&update).await?;
    *installed = Some(update.version.clone());
    Ok(installed.clone())
}

async fn check(app: &AppHandle) -> Result<Option<Update>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let checked = updater.check().await.map_err(|e| e.to_string());
    match &checked {
        Ok(Some(update)) => diagnostics::info("updater", format!("Found {}", update.version)).record(),
        Ok(None) => {}
        Err(e) => diagnostics::warn("updater", format!("Check failed: {}", e)).record(),
    }
    checked
}

async fn install(update: &Update) -> Result<(), String> {
    println!(
        "Installing update {} (running {})",
        update.version, update.current_version
    );
    let started = std::time::Instant::now();
    let silently = can_install_silently();
    let installed = if silently {
        update
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| e.to_string())
    } else {
        install_with_password(update).await
    };
    let event = match &installed {
        Ok(()) => diagnostics::info("updater", format!("Installed {}", update.version)),
        Err(e) => diagnostics::warn("updater", format!("Could not install {}: {}", update.version, e)),
    };
    event
        .took(started.elapsed())
        .data(serde_json::json!({ "from": update.current_version, "withPassword": !silently }))
        .record();
    installed
}

/// Installs an update into a folder this account can't write to. The plugin
/// would do this through an AppleScript admin prompt run on the main thread,
/// which beachballs every window until the prompt is answered, so instead
/// `osascript` asks for the password in a process of its own while this
/// waits off the main thread. `download` checks the signature, as
/// `download_and_install` does.
#[cfg(target_os = "macos")]
async fn install_with_password(update: &Update) -> Result<(), String> {
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    let bundle = current_bundle().ok_or("Couldn't find the running app")?;
    tauri::async_runtime::spawn_blocking(move || replace_with_password(&bytes, &bundle))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(not(target_os = "macos"))]
async fn install_with_password(update: &Update) -> Result<(), String> {
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())
}

/// Unpacks the update next to nothing that matters (a temp folder), then has
/// an admin move it over `bundle`.
#[cfg(target_os = "macos")]
fn replace_with_password(archive: &[u8], bundle: &std::path::Path) -> Result<(), String> {
    let staging = std::env::temp_dir().join(format!("timelapse-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let result = unpack_app(archive, &staging).and_then(|new_app| {
        let output = std::process::Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "set {oldApp, newApp} to {quoted form of item 1 of argv, quoted form of item 2 of argv}",
                "-e",
                "do shell script \"rm -rf \" & oldApp & \" && mv -f \" & newApp & \" \" & oldApp & \" && touch \" & oldApp with administrator privileges",
                "-e",
                "end run",
            ])
            .arg(bundle)
            .arg(&new_app)
            .output()
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    });
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Unpacks an updater archive (a gzipped tar of one `.app`) into `dir`, and
/// returns the app's path.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn unpack_app(archive: &[u8], dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    tar::Archive::new(flate2::read::GzDecoder::new(archive))
        .unpack(dir)
        .map_err(|e| format!("Couldn't unpack the update: {}", e))?;
    std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .ok_or_else(|| "The update has no app in it".to_string())
}

/// Whether the user cancelled the admin password prompt.
fn cancelled(error: &str) -> bool {
    error.contains("(-128)")
}

/// "Check for Updates…": check now and say what was found.
pub fn check_from_menu(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let current = app.package_info().version.to_string();
        if !ENABLED {
            tell(
                &app,
                "Updates are off in development builds",
                "A development build would replace itself with the release build, which uses your real library.",
            );
            return;
        }
        let updater = Arc::clone(app.state::<UpdaterState>().inner());
        let mut installed = updater.installed.lock().await;
        if let Some(version) = installed.clone() {
            drop(installed);
            offer_relaunch(&app, &version);
            return;
        }
        let update = match check(&app).await {
            Ok(Some(update)) => update,
            Ok(None) => {
                tell(
                    &app,
                    "You're up to date",
                    &format!("Timelapse App {} is the newest version.", current),
                );
                return;
            }
            Err(e) => {
                warn(&app, "Couldn't check for updates", &e);
                return;
            }
        };
        let version = update.version.clone();
        let install_it = app
            .dialog()
            .message(format!(
                "Timelapse App {} is available. You have {}.",
                version, current
            ))
            .title("A new version is available")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Install and Relaunch".into(),
                "Later".into(),
            ))
            .blocking_show();
        if !install_it {
            return;
        }
        match install(&update).await {
            Ok(()) => {
                *installed = Some(version);
                crate::before_quit(&app);
                app.restart();
            }
            Err(e) if cancelled(&e) => {}
            Err(e) => warn(&app, "Couldn't install the update", &e),
        }
    });
}

fn offer_relaunch(app: &AppHandle, version: &str) {
    let relaunch = app
        .dialog()
        .message(format!(
            "Timelapse App {} is installed and starts the next time the app opens.",
            version
        ))
        .title("An update is ready")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Relaunch Now".into(),
            "Later".into(),
        ))
        .blocking_show();
    if relaunch {
        crate::before_quit(app);
        app.restart();
    }
}

fn tell(app: &AppHandle, title: &str, message: &str) {
    app.dialog().message(message).title(title).blocking_show();
}

fn warn(app: &AppHandle, title: &str, message: &str) {
    app.dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .blocking_show();
}

fn window_is_focused(app: &AppHandle) -> bool {
    app.webview_windows()
        .values()
        .any(|window| window.is_focused().unwrap_or(false))
}

/// Whether this account can replace the running app bundle by itself. The
/// plugin moves the bundle out of its folder and the new one in, and asks for
/// an admin password when it can't.
fn can_install_silently() -> bool {
    current_bundle().is_some_and(|bundle| can_replace(&bundle))
}

/// The running app's `.app`, from `….app/Contents/MacOS/<binary>`.
fn current_bundle() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.ancestors().nth(3).map(std::path::Path::to_path_buf))
}

/// Moving a bundle needs write access to the folder it is in, and to the
/// bundle itself (a moved directory's `..` entry changes).
fn can_replace(bundle: &std::path::Path) -> bool {
    bundle.parent().is_some_and(writable) && writable(bundle)
}

#[cfg(target_os = "macos")]
fn writable(path: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is a valid NUL-terminated string that outlives the call.
    unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
}

#[cfg(not(target_os = "macos"))]
fn writable(path: &std::path::Path) -> bool {
    path.metadata().is_ok_and(|m| !m.permissions().readonly())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_in_a_writable_folder_can_be_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Timelapse App.app");
        std::fs::create_dir(&bundle).unwrap();
        assert!(can_replace(&bundle));
    }

    #[test]
    fn unpacks_the_app_from_an_update_archive() {
        let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let plist = b"<plist/>";
        let mut header = tar::Header::new_gnu();
        header.set_size(plist.len() as u64);
        header.set_mode(0o644);
        archive
            .append_data(
                &mut header,
                "Timelapse App.app/Contents/Info.plist",
                &plist[..],
            )
            .unwrap();
        let bytes = archive.into_inner().unwrap().finish().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let app = unpack_app(&bytes, dir.path()).unwrap();
        assert_eq!(app, dir.path().join("Timelapse App.app"));
        assert!(app.join("Contents/Info.plist").is_file());
    }

    #[test]
    fn recognises_a_cancelled_password_prompt() {
        assert!(cancelled("execution error: User canceled. (-128)"));
        assert!(!cancelled("execution error: rm: Permission denied (1)"));
    }

    #[test]
    fn a_missing_bundle_cannot_be_replaced() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!can_replace(&dir.path().join("Gone.app")));
    }

    // Root can write anywhere, so this only means something as a normal user
    // (as on the macOS CI runner).
    #[cfg(unix)]
    #[test]
    fn a_bundle_in_a_read_only_folder_cannot_be_replaced() {
        use std::os::unix::fs::PermissionsExt;
        if running_as_root() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Applications");
        let bundle = folder.join("Timelapse App.app");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
        let replaceable = can_replace(&bundle);
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!replaceable);
    }

    #[cfg(unix)]
    fn running_as_root() -> bool {
        std::env::var("USER").is_ok_and(|u| u == "root")
            || std::process::Command::new("id")
                .arg("-u")
                .output()
                .is_ok_and(|o| o.stdout.trim_ascii() == b"0")
    }
}
