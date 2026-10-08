import { getCurrentWindow } from "@tauri-apps/api/window";
import React from "react";
import ReactDOM from "react-dom/client";
import { changelog } from "virtual:changelog";

import { ABOUT_WINDOW } from "./about";
import { AboutView } from "./AboutView";
import { ACTIVITY_WINDOW } from "./activity";
import { ActivityView } from "./ActivityView";
import { App } from "./App";
import { SETTINGS_WINDOW } from "./settings";
import { SettingsView } from "./SettingsView";
import { initTimelapseRoot } from "./timelapseRoot";
import { getViewerPosition, type ViewerPosition } from "./viewerPosition";

// The header doubles as the macOS title bar and leaves room for the traffic lights.
if (navigator.userAgent.includes("Macintosh")) {
  document.documentElement.dataset.platform = "macos";
}

const label = getCurrentWindow().label;

function view(position: ViewerPosition | undefined): React.ReactNode {
  if (label === ACTIVITY_WINDOW) return <ActivityView />;
  if (label === SETTINGS_WINDOW) return <SettingsView />;
  if (label === ABOUT_WINDOW) return <AboutView changelog={changelog} />;
  return <App initialPosition={position} />;
}

// What the viewer showed when the app last quit. Asked for before the first
// render, so the viewer opens there instead of starting on the newest day and
// jumping. Without it the viewer opens the newest day.
function restoredPosition(): Promise<ViewerPosition | undefined> {
  if (label === ACTIVITY_WINDOW || label === SETTINGS_WINDOW || label === ABOUT_WINDOW) return Promise.resolve(undefined);
  return getViewerPosition().catch((error: unknown) => {
    console.error("Could not read the saved viewer position:", error);
    return undefined;
  });
}

// Resolve the library root from Rust before the first render — every path the
// UI builds depends on it, and guessing would risk a dev build reading the real
// library.
Promise.all([initTimelapseRoot(), restoredPosition()]).then(
  ([, position]) => {
    ReactDOM.createRoot(
      document.getElementById("root") as HTMLElement,
    ).render(
      <React.StrictMode>
        {/* About, Window → Activity and Settings… open this same page in windows of their own. */}
        {view(position)}
      </React.StrictMode>,
    );
  },
  (error: unknown) => {
    console.error("Could not resolve the timelapse directory:", error);
    const root = document.getElementById("root");
    if (root) {
      root.textContent =
        "Could not reach the Tauri backend to resolve the timelapse directory.";
    }
  },
);
