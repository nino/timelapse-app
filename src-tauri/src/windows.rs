//! The app's windows and menu: the main, Activity, Settings and About
//! windows, where each opens (the place it had last, kept on a screen), which
//! reopen at launch, and the menu items that open them.

use crate::app_state::{self, AppStateStore, Rect};
use crate::updater;
use tauri::menu::{Menu, MenuEvent, MenuItem, MenuItemKind, PredefinedMenuItem, WINDOW_SUBMENU_ID};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// Label of the main window, set in `tauri.conf.json`.
pub(crate) const MAIN_WINDOW: &str = "main";
/// Label of the Activity window, and id of the menu item that opens it.
pub(crate) const ACTIVITY_WINDOW: &str = "activity";
/// Label of the Settings window, and id of the menu item that opens it.
pub(crate) const SETTINGS_WINDOW: &str = "settings";
/// Label of the About window, and id of the menu item that opens it.
pub(crate) const ABOUT_WINDOW: &str = "about";
/// Id of the "Check for Updates…" menu item.
pub(crate) const CHECK_FOR_UPDATES: &str = "check-for-updates";

/// Opens the window a menu item names, or checks for updates.
pub(crate) fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let opened = match event.id().as_ref() {
        ACTIVITY_WINDOW => toggle_activity_window(app),
        SETTINGS_WINDOW => show_settings_window(app, Opening::Asked),
        ABOUT_WINDOW => show_about_window(app),
        CHECK_FOR_UPDATES => {
            updater::check_from_menu(app.clone());
            Ok(())
        }
        _ => Ok(()),
    };
    if let Err(e) = opened {
        eprintln!("Could not open the {} window: {}", event.id().as_ref(), e);
    }
}

/// The usable area of each screen (without the menu bar and Dock), in
/// logical pixels, with the primary screen first.
fn screens<R: Runtime>(app: &AppHandle<R>) -> Vec<Rect> {
    let primary = app.primary_monitor().ok().flatten().map(|m| m.name().cloned());
    let mut monitors = app.available_monitors().unwrap_or_default();
    if let Some(primary) = primary {
        monitors.sort_by_key(|m| m.name() != primary.as_ref());
    }
    monitors
        .iter()
        .map(|m| {
            let scale = m.scale_factor();
            let area = m.work_area();
            Rect {
                x: area.position.x as f64 / scale,
                y: area.position.y as f64 / scale,
                width: area.size.width as f64 / scale,
                height: area.size.height as f64 / scale,
            }
        })
        .collect()
}

/// Where to open the window `label`: where it was last, moved onto a screen
/// if that place is out of reach now. `None` the first time.
fn saved_place<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<Rect> {
    let saved = app.try_state::<AppStateStore>()?.window(label)?;
    let rect = Rect {
        x: saved.x,
        y: saved.y,
        width: saved.width,
        height: saved.height,
    };
    Some(app_state::fit(rect, &screens(app)))
}

/// Records where `window` is and how big, and that it is open. A minimised
/// or full-screen window keeps the place it had before.
fn remember_place<R: Runtime>(window: &tauri::Window<R>) {
    let Some(state) = window.try_state::<AppStateStore>() else { return };
    let place = || -> tauri::Result<Option<Rect>> {
        if window.is_minimized()? || window.is_fullscreen()? {
            return Ok(None);
        }
        let scale = window.scale_factor()?;
        let position = window.outer_position()?.to_logical::<f64>(scale);
        let size = window.inner_size()?.to_logical::<f64>(scale);
        if size.width <= 0.0 || size.height <= 0.0 {
            return Ok(None);
        }
        Ok(Some(Rect {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }))
    };
    match place() {
        Ok(Some(rect)) => state.set_window(window.label(), rect),
        Ok(None) => {}
        Err(e) => eprintln!("Could not read where the {} window is: {}", window.label(), e),
    }
}

