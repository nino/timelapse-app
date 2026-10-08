import { invoke } from "@tauri-apps/api/core";

/**
 * What the viewer shows, remembered across launches. Mirrors `ViewerPosition`
 * in `app_state.rs`. `day: null` means the newest day, whichever that is by
 * the next launch; `index: null` means the day's newest frame.
 */
export type ViewerPosition = {
  day: string | null;
  index: number | null;
};

/** What the viewer showed when the app last quit. */
export function getViewerPosition(): Promise<ViewerPosition> {
  return invoke<ViewerPosition>("get_viewer_position");
}

export function setViewerPosition(position: ViewerPosition): Promise<void> {
  return invoke<void>("set_viewer_position", { position });
}
