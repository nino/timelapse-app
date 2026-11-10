import React from "react";

import { getSettings, onSettingsChanged, type Settings } from "../settings";
import { useMountEffect } from "./useMountEffect";
import { useTauriEvent } from "./useTauriEvent";

/**
 * The app's settings, kept up to date when the Settings window changes one.
 * Null until they have loaded, or if they could not be read.
 */
export function useSettings(): Settings | null {
  const [settings, setSettings] = React.useState<Settings | null>(null);
  // Listening first (effects run in order), so a change made while the
  // settings load isn't lost.
  useTauriEvent(onSettingsChanged, setSettings, "settings changes");
  useMountEffect(() => {
    let unmounted = false;
    getSettings().then(
      (loaded) => {
        if (!unmounted) setSettings((current) => current ?? loaded);
      },
      (error: unknown) => console.error("Error loading the settings:", error),
    );
    return (): void => {
      unmounted = true;
    };
  });
  return settings;
}
