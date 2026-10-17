import React from "react";

/**
 * Calls `callback` every `delayMs`, or not at all while `delayMs` is null.
 * The latest `callback` is always called, so an inline function is fine and
 * won't reset the cadence on every render.
 */
export function useInterval(callback: () => void, delayMs: number | null): void {
  const onTick = React.useEffectEvent(callback);

  React.useEffect(() => {
    if (delayMs === null) return;
    const timer = setInterval(() => onTick(), delayMs);
    return (): void => clearInterval(timer);
  }, [delayMs]);
}
