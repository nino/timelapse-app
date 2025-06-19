mod timelapse;

use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
use timelapse::Photographer;

// Shared state to manage the timelapse photographer
type PhotographerState = Arc<Mutex<Option<Photographer>>>;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn start_timelapse(state: State<'_, PhotographerState>) -> Result<String, String> {
    let mut photographer_guard = state.lock().map_err(|e| e.to_string())?;

    if photographer_guard.is_none() {
        let photographer = Photographer::new().map_err(|e| e.to_string())?;
        photographer.start();
        *photographer_guard = Some(photographer);
        Ok("Timelapse started successfully".to_string())
    } else {
        Err("Timelapse is already running".to_string())
    }
}

#[tauri::command]
async fn stop_timelapse(state: State<'_, PhotographerState>) -> Result<String, String> {
    let mut photographer_guard = state.lock().map_err(|e| e.to_string())?;

    if let Some(photographer) = photographer_guard.take() {
        photographer.stop();
        Ok("Timelapse stopped successfully".to_string())
    } else {
        Err("Timelapse is not running".to_string())
    }
}

#[tauri::command]
async fn is_timelapse_running(state: State<'_, PhotographerState>) -> Result<bool, String> {
    let photographer_guard = state.lock().map_err(|e| e.to_string())?;
    Ok(photographer_guard.is_some())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let photographer_state: PhotographerState = Arc::new(Mutex::new(None));

    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .manage(photographer_state)
        .setup(|app| {
            // Start timelapse automatically when app is ready
            let photographer_state = app.state::<PhotographerState>();
            let state_clone = Arc::clone(&photographer_state.inner());

            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                match Photographer::new() {
                    Ok(photographer) => {
                        photographer.start();
                        let mut guard = state_clone.lock().unwrap();
                        *guard = Some(photographer);
                        println!("Timelapse started automatically on app startup");
                    }
                    Err(e) => {
                        eprintln!("Failed to start timelapse automatically: {}", e);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            start_timelapse,
            stop_timelapse,
            is_timelapse_running
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
