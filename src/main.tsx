import { getCurrentWindow } from "@tauri-apps/api/window";
import React from "react";
import ReactDOM from "react-dom/client";

import { ACTIVITY_WINDOW } from "./activity";
import { ActivityView } from "./ActivityView";
import { App } from "./App";
import { SETTINGS_WINDOW } from "./settings";
import { SettingsView } from "./SettingsView";
import { initTimelapseRoot } from "./timelapseRoot";

// The header doubles as the macOS title bar and leaves room for the traffic lights.
if (navigator.userAgent.includes("Macintosh")) {
  document.documentElement.dataset.platform = "macos";
}

function view(label: string): React.ReactNode {
  if (label === ACTIVITY_WINDOW) return <ActivityView />;
  if (label === SETTINGS_WINDOW) return <SettingsView />;
  return <App />;
}

// Resolve the library root from Rust before the first render — every path the
// UI builds depends on it, and guessing would risk a dev build reading the real
// library.
initTimelapseRoot().then(
  () => {
    ReactDOM.createRoot(
      document.getElementById("root") as HTMLElement,
    ).render(
      <React.StrictMode>
        {/* Window → Activity and Settings… open this same page in windows of their own. */}
        {view(getCurrentWindow().label)}
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
