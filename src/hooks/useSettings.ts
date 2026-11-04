import React from "react";

import { getSettings, onSettingsChanged, type Settings } from "../settings";
import { useMountEffect } from "./useMountEffect";

/**
 * The app's settings, kept up to date when the Settings window changes one.
 * Null until they have loaded, or if they could not be read.
 */
export function useSettings(): Settings | null {
  const [settings, setSettings] = React.useState<Settings | null>(null);
  useMountEffect(() => {
    let stop: (() => void) | null = null;
    let unmounted = false;
    // Listening first, so a change made while the settings load isn't lost.
    onSettingsChanged((changed) => setSettings(changed)).then(
      (unlisten) => {
        if (unmounted) unlisten();
        else stop = unlisten;
      },
      (error: unknown) => console.error("Error listening for settings changes:", error),
    );
    getSettings().then(
      (loaded) => {
        if (!unmounted) setSettings((current) => current ?? loaded);
      },
      (error: unknown) => console.error("Error loading the settings:", error),
    );
    return (): void => {
      unmounted = true;
      stop?.();
    };
  });
  return settings;
}
