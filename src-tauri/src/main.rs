// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if timelapse_app_lib::run_ocr_helper_if_asked() {
        return;
    }
    timelapse_app_lib::run()
}
