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
//! - "Check for Updates…" in the app menu, whatever the setting: it says what
//!   it found in a dialog and relaunches straight away if asked to.

use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::{Mutex, Notify};

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
            if app.state::<SettingsStore>().get().update_automatically {
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
    updater.check().await.map_err(|e| e.to_string())
}

async fn install(update: &Update) -> Result<(), String> {
    println!(
        "Installing update {} (running {})",
        update.version, update.current_version
    );
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())
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
                app.restart();
            }
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
