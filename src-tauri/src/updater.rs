//! Keeps release builds on the newest GitHub release.
//!
//! The endpoint in `tauri.conf.json` is the `latest.json` the release workflow
//! uploads to the rolling `latest` release, and the plugin only installs an
//! update whose signature checks out against the public key next to it. An
//! installed update replaces the bundle on disk but not the running process,
//! and this app is rarely quit, so it relaunches itself — but only while its
//! window is not focused, so nobody is scrubbing through a day when it goes.

use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

/// How often to look for a new release. Every push to `main` is one.
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// How often to look for a moment to relaunch once an update is installed.
const RELAUNCH_POLL: Duration = Duration::from_secs(30);

/// Checks now and then every `CHECK_INTERVAL` for the life of the app.
///
/// Debug builds never update: they would replace a `tauri dev` binary with the
/// release build, which uses the real library rather than `~/Timelapse_dev`.
pub fn spawn(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            match install_update(&app).await {
                Ok(true) => break,
                Ok(false) => {}
                Err(e) => eprintln!("Update check failed: {}", e),
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
        while window_is_focused(&app) {
            tokio::time::sleep(RELAUNCH_POLL).await;
        }
        app.restart();
    });
}

/// Downloads and installs a newer release if there is one. `Ok(true)` means
/// the bundle on disk is now the new version.
async fn install_update(app: &AppHandle) -> tauri_plugin_updater::Result<bool> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(false);
    };
    println!(
        "Installing update {} (running {})",
        update.version, update.current_version
    );
    update.download_and_install(|_, _| {}, || {}).await?;
    Ok(true)
}

fn window_is_focused(app: &AppHandle) -> bool {
    app.webview_windows()
        .values()
        .any(|window| window.is_focused().unwrap_or(false))
}
