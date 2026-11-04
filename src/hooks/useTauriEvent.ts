import type { UnlistenFn } from "@tauri-apps/api/event";
import React from "react";

/**
 * Listens with `subscribe` (one of the `on…` wrappers, such as
 * `onSettingsChanged`) while the component is mounted. The latest `handler`
 * is always called, so an inline function is fine. `what` names the event in
 * the error logged if listening fails.
 */
export function useTauriEvent<T>(
  subscribe: (handler: (payload: T) => void) => Promise<UnlistenFn>,
  handler: (payload: T) => void,
  what: string,
): void {
  const onEvent = React.useEffectEvent(handler);
  React.useEffect(() => {
    let stop: UnlistenFn | null = null;
    let unmounted = false;
    subscribe((payload) => onEvent(payload)).then(
      (unlisten) => {
        // Unmounted while the listener was being set up.
        if (unmounted) unlisten();
        else stop = unlisten;
      },
      (error: unknown) => console.error(`Error listening for ${what}:`, error),
    );
    return (): void => {
      unmounted = true;
      stop?.();
    };
  }, [subscribe, what]);
}
