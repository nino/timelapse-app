import { getCurrentWindow } from "@tauri-apps/api/window";
import React from "react";
import ReactDOM from "react-dom/client";

import { ACTIVITY_WINDOW } from "./activity";
import { ActivityView } from "./ActivityView";
import { App } from "./App";
import { initTimelapseRoot } from "./timelapseRoot";

// The header doubles as the macOS title bar and leaves room for the traffic lights.
if (navigator.userAgent.includes("Macintosh")) {
  document.documentElement.dataset.platform = "macos";
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
        {/* Window → Activity opens this same page in a window of its own. */}
        {getCurrentWindow().label === ACTIVITY_WINDOW ? <ActivityView /> : <App />}
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