/// Keeps track of where the app's windows are and which are open.
pub(crate) fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if ![MAIN_WINDOW, ACTIVITY_WINDOW, SETTINGS_WINDOW, ABOUT_WINDOW].contains(&window.label()) {
        return;
    }
    match event {
        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => remember_place(window),
        // Quitting closes no window this way, so whatever is open when the
        // app quits stays marked open.
        tauri::WindowEvent::CloseRequested { .. } => {
            if let Some(state) = window.try_state::<AppStateStore>() {
                state.set_open(window.label(), false);
            }
        }
        _ => {}
    }
}

/// Opens the main window, from its entry in `tauri.conf.json` (which has
/// `create: false` so it can be placed before it appears).
fn open_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
        .cloned()
        .unwrap_or_default();
    let mut builder = WebviewWindowBuilder::from_config(app, &config)?;
    if let Some(place) = saved_place(app, MAIN_WINDOW) {
        // A size saved before the window had a minimum may be below it.
        builder = builder.position(place.x, place.y).inner_size(
            place.width.max(config.min_width.unwrap_or(0.0)),
            place.height.max(config.min_height.unwrap_or(0.0)),
        );
    }
    let window = builder.build()?;
    remember_place(&window.as_ref().window());
    Ok(())
}

/// Puts the windows back the way they were when the app quit. The Activity
/// and Settings windows open without taking focus, and before the main
/// window, so the main window ends up in front and focused, as on a first
/// launch.
pub(crate) fn restore_windows<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let saved = app.state::<AppStateStore>().get();
    let was_open = |label: &str| saved.windows.get(label).is_some_and(|w| w.open);
    if was_open(ACTIVITY_WINDOW) {
        if let Err(e) = show_activity_window(app, Opening::Restored) {
            eprintln!("Could not reopen the Activity window: {}", e);
        }
    }
    if was_open(SETTINGS_WINDOW) {
        if let Err(e) = show_settings_window(app, Opening::Restored) {
            eprintln!("Could not reopen the Settings window: {}", e);
        }
    }
    open_main_window(app)
}

/// Whether a window opens because someone asked for it, or because it was
/// open when the app last quit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Opening {
    Asked,
    Restored,
}

/// The standard menu bar, with "Check for Updates…" and "Settings…" (⌘,)
/// under About in the app menu, and "Activity" (⌥⌘A) added to the Window
/// menu. About opens the app's own About window, which has the change log,
/// instead of the standard About panel.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn app_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::default(app)?;
    // On macOS the first submenu is the app menu, which starts with About.
    if let Some(MenuItemKind::Submenu(app_submenu)) = menu.items()?.first() {
        app_submenu.remove_at(0)?;
        let about = format!("About {}", app.package_info().name);
        app_submenu.insert(&MenuItem::with_id(app, ABOUT_WINDOW, about, true, None::<&str>)?, 0)?;
        app_submenu.insert(
            &MenuItem::with_id(
                app,
                CHECK_FOR_UPDATES,
                "Check for Updates…",
                true,
                None::<&str>,
            )?,
            1,
        )?;
        app_submenu.insert(&PredefinedMenuItem::separator(app)?, 2)?;
        app_submenu.insert(
            &MenuItem::with_id(app, SETTINGS_WINDOW, "Settings…", true, Some("CmdOrCtrl+,"))?,
            3,
        )?;
    }
    if let Some(MenuItemKind::Submenu(window)) = menu.get(WINDOW_SUBMENU_ID) {
        window.append(&PredefinedMenuItem::separator(app)?)?;
        window.append(&MenuItem::with_id(
            app,
            ACTIVITY_WINDOW,
            "Activity",
            true,
            Some("CmdOrCtrl+Alt+A"),
        )?)?;
    }
    Ok(menu)
}

