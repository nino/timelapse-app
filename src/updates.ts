import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** The version of an update installed and waiting for a relaunch, if any. */
export function getInstalledUpdate(): Promise<string | null> {
  return invoke<string | null>("get_installed_update");
}

/** Calls `handler` with the version when an update finishes installing in the background. Mirrors `UPDATE_READY` in `updater.rs`. */
export function onUpdateReady(handler: (version: string) => void): Promise<UnlistenFn> {
  return listen<string>("update-ready", (event) => handler(event.payload));
}

/** Quits and opens the installed update. */
export function relaunchIntoUpdate(): Promise<void> {
  return invoke<void>("relaunch_into_update");
}
