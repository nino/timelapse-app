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

#[tauri::command]
async fn get_error_logs(
    state: State<'_, PhotographerState>,
) -> Result<Vec<timelapse::ErrorLogEntry>, String> {
    let photographer_guard = state.lock().map_err(|e| e.to_string())?;

    if let Some(photographer) = &*photographer_guard {
        Ok(photographer.get_error_logs())
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
async fn clear_error_logs(state: State<'_, PhotographerState>) -> Result<String, String> {
    let photographer_guard = state.lock().map_err(|e| e.to_string())?;

    if let Some(photographer) = &*photographer_guard {
        photographer.clear_error_logs();
        Ok("Error logs cleared successfully".to_string())
    } else {
        Err("Timelapse is not running".to_string())
    }
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
            is_timelapse_running,
            get_error_logs,
            clear_error_logs
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greet() {
        let result = greet("Alice");
        assert_eq!(result, "Hello, Alice! You've been greeted from Rust!");

        let result = greet("Bob");
        assert_eq!(result, "Hello, Bob! You've been greeted from Rust!");
    }

    #[tokio::test]
    async fn test_start_timelapse_success() {
        let state: PhotographerState = Arc::new(Mutex::new(None));

        // Create a mock State wrapper
        let state_wrapper = State::from(&state);

        let result = start_timelapse(state_wrapper).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Timelapse started successfully");

        // Verify photographer was created
        let guard = state.lock().unwrap();
        assert!(guard.is_some());
    }

    #[tokio::test]
    async fn test_start_timelapse_already_running() {
        let state: PhotographerState = Arc::new(Mutex::new(None));

        // Start timelapse first time
        let state_wrapper = State::from(&state);
        let result = start_timelapse(state_wrapper).await;
        assert!(result.is_ok());

        // Try to start again - should fail
        let state_wrapper = State::from(&state);
        let result = start_timelapse(state_wrapper).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Timelapse is already running");
    }

    #[tokio::test]
    async fn test_stop_timelapse_success() {
        let state: PhotographerState = Arc::new(Mutex::new(None));

        // Start timelapse first
        let state_wrapper = State::from(&state);
        let _ = start_timelapse(state_wrapper).await;

        // Stop timelapse
        let state_wrapper = State::from(&state);
        let result = stop_timelapse(state_wrapper).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Timelapse stopped successfully");

        // Verify photographer was removed
        let guard = state.lock().unwrap();
        assert!(guard.is_none());
    }

    #[tokio::test]
    async fn test_stop_timelapse_not_running() {
        let state: PhotographerState = Arc::new(Mutex::new(None));

        let state_wrapper = State::from(&state);
        let result = stop_timelapse(state_wrapper).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Timelapse is not running");
    }

    #[tokio::test]
    async fn test_is_timelapse_running() {
        let state: PhotographerState = Arc::new(Mutex::new(None));

        // Initially not running
        let state_wrapper = State::from(&state);
        let result = is_timelapse_running(state_wrapper).await;
        assert!(result.is_ok());
        assert!(!result.unwrap());

        // Start timelapse
        let state_wrapper = State::from(&state);
        let _ = start_timelapse(state_wrapper).await;

        // Now should be running
        let state_wrapper = State::from(&state);
        let result = is_timelapse_running(state_wrapper).await;
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Stop timelapse
        let state_wrapper = State::from(&state);
        let _ = stop_timelapse(state_wrapper).await;

        // Should not be running again
        let state_wrapper = State::from(&state);
        let result = is_timelapse_running(state_wrapper).await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_get_error_logs_when_running() {
        let state: PhotographerState = Arc::new(Mutex::new(None));

        // Start timelapse
        let state_wrapper = State::from(&state);
        let _ = start_timelapse(state_wrapper).await;

        // Get error logs
        let state_wrapper = State::from(&state);
        let result = get_error_logs(state_wrapper).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_get_error_logs_when_not_running() {
        let state: PhotographerState = Arc::new(Mutex::new(None));

        let state_wrapper = State::from(&state);
        let result = get_error_logs(state_wrapper).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_clear_error_logs_success() {
        let state: PhotographerState = Arc::new(Mutex::new(None));

        // Start timelapse
        let state_wrapper = State::from(&state);
        let _ = start_timelapse(state_wrapper).await;

        // Clear error logs
        let state_wrapper = State::from(&state);
        let result = clear_error_logs(state_wrapper).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Error logs cleared successfully");
    }

    #[tokio::test]
    async fn test_clear_error_logs_not_running() {
        let state: PhotographerState = Arc::new(Mutex::new(None));

        let state_wrapper = State::from(&state);
        let result = clear_error_logs(state_wrapper).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Timelapse is not running");
    }
}
