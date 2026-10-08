import { invoke } from "@tauri-apps/api/core";

/** Label of the Settings window. Mirrors `SETTINGS_WINDOW` in Rust. */
export const SETTINGS_WINDOW = "settings";

/** Mirrors `Settings` in `settings.rs`. */
export type Settings = {
  updateAutomatically: boolean;
};

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

/** Saves the change and returns the settings as saved. */
export function setUpdateAutomatically(enabled: boolean): Promise<Settings> {
  return invoke<Settings>("set_update_automatically", { enabled });
}

/** Makes the Settings window `by` logical pixels taller (shorter when negative) and shows it. */
export function growSettingsWindow(by: number): Promise<void> {
  return invoke<void>("grow_settings_window", { by });
}
