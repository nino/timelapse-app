import React from "react";

import { getInstalledUpdate, onUpdateReady } from "../updates";
import { useMountEffect } from "./useMountEffect";

/**
 * The version of an update installed in the background and waiting for a
 * relaunch, or null. The app never relaunches by itself, because a relaunch
 * brings it to the front; the title bar offers the relaunch instead.
 */
export function useInstalledUpdate(): string | null {
  const [version, setVersion] = React.useState<string | null>(null);
  useMountEffect(() => {
    let stop: (() => void) | null = null;
    let unmounted = false;
    onUpdateReady((ready) => setVersion(ready)).then(
      (unlisten) => {
        if (unmounted) unlisten();
        else stop = unlisten;
      },
      (error: unknown) => {
        console.error("Error listening for updates:", error);
      },
    );
    // Installed before this window opened, as when it was closed and reopened.
    getInstalledUpdate().then(
      (installed) => {
        if (!unmounted && installed) setVersion(installed);
      },
      (error: unknown) => {
        console.error("Error asking for an installed update:", error);
      },
    );
    return (): void => {
      unmounted = true;
      stop?.();
    };
  });
  return version;
}
