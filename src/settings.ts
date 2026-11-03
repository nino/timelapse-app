import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Label of the Settings window. Mirrors `SETTINGS_WINDOW` in Rust. */
export const SETTINGS_WINDOW = "settings";

/** Mirrors `Settings` in `settings.rs`. */
export type Settings = {
  updateAutomatically: boolean;
  sendErrorReports: boolean;
  /** Beta: play hours kept as video in a `<video>` element (`VideoFrame.tsx`). */
  videoPlayerBeta: boolean;
};

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

/** Saves the change and returns the settings as saved. */
export function setUpdateAutomatically(enabled: boolean): Promise<Settings> {
  return invoke<Settings>("set_update_automatically", { enabled });
}

/** Saves the change and returns the settings as saved. */
export function setSendErrorReports(enabled: boolean): Promise<Settings> {
  return invoke<Settings>("set_send_error_reports", { enabled });
}

/** Saves the change and returns the settings as saved. */
export function setVideoPlayerBeta(enabled: boolean): Promise<Settings> {
  return invoke<Settings>("set_video_player_beta", { enabled });
}

/**
 * Calls `handler` with the new settings whenever the Settings window changes
 * one. Mirrors `SETTINGS_CHANGED` in Rust.
 */
export function onSettingsChanged(handler: (settings: Settings) => void): Promise<UnlistenFn> {
  return listen<Settings>("settings-changed", (event) => handler(event.payload));
}

/** Makes the Settings window `by` logical pixels taller (shorter when negative) and shows it. */
export function growSettingsWindow(by: number): Promise<void> {
  return invoke<void>("grow_settings_window", { by });
}