/// Bring the Settings window to the front, opening it if it isn't open. Like
/// a macOS settings window it has no Save button and doesn't resize.
fn show_settings_window<R: Runtime>(app: &AppHandle<R>, opening: Opening) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    // It opens where it was last, at the height it had then, which is the
    // height its content needed then. Only the width is fixed.
    let place = saved_place(app, SETTINGS_WINDOW);
    let restored = opening == Opening::Restored;
    // Asked for, it opens hidden, and `grow_settings_window` shows it once
    // it is sized to its content, so it never visibly jumps. Shown anyway
    // after a second in case the page never gets that far. Showing a window
    // focuses it, so one reopened at launch is visible from the start
    // instead, without focus.
    let mut builder =
        WebviewWindowBuilder::new(app, SETTINGS_WINDOW, WebviewUrl::App("index.html".into()))
            .title("Settings")
            .inner_size(SETTINGS_WIDTH, place.map_or(SETTINGS_HEIGHT, |p| p.height))
            .resizable(false)
            .minimizable(false)
            .maximizable(false)
            .visible(restored)
            .focused(!restored);
    if let Some(place) = place {
        builder = builder.position(place.x, place.y);
    }
    let window = builder.build()?;
    remember_place(&window.as_ref().window());
    if !restored {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let _ = window.show();
        });
    }
    Ok(())
}

/// Width of the Settings window, in logical pixels.
const SETTINGS_WIDTH: f64 = 440.0;
/// Its height the first time it opens, before it is fitted to its content.
const SETTINGS_HEIGHT: f64 = 140.0;

/// Makes the Settings window `by` logical pixels taller (shorter when
/// negative) and shows it. The page asks for the difference between its
/// content and its viewport rather than for a height, because what
/// `set_size` sets on macOS is not the viewport's height (on Nino's Mac a
/// 170pt window showed about 138pt of page), so only a change is reliable.
/// The top-left corner stays where it is, so a window reopened where it was
/// last stays there.
#[tauri::command]
pub(crate) fn grow_settings_window(by: f64, window: tauri::WebviewWindow) -> Result<(), String> {
    let resize = || -> tauri::Result<()> {
        if by.abs() >= 0.5 {
            let corner = window.outer_position()?;
            let size = window.inner_size()?.to_logical::<f64>(window.scale_factor()?);
            window.set_size(tauri::LogicalSize::new(size.width, (size.height + by).round()))?;
            window.set_position(corner)?;
        }
        // Already visible means it was reopened at launch, without focus;
        // showing it again would take focus from the main window.
        if !window.is_visible()? {
            window.show()?;
        }
        Ok(())
    };
    resize().map_err(|e| e.to_string())
}

/// What the Activity menu item (⌥⌘A) does: closes the Activity window when
/// it is the focused window, and otherwise brings it to the front, opening it
/// if it isn't open.
fn toggle_activity_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(ACTIVITY_WINDOW) {
        if window.is_focused()? {
            return window.close();
        }
    }
    show_activity_window(app, Opening::Asked)
}

/// Bring the Activity window to the front, opening it if it isn't open. It
/// loads the same page as the main window, which picks the view by the
/// window's label.
fn show_activity_window<R: Runtime>(app: &AppHandle<R>, opening: Opening) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(ACTIVITY_WINDOW) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    let mut builder =
        WebviewWindowBuilder::new(app, ACTIVITY_WINDOW, WebviewUrl::App("index.html".into()))
            .title("Activity")
            .inner_size(460.0, 700.0)
            .min_inner_size(360.0, 360.0)
            .focused(opening == Opening::Asked);
    if let Some(place) = saved_place(app, ACTIVITY_WINDOW) {
        builder = builder
            .position(place.x, place.y)
            .inner_size(place.width, place.height);
    }
    let window = builder.build()?;
    remember_place(&window.as_ref().window());
    Ok(())
}

/// Bring the About window to the front, opening it if it isn't open. It opens
/// where it was last, but unlike the Activity and Settings windows it isn't
/// reopened at launch.
fn show_about_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(ABOUT_WINDOW) {
        window.unminimize()?;
        window.show()?;
        return window.set_focus();
    }
    let mut builder =
        WebviewWindowBuilder::new(app, ABOUT_WINDOW, WebviewUrl::App("index.html".into()))
            .title(format!("About {}", app.package_info().name))
            .inner_size(420.0, 560.0)
            .min_inner_size(320.0, 320.0)
            .minimizable(false)
            .maximizable(false);
    if let Some(place) = saved_place(app, ABOUT_WINDOW) {
        builder = builder
            .position(place.x, place.y)
            .inner_size(place.width, place.height);
    }
    let window = builder.build()?;
    remember_place(&window.as_ref().window());
    Ok(())
}
